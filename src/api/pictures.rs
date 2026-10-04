//! Pictures the user chooses for a sender: uploaded, or taken from a web address.

use axum::{
    Json,
    extract::{Multipart, Path, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    auth::CurrentUser,
    error::{ApiError, ApiResult},
    mail::preview,
    state::{AppState, now},
};

/// Largest picture accepted, as uploaded or as downloaded. It is shrunk before it is stored.
pub const MAX_PICTURE_BYTES: usize = 5 * 1024 * 1024;

async fn own_sender(state: &AppState, user_id: i64, sender_id: i64) -> ApiResult<()> {
    let found: Option<i64> = sqlx::query_scalar("SELECT id FROM senders WHERE id = ? AND user_id = ?")
        .bind(sender_id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?;
    found.map(|_| ()).ok_or_else(ApiError::not_found)
}

/// Decodes whatever image came in and keeps a small JPEG of it. Nothing of the original bytes
/// is stored or served, so a file that only pretends to be a picture gets nowhere.
async fn keep(state: &AppState, user_id: i64, sender_id: i64, bytes: Vec<u8>, source: &str) -> ApiResult<Json<Value>> {
    let small = tokio::task::spawn_blocking(move || preview::thumbnail(&bytes))
        .await
        .map_err(anyhow::Error::from)?
        .map_err(|_| {
            ApiError::bad_request("This is not a picture Portier can read. Use a JPEG, PNG, GIF or WebP image.")
        })?;
    sqlx::query(
        "INSERT INTO sender_pictures (sender_id, data, source, updated_at) VALUES (?, ?, ?, ?)
         ON CONFLICT (sender_id) DO UPDATE SET data = excluded.data, source = excluded.source,
             updated_at = excluded.updated_at",
    )
    .bind(sender_id)
    .bind(small)
    .bind(source)
    .bind(now())
    .execute(&state.db)
    .await?;
    state.notify(user_id, "senders");
    Ok(Json(json!({ "ok": true })))
}

pub async fn upload(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(sender_id): Path<i64>,
    mut multipart: Multipart,
) -> ApiResult<Json<Value>> {
    own_sender(&state, user.id, sender_id).await?;
    let field = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request(format!("upload failed: {e}")))?
        .ok_or_else(|| ApiError::bad_request("choose a picture file"))?;
    let bytes = field
        .bytes()
        .await
        .map_err(|_| ApiError::bad_request("The picture is too large. It may have up to 5 MB."))?;
    keep(&state, user.id, sender_id, bytes.to_vec(), "upload").await
}

#[derive(Deserialize)]
pub struct FromUrl {
    url: String,
}

pub async fn from_url(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(sender_id): Path<i64>,
    Json(input): Json<FromUrl>,
) -> ApiResult<Json<Value>> {
    own_sender(&state, user.id, sender_id).await?;
    let url = input.url.trim().to_string();
    if !url.starts_with("https://") {
        return Err(ApiError::bad_request(
            "Enter the address of a picture, starting with https://",
        ));
    }
    let bytes = crate::avatar::fetch_picture(&url, MAX_PICTURE_BYTES)
        .await
        .map_err(|e| ApiError::bad_request(format!("The picture could not be loaded from that address: {e}.")))?;
    keep(&state, user.id, sender_id, bytes, &url).await
}

pub async fn remove(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(sender_id): Path<i64>,
) -> ApiResult<Json<Value>> {
    own_sender(&state, user.id, sender_id).await?;
    sqlx::query("DELETE FROM sender_pictures WHERE sender_id = ?")
        .bind(sender_id)
        .execute(&state.db)
        .await?;
    state.notify(user.id, "senders");
    Ok(Json(json!({ "ok": true })))
}
