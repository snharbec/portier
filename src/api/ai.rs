//! Summaries of new mail: the settings of the local model, and the briefing shown above the
//! unseen mail in Home.

use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    api::mail::{DELAYED, FLAGGED},
    auth::{AdminUser, CurrentUser},
    error::{ApiError, ApiResult},
    mail::summary,
    state::AppState,
};

/// What every user may know: whether there are summaries. The administrator also gets the
/// settings themselves.
pub async fn settings(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Value>> {
    let settings = summary::settings(&state).await?;
    let mut answer = json!({ "on": settings.on(), "reads_pdf": summary::reads_pdf() });
    if user.is_admin {
        answer["url"] = json!(settings.url);
        answer["model"] = json!(settings.model);
        answer["language"] = json!(settings.language);
    }
    Ok(Json(answer))
}

#[derive(Deserialize)]
pub struct Address {
    url: String,
}

/// An address the server may call: http or https, nothing else.
fn checked(url: &str) -> ApiResult<String> {
    let url = url.trim().trim_end_matches('/');
    let parsed =
        reqwest::Url::parse(url).map_err(|_| ApiError::bad_request("enter an address like http://127.0.0.1:11434"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(ApiError::bad_request("enter an address like http://127.0.0.1:11434"));
    }
    Ok(url.to_string())
}

/// The models the Ollama at an address offers; also the test that the address works.
pub async fn models(AdminUser(_): AdminUser, Json(input): Json<Address>) -> ApiResult<Json<Value>> {
    let url = checked(&input.url)?;
    let models = summary::models(&url)
        .await
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    Ok(Json(json!({ "models": models })))
}

#[derive(Deserialize)]
pub struct Update {
    url: String,
    model: String,
    language: String,
}

pub async fn update(
    State(state): State<AppState>,
    AdminUser(_): AdminUser,
    Json(input): Json<Update>,
) -> ApiResult<Json<Value>> {
    // An empty address turns summaries off.
    let url = if input.url.trim().is_empty() {
        String::new()
    } else {
        checked(&input.url)?
    };
    let model = input.model.trim();
    let language = input.language.trim();
    if !url.is_empty() && model.is_empty() {
        return Err(ApiError::bad_request("choose a model"));
    }
    if language.is_empty()
        || language.chars().count() > 40
        || !language.chars().all(|c| c.is_alphabetic() || c == ' ' || c == '-')
    {
        return Err(ApiError::bad_request(
            "name the language in a word or two, e.g. English or Deutsch",
        ));
    }
    sqlx::query("UPDATE ai_settings SET url = ?, model = ?, language = ? WHERE id = 1")
        .bind(&url)
        .bind(model)
        .bind(language)
        .execute(&state.db)
        .await?;
    // Mail the old settings failed on gets another try.
    sqlx::query("DELETE FROM summaries WHERE error IS NOT NULL")
        .execute(&state.db)
        .await?;
    state.summarize.notify_one();
    Ok(Json(json!({ "ok": true })))
}

#[derive(sqlx::FromRow)]
struct Row {
    message_id: i64,
    thread_id: i64,
    from_name: String,
    from_addr: String,
    subject: String,
    date: i64,
    text: Option<String>,
    attachments: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct Entry {
    message_id: i64,
    thread_id: i64,
    from_name: String,
    from_addr: String,
    subject: String,
    date: i64,
    /// What the mail says; empty while the model has not got to it yet.
    summary: String,
    /// One line per attachment the model could read.
    attachments: Value,
    /// The summary is still being written.
    pending: bool,
}

/// The summary of a conversation, as a page of its own shows it: the answer about its newest mail
/// that has one, or `{"on": false}` when summaries are off, or `{"found": false}` when the model
/// never got to this conversation. Kept apart from the conversation itself so that the summary,
/// which takes the model time to write, does not hold up the mail.
pub async fn thread_summary(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    // Only for a conversation of this user's.
    let owned: Option<i64> = sqlx::query_scalar("SELECT id FROM threads WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?;
    if owned.is_none() {
        return Err(ApiError::not_found());
    }
    let Some(by_thread) = summary::summaries_for_threads(&state, &[id]).await? else {
        return Ok(Json(json!({ "on": false, "found": false })));
    };
    Ok(match by_thread.get(&id) {
        Some(entry) => {
            let mut entry = entry.clone();
            entry["found"] = json!(true);
            entry["on"] = json!(true);
            Json(entry)
        }
        None => Json(json!({ "on": true, "found": false })),
    })
}

/// The unseen mail of Home, newest first, each with its summary: what the Unseen area lists,
/// told in a sentence or two.
pub async fn briefing(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Value>> {
    let settings = summary::settings(&state).await?;
    if !settings.on() {
        return Ok(Json(json!({ "on": false, "entries": [] })));
    }
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT m.id AS message_id, m.thread_id, m.from_name, m.from_addr, m.subject, m.date,
                x.text, x.attachments, x.error
         FROM messages m
         JOIN folders f ON f.id = m.folder_id AND f.role = 'inbox'
         JOIN threads t ON t.id = m.thread_id
         LEFT JOIN senders s ON s.id = t.sender_id
         LEFT JOIN summaries x ON x.message_id = m.id
         WHERE m.user_id = ? AND m.seen = 0 AND m.is_outgoing = 0
           AND (t.sender_id IS NULL OR s.category = 'important')
           AND NOT {FLAGGED} AND NOT {DELAYED}
         ORDER BY m.date DESC, m.id DESC LIMIT 50"
    )))
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    let fresh = crate::state::now() - summary::FRESH_SECONDS;
    let entries: Vec<Entry> = rows
        .into_iter()
        // A mail the model could not summarize is simply not in the briefing; it is in the list.
        // Neither is unseen mail from before the summaries: only fresh mail gets one.
        .filter(|row| row.error.is_none() && (row.text.is_some() || row.date >= fresh))
        .map(|row| Entry {
            message_id: row.message_id,
            thread_id: row.thread_id,
            from_name: row.from_name,
            from_addr: row.from_addr,
            subject: row.subject,
            date: row.date,
            pending: row.text.is_none(),
            summary: row.text.unwrap_or_default(),
            attachments: row
                .attachments
                .and_then(|list| serde_json::from_str(&list).ok())
                .unwrap_or_else(|| json!([])),
        })
        .collect();
    Ok(Json(json!({ "on": true, "entries": entries })))
}
