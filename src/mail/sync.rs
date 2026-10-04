//! One long-running task per mail account: mirror INBOX, Sent and Junk into the database, then
//! wait in IMAP IDLE until the server or the application signals a change.

use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{Result, anyhow};
use tokio::sync::Notify;

use crate::{
    mail::{
        imap::{self, ImapParams, Session},
        store,
    },
    models::{Account, Folder, LOCAL_SENT},
    state::{AppState, SyncHandle, now},
};

const FETCH_BATCH: usize = 25;
const FLAG_WINDOW: usize = 1000;
const IDLE_SECONDS: u64 = 5 * 60;
const TRASH_LIMIT: usize = 500;

pub async fn load_account(state: &AppState, account_id: i64) -> Result<Option<Account>> {
    Ok(sqlx::query_as::<_, Account>("SELECT * FROM accounts WHERE id = ?")
        .bind(account_id)
        .fetch_optional(&state.db)
        .await?)
}

pub async fn start_all(state: &AppState) -> Result<()> {
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM accounts")
        .fetch_all(&state.db)
        .await?;
    for id in ids {
        start(state, id).await;
    }
    Ok(())
}

/// Starts (or restarts, after a settings change) the sync task of an account.
pub async fn start(state: &AppState, account_id: i64) {
    stop(state, account_id).await;
    let wake = Arc::new(Notify::new());
    let task = tokio::spawn(run(state.clone(), account_id, wake.clone()));
    state.sync.lock().await.insert(account_id, SyncHandle { task, wake });
}

pub async fn stop(state: &AppState, account_id: i64) {
    if let Some(handle) = state.sync.lock().await.remove(&account_id) {
        handle.task.abort();
    }
}

async fn run(state: AppState, account_id: i64, wake: Arc<Notify>) {
    let mut backoff = 5;
    loop {
        match session_loop(&state, account_id, &wake).await {
            Ok(()) => return, // account was deleted
            Err(e) => {
                tracing::warn!(account_id, "sync failed: {e:#}");
                let _ = sqlx::query("UPDATE accounts SET last_error = ? WHERE id = ?")
                    .bind(format!("{e:#}"))
                    .bind(account_id)
                    .execute(&state.db)
                    .await;
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(backoff)) => {}
                    _ = wake.notified() => {}
                }
                backoff = (backoff * 2).min(300);
            }
        }
    }
}

/// Connects and keeps syncing until the connection breaks (Err) or the account is gone (Ok).
async fn session_loop(state: &AppState, account_id: i64, wake: &Notify) -> Result<()> {
    let Some(mut account) = load_account(state, account_id).await? else {
        return Ok(());
    };
    let params = ImapParams::for_account(&account, &state.config.master_key)?;
    let mut session = imap::connect(&params).await?;

    if account.junk_folder.is_empty()
        || account.sent_folder.is_empty()
        || account.trash_folder.is_empty()
        || account.archive_folder.is_empty()
    {
        let found = imap::discover_folders(&mut session).await?;
        if account.junk_folder.is_empty() {
            account.junk_folder = found.junk.unwrap_or_default();
        }
        if account.sent_folder.is_empty() {
            account.sent_folder = found.sent.unwrap_or_default();
        }
        if account.trash_folder.is_empty() {
            account.trash_folder = found.trash.unwrap_or_default();
        }
        if account.archive_folder.is_empty() {
            account.archive_folder = found.archive.unwrap_or_default();
        }
        sqlx::query(
            "UPDATE accounts SET junk_folder = ?, sent_folder = ?, trash_folder = ?, archive_folder = ? WHERE id = ?",
        )
        .bind(&account.junk_folder)
        .bind(&account.sent_folder)
        .bind(&account.trash_folder)
        .bind(&account.archive_folder)
        .bind(account.id)
        .execute(&state.db)
        .await?;
    }

    // Forget folders that are no longer configured.
    sqlx::query("DELETE FROM folders WHERE account_id = ? AND name NOT IN (?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(account.id)
        .bind(&account.inbox_folder)
        .bind(&account.junk_folder)
        .bind(&account.sent_folder)
        .bind(&account.archive_folder)
        .bind(&account.trash_folder)
        .bind(&account.feed_folder)
        .bind(&account.delayed_folder)
        .bind(LOCAL_SENT)
        .execute(&state.db)
        .await?;

    let inbox = store::ensure_folder(&state.db, account.id, &account.inbox_folder, "inbox").await?;
    let mut folders = vec![inbox.clone()];
    if !account.sent_folder.is_empty() {
        folders.push(store::ensure_folder(&state.db, account.id, &account.sent_folder, "sent").await?);
    }
    let junk = if account.junk_folder.is_empty() {
        None
    } else {
        Some(store::ensure_folder(&state.db, account.id, &account.junk_folder, "junk").await?)
    };
    folders.extend(junk.clone());
    // The folders for "Nice to know" and delayed mail are the only ones Portier creates on
    // the server.
    let feed = if account.feed_folder.is_empty() {
        None
    } else {
        imap::ensure_mailbox(&mut session, &account.feed_folder).await?;
        Some(store::ensure_folder(&state.db, account.id, &account.feed_folder, "feed").await?)
    };
    folders.extend(feed.clone());
    let delayed = if account.delayed_folder.is_empty() {
        None
    } else {
        imap::ensure_mailbox(&mut session, &account.delayed_folder).await?;
        Some(store::ensure_folder(&state.db, account.id, &account.delayed_folder, "delayed").await?)
    };
    folders.extend(delayed.clone());
    if !account.archive_folder.is_empty() {
        folders.push(store::ensure_folder(&state.db, account.id, &account.archive_folder, "archive").await?);
    }
    if !account.trash_folder.is_empty() {
        folders.push(store::ensure_folder(&state.db, account.id, &account.trash_folder, "trash").await?);
    }

    loop {
        if load_account(state, account_id).await?.is_none() {
            return Ok(());
        }
        let mut inbox_state = (0, None);
        for folder in &folders {
            let folder_state = sync_folder(state, &mut session, &account, folder, junk.as_ref()).await?;
            // Delayed first: a delayed conversation waits in its folder whoever sent it.
            if let Some(delayed) = &delayed {
                sort_delayed(state, &mut session, folder, &inbox, delayed).await?;
            }
            if let Some(feed) = &feed {
                sort_feed(state, &mut session, folder, &inbox, feed).await?;
            }
            if folder.role == "inbox" {
                inbox_state = folder_state;
            }
        }
        sqlx::query("UPDATE accounts SET last_error = NULL, last_sync_at = ? WHERE id = ?")
            .bind(now())
            .bind(account.id)
            .execute(&state.db)
            .await?;
        state.notify(account.user_id, "sync");

        // IDLE only reports what happens after it starts. Mail that arrived while the other
        // folders were syncing would wait for the next timeout, so look again first.
        let mailbox = session.select(&account.inbox_folder).await?;
        if (mailbox.exists, mailbox.uid_next) != inbox_state {
            continue;
        }
        let mut idle = session.idle();
        idle.init().await?;
        {
            let (server_event, _stop) = idle.wait_with_timeout(Duration::from_secs(IDLE_SECONDS));
            tokio::select! {
                result = server_event => { result?; }
                _ = wake.notified() => {}
            }
        }
        session = idle.done().await?;
    }
}

async fn sync_folder(
    state: &AppState,
    session: &mut Session,
    account: &Account,
    folder: &Folder,
    junk: Option<&Folder>,
) -> Result<(u32, Option<u32>)> {
    let db = &state.db;
    let mailbox = session.select(&folder.name).await?;

    let validity = mailbox.uid_validity.map(i64::from);
    let stored_validity: Option<i64> = sqlx::query_scalar("SELECT uidvalidity FROM folders WHERE id = ?")
        .bind(folder.id)
        .fetch_one(db)
        .await?;
    if stored_validity != validity {
        if stored_validity.is_some() {
            // UIDs were renumbered; everything known about this folder is void.
            let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM messages WHERE folder_id = ? AND uid IS NOT NULL")
                .bind(folder.id)
                .fetch_all(db)
                .await?;
            store::delete_messages(state, account, &ids).await?;
        }
        sqlx::query("UPDATE folders SET uidvalidity = ? WHERE id = ?")
            .bind(validity)
            .bind(folder.id)
            .execute(db)
            .await?;
    }

    let server = imap::all_uids(session).await?;
    let mut wanted: Vec<u32> = server.iter().copied().collect();
    wanted.sort_unstable_by(|a, b| b.cmp(a));
    // Trash is kept for looking something up again, not as an archive: mirror only its newest part.
    let limit = if folder.role == "trash" {
        TRASH_LIMIT
    } else {
        usize::MAX
    };
    wanted.truncate(state.config.sync_max_per_folder.min(limit));

    let local: Vec<(i64, i64, bool, bool)> =
        sqlx::query_as("SELECT id, uid, seen, flagged FROM messages WHERE folder_id = ? AND uid IS NOT NULL")
            .bind(folder.id)
            .fetch_all(db)
            .await?;
    let local_by_uid: HashMap<u32, (i64, bool, bool)> = local
        .iter()
        .map(|(id, uid, seen, flagged)| (*uid as u32, (*id, *seen, *flagged)))
        .collect();

    // Deleted or moved away on the server.
    let gone: Vec<i64> = local_by_uid
        .iter()
        .filter(|(uid, _)| !server.contains(uid))
        .map(|(_, (id, _, _))| *id)
        .collect();
    if !gone.is_empty() {
        store::delete_messages(state, account, &gone).await?;
        state.notify(account.user_id, "mail");
    }

    // Read state or the flag changed elsewhere.
    let oldest = wanted.get(FLAG_WINDOW.min(wanted.len()).saturating_sub(1)).copied();
    if let Some(oldest) = oldest
        && !local_by_uid.is_empty()
    {
        for (uid, seen, flagged) in imap::fetch_flags(session, oldest).await? {
            if let Some((id, local_seen, local_flagged)) = local_by_uid.get(&uid)
                && (*local_seen != seen || *local_flagged != flagged)
            {
                sqlx::query("UPDATE messages SET seen = ?, flagged = ? WHERE id = ?")
                    .bind(seen)
                    .bind(flagged)
                    .bind(id)
                    .execute(db)
                    .await?;
            }
        }
    }

    // New on the server, newest first so the UI fills from the top.
    let missing: Vec<u32> = wanted
        .into_iter()
        .filter(|uid| !local_by_uid.contains_key(uid))
        .collect();
    let mut to_junk: Vec<u32> = Vec::new();
    for batch in missing.chunks(FETCH_BATCH) {
        for fetched in imap::fetch_full(session, batch).await? {
            match store::store_message(
                state,
                account,
                folder,
                Some(fetched.uid),
                fetched.seen,
                fetched.flagged,
                &fetched.body,
            )
            .await
            {
                Ok(Some(stored)) if stored.junk_sender && folder.role == "inbox" => to_junk.push(fetched.uid),
                Ok(_) => {}
                Err(e) => tracing::warn!(account = account.id, uid = fetched.uid, "cannot store message: {e:#}"),
            }
        }
        state.notify(account.user_id, "mail");
    }

    // Mail from senders already screened out goes straight to Junk.
    if let (Some(junk), false) = (junk, to_junk.is_empty()) {
        move_messages(state, session, folder, &to_junk, junk).await?;
    }
    // What the folder looked like when this pass started; moves made above change it, which
    // makes the caller run one more pass.
    Ok((mailbox.exists, mailbox.uid_next))
}

/// Keeps "Nice to know" mail in its own folder: out of the inbox what such senders sent, and
/// back to the inbox what sits in that folder from a sender who is no longer one of them.
/// `folder` is the selected folder, just synced. Runs on every pass, so it also carries out a
/// changed decision about a sender and catches up when the folder is first set.
async fn sort_feed(
    state: &AppState,
    session: &mut Session,
    folder: &Folder,
    inbox: &Folder,
    feed: &Folder,
) -> Result<()> {
    let (wanted, target) = match folder.role.as_str() {
        "inbox" => ("s.category = 'feed'", feed),
        "feed" => ("(s.category IS NULL OR s.category != 'feed')", inbox),
        _ => return Ok(()),
    };
    let uids: Vec<u32> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT m.uid FROM messages m LEFT JOIN senders s ON s.id = m.sender_id
         WHERE m.folder_id = ? AND m.uid IS NOT NULL AND m.is_outgoing = 0 AND {wanted}"
    )))
    .bind(folder.id)
    .fetch_all(&state.db)
    .await?;
    if !uids.is_empty() {
        move_messages(state, session, folder, &uids, target).await?;
    }
    Ok(())
}

/// Keeps delayed conversations in their own folder: out of the inbox (and the "Nice to know"
/// folder) while the delay lasts, back to the inbox once it is over. `folder` is the selected
/// folder, just synced. Runs on every pass, like `sort_feed`.
async fn sort_delayed(
    state: &AppState,
    session: &mut Session,
    folder: &Folder,
    inbox: &Folder,
    delayed: &Folder,
) -> Result<()> {
    const WAITING: &str = "(t.snoozed_until IS NOT NULL AND t.snoozed_until > unixepoch())";
    let (wanted, target) = match folder.role.as_str() {
        "inbox" | "feed" => (WAITING.to_string(), delayed),
        "delayed" => (format!("NOT {WAITING}"), inbox),
        _ => return Ok(()),
    };
    // With each mail: whether its conversation came back because the delay ran out.
    let rows: Vec<(u32, bool)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT m.uid, t.returned_at IS NOT NULL FROM messages m JOIN threads t ON t.id = m.thread_id
         WHERE m.folder_id = ? AND m.uid IS NOT NULL AND m.is_outgoing = 0 AND {wanted}"
    )))
    .bind(folder.id)
    .fetch_all(&state.db)
    .await?;
    if rows.is_empty() {
        return Ok(());
    }
    let uids: Vec<u32> = rows.iter().map(|(uid, _)| *uid).collect();
    if folder.role == "delayed" {
        // Returned mail arrives unseen, whatever happened to its mark while it waited.
        let returned: Vec<u32> = rows.iter().filter(|(_, back)| *back).map(|(uid, _)| *uid).collect();
        if !returned.is_empty() {
            imap::set_seen(session, &returned, false).await?;
            sqlx::query("UPDATE messages SET seen = 0 WHERE folder_id = ? AND uid IN (SELECT value FROM json_each(?))")
                .bind(folder.id)
                .bind(serde_json::to_string(&returned)?)
                .execute(&state.db)
                .await?;
        }
    }
    move_messages(state, session, folder, &uids, target).await
}

/// Local rows as they were before a move: (message id, UID in the folder they left).
pub type Moved = Vec<(i64, i64)>;

/// Records locally that messages left `from` for `to`: they change folder and lose their UID.
/// The sync of `to` later finds the moved copies and matches them up by Message-ID. Until then a
/// sync pass sees rows that are expected in `to` rather than unknown mail to download again.
pub async fn move_local(state: &AppState, from: &Folder, uids: &[u32], to: &Folder) -> Result<Moved> {
    let uid_list = serde_json::to_string(uids)?;
    let rows: Moved =
        sqlx::query_as("SELECT id, uid FROM messages WHERE folder_id = ? AND uid IN (SELECT value FROM json_each(?))")
            .bind(from.id)
            .bind(&uid_list)
            .fetch_all(&state.db)
            .await?;
    sqlx::query(
        "UPDATE messages SET folder_id = ?, uid = NULL
         WHERE folder_id = ? AND uid IN (SELECT value FROM json_each(?))",
    )
    .bind(to.id)
    .bind(from.id)
    .bind(&uid_list)
    .execute(&state.db)
    .await?;
    Ok(rows)
}

/// Undoes `move_local` after the server refused the move.
pub async fn restore_local(state: &AppState, from: &Folder, rows: Moved) -> Result<()> {
    for (id, uid) in rows {
        sqlx::query("UPDATE messages SET folder_id = ?, uid = ? WHERE id = ?")
            .bind(from.id)
            .bind(uid)
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    Ok(())
}

/// Moves messages out of `from`, which must be the selected folder, into `to`: locally first,
/// then on the server, putting the local rows back if the server refuses.
pub async fn move_messages(
    state: &AppState,
    session: &mut Session,
    from: &Folder,
    uids: &[u32],
    to: &Folder,
) -> Result<()> {
    let rows = move_local(state, from, uids, to).await?;
    if let Err(e) = imap::move_uids(session, uids, &to.name).await {
        restore_local(state, from, rows).await?;
        return Err(e);
    }
    Ok(())
}

/// Runs a one-off IMAP action on its own connection, then nudges the sync task.
pub fn spawn_action<F, Fut>(state: &AppState, account_id: i64, what: &'static str, action: F)
where
    F: FnOnce(Session) -> Fut + Send + 'static,
    Fut: Future<Output = Result<()>> + Send,
{
    let state = state.clone();
    tokio::spawn(async move {
        let result: Result<()> = async {
            let account = load_account(&state, account_id)
                .await?
                .ok_or_else(|| anyhow!("account deleted"))?;
            let params = ImapParams::for_account(&account, &state.config.master_key)?;
            let session = imap::connect(&params).await?;
            action(session).await
        }
        .await;
        if let Err(e) = result {
            tracing::warn!(account_id, "{what} failed: {e:#}");
        }
        state.wake_sync(account_id).await;
    });
}
