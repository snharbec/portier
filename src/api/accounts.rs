use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    auth::CurrentUser,
    crypto,
    error::{ApiError, ApiResult},
    mail::{
        imap::{self, ImapParams},
        smtp::SmtpParams,
        sync,
    },
    state::AppState,
};

#[derive(Serialize, sqlx::FromRow)]
pub struct AccountView {
    id: i64,
    label: String,
    address: String,
    display_name: String,
    imap_host: String,
    imap_port: i64,
    imap_security: String,
    imap_username: String,
    smtp_host: String,
    smtp_port: i64,
    smtp_security: String,
    smtp_username: String,
    inbox_folder: String,
    junk_folder: String,
    sent_folder: String,
    trash_folder: String,
    archive_folder: String,
    append_sent: bool,
    last_error: Option<String>,
    last_sync_at: Option<i64>,
}

const VIEW_COLUMNS: &str = "id, label, address, display_name, imap_host, imap_port, imap_security,
    imap_username, smtp_host, smtp_port, smtp_security, smtp_username, inbox_folder, junk_folder,
    sent_folder, trash_folder, archive_folder, append_sent, last_error, last_sync_at";

#[derive(Deserialize)]
pub struct AccountInput {
    /// Existing account whose stored password is used when `password` is empty.
    #[serde(default)]
    id: Option<i64>,
    label: String,
    address: String,
    #[serde(default)]
    display_name: String,
    imap_host: String,
    imap_port: u16,
    imap_security: String,
    imap_username: String,
    smtp_host: String,
    smtp_port: u16,
    smtp_security: String,
    smtp_username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    inbox_folder: String,
    #[serde(default)]
    junk_folder: String,
    #[serde(default)]
    sent_folder: String,
    #[serde(default)]
    trash_folder: String,
    #[serde(default)]
    archive_folder: String,
    #[serde(default = "default_true")]
    append_sent: bool,
}

fn default_true() -> bool {
    true
}

impl AccountInput {
    fn validate(&mut self) -> ApiResult<()> {
        for field in [
            &mut self.label,
            &mut self.address,
            &mut self.imap_host,
            &mut self.imap_username,
            &mut self.smtp_host,
            &mut self.smtp_username,
            &mut self.inbox_folder,
            &mut self.junk_folder,
            &mut self.sent_folder,
            &mut self.trash_folder,
            &mut self.archive_folder,
        ] {
            *field = field.trim().to_string();
        }
        self.address = self.address.to_lowercase();
        if self.inbox_folder.is_empty() {
            self.inbox_folder = "INBOX".into();
        }
        if self.label.is_empty() {
            self.label = self.address.clone();
        }
        if !self.address.contains('@') {
            return Err(ApiError::bad_request("enter a valid email address"));
        }
        if self.imap_host.is_empty() || self.smtp_host.is_empty() {
            return Err(ApiError::bad_request("IMAP and SMTP host are required"));
        }
        let modes = ["tls", "starttls", "none"];
        if !modes.contains(&self.imap_security.as_str()) || !modes.contains(&self.smtp_security.as_str()) {
            return Err(ApiError::bad_request("security must be tls, starttls or none"));
        }
        Ok(())
    }

    /// Password from the request, or the stored one of the user's existing account.
    async fn resolve_password(&self, state: &AppState, user_id: i64, existing: Option<i64>) -> ApiResult<String> {
        if !self.password.is_empty() {
            return Ok(self.password.clone());
        }
        let Some(id) = existing else {
            return Err(ApiError::bad_request("password is required"));
        };
        let enc: String = sqlx::query_scalar("SELECT password_enc FROM accounts WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(ApiError::not_found)?;
        Ok(crypto::decrypt(&state.config.master_key, &enc)?)
    }

    fn imap(&self, password: &str) -> ImapParams {
        ImapParams {
            host: self.imap_host.clone(),
            port: self.imap_port,
            security: self.imap_security.clone(),
            username: self.imap_username.clone(),
            password: password.to_string(),
        }
    }

    fn smtp(&self, password: &str) -> SmtpParams {
        SmtpParams {
            host: self.smtp_host.clone(),
            port: self.smtp_port,
            security: self.smtp_security.clone(),
            username: self.smtp_username.clone(),
            password: password.to_string(),
        }
    }
}

pub async fn list(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Vec<AccountView>>> {
    Ok(Json(
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {VIEW_COLUMNS} FROM accounts WHERE user_id = ? ORDER BY id"
        )))
        .bind(user.id)
        .fetch_all(&state.db)
        .await?,
    ))
}

/// Tries IMAP and SMTP with the given settings without saving anything.
pub async fn test(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(mut input): Json<AccountInput>,
) -> ApiResult<Json<Value>> {
    input.validate()?;
    let password = input.resolve_password(&state, user.id, input.id).await?;

    let mut folders = imap::SpecialFolders::default();
    let imap_error = match imap::connect(&input.imap(&password)).await {
        Ok(mut session) => {
            let discovered = imap::discover_folders(&mut session).await;
            let _ = session.logout().await;
            match discovered {
                Ok(found) => {
                    folders = found;
                    None
                }
                Err(e) => Some(format!("{e:#}")),
            }
        }
        Err(e) => Some(format!("{e:#}")),
    };
    let smtp_error = input.smtp(&password).test().await.err().map(|e| format!("{e:#}"));

    Ok(Json(json!({
        "ok": imap_error.is_none() && smtp_error.is_none(),
        "imap_error": imap_error,
        "smtp_error": smtp_error,
        "junk_folder": folders.junk,
        "sent_folder": folders.sent,
        "trash_folder": folders.trash,
        "archive_folder": folders.archive,
        "folders": folders.all,
    })))
}

async fn check_login(input: &AccountInput, password: &str) -> ApiResult<()> {
    let mut session = imap::connect(&input.imap(password))
        .await
        .map_err(|e| ApiError::bad_request(format!("IMAP: {e:#}")))?;
    let _ = session.logout().await;
    input
        .smtp(password)
        .test()
        .await
        .map_err(|e| ApiError::bad_request(format!("SMTP: {e:#}")))
}

pub async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(mut input): Json<AccountInput>,
) -> ApiResult<Json<Value>> {
    input.validate()?;
    let password = input.resolve_password(&state, user.id, None).await?;
    check_login(&input, &password).await?;

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO accounts (user_id, label, address, display_name, imap_host, imap_port, imap_security,
             imap_username, smtp_host, smtp_port, smtp_security, smtp_username, password_enc, inbox_folder,
             junk_folder, sent_folder, trash_folder, archive_folder, append_sent)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(user.id)
    .bind(&input.label)
    .bind(&input.address)
    .bind(&input.display_name)
    .bind(&input.imap_host)
    .bind(input.imap_port)
    .bind(&input.imap_security)
    .bind(&input.imap_username)
    .bind(&input.smtp_host)
    .bind(input.smtp_port)
    .bind(&input.smtp_security)
    .bind(&input.smtp_username)
    .bind(crypto::encrypt(&state.config.master_key, &password)?)
    .bind(&input.inbox_folder)
    .bind(&input.junk_folder)
    .bind(&input.sent_folder)
    .bind(&input.trash_folder)
    .bind(&input.archive_folder)
    .bind(input.append_sent)
    .fetch_one(&state.db)
    .await?;

    sync::start(&state, id).await;
    Ok(Json(json!({ "id": id })))
}

pub async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(mut input): Json<AccountInput>,
) -> ApiResult<Json<Value>> {
    input.validate()?;
    let password = input.resolve_password(&state, user.id, Some(id)).await?;
    check_login(&input, &password).await?;

    let result = sqlx::query(
        "UPDATE accounts SET label = ?, address = ?, display_name = ?, imap_host = ?, imap_port = ?,
             imap_security = ?, imap_username = ?, smtp_host = ?, smtp_port = ?, smtp_security = ?,
             smtp_username = ?, password_enc = ?, inbox_folder = ?, junk_folder = ?, sent_folder = ?,
             trash_folder = ?, archive_folder = ?, append_sent = ?, last_error = NULL
         WHERE id = ? AND user_id = ?",
    )
    .bind(&input.label)
    .bind(&input.address)
    .bind(&input.display_name)
    .bind(&input.imap_host)
    .bind(input.imap_port)
    .bind(&input.imap_security)
    .bind(&input.imap_username)
    .bind(&input.smtp_host)
    .bind(input.smtp_port)
    .bind(&input.smtp_security)
    .bind(&input.smtp_username)
    .bind(crypto::encrypt(&state.config.master_key, &password)?)
    .bind(&input.inbox_folder)
    .bind(&input.junk_folder)
    .bind(&input.sent_folder)
    .bind(&input.trash_folder)
    .bind(&input.archive_folder)
    .bind(input.append_sent)
    .bind(id)
    .bind(user.id)
    .execute(&state.db)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }

    sync::start(&state, id).await;
    Ok(Json(json!({ "id": id })))
}

/// The account's folders as the server lists them, for choosing where to move mail.
pub async fn folders(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let account: crate::models::Account = sqlx::query_as("SELECT * FROM accounts WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let params = ImapParams::for_account(&account, &state.config.master_key)?;
    let mut session = imap::connect(&params)
        .await
        .map_err(|e| ApiError(axum::http::StatusCode::BAD_GATEWAY, format!("{e:#}")))?;
    let found = imap::discover_folders(&mut session).await;
    let _ = session.logout().await;
    let mut folders = found
        .map_err(|e| ApiError(axum::http::StatusCode::BAD_GATEWAY, format!("{e:#}")))?
        .all;
    folders.sort_by_key(|name| name.to_lowercase());
    Ok(Json(json!({ "folders": folders })))
}

pub async fn delete(State(state): State<AppState>, user: CurrentUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let owned: Option<i64> = sqlx::query_scalar("SELECT id FROM accounts WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?;
    if owned.is_none() {
        return Err(ApiError::not_found());
    }
    sync::stop(&state, id).await;
    sqlx::query("DELETE FROM accounts WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    sqlx::query(
        "DELETE FROM threads WHERE user_id = ?
         AND NOT EXISTS (SELECT 1 FROM messages WHERE messages.thread_id = threads.id)",
    )
    .bind(user.id)
    .execute(&state.db)
    .await?;
    let _ = tokio::fs::remove_dir_all(state.config.data_dir.join("mail").join(id.to_string())).await;
    state.notify(user.id, "mail");
    Ok(Json(json!({ "ok": true })))
}
