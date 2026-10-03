//! The Attachments page: recent attachments and their previews.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    api::mail::load_part,
    auth::CurrentUser,
    error::{ApiError, ApiResult},
    mail::preview::{self, Kind},
    state::{AppState, now},
};

#[derive(Deserialize)]
pub struct Range {
    weeks: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct Row {
    message_id: i64,
    idx: i64,
    filename: String,
    mime: String,
    size: i64,
    date: i64,
    thread_id: i64,
    subject: String,
    from_name: String,
    from_addr: String,
}

#[derive(Serialize)]
struct FileEntry {
    message_id: i64,
    idx: i64,
    filename: String,
    size: i64,
    kind: &'static str,
    date: i64,
    thread_id: i64,
    subject: String,
    from_name: String,
    from_addr: String,
}

/// Attachments received in the last weeks from senders the user let in.
pub async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(range): Query<Range>,
) -> ApiResult<Json<Value>> {
    let weeks = range.weeks.unwrap_or(4).clamp(1, 52);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT m.id AS message_id, a.idx, a.filename, a.mime, a.size, m.date, m.thread_id, m.subject,
                m.from_name, m.from_addr
         FROM attachments a
         JOIN messages m ON m.id = a.message_id
         JOIN senders s ON s.id = m.sender_id
         WHERE m.user_id = ?1 AND m.is_outgoing = 0 AND m.date >= ?2
           AND s.category IN ('important', 'feed')
           AND (a.inline = 0 OR a.content_id IS NULL)
           AND m.id IN (SELECT MIN(id) FROM messages WHERE user_id = ?1 GROUP BY message_id)
         ORDER BY m.date DESC, m.id DESC, a.idx
         LIMIT 500",
    )
    .bind(user.id)
    .bind(now() - weeks * 7 * 24 * 3600)
    .fetch_all(&state.db)
    .await?;

    let files: Vec<FileEntry> = rows
        .into_iter()
        .map(|r| FileEntry {
            kind: preview::kind(&r.mime, &r.filename).as_str(),
            message_id: r.message_id,
            idx: r.idx,
            filename: r.filename,
            size: r.size,
            date: r.date,
            thread_id: r.thread_id,
            subject: r.subject,
            from_name: r.from_name,
            from_addr: r.from_addr,
        })
        .collect();
    Ok(Json(json!({
        "office_previews": state.config.soffice.is_some(),
        "files": files,
    })))
}

fn no_preview(reason: &str) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, reason.to_string())
}

fn headers(content_type: &'static str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, max-age=86400"));
    headers
}

/// Small JPEG rendition of an image attachment, made once and kept next to the message.
pub async fn thumb(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, idx)): Path<(i64, i64)>,
) -> ApiResult<(HeaderMap, Vec<u8>)> {
    let part = load_part(&state, user.id, id, idx).await?;
    if preview::kind(&part.mime, &part.filename) != Kind::Image {
        return Err(no_preview("this attachment is not an image"));
    }
    let dir = state.preview_dir(part.account_id, id);
    let cached = dir.join(format!("{idx}.jpg"));
    if let Ok(bytes) = tokio::fs::read(&cached).await {
        return Ok((headers("image/jpeg"), bytes));
    }
    let bytes = tokio::task::spawn_blocking(move || preview::thumbnail(&part.data))
        .await
        .map_err(anyhow::Error::from)?
        .map_err(|e| no_preview(&format!("no preview: {e}")))?;
    tokio::fs::create_dir_all(&dir).await?;
    tokio::fs::write(&cached, &bytes).await?;
    Ok((headers("image/jpeg"), bytes))
}

/// The image itself, for the viewer. Only raster formats, recognised by content, are shown.
pub async fn view(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, idx)): Path<(i64, i64)>,
) -> ApiResult<(HeaderMap, Vec<u8>)> {
    let part = load_part(&state, user.id, id, idx).await?;
    let mime = preview::raster_mime(&part.data).ok_or_else(|| no_preview("this attachment is not an image"))?;
    Ok((headers(mime), part.data))
}

/// PDF rendition of an Office document. Delivered as opaque bytes: the page draws it with pdf.js.
pub async fn pdf(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, idx)): Path<(i64, i64)>,
) -> ApiResult<(HeaderMap, Vec<u8>)> {
    let part = load_part(&state, user.id, id, idx).await?;
    if preview::kind(&part.mime, &part.filename) != Kind::Office {
        return Err(no_preview("this attachment is not an Office document"));
    }
    let Some(soffice) = state.config.soffice.clone() else {
        return Err(no_preview("LibreOffice is not installed on the server"));
    };
    let dir = state.preview_dir(part.account_id, id);
    let cached = dir.join(format!("{idx}.pdf"));
    if let Ok(bytes) = tokio::fs::read(&cached).await {
        return Ok((headers("application/octet-stream"), bytes));
    }

    // LibreOffice is heavy; convert one document at a time.
    let _permit = state.convert.acquire().await.map_err(anyhow::Error::from)?;
    if let Ok(bytes) = tokio::fs::read(&cached).await {
        return Ok((headers("application/octet-stream"), bytes));
    }
    let bytes = preview::office_to_pdf(&soffice, &state.config.data_dir, &part.data, &part.filename)
        .await
        .map_err(|e| {
            tracing::warn!(message = id, "Office preview failed: {e:#}");
            no_preview("this document could not be converted for preview")
        })?;
    tokio::fs::create_dir_all(&dir).await?;
    tokio::fs::write(&cached, &bytes).await?;
    Ok((headers("application/octet-stream"), bytes))
}
