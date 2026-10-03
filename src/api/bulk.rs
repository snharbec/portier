//! Actions on several selected mails at once: mark read or unread, move to Trash, move to a folder.

use std::collections::HashMap;

use axum::{Json, extract::State};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    auth::CurrentUser,
    error::{ApiError, ApiResult},
    mail::{imap, store, sync},
    models::Account,
    state::AppState,
};

const MAX_SELECTION: usize = 1000;

#[derive(Deserialize)]
pub struct BulkInput {
    /// read | unread | trash | move
    action: String,
    /// Whole conversations: every message in them is affected.
    #[serde(default)]
    thread_ids: Vec<i64>,
    #[serde(default)]
    message_ids: Vec<i64>,
    /// For `move`: the account whose folder is the target.
    account_id: Option<i64>,
    folder: Option<String>,
}

#[derive(sqlx::FromRow)]
struct Target {
    id: i64,
    account_id: i64,
    folder_name: String,
    uid: Option<u32>,
    is_outgoing: bool,
}

pub async fn apply(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(input): Json<BulkInput>,
) -> ApiResult<Json<Value>> {
    if input.thread_ids.len() + input.message_ids.len() > MAX_SELECTION {
        return Err(ApiError::bad_request("too many mails selected at once"));
    }
    // A message can exist twice (a copy in Sent and one in the inbox); selecting one means both.
    let targets: Vec<Target> = sqlx::query_as(
        "SELECT m.id, m.account_id, f.name AS folder_name, m.uid, m.is_outgoing
         FROM messages m JOIN folders f ON f.id = m.folder_id
         WHERE m.user_id = ?1
           AND (m.thread_id IN (SELECT value FROM json_each(?2))
                OR (m.account_id, m.message_id) IN (
                    SELECT account_id, message_id FROM messages
                    WHERE user_id = ?1 AND id IN (SELECT value FROM json_each(?3))))",
    )
    .bind(user.id)
    .bind(serde_json::to_string(&input.thread_ids).map_err(anyhow::Error::from)?)
    .bind(serde_json::to_string(&input.message_ids).map_err(anyhow::Error::from)?)
    .fetch_all(&state.db)
    .await?;
    if targets.is_empty() {
        return Ok(Json(json!({ "affected": 0 })));
    }

    let affected = match input.action.as_str() {
        "read" => set_seen(&state, &targets, true).await?,
        "unread" => set_seen(&state, &targets, false).await?,
        "trash" => relocate(&state, user.id, targets, None).await?,
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
