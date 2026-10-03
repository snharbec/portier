//! Searches a user saved under a name; the side bar lists them with their unread count.

use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    api::mail::{SearchOutput, search_sql},
    auth::CurrentUser,
    error::{ApiError, ApiResult},
    state::AppState,
};

const MAX_NAME: usize = 40;
const MAX_SAVED: i64 = 30;

#[derive(Serialize)]
pub struct SavedSearch {
    id: i64,
    name: String,
    query: String,
    /// Unread received mails the search finds right now.
    unread: i64,
}

/// A saved search is run afresh each time, so "last month" keeps meaning the previous month.
async fn unread(state: &AppState, user_id: i64, query: &str) -> i64 {
    let today = chrono::Local::now().date_naive();
    let Ok(parsed) = crate::search::parse(query, today) else {
        return 0;
    };
    if parsed.is_empty() {
        return 0;
    }
    search_sql(&parsed, user_id, SearchOutput::UnreadCount)
        .build_query_scalar()
        .fetch_one(&state.db)
        .await
        .unwrap_or(0)
}

pub async fn list(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Vec<SavedSearch>>> {
    let rows: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT id, name, query FROM saved_searches WHERE user_id = ? ORDER BY name COLLATE NOCASE, id")
            .bind(user.id)
            .fetch_all(&state.db)
            .await?;
    let mut searches = Vec::with_capacity(rows.len());
    for (id, name, query) in rows {
        searches.push(SavedSearch {
            unread: unread(&state, user.id, &query).await,
            id,
            name,
            query,
        });
    }
    Ok(Json(searches))
}

#[derive(Deserialize)]
pub struct SavedInput {
    name: String,
    query: String,
}

/// Name and query as they are stored; a query that cannot be run is refused with the reason.
fn checked(input: &SavedInput) -> ApiResult<(String, String)> {
    let name = input.name.trim().to_string();
    let query = input.query.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::bad_request("give the search a name"));
    }
    if name.chars().count() > MAX_NAME {
        return Err(ApiError::bad_request(format!(
            "the name can be at most {MAX_NAME} characters"
        )));
    }
    let today = chrono::Local::now().date_naive();
    let parsed = crate::search::parse(&query, today).map_err(ApiError::bad_request)?;
    if parsed.is_empty() {
        return Err(ApiError::bad_request("there is nothing to search for yet"));
    }
    Ok((name, query))
}

pub async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(input): Json<SavedInput>,
) -> ApiResult<Json<Value>> {
    let (name, query) = checked(&input)?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM saved_searches WHERE user_id = ?")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    if count >= MAX_SAVED {
        return Err(ApiError::bad_request(format!(
            "{MAX_SAVED} searches are saved already; remove one first"
        )));
    }
    let id: i64 = sqlx::query_scalar("INSERT INTO saved_searches (user_id, name, query) VALUES (?, ?, ?) RETURNING id")
        .bind(user.id)
        .bind(name)
        .bind(query)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(json!({ "id": id })))
}

pub async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(input): Json<SavedInput>,
) -> ApiResult<Json<Value>> {
    let (name, query) = checked(&input)?;
    let result = sqlx::query("UPDATE saved_searches SET name = ?, query = ? WHERE id = ? AND user_id = ?")
        .bind(name)
        .bind(query)
        .bind(id)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    Ok(Json(json!({ "ok": true })))
}

pub async fn delete(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let result = sqlx::query("DELETE FROM saved_searches WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    Ok(Json(json!({ "ok": true })))
}
