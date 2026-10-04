//! Groups of recipients: a name that stands for several addresses. The composer offers a group
//! by its name and puts its addresses in the recipient field, so a mail to a group is an
//! ordinary mail to each of its members.

use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    auth::CurrentUser,
    error::{ApiError, ApiResult},
    mail::smtp::parse_recipients,
    state::AppState,
};

const MAX_NAME: usize = 40;
const MAX_MEMBERS: usize = 100;
const MAX_GROUPS: i64 = 50;

#[derive(Serialize, sqlx::FromRow)]
pub struct Group {
    id: i64,
    name: String,
    /// The addresses, separated by ", ".
    members: String,
}

pub async fn list(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Vec<Group>>> {
    let groups = sqlx::query_as("SELECT id, name, members FROM recipient_groups WHERE user_id = ? ORDER BY name, id")
        .bind(user.id)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(groups))
}

#[derive(Deserialize)]
pub struct GroupInput {
    name: String,
    /// Addresses separated by commas, semicolons or line breaks; "Name <address>" is taken too.
    members: String,
}

/// Name and members as they are stored: the bare addresses, each once, in the order given.
fn checked(input: &GroupInput) -> ApiResult<(String, String)> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::bad_request("give the group a name"));
    }
    if name.chars().count() > MAX_NAME {
        return Err(ApiError::bad_request(format!(
            "the name can be at most {MAX_NAME} characters"
        )));
    }
    // A comma would end the name in the recipient field, an @ would make it look like an address.
    if name.contains([',', ';', '@', '<', '>']) {
        return Err(ApiError::bad_request("the name cannot contain , ; @ < or >"));
    }
    let parsed = parse_recipients(&input.members.replace(['\n', '\r'], ","))
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let mut members: Vec<String> = Vec::new();
    for mailbox in parsed {
        let address = mailbox.email.to_string();
        if !members.iter().any(|known| known.eq_ignore_ascii_case(&address)) {
            members.push(address);
        }
    }
    if members.is_empty() {
        return Err(ApiError::bad_request("a group needs at least one address"));
    }
    if members.len() > MAX_MEMBERS {
        return Err(ApiError::bad_request(format!(
            "a group can have at most {MAX_MEMBERS} addresses"
        )));
    }
    Ok((name, members.join(", ")))
}

/// "A group with this name exists" instead of the database's own words.
fn named(error: sqlx::Error) -> ApiError {
    match &error {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            ApiError::bad_request("a group with this name exists already")
        }
        _ => error.into(),
    }
}

pub async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(input): Json<GroupInput>,
) -> ApiResult<Json<Group>> {
    let (name, members) = checked(&input)?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM recipient_groups WHERE user_id = ?")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    if count >= MAX_GROUPS {
        return Err(ApiError::bad_request(format!(
            "at most {MAX_GROUPS} groups can be set up"
        )));
    }
    let id = sqlx::query("INSERT INTO recipient_groups (user_id, name, members) VALUES (?, ?, ?)")
        .bind(user.id)
        .bind(&name)
        .bind(&members)
        .execute(&state.db)
        .await
        .map_err(named)?
        .last_insert_rowid();
    Ok(Json(Group { id, name, members }))
}

pub async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(input): Json<GroupInput>,
) -> ApiResult<Json<Group>> {
    let (name, members) = checked(&input)?;
    let changed = sqlx::query("UPDATE recipient_groups SET name = ?, members = ? WHERE id = ? AND user_id = ?")
        .bind(&name)
        .bind(&members)
        .bind(id)
        .bind(user.id)
        .execute(&state.db)
        .await
        .map_err(named)?
        .rows_affected();
    if changed == 0 {
        return Err(ApiError::not_found());
    }
    Ok(Json(Group { id, name, members }))
}

pub async fn delete(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let gone = sqlx::query("DELETE FROM recipient_groups WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user.id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if gone == 0 {
        return Err(ApiError::not_found());
    }
    Ok(Json(json!({ "ok": true })))
}
