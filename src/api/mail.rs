use std::{collections::HashMap, convert::Infallible};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{
        HeaderMap, HeaderValue,
        header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS},
    },
    response::sse::{Event as SseEvent, KeepAlive, Sse},
};
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_stream::wrappers::BroadcastStream;

use crate::{
    auth::CurrentUser,
    error::{ApiError, ApiResult},
    mail::{classify, imap, parse, preview, sync},
    models::Addr,
    state::AppState,
};

// ---- Screener and senders -------------------------------------------------------------------

#[derive(Serialize, sqlx::FromRow)]
pub struct ScreenerRow {
    id: i64,
    address: String,
    display_name: String,
    count: i64,
    message_id: i64,
    thread_id: i64,
    subject: String,
    snippet: String,
    date: i64,
}

/// Senders awaiting a decision, with their latest inbox message.
pub async fn screener(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Vec<ScreenerRow>>> {
    Ok(Json(
        sqlx::query_as(
            "SELECT s.id, s.address, s.display_name,
                    (SELECT COUNT(*) FROM messages m JOIN folders f ON f.id = m.folder_id
                      WHERE m.sender_id = s.id AND f.role = 'inbox') AS count,
                    lm.id AS message_id, lm.thread_id, lm.subject, lm.snippet, lm.date
             FROM senders s
             JOIN messages lm ON lm.id = (
                 SELECT m.id FROM messages m JOIN folders f ON f.id = m.folder_id
                 WHERE m.sender_id = s.id AND f.role = 'inbox' ORDER BY m.date DESC, m.id DESC LIMIT 1)
             WHERE s.user_id = ? AND s.category IS NULL
             ORDER BY lm.date DESC",
        )
        .bind(user.id)
        .fetch_all(&state.db)
        .await?,
    ))
}

#[derive(Deserialize)]
pub struct SenderQuery {
    category: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct SenderRow {
    id: i64,
    address: String,
    display_name: String,
    category: Option<String>,
    decided_at: Option<i64>,
    count: i64,
}

pub async fn senders(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<SenderQuery>,
) -> ApiResult<Json<Vec<SenderRow>>> {
    Ok(Json(
        sqlx::query_as(
            "SELECT s.id, s.address, s.display_name, s.category, s.decided_at,
                    (SELECT COUNT(*) FROM messages m WHERE m.sender_id = s.id) AS count
             FROM senders s
             WHERE s.user_id = ?1 AND s.category IS NOT NULL AND (?2 IS NULL OR s.category = ?2)
             ORDER BY s.decided_at DESC",
        )
        .bind(user.id)
        .bind(q.category)
        .fetch_all(&state.db)
        .await?,
    ))
}

#[derive(Deserialize)]
pub struct ImagesInput {
    show: bool,
}

/// Remembers whether remote images in this sender's mail are loaded without asking.
pub async fn set_images(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(input): Json<ImagesInput>,
) -> ApiResult<Json<Value>> {
    let result = sqlx::query("UPDATE senders SET show_images = ? WHERE id = ? AND user_id = ?")
        .bind(input.show)
        .bind(id)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct CategoryInput {
    category: Option<String>,
}

pub async fn set_category(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(input): Json<CategoryInput>,
) -> ApiResult<Json<Value>> {
    classify::set_category(&state, user.id, id, input.category.as_deref()).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ContactQuery {
    #[serde(default)]
    q: String,
}

/// Recipient suggestions for the composer.
pub async fn contacts(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<ContactQuery>,
) -> ApiResult<Json<Vec<Addr>>> {
    let pattern = format!("%{}%", q.q.trim().replace(['%', '_'], ""));
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT display_name, address FROM senders
         WHERE user_id = ? AND (category IS NULL OR category != 'junk')
           AND (address LIKE ? OR display_name LIKE ?)
         ORDER BY category = 'important' DESC, display_name LIMIT 8",
    )
    .bind(user.id)
    .bind(&pattern)
    .bind(&pattern)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter().map(|(name, address)| Addr { name, address }).collect(),
    ))
}

// ---- Thread lists ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct BoxQuery {
    #[serde(rename = "box")]
    mailbox: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct ThreadRow {
    id: i64,
    subject: String,
    count: i64,
    unread: i64,
    date: i64,
    snippet: String,
    from_name: String,
    from_addr: String,
    is_outgoing: bool,
    account_id: i64,
    has_attachments: bool,
    sender_name: Option<String>,
    sender_address: Option<String>,
}

pub async fn threads(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<BoxQuery>,
) -> ApiResult<Json<Vec<ThreadRow>>> {
    let condition = match q.mailbox.as_str() {
        // Threads the user started count as important once somebody answers.
        "important" => {
            "(t.sender_id IS NULL OR s.category = 'important')
             AND EXISTS (SELECT 1 FROM messages m WHERE m.thread_id = t.id AND m.is_outgoing = 0)"
        }
        "feed" => "s.category = 'feed'",
        "junk" => "s.category = 'junk'",
        "sent" => "EXISTS (SELECT 1 FROM messages m WHERE m.thread_id = t.id AND m.is_outgoing = 1)",
        _ => return Err(ApiError::bad_request("unknown box")),
    };
    let sql = format!(
        "SELECT t.id, t.subject,
                (SELECT COUNT(DISTINCT m.message_id) FROM messages m WHERE m.thread_id = t.id) AS count,
                (SELECT COUNT(*) FROM messages m WHERE m.thread_id = t.id AND m.seen = 0 AND m.is_outgoing = 0) AS unread,
                lm.date, lm.snippet, lm.from_name, lm.from_addr, lm.is_outgoing, lm.account_id,
                EXISTS (SELECT 1 FROM messages m WHERE m.thread_id = t.id AND m.has_attachments = 1) AS has_attachments,
                s.display_name AS sender_name, s.address AS sender_address
         FROM threads t
         JOIN messages lm ON lm.id = (
             SELECT id FROM messages WHERE thread_id = t.id ORDER BY date DESC, id DESC LIMIT 1)
         LEFT JOIN senders s ON s.id = t.sender_id
         WHERE t.user_id = ? AND {condition}
         ORDER BY lm.date DESC LIMIT 300"
    );
    Ok(Json(
        sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(user.id)
            .fetch_all(&state.db)
            .await?,
    ))
}

pub async fn counts(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Value>> {
    let screener: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM senders s WHERE s.user_id = ? AND s.category IS NULL
         AND EXISTS (SELECT 1 FROM messages m JOIN folders f ON f.id = m.folder_id
                     WHERE m.sender_id = s.id AND f.role = 'inbox')",
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    let unread: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT t.id) FROM threads t
         LEFT JOIN senders s ON s.id = t.sender_id
         JOIN messages m ON m.thread_id = t.id AND m.seen = 0 AND m.is_outgoing = 0
         WHERE t.user_id = ? AND (t.sender_id IS NULL OR s.category = 'important')",
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    let drafts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM drafts WHERE user_id = ?")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(
        json!({ "screener": screener, "unread_important": unread, "drafts": drafts }),
    ))
}

// ---- Messages -------------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct MessageRow {
    id: i64,
    thread_id: i64,
    account_id: i64,
    sender_id: Option<i64>,
    sender_category: Option<String>,
    show_images: bool,
    from_name: String,
    from_addr: String,
    to_addrs: String,
    cc_addrs: String,
    subject: String,
    date: i64,
    seen: bool,
    is_outgoing: bool,
    body_text: String,
    body_html: String,
}

#[derive(Serialize)]
pub struct AttachmentView {
    idx: i64,
    filename: String,
    mime: String,
    size: i64,
    /// Which preview the file can get: image, pdf, office or other.
    kind: &'static str,
}

#[derive(Serialize)]
pub struct MessageView {
    id: i64,
    thread_id: i64,
    account_id: i64,
    sender_id: Option<i64>,
    sender_category: Option<String>,
    /// The user chose to always load remote images from this sender.
    show_images: bool,
    from: Addr,
    to: Vec<Addr>,
    cc: Vec<Addr>,
    subject: String,
    date: i64,
    seen: bool,
    is_outgoing: bool,
    body_text: String,
    body_html: String,
    attachments: Vec<AttachmentView>,
}

const MESSAGE_COLUMNS: &str = "m.id, m.thread_id, m.account_id, m.sender_id, s.category AS sender_category,
    COALESCE(s.show_images, 0) AS show_images,
    m.from_name, m.from_addr, m.to_addrs, m.cc_addrs, m.subject, m.date, m.seen, m.is_outgoing,
    m.body_text, m.body_html";

/// Adds attachments and points inline `cid:` images at the API.
async fn to_views(state: &AppState, rows: Vec<MessageRow>) -> ApiResult<Vec<MessageView>> {
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let attachment_rows: Vec<(i64, i64, String, String, i64)> = sqlx::query_as(
        "SELECT message_id, idx, filename, mime, size FROM attachments
         WHERE message_id IN (SELECT value FROM json_each(?)) AND (inline = 0 OR content_id IS NULL)
         ORDER BY idx",
    )
    .bind(serde_json::to_string(&ids).map_err(anyhow::Error::from)?)
    .fetch_all(&state.db)
    .await?;
    let mut by_message: HashMap<i64, Vec<AttachmentView>> = HashMap::new();
    for (message_id, idx, filename, mime, size) in attachment_rows {
        by_message.entry(message_id).or_default().push(AttachmentView {
            kind: preview::kind(&mime, &filename).as_str(),
            idx,
            filename,
            mime,
            size,
        });
    }

    Ok(rows
        .into_iter()
        .map(|r| MessageView {
            attachments: by_message.remove(&r.id).unwrap_or_default(),
            body_html: r
                .body_html
                .replace("src=\"cid:", &format!("src=\"/api/messages/{}/cid/", r.id)),
            id: r.id,
            thread_id: r.thread_id,
            account_id: r.account_id,
            sender_id: r.sender_id,
            sender_category: r.sender_category,
            show_images: r.show_images,
            from: Addr {
                name: r.from_name,
                address: r.from_addr,
            },
            to: serde_json::from_str(&r.to_addrs).unwrap_or_default(),
            cc: serde_json::from_str(&r.cc_addrs).unwrap_or_default(),
            subject: r.subject,
            date: r.date,
            seen: r.seen,
            is_outgoing: r.is_outgoing,
            body_text: r.body_text,
        })
        .collect())
}

pub async fn thread(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let head: (String, Option<i64>, Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT t.subject, s.id, s.address, s.display_name, s.category
         FROM threads t LEFT JOIN senders s ON s.id = t.sender_id
         WHERE t.id = ? AND t.user_id = ?",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(ApiError::not_found)?;

    // The same message can sit in two folders (mail to yourself); show it once.
    let rows: Vec<MessageRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {MESSAGE_COLUMNS} FROM messages m LEFT JOIN senders s ON s.id = m.sender_id
         WHERE m.id IN (SELECT MIN(id) FROM messages WHERE thread_id = ? GROUP BY message_id)
         ORDER BY m.date, m.id"
    )))
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(json!({
        "id": id,
        "subject": head.0,
        "sender": head.1.map(|sender_id| json!({
            "id": sender_id, "address": head.2, "display_name": head.3, "category": head.4,
        })),
        "messages": to_views(&state, rows).await?,
    })))
}

pub async fn message(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<MessageView>> {
    let rows: Vec<MessageRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {MESSAGE_COLUMNS} FROM messages m LEFT JOIN senders s ON s.id = m.sender_id
         WHERE m.id = ? AND m.user_id = ?"
    )))
    .bind(id)
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    to_views(&state, rows)
        .await?
        .pop()
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

#[derive(Deserialize)]
pub struct Page {
    /// important | feed | junk | sent
    #[serde(rename = "box", default = "default_box")]
    mailbox: String,
    #[serde(default)]
    offset: i64,
    limit: Option<i64>,
}

fn default_box() -> String {
    "feed".into()
}

/// The mails of a list as whole messages, newest first, for reading them on one page.
/// A mail belongs to the list its conversation is shown in.
pub async fn feed(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(page): Query<Page>,
) -> ApiResult<Json<Vec<MessageView>>> {
    let condition = match page.mailbox.as_str() {
        "important" => "m.is_outgoing = 0 AND (t.sender_id IS NULL OR ts.category = 'important')",
        "feed" => "m.is_outgoing = 0 AND ts.category = 'feed'",
        "junk" => "m.is_outgoing = 0 AND ts.category = 'junk'",
        "sent" => "m.is_outgoing = 1",
        _ => return Err(ApiError::bad_request("unknown box")),
    };
    let rows: Vec<MessageRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {MESSAGE_COLUMNS} FROM messages m
         JOIN threads t ON t.id = m.thread_id
         LEFT JOIN senders ts ON ts.id = t.sender_id
         LEFT JOIN senders s ON s.id = m.sender_id
         WHERE m.user_id = ?1 AND {condition}
           AND m.id IN (SELECT MIN(id) FROM messages WHERE user_id = ?1 GROUP BY account_id, message_id)
         ORDER BY m.date DESC, m.id DESC LIMIT ?2 OFFSET ?3"
    )))
    .bind(user.id)
    .bind(page.limit.unwrap_or(20).clamp(1, 200))
    .bind(page.offset.max(0))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(to_views(&state, rows).await?))
}

/// Marks every message of a thread as read, locally and on the server.
pub async fn mark_seen(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let unseen: Vec<(i64, String, Option<u32>)> = sqlx::query_as(
        "SELECT m.account_id, f.name, m.uid FROM messages m JOIN folders f ON f.id = m.folder_id
         WHERE m.thread_id = ? AND m.user_id = ? AND m.seen = 0",
    )
    .bind(id)
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    if unseen.is_empty() {
        return Ok(Json(json!({ "ok": true })));
    }
    sqlx::query("UPDATE messages SET seen = 1 WHERE thread_id = ? AND user_id = ?")
        .bind(id)
        .bind(user.id)
        .execute(&state.db)
        .await?;

    let mut by_folder: HashMap<(i64, String), Vec<u32>> = HashMap::new();
    for (account_id, folder, uid) in unseen {
        if let Some(uid) = uid {
            by_folder.entry((account_id, folder)).or_default().push(uid);
        }
    }
    for ((account_id, folder), uids) in by_folder {
        sync::spawn_action(&state, account_id, "marking read", move |mut session| async move {
            session.select(&folder).await?;
            imap::set_seen(&mut session, &uids, true).await?;
            let _ = session.logout().await;
            Ok(())
        });
    }
    state.notify(user.id, "mail");
    Ok(Json(json!({ "ok": true })))
}

// ---- Attachments ----------------------------------------------------------------------------

pub(crate) struct Part {
    pub account_id: i64,
    pub filename: String,
    pub mime: String,
    pub data: Vec<u8>,
}

/// One attachment of a message the user owns, read from the stored raw message.
pub(crate) async fn load_part(state: &AppState, user_id: i64, message_id: i64, idx: i64) -> ApiResult<Part> {
    let account_id: i64 = sqlx::query_scalar("SELECT account_id FROM messages WHERE id = ? AND user_id = ?")
        .bind(message_id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let raw = tokio::fs::read(state.raw_path(account_id, message_id))
        .await
        .map_err(|_| ApiError::not_found())?;
    let (filename, mime, data) = parse::attachment(&raw, idx as u32).ok_or_else(ApiError::not_found)?;
    Ok(Part {
        account_id,
        filename,
        mime,
        data,
    })
}

fn header(value: &str) -> HeaderValue {
    HeaderValue::from_str(value).unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"))
}

/// Always a download: attachments are untrusted and must never render on this origin.
pub async fn attachment(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, idx)): Path<(i64, i64)>,
) -> ApiResult<(HeaderMap, Vec<u8>)> {
    let Part { filename, data, .. } = load_part(&state, user.id, id, idx).await?;
    let safe_name: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._- ".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(
        CONTENT_DISPOSITION,
        header(&format!("attachment; filename=\"{safe_name}\"")),
    );
    Ok((headers, data))
}

/// Inline images referenced by `cid:` in the HTML body. Only raster images are served.
pub async fn inline_image(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, cid)): Path<(i64, String)>,
) -> ApiResult<(HeaderMap, Vec<u8>)> {
    let idx: i64 = sqlx::query_scalar(
        "SELECT a.idx FROM attachments a JOIN messages m ON m.id = a.message_id
         WHERE a.message_id = ? AND m.user_id = ? AND a.content_id = ?",
    )
    .bind(id)
    .bind(user.id)
    .bind(cid)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(ApiError::not_found)?;
    let Part { mime, data, .. } = load_part(&state, user.id, id, idx).await?;
    if !["image/png", "image/jpeg", "image/gif", "image/webp"].contains(&mime.to_lowercase().as_str()) {
        return Err(ApiError::not_found());
    }
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, header(&mime));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, max-age=86400"));
    Ok((headers, data))
}

// ---- Search and live updates ----------------------------------------------------------------

#[derive(Serialize, sqlx::FromRow)]
pub struct SearchRow {
    id: i64,
    thread_id: i64,
    account_id: i64,
    subject: String,
    from_name: String,
    from_addr: String,
    date: i64,
    seen: bool,
    excerpt: String,
}

/// LIKE pattern matching `needle` anywhere, with LIKE's own wildcards taken literally.
fn contains(needle: &str) -> String {
    let escaped = needle.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    format!("%{escaped}%")
}

/// Search with the filters of `crate::search`. Dates are days in the server's time zone.
pub async fn search(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<ContactQuery>,
) -> ApiResult<Json<Vec<SearchRow>>> {
    let today = chrono::Local::now().date_naive();
    let query = crate::search::parse(&q.q, today).map_err(ApiError::bad_request)?;
    if query.is_empty() {
        return Ok(Json(vec![]));
    }
    let fts = query.fts();
    let columns = "m.id, m.thread_id, m.account_id, m.subject, m.from_name, m.from_addr, m.date, m.seen";

    let mut sql = sqlx::QueryBuilder::<sqlx::Sqlite>::new("SELECT ");
    sql.push(columns);
    if fts.is_empty() {
        // Filters only: no text to match, so the stored preview stands in for the excerpt.
        sql.push(", m.snippet AS excerpt FROM messages m WHERE m.user_id = ")
            .push_bind(user.id);
    } else {
        sql.push(
            ", snippet(messages_fts, 4, '', '', '…', 16) AS excerpt
             FROM messages_fts JOIN messages m ON m.id = messages_fts.rowid
             WHERE messages_fts MATCH ",
        )
        .push_bind(fts)
        .push(" AND m.user_id = ")
        .push_bind(user.id);
    }
    for sender in &query.from {
        sql.push(" AND (m.from_name LIKE ")
            .push_bind(contains(sender))
            .push(" ESCAPE '\\' OR m.from_addr LIKE ")
            .push_bind(contains(sender))
            .push(" ESCAPE '\\')");
    }
    for recipient in &query.to {
        sql.push(" AND (m.to_addrs LIKE ")
            .push_bind(contains(recipient))
            .push(" ESCAPE '\\' OR m.cc_addrs LIKE ")
            .push_bind(contains(recipient))
            .push(" ESCAPE '\\')");
    }
    for subject in &query.subject {
        sql.push(" AND m.subject LIKE ")
            .push_bind(contains(subject))
            .push(" ESCAPE '\\'");
    }
    if let Some(wanted) = query.attachment {
        sql.push(" AND m.has_attachments = ").push_bind(wanted);
    }
    if let Some((first, last)) = query.received {
        // Midnight in the server's zone; a day that starts twice or not at all (clock change) still gets one.
        let midnight = |day: chrono::NaiveDate| {
            day.and_hms_opt(0, 0, 0)
                .and_then(|t| t.and_local_timezone(chrono::Local).earliest())
                .map(|t| t.timestamp())
        };
        if let Some(from) = first.and_then(midnight) {
            sql.push(" AND m.date >= ").push_bind(from);
        }
        if let Some(until) = last.and_then(|day| day.succ_opt()).and_then(midnight) {
            sql.push(" AND m.date < ").push_bind(until);
        }
    }
    sql.push(" ORDER BY m.date DESC, m.id DESC LIMIT 200");
    Ok(Json(sql.build_query_as().fetch_all(&state.db).await?))
}

pub async fn events(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let stream = BroadcastStream::new(state.events.subscribe()).filter_map(move |event| {
        let item = match event {
            Ok(e) if e.user_id == user.id => Some(Ok(SseEvent::default().data(e.kind))),
            // Fell behind: tell the client to reload rather than miss a change.
            Err(_) => Some(Ok(SseEvent::default().data("mail"))),
            _ => None,
        };
        async move { item }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
