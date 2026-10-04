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
    /// A picture of the user's own choosing is set for the sender.
    has_picture: bool,
}

pub async fn senders(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<SenderQuery>,
) -> ApiResult<Json<Vec<SenderRow>>> {
    Ok(Json(
        sqlx::query_as(
            "SELECT s.id, s.address, s.display_name, s.category, s.decided_at,
                    (SELECT COUNT(*) FROM messages m WHERE m.sender_id = s.id) AS count,
                    EXISTS (SELECT 1 FROM sender_pictures p WHERE p.sender_id = s.id) AS has_picture
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

/// What the composer offers for a recipient being typed.
#[derive(Serialize)]
pub struct Contact {
    name: String,
    /// The address; for a group all its addresses, separated by ", ".
    address: String,
    group: bool,
}

/// Recipient suggestions for the composer.
pub async fn contacts(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<ContactQuery>,
) -> ApiResult<Json<Vec<Contact>>> {
    let pattern = format!("%{}%", q.q.trim().replace(['%', '_'], ""));
    // The user's own groups first: a name that stands for all its addresses.
    let groups: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, members FROM recipient_groups WHERE user_id = ? AND name LIKE ? ORDER BY name LIMIT 4",
    )
    .bind(user.id)
    .bind(&pattern)
    .fetch_all(&state.db)
    .await?;
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
    let contact = |group| move |(name, address)| Contact { name, address, group };
    Ok(Json(
        groups
            .into_iter()
            .map(contact(true))
            .chain(rows.into_iter().map(contact(false)))
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct AvatarQuery {
    address: String,
}

/// Picture of a sender (the user's own choice, else Gravatar or BIMI logo), or 404 so the page
/// shows initials instead.
/// Only for addresses the user actually has mail from, so this cannot be used to look up others.
pub async fn avatar(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<AvatarQuery>,
) -> Result<(HeaderMap, Vec<u8>), (HeaderMap, ApiError)> {
    // "Not found" may be cached by the browser for a while too.
    let missing = || {
        let mut headers = HeaderMap::new();
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, max-age=3600"));
        (headers, ApiError::not_found())
    };
    let address = q.address.trim().to_lowercase();
    // A picture the user chose comes first, and needs no lookup anywhere.
    let own: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT p.data FROM sender_pictures p JOIN senders s ON s.id = p.sender_id
         WHERE s.user_id = ? AND s.address = ?",
    )
    .bind(user.id)
    .bind(&address)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (HeaderMap::new(), e.into()))?;
    if let Some(data) = own {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
        headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, max-age=3600"));
        return Ok((headers, data));
    }
    if !state.config.avatars {
        return Err(missing());
    }
    let known: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM senders WHERE user_id = ?1 AND address = ?2)
             OR EXISTS (SELECT 1 FROM accounts WHERE user_id = ?1 AND lower(address) = ?2)",
    )
    .bind(user.id)
    .bind(&address)
    .fetch_one(&state.db)
    .await
    .map_err(|e| (HeaderMap::new(), e.into()))?;
    if !known {
        return Err(missing());
    }
    let found = crate::avatar::lookup(&state, &address)
        .await
        .map_err(|e| (HeaderMap::new(), e.into()))?;
    let Some(picture) = found else {
        return Err(missing());
    };
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, header(&picture.mime));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, max-age=86400"));
    // An SVG logo is a document; opened directly it must not be able to do anything on this origin.
    headers.insert(
        axum::http::header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; sandbox"),
    );
    Ok((headers, picture.data))
}

// ---- Thread lists ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct BoxQuery {
    #[serde(rename = "box")]
    mailbox: String,
    /// `1`: only conversations with unseen mail.
    unseen: Option<String>,
    /// How many conversations at most (300 unless said), and how many to skip: the list is
    /// read in pages, newest first.
    limit: Option<i64>,
    #[serde(default)]
    offset: i64,
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
    /// When a delayed conversation returns to the inbox.
    snoozed_until: Option<i64>,
    /// Your own note on the conversation; empty when there is none.
    note: String,
}

/// A conversation's own data and its sender, for the mail view.
#[derive(sqlx::FromRow)]
struct ThreadHead {
    subject: String,
    sender_id: Option<i64>,
    address: Option<String>,
    display_name: Option<String>,
    category: Option<String>,
    note: String,
    has_picture: bool,
}

/// SQL conditions on a thread `t`, shared by the lists, the counts and the one-page views.
/// Received mail of it that is neither archived nor in the Trash:
pub(crate) const RECEIVED: &str = "EXISTS (SELECT 1 FROM messages rm JOIN folders rf ON rf.id = rm.folder_id
    WHERE rm.thread_id = t.id AND rm.is_outgoing = 0 AND rf.role NOT IN ('archive', 'trash', 'limbo'))";
/// A flagged mail of it sits in the inbox (or the "Nice to know" folder): the conversation is in Important.
pub(crate) const FLAGGED: &str = "EXISTS (SELECT 1 FROM messages fm JOIN folders ff ON ff.id = fm.folder_id
    WHERE fm.thread_id = t.id AND fm.flagged = 1 AND ff.role IN ('inbox', 'feed', 'delayed'))";
/// A message `m` in folder `f` that is neither archived nor in the Trash.
const HERE: &str = "f.role NOT IN ('archive', 'trash', 'limbo')";
/// It is delayed and has not come back yet.
pub(crate) const DELAYED: &str = "(t.snoozed_until IS NOT NULL AND t.snoozed_until > unixepoch())";

pub async fn threads(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<BoxQuery>,
) -> ApiResult<Json<Vec<ThreadRow>>> {
    let condition = match q.mailbox.as_str() {
        // Internally the Inbox is still called "important", after its sender category.
        // Threads the user started belong to it once somebody answers.
        "important" => format!(
            "(t.sender_id IS NULL OR s.category = 'important') AND {RECEIVED} AND NOT {FLAGGED} AND NOT {DELAYED}"
        ),
        // A conversation leaves its list once every received message of it is archived.
        "feed" => format!("s.category = 'feed' AND {RECEIVED} AND NOT {FLAGGED} AND NOT {DELAYED}"),
        "junk" => format!("s.category = 'junk' AND {RECEIVED}"),
        // The Important list: conversations with a flagged mail in the inbox, whoever sent them.
        "flagged" => format!("{FLAGGED} AND NOT {DELAYED}"),
        "delayed" => DELAYED.to_string(),
        "archive" => "EXISTS (SELECT 1 FROM messages m JOIN folders f ON f.id = m.folder_id
                              WHERE m.thread_id = t.id AND f.role = 'archive')"
            .to_string(),
        "trash" => "EXISTS (SELECT 1 FROM messages m JOIN folders f ON f.id = m.folder_id
                            WHERE m.thread_id = t.id AND f.role = 'trash')"
            .to_string(),
        "sent" => "EXISTS (SELECT 1 FROM messages m JOIN folders f ON f.id = m.folder_id
                           WHERE m.thread_id = t.id AND m.is_outgoing = 1 AND f.role NOT IN ('trash', 'limbo'))"
            .to_string(),
        _ => return Err(ApiError::bad_request("unknown box")),
    };
    // A conversation that came back from a delay sorts by the moment it returned.
    let order = if q.mailbox == "delayed" {
        "t.snoozed_until, lm.date DESC"
    } else {
        "MAX(lm.date, COALESCE(t.returned_at, 0)) DESC, lm.id DESC"
    };
    // The same test the `unread` column of a row makes.
    let unseen = if matches!(q.unseen.as_deref(), Some("1" | "true")) {
        " AND EXISTS (SELECT 1 FROM messages um WHERE um.thread_id = t.id AND um.seen = 0 AND um.is_outgoing = 0)"
    } else {
        ""
    };
    let sql = format!(
        "SELECT t.id, t.subject,
                (SELECT COUNT(DISTINCT m.message_id) FROM messages m WHERE m.thread_id = t.id) AS count,
                (SELECT COUNT(*) FROM messages m WHERE m.thread_id = t.id AND m.seen = 0 AND m.is_outgoing = 0) AS unread,
                lm.date, lm.snippet, lm.from_name, lm.from_addr, lm.is_outgoing, lm.account_id,
                EXISTS (SELECT 1 FROM messages m WHERE m.thread_id = t.id AND m.has_attachments = 1) AS has_attachments,
                s.display_name AS sender_name, s.address AS sender_address, t.snoozed_until, t.note
         FROM threads t
         JOIN messages lm ON lm.id = (
             SELECT id FROM messages WHERE thread_id = t.id ORDER BY date DESC, id DESC LIMIT 1)
         LEFT JOIN senders s ON s.id = t.sender_id
         WHERE t.user_id = ? AND {condition}{unseen}
         ORDER BY {order} LIMIT ? OFFSET ?"
    );
    Ok(Json(
        sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(user.id)
            .bind(q.limit.unwrap_or(300).clamp(1, 1000))
            .bind(q.offset.max(0))
            .fetch_all(&state.db)
            .await?,
    ))
}

/// Conversations in the Inbox's "Seen" area whose newest mail is older than `cutoff` (unix
/// seconds): let in by the Screener, nothing unread, not Important, not delayed, and with mail
/// still in the inbox to archive. A conversation that returned from a delay counts from its return.
pub(crate) async fn seen_inbox_older_than(
    state: &AppState,
    user_id: i64,
    cutoff: i64,
    limit: i64,
) -> ApiResult<Vec<i64>> {
    let sql = format!(
        "SELECT t.id FROM threads t
         LEFT JOIN senders s ON s.id = t.sender_id
         JOIN messages lm ON lm.id = (
             SELECT id FROM messages WHERE thread_id = t.id ORDER BY date DESC, id DESC LIMIT 1)
         WHERE t.user_id = ?1
           AND (t.sender_id IS NULL OR s.category = 'important')
           AND {RECEIVED} AND NOT {FLAGGED} AND NOT {DELAYED}
           AND NOT EXISTS (SELECT 1 FROM messages um JOIN folders uf ON uf.id = um.folder_id
                           WHERE um.thread_id = t.id AND um.seen = 0 AND um.is_outgoing = 0
                             AND uf.role NOT IN ('archive', 'trash', 'limbo'))
           AND EXISTS (SELECT 1 FROM messages am JOIN folders af ON af.id = am.folder_id
                       WHERE am.thread_id = t.id AND af.role IN ('inbox', 'junk', 'feed', 'delayed') AND am.uid IS NOT NULL)
           AND MAX(lm.date, COALESCE(t.returned_at, 0)) < ?2
         ORDER BY lm.date LIMIT ?3"
    );
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .bind(cutoff)
        .bind(limit)
        .fetch_all(&state.db)
        .await?)
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
    let unread_threads = "SELECT COUNT(DISTINCT t.id) FROM threads t
         LEFT JOIN senders s ON s.id = t.sender_id
         JOIN messages m ON m.thread_id = t.id AND m.seen = 0 AND m.is_outgoing = 0
         JOIN folders f ON f.id = m.folder_id AND f.role NOT IN ('archive', 'trash', 'limbo')
         WHERE t.user_id = ?";
    let unread: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "{unread_threads} AND (t.sender_id IS NULL OR s.category = 'important') AND NOT {FLAGGED} AND NOT {DELAYED}"
    )))
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    let unread_flagged: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "{unread_threads} AND {FLAGGED} AND NOT {DELAYED}"
    )))
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    // The other lists, each counted the way its list selects conversations.
    let unread_in = async |condition: String| -> ApiResult<i64> {
        Ok(
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("{unread_threads} AND {condition}")))
                .bind(user.id)
                .fetch_one(&state.db)
                .await?,
        )
    };
    let unread_feed = unread_in(format!("s.category = 'feed' AND NOT {FLAGGED} AND NOT {DELAYED}")).await?;
    let unread_junk = unread_in("s.category = 'junk'".to_string()).await?;
    let unread_delayed = unread_in(DELAYED.to_string()).await?;
    // Archive and Trash hold mail by folder, whoever sent it.
    let unread_folder = "SELECT COUNT(DISTINCT m.thread_id) FROM messages m
         JOIN folders f ON f.id = m.folder_id
         WHERE m.user_id = ? AND m.seen = 0 AND m.is_outgoing = 0 AND f.role = ?";
    let unread_archive: i64 = sqlx::query_scalar(unread_folder)
        .bind(user.id)
        .bind("archive")
        .fetch_one(&state.db)
        .await?;
    let unread_trash: i64 = sqlx::query_scalar(unread_folder)
        .bind(user.id)
        .bind("trash")
        .fetch_one(&state.db)
        .await?;
    let delayed: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*) FROM threads t WHERE t.user_id = ? AND {DELAYED}"
    )))
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    let drafts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM drafts WHERE user_id = ? AND sending_at IS NULL")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(json!({
        "screener": screener,
        "unread_important": unread,
        "unread_flagged": unread_flagged,
        "unread_feed": unread_feed,
        "unread_junk": unread_junk,
        "unread_delayed": unread_delayed,
        "unread_archive": unread_archive,
        "unread_trash": unread_trash,
        "delayed": delayed,
        "drafts": drafts,
    })))
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
    let head: ThreadHead = sqlx::query_as(
        "SELECT t.subject, s.id AS sender_id, s.address, s.display_name, s.category, t.note,
                EXISTS (SELECT 1 FROM sender_pictures p WHERE p.sender_id = s.id) AS has_picture
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

    // Archiving applies to received mail that still sits in the inbox (or Junk).
    let can_archive: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM messages m JOIN folders f ON f.id = m.folder_id
                        WHERE m.thread_id = ? AND f.role IN ('inbox', 'junk', 'feed', 'delayed') AND m.uid IS NOT NULL)",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    let (flagged, snoozed_until): (bool, Option<i64>) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {FLAGGED}, CASE WHEN {DELAYED} THEN t.snoozed_until END FROM threads t WHERE t.id = ?"
    )))
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    let can_trash: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM messages m JOIN folders f ON f.id = m.folder_id
                        WHERE m.thread_id = ? AND f.role != 'trash')",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    let can_restore: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM messages m JOIN folders f ON f.id = m.folder_id
                        WHERE m.thread_id = ? AND f.role = 'trash' AND m.uid IS NOT NULL)",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(json!({
        "id": id,
        "can_archive": can_archive,
        "can_trash": can_trash,
        "can_restore": can_restore,
        "important": flagged,
        "snoozed_until": snoozed_until,
        "subject": head.subject,
        "note": head.note,
        "sender": head.sender_id.map(|sender_id| json!({
            "id": sender_id, "address": head.address, "display_name": head.display_name, "category": head.category,
            "has_picture": head.has_picture,
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
    /// important (the Inbox) | flagged (Important) | delayed | feed | junk | sent | archive | trash
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
        "important" => format!(
            "m.is_outgoing = 0 AND {HERE} AND (t.sender_id IS NULL OR ts.category = 'important')
             AND NOT {FLAGGED} AND NOT {DELAYED}"
        ),
        "feed" => format!("m.is_outgoing = 0 AND {HERE} AND ts.category = 'feed' AND NOT {FLAGGED} AND NOT {DELAYED}"),
        "junk" => format!("m.is_outgoing = 0 AND {HERE} AND ts.category = 'junk'"),
        "flagged" => format!("m.is_outgoing = 0 AND {HERE} AND {FLAGGED} AND NOT {DELAYED}"),
        "delayed" => format!("m.is_outgoing = 0 AND {HERE} AND {DELAYED}"),
        "sent" => "m.is_outgoing = 1 AND f.role NOT IN ('trash', 'limbo')".to_string(),
        "archive" => "f.role = 'archive'".to_string(),
        "trash" => "f.role = 'trash'".to_string(),
        _ => return Err(ApiError::bad_request("unknown box")),
    };
    let rows: Vec<MessageRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {MESSAGE_COLUMNS} FROM messages m
         JOIN threads t ON t.id = m.thread_id
         JOIN folders f ON f.id = m.folder_id
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

#[derive(Deserialize)]
pub struct NoteInput {
    note: String,
}

const MAX_NOTE: usize = 2000;

/// Sets your note on a conversation; an empty one removes it.
pub async fn set_note(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(input): Json<NoteInput>,
) -> ApiResult<Json<Value>> {
    let note = input.note.trim();
    if note.chars().count() > MAX_NOTE {
        return Err(ApiError::bad_request(format!(
            "A note can have up to {MAX_NOTE} characters."
        )));
    }
    let before: String = sqlx::query_scalar("SELECT note FROM threads WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(ApiError::not_found)?;
    sqlx::query("UPDATE threads SET note = ? WHERE id = ?")
        .bind(note)
        .bind(id)
        .execute(&state.db)
        .await?;
    state.notify(user.id, "mail");
    // A note lives in Portier only: there is nothing left to do once it cannot be undone.
    let step = crate::undo::Step::Note {
        thread: id,
        text: before,
    };
    let token = crate::undo::hold(&state, user.id, vec![step], async {});
    Ok(Json(json!({ "note": note, "undo": token })))
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
    /// Your note on the conversation the mail belongs to.
    note: String,
    /// The list the mail is found in: home | flagged | delayed | feed | screener | archive | sent | junk | trash.
    place: String,
}

/// LIKE pattern matching `needle` anywhere, with LIKE's own wildcards taken literally.
fn contains(needle: &str) -> String {
    let escaped = needle.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    format!("%{escaped}%")
}

/// What a search is asked to return.
pub(crate) enum SearchOutput {
    /// The matching mails with an excerpt.
    Rows,
    /// How many matching received mails are unread.
    UnreadCount,
}

/// Builds the SQL of a search up to and including its WHERE clause. Dates are days in the
/// server's time zone.
pub(crate) fn search_sql(
    query: &crate::search::SearchQuery,
    user_id: i64,
    output: SearchOutput,
) -> sqlx::QueryBuilder<sqlx::Sqlite> {
    let fts = query.fts();
    // `place` follows the rules the lists themselves select by.
    let columns = format!(
        "m.id, m.thread_id, m.account_id, m.subject, m.from_name, m.from_addr, m.date, m.seen, t.note,
         CASE WHEN f.role = 'archive' THEN 'archive'
              WHEN f.role = 'trash' THEN 'trash'
              WHEN m.is_outgoing = 1 THEN 'sent'
              WHEN f.role = 'junk' OR ts.category = 'junk' THEN 'junk'
              WHEN {DELAYED} THEN 'delayed'
              WHEN {FLAGGED} THEN 'flagged'
              WHEN ts.category = 'feed' THEN 'feed'
              WHEN t.sender_id IS NOT NULL AND ts.category IS NULL THEN 'screener'
              ELSE 'home' END AS place"
    );
    let mut sql = sqlx::QueryBuilder::<sqlx::Sqlite>::new("SELECT ");
    match (&output, fts.is_empty()) {
        (SearchOutput::UnreadCount, _) => {
            sql.push("COUNT(*)");
        }
        // Filters only: no text to match, so the stored preview stands in for the excerpt.
        (SearchOutput::Rows, true) => {
            sql.push(&columns).push(", m.snippet AS excerpt");
        }
        // The place in the mail where the words were found; a mail found by its note has none.
        (SearchOutput::Rows, false) => {
            sql.push(&columns)
                .push(
                    ", COALESCE((SELECT snippet(messages_fts, 4, '', '', '…', 16) FROM messages_fts
                                 WHERE messages_fts MATCH ",
                )
                .push_bind(fts.clone())
                .push(" AND messages_fts.rowid = m.id), m.snippet) AS excerpt");
        }
    }
    sql.push(
        " FROM messages m JOIN threads t ON t.id = m.thread_id JOIN folders f ON f.id = m.folder_id
          LEFT JOIN senders ts ON ts.id = t.sender_id WHERE m.user_id = ",
    )
    .push_bind(user_id);
    if !fts.is_empty() {
        // Words are looked for in the mail, and in your note on its conversation; a conversation
        // found by its note is listed once, with its newest mail.
        sql.push(" AND (m.id IN (SELECT rowid FROM messages_fts WHERE messages_fts MATCH ")
            .push_bind(fts)
            .push(
                ") OR (t.note != '' AND m.id = (SELECT id FROM messages WHERE thread_id = t.id
                                                ORDER BY date DESC, id DESC LIMIT 1)",
            );
        for word in &query.text {
            sql.push(" AND t.note LIKE ")
                .push_bind(contains(word))
                .push(" ESCAPE '\\'");
        }
        sql.push("))");
    }
    if query.noted {
        sql.push(" AND t.note != ''");
    }
    // What is in the Trash is not searched.
    sql.push(" AND m.folder_id NOT IN (SELECT id FROM folders WHERE role IN ('trash', 'limbo'))");
    if matches!(output, SearchOutput::UnreadCount) {
        sql.push(" AND m.seen = 0 AND m.is_outgoing = 0");
    }
    // Values of one filter are alternatives (OR); the filters themselves all have to hold (AND).
    let text_filters: [(&[String], &[&str]); 4] = [
        (&query.from, &["m.from_name", "m.from_addr"]),
        (&query.to, &["m.to_addrs", "m.cc_addrs"]),
        (&query.subject, &["m.subject"]),
        (&query.note, &["t.note"]),
    ];
    for (values, columns) in text_filters {
        if values.is_empty() {
            continue;
        }
        sql.push(" AND (");
        let mut first = true;
        for value in values {
            for column in columns {
                sql.push(if first { "" } else { " OR " });
                first = false;
                sql.push(*column)
                    .push(" LIKE ")
                    .push_bind(contains(value))
                    .push(" ESCAPE '\\'");
            }
        }
        sql.push(")");
    }
    if let Some(wanted) = query.attachment {
        sql.push(" AND m.has_attachments = ").push_bind(wanted);
    }
    if !query.received.is_empty() {
        // Midnight in the server's zone; a day that starts twice or not at all (clock change) still gets one.
        let midnight = |day: chrono::NaiveDate| {
            day.and_hms_opt(0, 0, 0)
                .and_then(|t| t.and_local_timezone(chrono::Local).earliest())
                .map(|t| t.timestamp())
        };
        sql.push(" AND (");
        for (index, (first, last)) in query.received.iter().enumerate() {
            sql.push(if index == 0 { "(1 = 1" } else { " OR (1 = 1" });
            if let Some(from) = first.and_then(midnight) {
                sql.push(" AND m.date >= ").push_bind(from);
            }
            if let Some(until) = last.and_then(|day| day.succ_opt()).and_then(midnight) {
                sql.push(" AND m.date < ").push_bind(until);
            }
            sql.push(")");
        }
        sql.push(")");
    }
    sql
}

/// Search with the filters of `crate::search`.
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
    let mut sql = search_sql(&query, user.id, SearchOutput::Rows);
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

/// Takes back the action a token was given for, while its time to undo lasts.
pub async fn undo(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(token): Path<String>,
) -> ApiResult<Json<Value>> {
    let draft = crate::undo::undo(&state, user.id, &token).await?;
    Ok(Json(json!({ "ok": true, "draft": draft })))
}
