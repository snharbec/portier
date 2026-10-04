//! Actions on several selected mails at once: mark read or unread, archive, move to Trash and back,
//! move to a folder.

use std::collections::HashMap;

use axum::{Json, extract::State};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    auth::CurrentUser,
    error::{ApiError, ApiResult},
    mail::{delay, imap, store, sync},
    models::{Account, Folder},
    state::AppState,
};

const MAX_SELECTION: usize = 1000;

#[derive(Deserialize)]
pub struct BulkInput {
    /// read | unread | important | unimportant | delay | undelay | archive | trash | untrash | move
    action: String,
    /// Whole conversations: every message in them is affected.
    #[serde(default)]
    thread_ids: Vec<i64>,
    #[serde(default)]
    message_ids: Vec<i64>,
    /// For `move`: the account whose folder is the target.
    account_id: Option<i64>,
    folder: Option<String>,
    /// For `delay`: 1, 2, 3 or 7.
    days: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct Target {
    id: i64,
    thread_id: i64,
    account_id: i64,
    folder_id: i64,
    folder_name: String,
    folder_role: String,
    uid: Option<u32>,
    is_outgoing: bool,
}

/// The messages a selection of conversations and single mails stands for, limited to the user's own.
async fn targets_for(
    state: &AppState,
    user_id: i64,
    thread_ids: &[i64],
    message_ids: &[i64],
) -> ApiResult<Vec<Target>> {
    // A message can exist twice (a copy in Sent and one in the inbox); selecting one means both.
    Ok(sqlx::query_as(
        "SELECT m.id, m.thread_id, m.account_id, f.id AS folder_id, f.name AS folder_name, f.role AS folder_role, m.uid,
                m.is_outgoing
         FROM messages m JOIN folders f ON f.id = m.folder_id
         WHERE m.user_id = ?1
           AND (m.thread_id IN (SELECT value FROM json_each(?2))
                OR (m.account_id, m.message_id) IN (
                    SELECT account_id, message_id FROM messages
                    WHERE user_id = ?1 AND id IN (SELECT value FROM json_each(?3))))",
    )
    .bind(user_id)
    .bind(serde_json::to_string(thread_ids).map_err(anyhow::Error::from)?)
    .bind(serde_json::to_string(message_ids).map_err(anyhow::Error::from)?)
    .fetch_all(&state.db)
    .await?)
}

/// Archives whole conversations, as the Archive action does. Used by the automatic archive.
pub(crate) async fn archive_conversations(state: &AppState, user_id: i64, thread_ids: &[i64]) -> ApiResult<usize> {
    let targets = targets_for(state, user_id, thread_ids, &[]).await?;
    set_delay(state, user_id, &targets, None).await?;
    file_away(state, user_id, targets, Shelf::Archive).await
}

pub async fn apply(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(input): Json<BulkInput>,
) -> ApiResult<Json<Value>> {
    if input.thread_ids.len() + input.message_ids.len() > MAX_SELECTION {
        return Err(ApiError::bad_request("too many mails selected at once"));
    }
    let targets = targets_for(&state, user.id, &input.thread_ids, &input.message_ids).await?;
    if targets.is_empty() {
        return Ok(Json(json!({ "affected": 0 })));
    }

    let affected = match input.action.as_str() {
        "read" => set_seen(&state, &targets, true).await?,
        "unread" => set_seen(&state, &targets, false).await?,
        "important" => set_flagged(&state, &targets, true).await?,
        "unimportant" => set_flagged(&state, &targets, false).await?,
        "delay" => {
            let days = input.days.filter(|d| delay::CHOICES.contains(d));
            let days = days.ok_or_else(|| ApiError::bad_request("choose a delay of 1, 2, 3 or 7 days"))?;
            set_delay(&state, user.id, &targets, Some(delay::return_time_from_now(days))).await?
        }
        "undelay" => set_delay(&state, user.id, &targets, None).await?,
        "archive" => {
            // Filed away for good: a delay must not bring it back.
            set_delay(&state, user.id, &targets, None).await?;
            file_away(&state, user.id, targets, Shelf::Archive).await?
        }
        "trash" => {
            set_delay(&state, user.id, &targets, None).await?;
            file_away(&state, user.id, targets, Shelf::Trash).await?
        }
        "untrash" => {
            // Back to where it came from: received mail to the inbox, your own to Sent.
            let (sent, received): (Vec<Target>, Vec<Target>) = targets.into_iter().partition(|t| t.is_outgoing);
            let moved = file_away(&state, user.id, received, Shelf::Inbox).await?
                + file_away(&state, user.id, sent, Shelf::Sent).await?;
            if moved == 0 {
                // Just moved to the Trash and not yet seen there by the sync, or not in the Trash at all.
                return Err(ApiError::bad_request(
                    "Nothing to take out of the Trash yet. Mail that was just moved there needs a moment; try again.",
                ));
            }
            moved
        }
        "move" => {
            let account_id = input
                .account_id
                .ok_or_else(|| ApiError::bad_request("account is required"))?;
            let folder = input.folder.as_deref().map(str::trim).unwrap_or_default();
            if folder.is_empty() {
                return Err(ApiError::bad_request("choose a folder"));
            }
            if targets.iter().any(|t| t.account_id != account_id) {
                return Err(ApiError::bad_request(
                    "the selected mails belong to different accounts; move them one account at a time",
                ));
            }
            relocate(&state, user.id, targets, Some(folder.to_string())).await?
        }
        _ => return Err(ApiError::bad_request("unknown action")),
    };
    state.notify(user.id, "mail");
    Ok(Json(json!({ "affected": affected })))
}

/// Important is the server's flag on mail in the inbox; other mail programs show it as flag or star.
async fn set_flagged(state: &AppState, targets: &[Target], flagged: bool) -> ApiResult<usize> {
    let in_inbox: Vec<&Target> = targets
        .iter()
        .filter(|t| matches!(t.folder_role.as_str(), "inbox" | "feed" | "delayed"))
        .collect();
    let ids: Vec<i64> = in_inbox.iter().map(|t| t.id).collect();
    sqlx::query("UPDATE messages SET flagged = ? WHERE id IN (SELECT value FROM json_each(?))")
        .bind(flagged)
        .bind(serde_json::to_string(&ids).map_err(anyhow::Error::from)?)
        .execute(&state.db)
        .await?;

    let mut by_folder: HashMap<(i64, String), Vec<u32>> = HashMap::new();
    for target in &in_inbox {
        if let Some(uid) = target.uid {
            by_folder
                .entry((target.account_id, target.folder_name.clone()))
                .or_default()
                .push(uid);
        }
    }
    for ((account_id, folder), uids) in by_folder {
        sync::spawn_action(state, account_id, "changing the flag", move |mut session| async move {
            session.select(&folder).await?;
            imap::set_flagged(&mut session, &uids, flagged).await?;
            let _ = session.logout().await;
            Ok(())
        });
    }
    Ok(ids.len())
}

/// Delays the conversations of the selection until `until`, or ends their delay with `None`.
/// Kept in Email Screen only: on the mail server nothing changes until the mail returns.
async fn set_delay(state: &AppState, user_id: i64, targets: &[Target], until: Option<i64>) -> ApiResult<usize> {
    let mut threads: Vec<i64> = targets.iter().map(|t| t.thread_id).collect();
    threads.sort_unstable();
    threads.dedup();
    let result = sqlx::query(
        "UPDATE threads SET snoozed_until = ?1, returned_at = CASE WHEN ?1 IS NULL THEN returned_at END
         WHERE user_id = ?2 AND id IN (SELECT value FROM json_each(?3))",
    )
    .bind(until)
    .bind(user_id)
    .bind(serde_json::to_string(&threads).map_err(anyhow::Error::from)?)
    .execute(&state.db)
    .await?;
    // Where delayed mail has a folder of its own on the server, the sync moves it there or back.
    let mut accounts: Vec<i64> = targets.iter().map(|t| t.account_id).collect();
    accounts.sort_unstable();
    accounts.dedup();
    for account_id in accounts {
        state.wake_sync(account_id).await;
    }
    Ok(result.rows_affected() as usize)
}

/// Received mail only: your own sent messages have no unread state.
async fn set_seen(state: &AppState, targets: &[Target], seen: bool) -> ApiResult<usize> {
    let inbound: Vec<&Target> = targets.iter().filter(|t| !t.is_outgoing).collect();
    let ids: Vec<i64> = inbound.iter().map(|t| t.id).collect();
    sqlx::query("UPDATE messages SET seen = ? WHERE id IN (SELECT value FROM json_each(?))")
        .bind(seen)
        .bind(serde_json::to_string(&ids).map_err(anyhow::Error::from)?)
        .execute(&state.db)
        .await?;

    let mut by_folder: HashMap<(i64, String), Vec<u32>> = HashMap::new();
    for target in &inbound {
        if let Some(uid) = target.uid {
            by_folder
                .entry((target.account_id, target.folder_name.clone()))
                .or_default()
                .push(uid);
        }
    }
    for ((account_id, folder), uids) in by_folder {
        sync::spawn_action(
            state,
            account_id,
            "changing read state",
            move |mut session| async move {
                session.select(&folder).await?;
                imap::set_seen(&mut session, &uids, seen).await?;
                let _ = session.logout().await;
                Ok(())
            },
        );
    }
    Ok(ids.len())
}

async fn account_for_task(state: &AppState, account_id: i64) -> anyhow::Result<Account> {
    sync::load_account(state, account_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("account deleted"))
}

fn targets_without_uid(messages: &[(i64, Option<u32>)]) -> Vec<i64> {
    messages
        .iter()
        .filter(|(_, uid)| uid.is_none())
        .map(|(id, _)| *id)
        .collect()
}

/// The mirrored folders mail is moved between.
#[derive(Clone, Copy)]
enum Shelf {
    /// Received mail from the inbox or Junk; sent copies of a conversation stay in Sent.
    Archive,
    /// Mail from anywhere.
    Trash,
    /// Received mail out of the Trash.
    Inbox,
    /// Your own mail out of the Trash.
    Sent,
}

/// Moves mail to the account's Archive or Trash folder, or out of the Trash back to the inbox or
/// Sent. Unlike other moves the mail stays in Email Screen: these folders are mirrored and shown
/// as their own lists.
async fn file_away(state: &AppState, user_id: i64, targets: Vec<Target>, shelf: Shelf) -> ApiResult<usize> {
    let (role, what) = match shelf {
        Shelf::Archive => ("archive", "Archive"),
        Shelf::Trash => ("trash", "Trash"),
        Shelf::Inbox => ("inbox", "inbox"),
        Shelf::Sent => ("sent", "Sent"),
    };
    let mut by_account: HashMap<i64, Vec<Target>> = HashMap::new();
    for target in targets {
        let movable = match shelf {
            Shelf::Archive => matches!(target.folder_role.as_str(), "inbox" | "junk" | "feed" | "delayed"),
            Shelf::Trash => target.folder_role != "trash",
            Shelf::Inbox | Shelf::Sent => target.folder_role == "trash",
        };
        if movable {
            by_account.entry(target.account_id).or_default().push(target);
        }
    }

    // Resolve every destination first, so nothing happens if one account has none.
    let mut plans = Vec::new();
    for (account_id, targets) in by_account {
        let account: Account = sqlx::query_as("SELECT * FROM accounts WHERE id = ? AND user_id = ?")
            .bind(account_id)
            .bind(user_id)
            .fetch_one(&state.db)
            .await?;
        let folder_name = match shelf {
            Shelf::Archive => &account.archive_folder,
            Shelf::Trash => &account.trash_folder,
            Shelf::Inbox => &account.inbox_folder,
            Shelf::Sent => &account.sent_folder,
        };
        if folder_name.is_empty() {
            return Err(ApiError::bad_request(format!(
                "No {what} folder is known for the account {}. Set it in Settings.",
                account.address
            )));
        }
        let destination = store::ensure_folder(&state.db, account.id, folder_name, role).await?;
        plans.push((account, destination, targets));
    }

    let mut affected = 0;
    for (account, destination, targets) in plans {
        let mut by_folder: HashMap<i64, (String, Vec<u32>)> = HashMap::new();
        let mut by_folder_ids: Vec<(i64, Option<u32>)> = Vec::new();
        for target in targets {
            let entry = by_folder
                .entry(target.folder_id)
                .or_insert_with(|| (target.folder_name.clone(), Vec::new()));
            entry.1.extend(target.uid);
            by_folder_ids.push((target.id, target.uid));
        }
        // Mail that exists only here (sent without a Sent folder on the server) has nothing to move.
        let local_only: Vec<i64> = targets_without_uid(&by_folder_ids);
        if matches!(shelf, Shelf::Trash) && !local_only.is_empty() {
            affected += local_only.len();
            store::delete_messages(state, &account, &local_only).await?;
        }
        for (folder_id, (name, uids)) in by_folder {
            if uids.is_empty() {
                continue;
            }
            affected += uids.len();
            let source = Folder {
                id: folder_id,
                name,
                role: String::new(),
            };
            // Locally at once, so the lists are right when this request returns.
            let moved = sync::move_local(state, &source, &uids, &destination).await?;
            let (task, destination, account_id) = (state.clone(), destination.clone(), account.id);
            tokio::spawn(async move {
                let on_server: anyhow::Result<()> = async {
                    let params = imap::ImapParams::for_account(
                        &account_for_task(&task, account_id).await?,
                        &task.config.master_key,
                    )?;
                    let mut session = imap::connect(&params).await?;
                    session.select(&source.name).await?;
                    imap::move_uids(&mut session, &uids, &destination.name).await?;
                    let _ = session.logout().await;
                    Ok(())
                }
                .await;
                if let Err(e) = on_server {
                    // Nothing moved on the server, whatever the reason: show the mail where it still is.
                    tracing::warn!(account_id, "moving mail to {what} failed: {e:#}");
                    if let Err(e) = sync::restore_local(&task, &source, moved).await {
                        tracing::error!(account_id, "moved mail could not be put back locally: {e:#}");
                    }
                    task.notify(user_id, "mail");
                }
                task.wake_sync(account_id).await;
            });
        }
    }
    Ok(affected)
}

/// Moves mail on the server to `folder`, or to each account's Trash when `folder` is `None`.
/// The target is not a folder Email Screen mirrors, so locally the mail is simply removed; if the
/// server refuses the move, the next sync brings it back.
async fn relocate(state: &AppState, user_id: i64, targets: Vec<Target>, folder: Option<String>) -> ApiResult<usize> {
    let mut by_account: HashMap<i64, Vec<Target>> = HashMap::new();
    for target in targets {
        by_account.entry(target.account_id).or_default().push(target);
    }

    // Resolve every destination first, so nothing happens if one account has none.
    let mut plans: Vec<(Account, String, Vec<Target>)> = Vec::new();
    for (account_id, targets) in by_account {
        let account: Account = sqlx::query_as("SELECT * FROM accounts WHERE id = ? AND user_id = ?")
            .bind(account_id)
            .bind(user_id)
            .fetch_one(&state.db)
            .await?;
        let destination = match &folder {
            Some(folder) => folder.clone(),
            None if account.trash_folder.is_empty() => {
                return Err(ApiError::bad_request(format!(
                    "No Trash folder is known for the account {}. Set it in Settings.",
                    account.address
                )));
            }
            None => account.trash_folder.clone(),
        };
        plans.push((account, destination, targets));
    }

    let mut affected = 0;
    for (account, destination, targets) in plans {
        let mut ids = Vec::new();
        let mut by_folder: HashMap<String, Vec<u32>> = HashMap::new();
        for target in targets {
            if target.folder_name == destination {
                continue; // already there
            }
            ids.push(target.id);
            if let Some(uid) = target.uid {
                by_folder.entry(target.folder_name).or_default().push(uid);
            }
        }
        affected += ids.len();
        store::delete_messages(state, &account, &ids).await?;
        for (source, uids) in by_folder {
            let destination = destination.clone();
            sync::spawn_action(state, account.id, "moving mail", move |mut session| async move {
                session.select(&source).await?;
                imap::move_uids(&mut session, &uids, &destination).await?;
                let _ = session.logout().await;
                Ok(())
            });
        }
    }
    Ok(affected)
}
