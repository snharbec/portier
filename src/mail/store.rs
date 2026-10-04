//! Writing fetched or sent messages into the database: sender lookup, threading, attachments.

use anyhow::Result;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::{
    mail::parse,
    models::{Account, Folder},
    state::{AppState, now},
};

pub struct Stored {
    /// Inbound message from a sender the user classified as junk.
    pub junk_sender: bool,
}

pub async fn ensure_folder(db: &SqlitePool, account_id: i64, name: &str, role: &str) -> Result<Folder> {
    Ok(sqlx::query_as::<_, Folder>(
        "INSERT INTO folders (account_id, name, role) VALUES (?, ?, ?)
         ON CONFLICT (account_id, name) DO UPDATE SET role = excluded.role
         RETURNING id, account_id, name, role, uidvalidity",
    )
    .bind(account_id)
    .bind(name)
    .bind(role)
    .fetch_one(db)
    .await?)
}

pub async fn folder_by_name(db: &SqlitePool, account_id: i64, name: &str) -> Result<Option<Folder>> {
    Ok(sqlx::query_as::<_, Folder>(
        "SELECT id, account_id, name, role, uidvalidity FROM folders WHERE account_id = ? AND name = ?",
    )
    .bind(account_id)
    .bind(name)
    .fetch_optional(db)
    .await?)
}

/// Finds the thread a message belongs to through its reply headers, in either direction.
async fn find_thread(db: &SqlitePool, user_id: i64, p: &parse::Parsed) -> Result<Option<i64>> {
    // Another copy of the same mail (in Sent and in the inbox, when you write to yourself).
    let copy: Option<i64> =
        sqlx::query_scalar("SELECT thread_id FROM messages WHERE user_id = ? AND message_id = ? LIMIT 1")
            .bind(user_id)
            .bind(&p.message_id)
            .fetch_optional(db)
            .await?;
    if copy.is_some() {
        return Ok(copy);
    }
    let mut ancestors = p.refs.clone();
    if !p.in_reply_to.is_empty() {
        ancestors.push(p.in_reply_to.clone());
    }
    if !ancestors.is_empty() {
        let found: Option<i64> = sqlx::query_scalar(
            "SELECT thread_id FROM messages
             WHERE user_id = ? AND message_id IN (SELECT value FROM json_each(?)) LIMIT 1",
        )
        .bind(user_id)
        .bind(serde_json::to_string(&ancestors)?)
        .fetch_optional(db)
        .await?;
        if found.is_some() {
            return Ok(found);
        }
    }
    // A reply may have been stored before the message it answers (sync runs newest first).
    Ok(
        sqlx::query_scalar("SELECT thread_id FROM messages WHERE user_id = ? AND in_reply_to = ? LIMIT 1")
            .bind(user_id)
            .bind(&p.message_id)
            .fetch_optional(db)
            .await?,
    )
}

/// Stores one message. `uid` is `None` for mail Portier sent itself and has not yet seen on the
/// server. Returns `None` if the bytes are not a parseable message.
pub async fn store_message(
    state: &AppState,
    account: &Account,
    folder: &Folder,
    uid: Option<u32>,
    seen: bool,
    flagged: bool,
    raw: &[u8],
) -> Result<Option<Stored>> {
    let Some(mut p) = parse::parse(raw) else {
        return Ok(None);
    };
    if p.message_id.is_empty() {
        // No Message-ID header: identify the message by its content, which stays the same when it
        // moves between folders.
        let digest = Sha256::digest(raw);
        let hex: String = digest.iter().take(16).map(|b| format!("{b:02x}")).collect();
        // The name in this identifier is the app's former one. It stays: mail stored earlier is
        // recognised by it.
        p.message_id = format!("{hex}@emscreen.local");
    }
    let db = &state.db;

    // A copy Portier already knows about (sent by it, or moved by it) now shows up on the server.
    if let Some(uid) = uid {
        let pending: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM messages WHERE folder_id = ? AND message_id = ? AND uid IS NULL LIMIT 1",
        )
        .bind(folder.id)
        .bind(&p.message_id)
        .fetch_optional(db)
        .await?;
        if let Some(id) = pending {
            sqlx::query("UPDATE messages SET uid = ?, seen = ?, flagged = ? WHERE id = ?")
                .bind(uid)
                .bind(seen)
                .bind(flagged)
                .bind(id)
                .execute(db)
                .await?;
            return Ok(Some(Stored { junk_sender: false }));
        }
        // The message is on its way to another folder (see `sync::move_messages`) and the server
        // has not caught up yet. Storing it here as well would duplicate it.
        let in_transit: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM messages m JOIN folders f ON f.id = m.folder_id
             WHERE m.account_id = ? AND m.message_id = ? AND m.uid IS NULL AND f.role != 'sent')",
        )
        .bind(account.id)
        .bind(&p.message_id)
        .fetch_one(db)
        .await?;
        if in_transit {
            return Ok(None);
        }
    }

    let own_address: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM accounts WHERE user_id = ? AND lower(address) = ?)")
            .bind(account.user_id)
            .bind(&p.from.address)
            .fetch_one(db)
            .await?;
    // Mail from yourself that arrives in the inbox was sent to yourself: it is received mail too.
    let to_self = own_address && folder.role == "inbox";
    let is_outgoing = folder.role == "sent" || (own_address && !to_self);

    let mut sender_id = None;
    let mut category: Option<String> = None;
    if !is_outgoing && !p.from.address.is_empty() {
        let row: (i64, Option<String>) = sqlx::query_as(
            "INSERT INTO senders (user_id, address, display_name) VALUES (?, ?, ?)
             ON CONFLICT (user_id, address) DO UPDATE SET display_name =
                 CASE WHEN excluded.display_name != '' THEN excluded.display_name ELSE display_name END
             RETURNING id, category",
        )
        .bind(account.user_id)
        .bind(&p.from.address)
        .bind(&p.from.name)
        .fetch_one(db)
        .await?;
        sender_id = Some(row.0);
        category = row.1;
        // Nobody needs to screen themselves.
        if to_self && category.is_none() {
            sqlx::query("UPDATE senders SET category = 'important', decided_at = unixepoch() WHERE id = ?")
                .bind(row.0)
                .execute(db)
                .await?;
            category = Some("important".to_string());
        }
    }

    let thread_id = match find_thread(db, account.user_id, &p).await? {
        Some(id) => {
            // A new mail in a delayed conversation ends the delay: the answer should not stay hidden.
            if !is_outgoing {
                sqlx::query("UPDATE threads SET snoozed_until = NULL WHERE id = ? AND snoozed_until IS NOT NULL")
                    .bind(id)
                    .execute(db)
                    .await?;
            }
            id
        }
        None => {
            sqlx::query_scalar("INSERT INTO threads (user_id, subject, sender_id) VALUES (?, ?, ?) RETURNING id")
                .bind(account.user_id)
                .bind(&p.subject)
                .bind(sender_id)
                .fetch_one(db)
                .await?
        }
    };

    let visible_attachments = p.attachments.iter().any(|a| !a.inline || a.content_id.is_none());
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO messages (user_id, account_id, folder_id, uid, thread_id, sender_id, message_id,
             in_reply_to, refs, from_name, from_addr, to_addrs, cc_addrs, subject, date, snippet, seen,
             is_outgoing, has_attachments, body_text, body_html, flagged)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(account.user_id)
    .bind(account.id)
    .bind(folder.id)
    .bind(uid)
    .bind(thread_id)
    .bind(sender_id)
    .bind(&p.message_id)
    .bind(&p.in_reply_to)
    .bind(p.refs.join(" "))
    .bind(&p.from.name)
    .bind(&p.from.address)
    .bind(serde_json::to_string(&p.to)?)
    .bind(serde_json::to_string(&p.cc)?)
    .bind(&p.subject)
    .bind(p.date.unwrap_or_else(now))
    .bind(&p.snippet)
    .bind(seen || is_outgoing)
    .bind(is_outgoing)
    .bind(visible_attachments)
    .bind(&p.body_text)
    .bind(&p.body_html)
    .bind(flagged)
    .fetch_one(db)
    .await?;

    for (idx, att) in p.attachments.iter().enumerate() {
        sqlx::query(
            "INSERT INTO attachments (message_id, idx, filename, mime, size, content_id, inline)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(idx as i64)
        .bind(&att.filename)
        .bind(&att.mime)
        .bind(att.size)
        .bind(&att.content_id)
        .bind(att.inline)
        .execute(db)
        .await?;
    }

    let path = state.raw_path(account.id, id);
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    tokio::fs::write(&path, raw).await?;

    Ok(Some(Stored {
        junk_sender: !is_outgoing && category.as_deref() == Some("junk"),
    }))
}

/// Deletes message rows and their raw files, then any thread left empty.
pub async fn delete_messages(state: &AppState, account: &Account, ids: &[i64]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    sqlx::query("DELETE FROM messages WHERE id IN (SELECT value FROM json_each(?))")
        .bind(serde_json::to_string(ids)?)
        .execute(&state.db)
        .await?;
    for id in ids {
        let _ = tokio::fs::remove_file(state.raw_path(account.id, *id)).await;
        let _ = tokio::fs::remove_dir_all(state.preview_dir(account.id, *id)).await;
    }
    sqlx::query(
        "DELETE FROM threads WHERE user_id = ?
         AND NOT EXISTS (SELECT 1 FROM messages WHERE messages.thread_id = threads.id)",
    )
    .bind(account.user_id)
    .execute(&state.db)
    .await?;
    Ok(())
}
