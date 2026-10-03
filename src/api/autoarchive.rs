//! Automatic archive: read conversations of the Inbox ("Seen") whose last mail is older than a
//! number of weeks the user chose move to the Archive folder. Unread, Important and delayed
//! conversations, and the other lists, are never touched.

use std::time::Duration;

use axum::{
    Json,
    extract::{Query, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{bulk, mail},
    auth::CurrentUser,
    error::{ApiError, ApiResult},
    state::{AppState, now},
};

/// Upper bound for the setting: ten years.
pub const MAX_WEEKS: i64 = 520;
/// Conversations moved per user and run; a large backlog is worked off over several runs.
const BATCH: i64 = 300;
const FIRST_RUN_AFTER: Duration = Duration::from_secs(120);
const EVERY: Duration = Duration::from_secs(3600);

fn cutoff(weeks: i64) -> i64 {
    now() - weeks * 7 * 24 * 3600
}

/// Archives what is due for one user. Returns how many mails were moved.
pub async fn run_for_user(state: &AppState, user_id: i64, weeks: i64) -> ApiResult<usize> {
    if weeks <= 0 {
        return Ok(0);
    }
    let due = mail::seen_inbox_older_than(state, user_id, cutoff(weeks), BATCH).await?;
    if due.is_empty() {
        return Ok(0);
    }
    let moved = bulk::archive_conversations(state, user_id, &due).await?;
    if moved > 0 {
        tracing::info!(user_id, moved, "archived automatically");
        state.notify(user_id, "mail");
    }
    Ok(moved)
}

async fn run_all(state: &AppState) -> anyhow::Result<()> {
    let users: Vec<(i64, i64)> =
        sqlx::query_as("SELECT id, auto_archive_weeks FROM users WHERE auto_archive_weeks > 0")
            .fetch_all(&state.db)
            .await?;
    for (user_id, weeks) in users {
        // One user's problem (say, no Archive folder) must not stop the others.
        if let Err(e) = run_for_user(state, user_id, weeks).await {
            tracing::warn!(user_id, "automatic archive failed: {}", e.1);
        }
    }
    Ok(())
}

/// Runs shortly after start, giving the first sync time to finish, then once an hour.
pub async fn run(state: AppState) {
    tokio::time::sleep(FIRST_RUN_AFTER).await;
    loop {
        if let Err(e) = run_all(&state).await {
            tracing::warn!("automatic archive failed: {e:#}");
        }
        tokio::time::sleep(EVERY).await;
    }
}

#[derive(Deserialize)]
pub struct Weeks {
    weeks: i64,
}

fn checked(weeks: i64) -> ApiResult<i64> {
    if (0..=MAX_WEEKS).contains(&weeks) {
        Ok(weeks)
    } else {
        Err(ApiError::bad_request(format!(
            "choose between 1 and {MAX_WEEKS} weeks, or turn it off"
        )))
    }
}

/// How many conversations a setting would archive right now, so it can be shown before saving.
pub async fn preview(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<Weeks>,
) -> ApiResult<Json<Value>> {
    let weeks = checked(q.weeks)?;
    let count = if weeks == 0 {
        0
    } else {
        mail::seen_inbox_older_than(&state, user.id, cutoff(weeks), i64::MAX)
            .await?
            .len()
    };
    Ok(Json(json!({ "count": count })))
}

/// Saves the age and, when it is on, archives what is due straight away.
pub async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(input): Json<Weeks>,
) -> ApiResult<Json<Value>> {
    let weeks = checked(input.weeks)?;
    sqlx::query("UPDATE users SET auto_archive_weeks = ? WHERE id = ?")
        .bind(weeks)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    let archived = run_for_user(&state, user.id, weeks).await?;
    Ok(Json(json!({ "auto_archive_weeks": weeks, "archived": archived })))
}
