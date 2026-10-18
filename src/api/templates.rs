//! Templates: texts kept for mail written again and again. The composer puts one into the text
//! of a draft, filling in `{first name}`, `{name}` and `{date}` as it does.

use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    auth::CurrentUser,
    error::{ApiError, ApiResult},
    mail::sanitize,
    state::AppState,
};

pub const MAX_NAME: usize = 40;
pub const MAX_BODY: usize = 100_000;
const MAX_TEMPLATES: i64 = 100;

#[derive(Serialize, sqlx::FromRow)]
pub struct Template {
    id: i64,
    name: String,
    body_html: String,
}

pub async fn list(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Vec<Template>>> {
    let templates =
        sqlx::query_as("SELECT id, name, body_html FROM mail_templates WHERE user_id = ? ORDER BY name, id")
            .bind(user.id)
            .fetch_all(&state.db)
            .await?;
    Ok(Json(templates))
}

#[derive(Deserialize)]
pub struct TemplateInput {
    name: String,
    body_html: String,
}

/// Name and text as they are stored: the name trimmed, the text cleaned like received mail.
pub fn checked(name: &str, body_html: &str) -> Result<(String, String), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("give the template a name".into());
    }
    if name.chars().count() > MAX_NAME {
        return Err(format!("the name can be at most {MAX_NAME} characters"));
    }
    if body_html.len() > MAX_BODY {
        return Err("the text of a template can be at most 100 KB".into());
    }
    let body = sanitize::clean_html(body_html);
    if sanitize::html_to_text(&body).trim().is_empty() {
        return Err("a template needs some text".into());
    }
    Ok((name, body))
}

/// "A template with this name exists" instead of the database's own words.
fn named(error: sqlx::Error) -> ApiError {
    match &error {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            ApiError::bad_request("a template with this name exists already")
        }
        _ => error.into(),
    }
}

pub async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(input): Json<TemplateInput>,
) -> ApiResult<Json<Template>> {
    let (name, body_html) = checked(&input.name, &input.body_html).map_err(ApiError::bad_request)?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mail_templates WHERE user_id = ?")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    if count >= MAX_TEMPLATES {
        return Err(ApiError::bad_request(format!(
            "at most {MAX_TEMPLATES} templates can be kept"
        )));
    }
    let id = sqlx::query("INSERT INTO mail_templates (user_id, name, body_html) VALUES (?, ?, ?)")
        .bind(user.id)
        .bind(&name)
        .bind(&body_html)
        .execute(&state.db)
        .await
        .map_err(named)?
        .last_insert_rowid();
    Ok(Json(Template { id, name, body_html }))
}

pub async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(input): Json<TemplateInput>,
) -> ApiResult<Json<Template>> {
    let (name, body_html) = checked(&input.name, &input.body_html).map_err(ApiError::bad_request)?;
    let changed = sqlx::query("UPDATE mail_templates SET name = ?, body_html = ? WHERE id = ? AND user_id = ?")
        .bind(&name)
        .bind(&body_html)
        .bind(id)
        .bind(user.id)
        .execute(&state.db)
        .await
        .map_err(named)?
        .rows_affected();
    if changed == 0 {
        return Err(ApiError::not_found());
    }
    Ok(Json(Template { id, name, body_html }))
}

pub async fn delete(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let gone = sqlx::query("DELETE FROM mail_templates WHERE id = ? AND user_id = ?")
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
