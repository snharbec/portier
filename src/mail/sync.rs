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

    if account.junk_folder.is_empty() || account.sent_folder.is_empty() {
        let found = imap::discover_folders(&mut session).await?;
        if account.junk_folder.is_empty() {
            account.junk_folder = found.junk.unwrap_or_default();
        }
        if account.sent_folder.is_empty() {
            account.sent_folder = found.sent.unwrap_or_default();
        }
        sqlx::query("UPDATE accounts SET junk_folder = ?, sent_folder = ? WHERE id = ?")
            .bind(&account.junk_folder)
            .bind(&account.sent_folder)
            .bind(account.id)
            .execute(&state.db)
            .await?;
    }

    // Forget folders that are no longer configured.
    sqlx::query("DELETE FROM folders WHERE account_id = ? AND name NOT IN (?, ?, ?, ?)")
        .bind(account.id)
        .bind(&account.inbox_folder)
        .bind(&account.junk_folder)
        .bind(&account.sent_folder)
        .bind(LOCAL_SENT)
        .execute(&state.db)
        .await?;

    let inbox = store::ensure_folder(&state.db, account.id, &account.inbox_folder, "inbox").await?;
    let mut folders = vec![inbox];
    if !account.sent_folder.is_empty() {
        folders.push(store::ensure_folder(&state.db, account.id, &account.sent_folder, "sent").await?);
    }
    let junk = if account.junk_folder.is_empty() {
        None
    } else {
        Some(store::ensure_folder(&state.db, account.id, &account.junk_folder, "junk").await?)
    };
    folders.extend(junk.clone());

    loop {
        if load_account(state, account_id).await?.is_none() {
            return Ok(());
        }
        let mut inbox_state = (0, None);
        for folder in &folders {
            let folder_state = sync_folder(state, &mut session, &account, folder, junk.as_ref()).await?;
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
    wanted.truncate(state.config.sync_max_per_folder);

    let local: Vec<(i64, i64, bool)> =
        sqlx::query_as("SELECT id, uid, seen FROM messages WHERE folder_id = ? AND uid IS NOT NULL")
            .bind(folder.id)
            .fetch_all(db)
            .await?;
    let local_by_uid: HashMap<u32, (i64, bool)> = local
        .iter()
        .map(|(id, uid, seen)| (*uid as u32, (*id, *seen)))
        .collect();

    // Deleted or moved away on the server.
    let gone: Vec<i64> = local_by_uid
        .iter()
        .filter(|(uid, _)| !server.contains(uid))
        .map(|(_, (id, _))| *id)
        .collect();
    if !gone.is_empty() {
        store::delete_messages(state, account, &gone).await?;
        state.notify(account.user_id, "mail");
    }

    // Read state changed elsewhere.
    let oldest = wanted.get(FLAG_WINDOW.min(wanted.len()).saturating_sub(1)).copied();
    if let Some(oldest) = oldest
        && !local_by_uid.is_empty()
    {
        for (uid, seen) in imap::fetch_flags(session, oldest).await? {
            if let Some((id, local_seen)) = local_by_uid.get(&uid)
                && *local_seen != seen
            {
                sqlx::query("UPDATE messages SET seen = ? WHERE id = ?")
                    .bind(seen)
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
            match store::store_message(state, account, folder, Some(fetched.uid), fetched.seen, &fetched.body).await {
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

/// Moves messages out of `from`, which must be the selected folder, into `to`.
///
/// The local rows change folder first and lose their UID; the sync of `to` finds the moved
/// copies and matches them up by Message-ID. Doing it in this order means a sync pass that runs
/// in between sees rows that are expected in `to` rather than unknown mail to download again.
pub async fn move_messages(
    state: &AppState,
    session: &mut Session,
    from: &Folder,
    uids: &[u32],
    to: &Folder,
) -> Result<()> {
    let uid_list = serde_json::to_string(uids)?;
    let rows: Vec<(i64, i64)> =
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

    if let Err(e) = imap::move_uids(session, uids, &to.name).await {
        // Nothing moved on the server: put the rows back where they were.
        for (id, uid) in rows {
            sqlx::query("UPDATE messages SET folder_id = ?, uid = ? WHERE id = ?")
                .bind(from.id)
                .bind(uid)
                .bind(id)
                .execute(&state.db)
                .await?;
        }
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
