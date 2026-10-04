//! Taking things out of Portier and bringing them in: a user's own settings as one file, mail
//! as files other mail programs read, and (for the administrator) backups of the whole
//! installation.

use std::path::{Path as FilePath, PathBuf};

use axum::{
    Json,
    body::Body,
    extract::{Query, State},
    http::{
        HeaderMap, HeaderValue,
        header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS},
    },
};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::{
    api::{auth::SWIPE_ACTIONS, mail::box_condition},
    auth::{AdminUser, CurrentUser},
    backup,
    error::{ApiError, ApiResult},
    mail::{classify, preview, smtp::parse_recipients},
    models::CATEGORIES,
    state::{AppState, now},
};

/// The settings file may carry many pictures.
pub const MAX_IMPORT_BYTES: usize = 64 * 1024 * 1024;

type Download = (HeaderMap, Body);

/// Hands a file out as a download and removes it from the disk: it was made for this one answer.
async fn download(path: &FilePath, name: &str, mime: &'static str) -> ApiResult<Download> {
    let file = tokio::fs::File::open(path).await?;
    let size = file.metadata().await?.len();
    // The open file stays readable after its name is gone.
    let _ = tokio::fs::remove_file(path).await;
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(mime));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(CONTENT_LENGTH, HeaderValue::from(size));
    let disposition = format!("attachment; filename=\"{}\"", safe_name(name));
    headers.insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition).unwrap_or(HeaderValue::from_static("attachment")),
    );
    Ok((headers, Body::from_stream(tokio_util::io::ReaderStream::new(file))))
}

/// A file name made of harmless characters only.
fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._- ".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() {
        "mail".into()
    } else {
        cleaned.chars().take(80).collect()
    }
}

async fn temp_file(state: &AppState) -> ApiResult<PathBuf> {
    let dir = state.config.data_dir.join("tmp");
    tokio::fs::create_dir_all(&dir).await?;
    Ok(dir.join(crate::crypto::random_token()))
}

// ---- Mail as files -------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct MailQuery {
    /// One mail, as an `.eml` file.
    message: Option<i64>,
    /// Conversations by their ids, separated by commas, as one mbox file.
    threads: Option<String>,
    /// A whole list (`important`, `archive`, …), or `all` for every mail there is.
    #[serde(rename = "box")]
    mailbox: Option<String>,
}

/// One mail as it is written into an mbox file: a "From " line, the mail with Unix line ends
/// and its own "From " lines quoted, and an empty line.
fn mbox_entry(from: &str, date: i64, raw: &[u8]) -> Vec<u8> {
    let stamp = chrono::DateTime::from_timestamp(date, 0)
        .unwrap_or_default()
        .format("%a %b %e %H:%M:%S %Y");
    let sender = if from.is_empty() || from.contains(char::is_whitespace) {
        "MAILER-DAEMON"
    } else {
        from
    };
    let mut out = format!("From {sender} {stamp}\n").into_bytes();
    for line in raw.split_inclusive(|byte| *byte == b'\n') {
        let line = line
            .strip_suffix(b"\r\n")
            .or_else(|| line.strip_suffix(b"\n"))
            .unwrap_or(line);
        // "From " at the start of a line would look like the next mail; also when quoted already.
        let quoted = line.iter().take_while(|byte| **byte == b'>').count();
        if line[quoted..].starts_with(b"From ") {
            out.push(b'>');
        }
        out.extend_from_slice(line);
        out.push(b'\n');
    }
    out.push(b'\n');
    out
}

/// Mail of the signed-in user as a download: one mail as `.eml`, or conversations or a whole
/// list as an mbox file, which other mail programs can open and import.
pub async fn mail(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<MailQuery>,
) -> ApiResult<Download> {
    if let Some(id) = query.message {
        let (account, subject): (i64, String) =
            sqlx::query_as("SELECT account_id, subject FROM messages WHERE id = ? AND user_id = ?")
                .bind(id)
                .bind(user.id)
                .fetch_optional(&state.db)
                .await?
                .ok_or_else(ApiError::not_found)?;
        let raw = tokio::fs::read(state.raw_path(account, id))
            .await
            .map_err(|_| ApiError::not_found())?;
        let path = temp_file(&state).await?;
        tokio::fs::write(&path, raw).await?;
        return download(&path, &format!("{}.eml", safe_name(&subject)), "message/rfc822").await;
    }

    // Which conversations: the ones named, or those of a list.
    let (chosen, name) = match (&query.threads, query.mailbox.as_deref()) {
        (Some(ids), _) => {
            let ids: Vec<i64> = ids.split(',').filter_map(|id| id.trim().parse().ok()).collect();
            if ids.is_empty() || ids.len() > 1000 {
                return Err(ApiError::bad_request("name between 1 and 1000 conversations"));
            }
            let list = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
            (format!("m.thread_id IN ({list})"), "mail".to_string())
        }
        (None, Some("all")) => ("1".to_string(), "all-mail".to_string()),
        (None, Some(mailbox)) => (
            format!(
                "m.thread_id IN (SELECT t.id FROM threads t LEFT JOIN senders s ON s.id = t.sender_id
                                 WHERE t.user_id = ?1 AND {})",
                box_condition(mailbox)?
            ),
            match mailbox {
                "important" => "home",
                "flagged" => "important",
                "feed" => "nice-to-know",
                other => other,
            }
            .to_string(),
        ),
        (None, None) => return Err(ApiError::bad_request("say which mail to export")),
    };
    // Each mail once, although a copy of it may sit in several folders; oldest first, as mbox files are.
    let rows: Vec<(i64, i64, String, i64, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT m.id, m.account_id, m.from_addr, m.date, m.subject FROM messages m
         JOIN folders f ON f.id = m.folder_id
         WHERE m.user_id = ?1 AND f.role != 'limbo' AND {chosen}
           AND m.id IN (SELECT MIN(id) FROM messages WHERE user_id = ?1 GROUP BY account_id, message_id)
         ORDER BY m.date, m.id"
    )))
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    if rows.is_empty() {
        return Err(ApiError::bad_request("there is no mail to export"));
    }

    let path = temp_file(&state).await?;
    let mut file = tokio::fs::File::create(&path).await?;
    let mut written = 0;
    for (id, account, from, date, _) in &rows {
        // A mail whose stored copy is gone is left out.
        let Ok(raw) = tokio::fs::read(state.raw_path(*account, *id)).await else {
            continue;
        };
        file.write_all(&mbox_entry(from, *date, &raw)).await?;
        written += 1;
    }
    file.flush().await?;
    drop(file);
    if written == 0 {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(ApiError::bad_request("there is no mail to export"));
    }
    // A single conversation is named after its subject.
    let name = match (&query.threads, rows.first()) {
        (Some(ids), Some(first)) if !ids.contains(',') => safe_name(&first.4),
        _ => format!("portier-{name}-{}", chrono::Local::now().format("%Y-%m-%d")),
    };
    download(&path, &format!("{name}.mbox"), "application/mbox").await
}

// ---- A user's settings as a file -----------------------------------------------------------

#[derive(Serialize, Deserialize, Default)]
struct SenderEntry {
    address: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    show_images: bool,
    /// The user's own picture for the sender, base64.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    picture: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    picture_source: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct GroupEntry {
    name: String,
    members: String,
}

#[derive(Serialize, Deserialize)]
struct SearchEntry {
    name: String,
    query: String,
}

/// A note belongs to a conversation; it is found again by the Message-IDs of its mails.
#[derive(Serialize, Deserialize)]
struct NoteEntry {
    #[serde(default)]
    subject: String,
    note: String,
    message_ids: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct Preferences {
    #[serde(default)]
    swipe_left: Option<String>,
    #[serde(default)]
    swipe_right: Option<String>,
    #[serde(default)]
    auto_archive_weeks: Option<i64>,
}

#[derive(Serialize, Deserialize)]
pub struct SettingsFile {
    /// Marks the file and its version.
    portier_settings: i64,
    #[serde(default)]
    exported_at: i64,
    #[serde(default)]
    exported_by: String,
    #[serde(default)]
    senders: Vec<SenderEntry>,
    #[serde(default)]
    groups: Vec<GroupEntry>,
    #[serde(default)]
    saved_searches: Vec<SearchEntry>,
    #[serde(default)]
    notes: Vec<NoteEntry>,
    #[serde(default)]
    preferences: Preferences,
    /// What the browser keeps (the look); the web client fills it in and takes it out again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    browser: Option<Value>,
}

/// A sender as the database has it: address, name, category, whether pictures show, own picture and its source.
type SenderRow = (String, String, Option<String>, bool, Option<Vec<u8>>, Option<String>);

/// Everything the user set up in Portier, without the mail and without the mail accounts
/// (their passwords stay on this server).
pub async fn export_settings(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<SettingsFile>> {
    let db = &state.db;
    // Senders the user decided about or gave a picture; the rest is just who wrote once.
    let senders: Vec<SenderRow> = sqlx::query_as(
        "SELECT s.address, s.display_name, s.category, s.show_images, p.data, p.source
         FROM senders s LEFT JOIN sender_pictures p ON p.sender_id = s.id
         WHERE s.user_id = ? AND (s.category IS NOT NULL OR s.show_images = 1 OR p.sender_id IS NOT NULL)
         ORDER BY s.address",
    )
    .bind(user.id)
    .fetch_all(db)
    .await?;
    let groups: Vec<(String, String)> =
        sqlx::query_as("SELECT name, members FROM recipient_groups WHERE user_id = ? ORDER BY name")
            .bind(user.id)
            .fetch_all(db)
            .await?;
    let searches: Vec<(String, String)> =
        sqlx::query_as("SELECT name, query FROM saved_searches WHERE user_id = ? ORDER BY name COLLATE NOCASE, id")
            .bind(user.id)
            .fetch_all(db)
            .await?;
    let notes: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT t.subject, t.note,
                (SELECT json_group_array(DISTINCT m.message_id) FROM messages m
                 WHERE m.thread_id = t.id AND m.message_id != '')
         FROM threads t WHERE t.user_id = ? AND t.note != '' ORDER BY t.id",
    )
    .bind(user.id)
    .fetch_all(db)
    .await?;
    let (swipe_left, swipe_right, weeks): (String, String, i64) =
        sqlx::query_as("SELECT swipe_left, swipe_right, auto_archive_weeks FROM users WHERE id = ?")
            .bind(user.id)
            .fetch_one(db)
            .await?;
    Ok(Json(SettingsFile {
        portier_settings: 1,
        exported_at: now(),
        exported_by: user.email,
        senders: senders
            .into_iter()
            .map(
                |(address, display_name, category, show_images, picture, source)| SenderEntry {
                    address,
                    display_name,
                    category,
                    show_images,
                    picture: picture.map(|data| B64.encode(data)),
                    picture_source: source,
                },
            )
            .collect(),
        groups: groups
            .into_iter()
            .map(|(name, members)| GroupEntry { name, members })
            .collect(),
        saved_searches: searches
            .into_iter()
            .map(|(name, query)| SearchEntry { name, query })
            .collect(),
        notes: notes
            .into_iter()
            .map(|(subject, note, ids)| NoteEntry {
                subject,
                note,
                message_ids: serde_json::from_str(&ids).unwrap_or_default(),
            })
            .filter(|note| !note.message_ids.is_empty())
            .collect(),
        preferences: Preferences {
            swipe_left: Some(swipe_left),
            swipe_right: Some(swipe_right),
            auto_archive_weeks: Some(weeks),
        },
        browser: None,
    }))
}

/// Merges a settings file into what the user has: what the file names is set as the file says,
/// everything else stays. Returns what was taken over.
pub async fn import_settings(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(file): Json<SettingsFile>,
) -> ApiResult<Json<Value>> {
    if file.portier_settings != 1 {
        return Err(ApiError::bad_request(
            "this is not a Portier settings file this version can read",
        ));
    }
    let db = &state.db;
    let mut problems: Vec<String> = Vec::new();

    // Senders: the decision where their mail goes, whether pictures in their mail show, their picture.
    let (mut senders, mut pictures) = (0, 0);
    for entry in &file.senders {
        let address = entry.address.trim().to_lowercase();
        let category = entry.category.as_deref();
        if !address.contains('@') || category.is_some_and(|c| !CATEGORIES.contains(&c)) {
            problems.push(format!("sender {address}: not understood"));
            continue;
        }
        let known: Option<(i64, Option<String>)> =
            sqlx::query_as("SELECT id, category FROM senders WHERE user_id = ? AND address = ?")
                .bind(user.id)
                .bind(&address)
                .fetch_optional(db)
                .await?;
        let id = match known {
            Some((id, current)) => {
                // A decision in the file moves the sender's mail as deciding in the Screener does.
                if category.is_some()
                    && current.as_deref() != category
                    && let Err(error) = classify::set_category(&state, user.id, id, category).await
                {
                    problems.push(format!("sender {address}: {}", error.1));
                    continue;
                }
                id
            }
            // Nobody who wrote yet: the decision waits for their first mail.
            None => sqlx::query(
                "INSERT INTO senders (user_id, address, display_name, category, decided_at) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(user.id)
            .bind(&address)
            .bind(entry.display_name.trim())
            .bind(category)
            .bind(category.map(|_| now()))
            .execute(db)
            .await?
            .last_insert_rowid(),
        };
        if entry.show_images {
            sqlx::query("UPDATE senders SET show_images = 1 WHERE id = ?")
                .bind(id)
                .execute(db)
                .await?;
        }
        senders += 1;
        if let Some(picture) = &entry.picture {
            // Read as a picture again and made small: the file is only what somebody says it is.
            let small = match B64.decode(picture) {
                Ok(bytes) => tokio::task::spawn_blocking(move || preview::thumbnail(&bytes))
                    .await
                    .ok()
                    .and_then(Result::ok),
                Err(_) => None,
            };
            let Some(small) = small else {
                problems.push(format!("picture of {address}: not a picture"));
                continue;
            };
            sqlx::query(
                "INSERT INTO sender_pictures (sender_id, data, source, updated_at) VALUES (?, ?, ?, ?)
                 ON CONFLICT (sender_id) DO UPDATE SET data = excluded.data, source = excluded.source,
                     updated_at = excluded.updated_at",
            )
            .bind(id)
            .bind(small)
            .bind(entry.picture_source.as_deref().unwrap_or("upload"))
            .bind(now())
            .execute(db)
            .await?;
            pictures += 1;
        }
    }

    // Groups and saved searches: by name; one of the same name is replaced.
    let mut groups = 0;
    for entry in &file.groups {
        let name = entry.name.trim();
        let members: Vec<String> = parse_recipients(&entry.members)
            .map(|list| list.into_iter().map(|mailbox| mailbox.email.to_string()).collect())
            .unwrap_or_default();
        if name.is_empty()
            || name.chars().count() > 40
            || name.contains([',', ';', '@', '<', '>'])
            || members.is_empty()
        {
            problems.push(format!("group {name}: not understood"));
            continue;
        }
        sqlx::query(
            "INSERT INTO recipient_groups (user_id, name, members) VALUES (?, ?, ?)
             ON CONFLICT (user_id, name) DO UPDATE SET members = excluded.members",
        )
        .bind(user.id)
        .bind(name)
        .bind(members.join(", "))
        .execute(db)
        .await?;
        groups += 1;
    }
    let mut searches = 0;
    let today = chrono::Local::now().date_naive();
    for entry in &file.saved_searches {
        let (name, query) = (entry.name.trim(), entry.query.trim());
        if name.is_empty() || name.chars().count() > 40 || crate::search::parse(query, today).is_err() {
            problems.push(format!("saved search {name}: not understood"));
            continue;
        }
        let changed = sqlx::query("UPDATE saved_searches SET query = ? WHERE user_id = ? AND name = ?")
            .bind(query)
            .bind(user.id)
            .bind(name)
            .execute(db)
            .await?
            .rows_affected();
        if changed == 0 {
            sqlx::query("INSERT INTO saved_searches (user_id, name, query) VALUES (?, ?, ?)")
                .bind(user.id)
                .bind(name)
                .bind(query)
                .execute(db)
                .await?;
        }
        searches += 1;
    }

    // Notes: on the conversation that holds one of the mails the note was written on.
    let (mut notes, mut notes_without_mail) = (0, 0);
    for entry in &file.notes {
        let note: String = entry.note.trim().chars().take(2000).collect();
        if note.is_empty() {
            continue;
        }
        let mut thread: Option<i64> = None;
        for message_id in &entry.message_ids {
            thread = sqlx::query_scalar("SELECT thread_id FROM messages WHERE user_id = ? AND message_id = ? LIMIT 1")
                .bind(user.id)
                .bind(message_id)
                .fetch_optional(db)
                .await?;
            if thread.is_some() {
                break;
            }
        }
        match thread {
            Some(thread) => {
                sqlx::query("UPDATE threads SET note = ? WHERE id = ? AND user_id = ?")
                    .bind(&note)
                    .bind(thread)
                    .bind(user.id)
                    .execute(db)
                    .await?;
                notes += 1;
            }
            None => notes_without_mail += 1,
        }
    }

    // Preferences: only values this version knows.
    let known = |list: &str| !list.is_empty() && list.split(',').all(|action| SWIPE_ACTIONS.contains(&action));
    let preferences = &file.preferences;
    if let Some(left) = preferences.swipe_left.as_deref().filter(|list| known(list)) {
        sqlx::query("UPDATE users SET swipe_left = ? WHERE id = ?")
            .bind(left)
            .bind(user.id)
            .execute(db)
            .await?;
    }
    if let Some(right) = preferences.swipe_right.as_deref().filter(|list| known(list)) {
        sqlx::query("UPDATE users SET swipe_right = ? WHERE id = ?")
            .bind(right)
            .bind(user.id)
            .execute(db)
            .await?;
    }
    if let Some(weeks) = preferences.auto_archive_weeks.filter(|weeks| (0..=520).contains(weeks)) {
        sqlx::query("UPDATE users SET auto_archive_weeks = ? WHERE id = ?")
            .bind(weeks)
            .bind(user.id)
            .execute(db)
            .await?;
    }

    state.notify(user.id, "senders");
    state.notify(user.id, "mail");
    problems.truncate(20);
    Ok(Json(json!({
        "senders": senders,
        "pictures": pictures,
        "groups": groups,
        "saved_searches": searches,
        "notes": notes,
        "notes_without_mail": notes_without_mail,
        "problems": problems,
    })))
}

// ---- Backups of the whole installation (administrator) -------------------------------------

pub async fn backup_settings(State(state): State<AppState>, AdminUser(_): AdminUser) -> ApiResult<Json<Value>> {
    backup_state(&state).await
}

/// Where the daily backups go and how the last one went.
async fn backup_state(state: &AppState) -> ApiResult<Json<Value>> {
    let settings = backup::settings(&state.db).await?;
    Ok(Json(json!({
        "dir": settings.dir,
        "last_at": settings.last_at,
        "last_file": settings.last_file,
        "last_error": settings.last_error,
        "keep": backup::KEEP,
        "data_dir": std::path::absolute(&state.config.data_dir).unwrap_or_else(|_| state.config.data_dir.clone()),
    })))
}

#[derive(Deserialize)]
pub struct BackupFolder {
    dir: String,
}

/// Sets the folder the daily backups go to (empty: none) and, when one is set, writes the first
/// backup at once: that shows right away whether the folder can be written.
pub async fn set_backup_folder(
    State(state): State<AppState>,
    AdminUser(_): AdminUser,
    Json(input): Json<BackupFolder>,
) -> ApiResult<Json<Value>> {
    let dir = input.dir.trim();
    if !dir.is_empty() {
        let path = FilePath::new(dir);
        if !path.is_absolute() {
            return Err(ApiError::bad_request(
                "give the whole path of the folder, starting with /",
            ));
        }
        let data = std::path::absolute(&state.config.data_dir).unwrap_or_else(|_| state.config.data_dir.clone());
        if path.starts_with(data.join("mail")) || path.starts_with(data.join("drafts")) {
            return Err(ApiError::bad_request("choose a folder outside the stored mail"));
        }
    }
    sqlx::query("UPDATE backup_settings SET dir = ?, last_error = NULL WHERE id = 1")
        .bind(dir)
        .execute(&state.db)
        .await?;
    if !dir.is_empty()
        && let Err(error) = backup::write_to_folder(&state, false).await
    {
        return Err(ApiError::bad_request(format!(
            "the backup could not be written: {error:#}"
        )));
    }
    backup_state(&state).await
}

/// A backup made now, as a download.
pub async fn backup_download(State(state): State<AppState>, AdminUser(_): AdminUser) -> ApiResult<Download> {
    let path = temp_file(&state).await?;
    backup::write(&state.db, &state.config.data_dir, &path).await?;
    download(&path, &backup::file_name(true), "application/gzip").await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mbox_entries_have_unix_lines_and_quote_from_lines() {
        let raw = b"Subject: Hi\r\n\r\nFrom here on\r\n>From there\r\nfrom lower\r\nlast";
        let entry = String::from_utf8(mbox_entry("anna@example.org", 0, raw)).unwrap();
        assert_eq!(
            entry,
            "From anna@example.org Thu Jan  1 00:00:00 1970\nSubject: Hi\n\n>From here on\n>>From there\nfrom lower\nlast\n\n"
        );
        assert!(
            String::from_utf8(mbox_entry("", 0, b"x"))
                .unwrap()
                .starts_with("From MAILER-DAEMON ")
        );
    }

    #[test]
    fn file_names_are_harmless() {
        assert_eq!(safe_name("Re: Quote/2026 \"final\""), "Re_ Quote_2026 _final_");
        assert_eq!(safe_name("../.."), "_");
        assert_eq!(safe_name(""), "mail");
    }
}
