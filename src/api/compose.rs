use axum::{
    Json,
    extract::{Multipart, Path, State},
};
use lettre::message::Mailbox;
use mail_parser::{MessageParser, MimeHeaders};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    auth::CurrentUser,
    crypto,
    error::{ApiError, ApiResult},
    mail::{
        smtp::{self, Outgoing, OutgoingAttachment, SmtpParams},
        store, sync,
    },
    models::{Account, Addr, LOCAL_SENT},
    state::{AppState, now},
};

#[derive(Serialize, sqlx::FromRow)]
pub struct Draft {
    id: i64,
    account_id: Option<i64>,
    kind: String,
    source_message: Option<i64>,
    to_addrs: String,
    cc_addrs: String,
    bcc_addrs: String,
    subject: String,
    body_html: String,
    forward_attachments: bool,
    include_quote: bool,
    updated_at: i64,
    /// Why the last attempt to send it failed, if one did.
    last_error: Option<String>,
    /// When sending was interrupted after the mail may already have been handed to the mail
    /// server. Such a draft is not offered for sending again by itself.
    sent_at: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct DraftAttachment {
    id: i64,
    filename: String,
    mime: String,
    size: i64,
    #[serde(skip)]
    path: String,
}

#[derive(sqlx::FromRow)]
struct Source {
    id: i64,
    account_id: i64,
    message_id: String,
    refs: String,
    from_name: String,
    from_addr: String,
    to_addrs: String,
    cc_addrs: String,
    subject: String,
    date: i64,
    is_outgoing: bool,
    body_text: String,
    body_html: String,
}

async fn load_source(state: &AppState, user_id: i64, id: i64) -> ApiResult<Option<Source>> {
    Ok(sqlx::query_as(
        "SELECT id, account_id, message_id, refs, from_name, from_addr, to_addrs, cc_addrs, subject, date,
                is_outgoing, body_text, body_html
         FROM messages WHERE id = ? AND user_id = ?",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?)
}

async fn load_draft(state: &AppState, user_id: i64, id: i64) -> ApiResult<Draft> {
    sqlx::query_as("SELECT * FROM drafts WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(ApiError::not_found)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn format_date(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0)
        .map(|d| d.format("%-d %b %Y, %H:%M UTC").to_string())
        .unwrap_or_default()
}

fn with_prefix(prefix: &str, subject: &str) -> String {
    if subject.to_lowercase().starts_with(&prefix.to_lowercase()) {
        subject.to_string()
    } else {
        format!("{prefix} {subject}")
    }
}

fn join(addrs: &[Addr]) -> String {
    addrs.iter().map(|a| a.address.as_str()).collect::<Vec<_>>().join(", ")
}

/// The original message as it is appended below a reply or forward.
fn quoted_html(kind: &str, source: &Source) -> String {
    let body = if source.body_html.is_empty() {
        format!(
            "<pre style=\"white-space:pre-wrap;font:inherit\">{}</pre>",
            escape(&source.body_text)
        )
    } else {
        source.body_html.clone()
    };
    let who = if source.from_name.is_empty() {
        escape(&source.from_addr)
    } else {
        format!("{} &lt;{}&gt;", escape(&source.from_name), escape(&source.from_addr))
    };
    if kind == "forward" {
        let to: Vec<Addr> = serde_json::from_str(&source.to_addrs).unwrap_or_default();
        format!(
            "<br><div><p>---------- Forwarded message ----------<br>From: {who}<br>Date: {}<br>Subject: {}<br>To: {}</p>{body}</div>",
            format_date(source.date),
            escape(&source.subject),
            escape(&join(&to)),
        )
    } else {
        format!(
            "<br><div><p>On {}, {who} wrote:</p><blockquote style=\"margin:0 0 0 .8ex;border-left:2px solid #ccc;padding-left:1ex\">{body}</blockquote></div>",
            format_date(source.date),
        )
    }
}

#[derive(Deserialize)]
pub struct NewDraft {
    #[serde(default = "default_kind")]
    kind: String,
    source_message: Option<i64>,
    #[serde(default)]
    to: String,
}

fn default_kind() -> String {
    "new".into()
}

/// Creates a draft; replies and forwards are pre-filled from the source message.
pub async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(input): Json<NewDraft>,
) -> ApiResult<Json<Value>> {
    if !["new", "reply", "reply_all", "forward"].contains(&input.kind.as_str()) {
        return Err(ApiError::bad_request("unknown draft kind"));
    }
    let own: Vec<String> = sqlx::query_scalar("SELECT lower(address) FROM accounts WHERE user_id = ? ORDER BY id")
        .bind(user.id)
        .fetch_all(&state.db)
        .await?;
    let mut account_id: Option<i64> =
        sqlx::query_scalar("SELECT id FROM accounts WHERE user_id = ? ORDER BY id LIMIT 1")
            .bind(user.id)
            .fetch_optional(&state.db)
            .await?;
    let (mut to, mut cc, mut subject) = (input.to.clone(), String::new(), String::new());
    let mut source_id = None;

    if input.kind != "new" {
        let source = match input.source_message {
            Some(id) => load_source(&state, user.id, id).await?,
            None => None,
        }
        .ok_or_else(|| ApiError::bad_request("source message not found"))?;
        source_id = Some(source.id);
        account_id = Some(source.account_id);
        let source_to: Vec<Addr> = serde_json::from_str(&source.to_addrs).unwrap_or_default();
        let source_cc: Vec<Addr> = serde_json::from_str(&source.cc_addrs).unwrap_or_default();
        let others =
            |list: &[Addr]| -> Vec<Addr> { list.iter().filter(|a| !own.contains(&a.address)).cloned().collect() };
        match input.kind.as_str() {
            "forward" => subject = with_prefix("Fwd:", &source.subject),
            kind => {
                subject = with_prefix("Re:", &source.subject);
                // Replying to your own message continues the conversation with its recipients.
                let mut recipients = if source.is_outgoing {
                    source_to.clone()
                } else {
                    vec![Addr {
                        name: source.from_name.clone(),
                        address: source.from_addr.clone(),
                    }]
                };
                if kind == "reply_all" {
                    if !source.is_outgoing {
                        recipients.extend(others(&source_to));
                    }
                    cc = join(&others(&source_cc));
                }
                recipients.dedup_by(|a, b| a.address == b.address);
                to = join(&recipients);
            }
        }
    }

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO drafts (user_id, account_id, kind, source_message, to_addrs, cc_addrs, subject,
             forward_attachments)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(user.id)
    .bind(account_id)
    .bind(&input.kind)
    .bind(source_id)
    .bind(to)
    .bind(cc)
    .bind(subject)
    .bind(input.kind == "forward")
    .fetch_one(&state.db)
    .await?;
    Ok(Json(json!({ "id": id })))
}

pub async fn list(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Vec<Draft>>> {
    Ok(Json(
        // A mail on its way out (sending can still be undone) is no draft to list.
        sqlx::query_as("SELECT * FROM drafts WHERE user_id = ? AND sending_at IS NULL ORDER BY updated_at DESC")
            .bind(user.id)
            .fetch_all(&state.db)
            .await?,
    ))
}

async fn draft_attachments(state: &AppState, draft_id: i64) -> ApiResult<Vec<DraftAttachment>> {
    Ok(
        sqlx::query_as("SELECT id, filename, mime, size, path FROM draft_attachments WHERE draft_id = ? ORDER BY id")
            .bind(draft_id)
            .fetch_all(&state.db)
            .await?,
    )
}

pub async fn get(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let draft = load_draft(&state, user.id, id).await?;
    let source = match draft.source_message {
        Some(source_id) => load_source(&state, user.id, source_id).await?,
        None => None,
    };
    let source_attachments: i64 = match &source {
        Some(s) => {
            sqlx::query_scalar("SELECT COUNT(*) FROM attachments WHERE message_id = ? AND inline = 0")
                .bind(s.id)
                .fetch_one(&state.db)
                .await?
        }
        None => 0,
    };
    Ok(Json(json!({
        "attachments": draft_attachments(&state, id).await?,
        "source": source.map(|s| json!({
            "id": s.id,
            "from": Addr { name: s.from_name, address: s.from_addr },
            "subject": s.subject,
            "date": s.date,
            "attachments": source_attachments,
        })),
        "draft": draft,
    })))
}

#[derive(Deserialize)]
pub struct DraftUpdate {
    account_id: Option<i64>,
    to_addrs: String,
    cc_addrs: String,
    bcc_addrs: String,
    subject: String,
    body_html: String,
    forward_attachments: bool,
    include_quote: bool,
}

pub async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(input): Json<DraftUpdate>,
) -> ApiResult<Json<Value>> {
    if let Some(account_id) = input.account_id {
        let owned: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM accounts WHERE id = ? AND user_id = ?)")
            .bind(account_id)
            .bind(user.id)
            .fetch_one(&state.db)
            .await?;
        if !owned {
            return Err(ApiError::bad_request("unknown account"));
        }
    }
    let result = sqlx::query(
        "UPDATE drafts SET account_id = ?, to_addrs = ?, cc_addrs = ?, bcc_addrs = ?, subject = ?,
             body_html = ?, forward_attachments = ?, include_quote = ?, updated_at = ?, last_error = NULL
         WHERE id = ? AND user_id = ? AND sending_at IS NULL",
    )
    .bind(input.account_id)
    .bind(&input.to_addrs)
    .bind(&input.cc_addrs)
    .bind(&input.bcc_addrs)
    .bind(&input.subject)
    .bind(&input.body_html)
    .bind(input.forward_attachments)
    .bind(input.include_quote)
    .bind(now())
    .bind(id)
    .bind(user.id)
    .execute(&state.db)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    Ok(Json(json!({ "ok": true })))
}

async fn remove_draft(state: &AppState, id: i64) -> ApiResult<()> {
    sqlx::query("DELETE FROM drafts WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    let _ = tokio::fs::remove_dir_all(state.draft_dir(id)).await;
    Ok(())
}

pub async fn delete(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    load_draft(&state, user.id, id).await?;
    remove_draft(&state, id).await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn upload(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    mut multipart: Multipart,
) -> ApiResult<Json<Vec<DraftAttachment>>> {
    load_draft(&state, user.id, id).await?;
    let dir = state.draft_dir(id);
    tokio::fs::create_dir_all(&dir).await?;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request(format!("upload failed: {e}")))?
    {
        let filename = field.file_name().unwrap_or("attachment").to_string();
        let mime = field
            .content_type()
            .map(str::to_string)
            .unwrap_or_else(|| mime_guess::from_path(&filename).first_or_octet_stream().to_string());
        let data = field
            .bytes()
            .await
            .map_err(|e| ApiError::bad_request(format!("upload failed: {e}")))?;
        // The stored name is random; the user's file name only ever lives in the database.
        let path = dir.join(crypto::random_token());
        tokio::fs::write(&path, &data).await?;
        sqlx::query("INSERT INTO draft_attachments (draft_id, filename, mime, size, path) VALUES (?, ?, ?, ?, ?)")
            .bind(id)
            .bind(filename)
            .bind(mime)
            .bind(data.len() as i64)
            .bind(path.to_string_lossy().to_string())
            .execute(&state.db)
            .await?;
    }
    Ok(Json(draft_attachments(&state, id).await?))
}

pub async fn delete_attachment(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, attachment_id)): Path<(i64, i64)>,
) -> ApiResult<Json<Vec<DraftAttachment>>> {
    load_draft(&state, user.id, id).await?;
    let path: Option<String> =
        sqlx::query_scalar("DELETE FROM draft_attachments WHERE id = ? AND draft_id = ? RETURNING path")
            .bind(attachment_id)
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    if let Some(path) = path {
        let _ = tokio::fs::remove_file(path).await;
    }
    Ok(Json(draft_attachments(&state, id).await?))
}

pub async fn send(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let draft = load_draft(&state, user.id, id).await?;
    let account: Account = sqlx::query_as("SELECT * FROM accounts WHERE id = ? AND user_id = ?")
        .bind(draft.account_id)
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::bad_request("choose an account to send from"))?;

    let bad = |e: anyhow::Error| ApiError::bad_request(e.to_string());
    let to = smtp::parse_recipients(&draft.to_addrs).map_err(bad)?;
    let cc = smtp::parse_recipients(&draft.cc_addrs).map_err(bad)?;
    let bcc = smtp::parse_recipients(&draft.bcc_addrs).map_err(bad)?;
    if to.is_empty() && cc.is_empty() && bcc.is_empty() {
        return Err(ApiError::bad_request("add at least one recipient"));
    }
    let from = Mailbox::new(
        Some(account.display_name.clone()).filter(|n| !n.is_empty()),
        account
            .address
            .parse()
            .map_err(|_| ApiError::bad_request("account address is invalid"))?,
    );

    let source = match draft.source_message {
        Some(source_id) => load_source(&state, user.id, source_id).await?,
        None => None,
    };
    let mut html = draft.body_html.clone();
    let mut in_reply_to = None;
    let mut references = Vec::new();
    let mut attachments = Vec::new();
    if let Some(source) = &source {
        if draft.include_quote {
            html.push_str(&quoted_html(&draft.kind, source));
        }
        if draft.kind != "forward" {
            references = source.refs.split_whitespace().map(str::to_string).collect();
            references.push(source.message_id.clone());
            in_reply_to = Some(source.message_id.clone());
        } else if draft.forward_attachments
            && let Ok(raw) = tokio::fs::read(state.raw_path(source.account_id, source.id)).await
            && let Some(message) = MessageParser::default().parse(&raw)
        {
            for part in message.attachments() {
                let mime = part
                    .content_type()
                    .map(|ct| format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or("octet-stream")))
                    .unwrap_or_else(|| "application/octet-stream".into());
                attachments.push(OutgoingAttachment {
                    filename: part.attachment_name().unwrap_or("attachment").to_string(),
                    mime,
                    data: part.contents().to_vec(),
                });
            }
        }
    }
    for att in draft_attachments(&state, id).await? {
        attachments.push(OutgoingAttachment {
            data: tokio::fs::read(&att.path).await?,
            filename: att.filename,
            mime: att.mime,
        });
    }

    let message = smtp::build(Outgoing {
        from,
        to,
        cc,
        bcc,
        subject: draft.subject.clone(),
        html,
        in_reply_to,
        references,
        attachments,
    })
    .map_err(|e| ApiError::bad_request(format!("cannot build message: {e}")))?;
    let raw = message.formatted();
    let smtp = SmtpParams::for_account(&account, &state.config.master_key)?;

    // Everything is checked and the mail is built. It goes out once sending can no longer be
    // undone; until then it is neither sent nor a draft to edit. `sent_at` is written with the
    // claim, before anything is handed to the mail server: a process that dies while sending
    // leaves the mark behind, so the mail is never quietly sent a second time.
    let claimed = sqlx::query(
        "UPDATE drafts SET sending_at = ?, sent_at = ?, last_error = NULL
         WHERE id = ? AND sending_at IS NULL",
    )
    .bind(now())
    .bind(now())
    .bind(id)
    .execute(&state.db)
    .await?;
    if claimed.rows_affected() == 0 {
        return Err(ApiError::bad_request("this mail is being sent already"));
    }
    state.notify(user.id, "mail");
    let task = state.clone();
    let user_id = user.id;
    let token = crate::undo::hold(&state, user_id, vec![crate::undo::Step::Draft(id)], async move {
        if let Err(e) = smtp.send(message).await {
            // The mail server refused it: it is a draft again, with the reason, and the mark
            // that it may be out is taken back.
            tracing::warn!("sending draft {id} failed: {e:#}");
            let kept = sqlx::query("UPDATE drafts SET sending_at = NULL, sent_at = NULL, last_error = ? WHERE id = ?")
                .bind(format!("{e:#}"))
                .bind(id)
                .execute(&task.db)
                .await;
            if let Err(e) = kept {
                tracing::error!("draft {id} could not be put back: {e:#}");
            }
            task.notify(user_id, "send_failed");
            return;
        }
        sent(&task, &account, id, raw).await;
        task.notify(user_id, "mail");
    });
    Ok(Json(json!({ "ok": true, "undo": token })))
}

/// Bookkeeping after a mail went out: none of it may undo the fact that it was sent.
async fn sent(state: &AppState, account: &Account, draft: i64, raw: Vec<u8>) {
    let sent_name = if account.sent_folder.is_empty() {
        LOCAL_SENT
    } else {
        account.sent_folder.as_str()
    };
    match store::ensure_folder(&state.db, account.id, sent_name, "sent").await {
        Ok(folder) => {
            if let Err(e) = store::store_message(state, account, &folder, None, true, false, &raw).await {
                tracing::warn!("sent message not stored locally: {e:#}");
            }
        }
        Err(e) => tracing::warn!("sent folder unavailable: {e:#}"),
    }
    if account.append_sent && !account.sent_folder.is_empty() {
        let folder = account.sent_folder.clone();
        sync::spawn_action(state, account.id, "saving to Sent", move |mut session| async move {
            session.append(&folder, Some("(\\Seen)"), None, &raw).await?;
            let _ = session.logout().await;
            Ok(())
        });
    }
    if let Err(e) = remove_draft(state, draft).await {
        tracing::warn!("draft {draft} not removed: {e:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_prefix_is_not_doubled() {
        assert_eq!(with_prefix("Re:", "Hello"), "Re: Hello");
        assert_eq!(with_prefix("Re:", "RE: Hello"), "RE: Hello");
        assert_eq!(with_prefix("Fwd:", "Re: Hello"), "Fwd: Re: Hello");
    }

    #[test]
    fn quote_escapes_headers() {
        let source = Source {
            id: 1,
            account_id: 1,
            message_id: "a@b".into(),
            refs: String::new(),
            from_name: "<b>Eve</b>".into(),
            from_addr: "eve@example.com".into(),
            to_addrs: "[]".into(),
            cc_addrs: "[]".into(),
            subject: "Hi <script>".into(),
            date: 0,
            is_outgoing: false,
            body_text: "1 < 2".into(),
            body_html: String::new(),
        };
        let reply = quoted_html("reply", &source);
        assert!(reply.contains("&lt;b&gt;Eve&lt;/b&gt;"));
        assert!(reply.contains("1 &lt; 2"));
        assert!(reply.contains("<blockquote"));
        let forward = quoted_html("forward", &source);
        assert!(forward.contains("Forwarded message"));
        assert!(forward.contains("Hi &lt;script&gt;"));
    }
}
