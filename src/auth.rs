use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use axum::{
    extract::FromRequestParts,
    http::{header::COOKIE, request::Parts},
};
use serde::Serialize;

use crate::{
    crypto,
    error::{ApiError, ApiResult},
    state::{AppState, now},
};

pub const COOKIE_NAME: &str = "emscreen_session";
/// The header a changing or expensive request carries the session's request token in.
pub const TOKEN_HEADER: &str = "x-portier-token";
const SESSION_SECONDS: i64 = 30 * 24 * 3600;

pub fn hash_password(password: &str) -> ApiResult<String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("password hashing failed: {e}").into())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct CurrentUser {
    pub id: i64,
    pub email: String,
    pub is_admin: bool,
}

pub struct AdminUser(pub CurrentUser);

pub fn session_token(parts: &Parts) -> Option<String> {
    parts
        .headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE_NAME)
        .map(|(_, value)| value.to_string())
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let token = session_token(parts).ok_or_else(ApiError::unauthorized)?;
        sqlx::query_as::<_, CurrentUser>(
            "SELECT u.id, u.email, u.is_admin FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.token = ? AND s.expires_at > ?",
        )
        .bind(token)
        .bind(now())
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(ApiError::unauthorized)
    }
}

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let user = CurrentUser::from_request_parts(parts, state).await?;
        if user.is_admin {
            Ok(AdminUser(user))
        } else {
            Err(ApiError::forbidden())
        }
    }
}

/// Creates a session row and returns the `Set-Cookie` value.
pub async fn create_session(state: &AppState, user_id: i64) -> ApiResult<String> {
    let token = crypto::random_token();
    sqlx::query("INSERT INTO sessions (token, user_id, expires_at) VALUES (?, ?, ?)")
        .bind(&token)
        .bind(user_id)
        .bind(now() + SESSION_SECONDS)
        .execute(&state.db)
        .await?;
    Ok(session_cookie(state, &token))
}

pub fn session_cookie(state: &AppState, token: &str) -> String {
    format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={SESSION_SECONDS}{}",
        secure(state)
    )
}

/// The request token of a session, made when it is first asked for.
pub async fn request_token(state: &AppState, session: &str) -> String {
    let mut tokens = state.tokens.lock().await;
    tokens
        .entry(session.to_string())
        .or_insert_with(crypto::random_token)
        .clone()
}

/// Ends every session of a user and forgets their request tokens.
/// Returns how many sessions were ended.
pub async fn end_sessions(state: &AppState, user_id: i64) -> ApiResult<u64> {
    let sessions: Vec<String> = sqlx::query_scalar("SELECT token FROM sessions WHERE user_id = ?")
        .bind(user_id)
        .fetch_all(&state.db)
        .await?;
    let ended = sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(user_id)
        .execute(&state.db)
        .await?
        .rows_affected();
    let mut tokens = state.tokens.lock().await;
    tokens.retain(|session, _| !sessions.iter().any(|gone| gone == session));
    Ok(ended)
}

/// Ends one session and forgets its request token.
pub async fn end_session(state: &AppState, session: &str) -> ApiResult<()> {
    sqlx::query("DELETE FROM sessions WHERE token = ?")
        .bind(session)
        .execute(&state.db)
        .await?;
    state.tokens.lock().await.remove(session);
    Ok(())
}

/// Whether a changing or expensive request carries the token of its session. A request without it
/// came from somewhere that does not know the page's token: a mail talking the browser into a
/// request, or another site. Downloads carry it in the address, since a link the browser follows
/// cannot set a header.
pub async fn request_allowed(state: &AppState, parts: &Parts) -> bool {
    let Some(session) = session_token(parts) else {
        return false;
    };
    let sent = parts
        .headers
        .get(TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .or_else(|| query_token(parts.uri.query()));
    let Some(sent) = sent else {
        return false;
    };
    request_token(state, &session).await == sent
}

/// The token from `?token=…`, for the downloads a page reaches through a plain link.
fn query_token(query: Option<&str>) -> Option<String> {
    query?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| *name == "token")
        .map(|(_, value)| value.to_string())
}

pub fn clear_cookie(state: &AppState) -> String {
    format!(
        "{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}",
        secure(state)
    )
}

/// Served over HTTPS, the session cookie is never sent over a plain connection. Where HTTPS is
/// added by a reverse proxy, nothing here can tell; `PORTIER_COOKIE_SECURE` says so.
fn secure(state: &AppState) -> &'static str {
    if state.config.tls.is_some() || state.config.cookie_secure {
        "; Secure"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trip() {
        let hash = hash_password("correct horse").unwrap();
        assert!(verify_password("correct horse", &hash));
        assert!(!verify_password("wrong", &hash));
        assert!(!verify_password("correct horse", "not a hash"));
    }
}
