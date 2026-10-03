//! Sender decisions and their effect on the mail server.

use anyhow::Result;

use crate::{
    error::{ApiError, ApiResult},
    mail::{store, sync},
    models::{Account, CATEGORIES},
    state::{AppState, now},
};

const RESTORE_WINDOW_SECONDS: i64 = 90 * 24 * 3600;

/// Sets (or clears, with `None`) the category of a sender. Junk moves the sender's inbox mail to
/// the server's Junk folder; leaving junk brings back the last 90 days.
pub async fn set_category(state: &AppState, user_id: i64, sender_id: i64, category: Option<&str>) -> ApiResult<()> {
    if category.is_some_and(|c| !CATEGORIES.contains(&c)) {
        return Err(ApiError::bad_request("unknown category"));
    }
    let old: Option<String> = sqlx::query_scalar("SELECT category FROM senders WHERE id = ? AND user_id = ?")
        .bind(sender_id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(ApiError::not_found)?;

    sqlx::query("UPDATE senders SET category = ?, decided_at = ? WHERE id = ?")
        .bind(category)
        .bind(category.map(|_| now()))
        .bind(sender_id)
        .execute(&state.db)
        .await?;

    let was_junk = old.as_deref() == Some("junk");
    let is_junk = category == Some("junk");
    if is_junk && !was_junk {
        move_sender_mail(state, user_id, sender_id, "inbox", "junk", 0).await?;
    } else if was_junk && !is_junk {
        move_sender_mail(
            state,
            user_id,
            sender_id,
            "junk",
            "inbox",
            now() - RESTORE_WINDOW_SECONDS,
        )
        .await?;
    }
    state.notify(user_id, "senders");
    Ok(())
}

async fn move_sender_mail(
    state: &AppState,
    user_id: i64,
    sender_id: i64,
    from_role: &'static str,
    to_role: &'static str,
    since: i64,
) -> Result<()> {
    let accounts: Vec<Account> = sqlx::query_as("SELECT * FROM accounts WHERE user_id = ?")
        .bind(user_id)
        .fetch_all(&state.db)
        .await?;
    for account in accounts {
        let name_of = |role: &str| match role {
            "junk" => account.junk_folder.clone(),
            _ => account.inbox_folder.clone(),
        };
        let (from_name, to_name) = (name_of(from_role), name_of(to_role));
        if from_name.is_empty() || to_name.is_empty() {
            continue; // no Junk folder on this server
        }
        let (Some(from), Some(to)) = (
            store::folder_by_name(&state.db, account.id, &from_name).await?,
            store::folder_by_name(&state.db, account.id, &to_name).await?,
        ) else {
            continue;
        };
        let uids: Vec<u32> = sqlx::query_scalar(
            "SELECT uid FROM messages WHERE folder_id = ? AND sender_id = ? AND uid IS NOT NULL AND date >= ?",
        )
        .bind(from.id)
        .bind(sender_id)
        .bind(since)
        .fetch_all(&state.db)
        .await?;
        if uids.is_empty() {
            continue;
        }
        let task_state = state.clone();
        sync::spawn_action(state, account.id, "moving sender mail", move |mut session| async move {
            session.select(&from.name).await?;
            sync::move_messages(&task_state, &mut session, &from, &uids, &to).await?;
            let _ = session.logout().await;
            Ok(())
        });
    }
    Ok(())
}
