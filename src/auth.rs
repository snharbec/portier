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
    Ok(format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={SESSION_SECONDS}{}",
        secure(state)
    ))
}

pub fn clear_cookie(state: &AppState) -> String {
    format!(
        "{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}",
        secure(state)
    )
}

/// Served over HTTPS, the session cookie is never sent over a plain connection.
fn secure(state: &AppState) -> &'static str {
    if state.config.tls.is_some() { "; Secure" } else { "" }
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
