use std::time::Duration;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, header::SET_COOKIE, request::Parts},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    auth::{self, AdminUser, CurrentUser, session_token},
    error::{ApiError, ApiResult},
    mail::sync,
    state::{AppState, now},
};

fn cookie_headers(cookie: String) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, cookie.parse().expect("cookie is a valid header value"));
    headers
}

async fn user_count(state: &AppState) -> ApiResult<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?)
}

pub async fn me(
    State(state): State<AppState>,
    parts: Parts,
    user: Result<CurrentUser, ApiError>,
) -> ApiResult<Json<Value>> {
    // The request token of this session, which the page sends with every request that changes
    // something or costs the server work. It is made when it is first asked for, and it is handed
    // out only here, to a caller that is signed in: a mail that talks the browser into a request
    // has no way to read this answer.
    let token = match (&user, session_token(&parts)) {
        (Ok(_), Some(session)) => Some(auth::request_token(&state, &session).await),
        _ => None,
    };
    Ok(Json(json!({
        "user": user.ok(),
        "setup_needed": user_count(&state).await? == 0,
        "open_registration": state.config.open_registration,
        "office_previews": state.config.soffice.is_some(),
        "token": token,
    })))
}

#[derive(Deserialize)]
pub struct Credentials {
    email: String,
    password: String,
}

fn validate(c: &Credentials) -> ApiResult<String> {
    let email = c.email.trim().to_lowercase();
    if !email.contains('@') {
        return Err(ApiError::bad_request("enter a valid email address"));
    }
    if c.password.chars().count() < 8 {
        return Err(ApiError::bad_request("password needs at least 8 characters"));
    }
    Ok(email)
}

async fn insert_user(state: &AppState, email: &str, password: &str, is_admin: bool) -> ApiResult<i64> {
    let hash = auth::hash_password(password)?;
    sqlx::query_scalar("INSERT INTO users (email, password_hash, is_admin) VALUES (?, ?, ?) RETURNING id")
        .bind(email)
        .bind(hash)
        .bind(is_admin)
        .fetch_one(&state.db)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                ApiError::bad_request("a user with this email already exists")
            }
            other => other.into(),
        })
}

/// First-run setup (the first user becomes admin) and, when enabled, open registration.
pub async fn register(
    State(state): State<AppState>,
    Json(input): Json<Credentials>,
) -> ApiResult<(HeaderMap, Json<Value>)> {
    let first = user_count(&state).await? == 0;
    if !first && !state.config.open_registration {
        return Err(ApiError::forbidden());
    }
    let email = validate(&input)?;
    let id = insert_user(&state, &email, &input.password, first).await?;
    let cookie = auth::create_session(&state, id).await?;
    Ok((cookie_headers(cookie), Json(json!({ "ok": true }))))
}

/// How long an address must wait after a failed sign-in, and how often it may try.
const LOGIN_ATTEMPTS: i64 = 8;
const LOGIN_WINDOW: i64 = 15 * 60;
/// The one answer that both a wrong password and an unknown address give.
const WRONG: &str = "wrong email or password";

/// A password hash of the same shape as a real one, made once. An address without a user is
/// verified against it, so that a wrong password and an unknown address cost the same work and
/// take the same time.
static DUMMY_HASH: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    auth::hash_password("a password that is nobody's").unwrap_or_else(|_| {
        "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQ$aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()
    })
});

/// Refuses, for a while, an address that has failed to sign in too often. The counters live in the
/// database, so they outlast a restart and are shared by everything on this installation.
async fn too_many_attempts(state: &AppState, address: &str) -> ApiResult<bool> {
    let since = now() - LOGIN_WINDOW;
    let failed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM login_failures WHERE address = ? AND at > ?")
        .bind(address)
        .bind(since)
        .fetch_one(&state.db)
        .await?;
    Ok(failed >= LOGIN_ATTEMPTS)
}

/// The failure path of a sign-in, for a wrong password and for an address that has no user alike.
/// Both end in the same answer and the same work, so the answer tells neither which addresses
/// exist nor which of them answer quickly.
async fn refuse_login(state: &AppState, address: &str) -> ApiError {
    sqlx::query("INSERT INTO login_failures (address, at) VALUES (?, ?)")
        .bind(address)
        .bind(now())
        .execute(&state.db)
        .await
        .ok();
    sqlx::query("DELETE FROM login_failures WHERE at < ?")
        .bind(now() - LOGIN_WINDOW)
        .execute(&state.db)
        .await
        .ok();
    ApiError::bad_request(WRONG)
}

pub async fn login(
    State(state): State<AppState>,
    Json(input): Json<Credentials>,
) -> ApiResult<(HeaderMap, Json<Value>)> {
    let address = input.email.trim().to_lowercase();
    if too_many_attempts(&state, &address).await? {
        return Err(ApiError(
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            "Too many attempts. Please wait a few minutes and try again.".into(),
        ));
    }
    let row: Option<(i64, String)> = sqlx::query_as("SELECT id, password_hash FROM users WHERE email = ?")
        .bind(&address)
        .fetch_optional(&state.db)
        .await?;
    // An address without a user is verified against a hash of the same shape, so that the two
    // costs — and so the answers — cannot be told apart.
    let hash = row
        .as_ref()
        .map(|(_, hash)| hash.clone())
        .unwrap_or_else(|| DUMMY_HASH.clone());
    let ok = auth::verify_password(&input.password, &hash);
    match row {
        Some((id, _)) if ok => {
            sqlx::query("DELETE FROM login_failures WHERE address = ?")
                .bind(&address)
                .execute(&state.db)
                .await?;
            let cookie = auth::create_session(&state, id).await?;
            Ok((cookie_headers(cookie), Json(json!({ "ok": true }))))
        }
        _ => {
            tokio::time::sleep(Duration::from_millis(500)).await;
            Err(refuse_login(&state, &address).await)
        }
    }
}

pub async fn logout(State(state): State<AppState>, parts: Parts) -> ApiResult<(HeaderMap, Json<Value>)> {
    if let Some(token) = auth::session_token(&parts) {
        auth::end_session(&state, &token).await?;
    }
    Ok((cookie_headers(auth::clear_cookie(&state)), Json(json!({ "ok": true }))))
}

#[derive(Deserialize)]
pub struct PasswordChange {
    current: String,
    new: String,
}

pub async fn change_password(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(input): Json<PasswordChange>,
) -> ApiResult<(HeaderMap, Json<Value>)> {
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    if !auth::verify_password(&input.current, &hash) {
        return Err(ApiError::bad_request("current password is wrong"));
    }
    if input.new.chars().count() < 8 {
        return Err(ApiError::bad_request("password needs at least 8 characters"));
    }
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(auth::hash_password(&input.new)?)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    // A password is changed because the old one may be known to somebody else. Every session the
    // user had ends with it, and this one goes on with a fresh session of its own, so that
    // whatever was signed in elsewhere is signed out. The new request token comes back with the
    // answer, so the page goes on working without a reload.
    auth::end_sessions(&state, user.id).await?;
    let cookie = auth::create_session(&state, user.id).await?;
    let token = auth::request_token(&state, &session_of(&cookie)).await;
    Ok((cookie_headers(cookie), Json(json!({ "ok": true, "token": token }))))
}

/// The session a `Set-Cookie` value carries.
fn session_of(cookie: &str) -> String {
    cookie
        .split(';')
        .next()
        .and_then(|pair| pair.trim().split_once('='))
        .map(|(_, value)| value.to_string())
        .unwrap_or_default()
}

#[derive(Serialize, sqlx::FromRow)]
pub struct UserRow {
    id: i64,
    email: String,
    is_admin: bool,
    created_at: i64,
}

pub async fn list_users(State(state): State<AppState>, _admin: AdminUser) -> ApiResult<Json<Vec<UserRow>>> {
    Ok(Json(
        sqlx::query_as("SELECT id, email, is_admin, created_at FROM users ORDER BY id")
            .fetch_all(&state.db)
            .await?,
    ))
}

pub async fn create_user(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(input): Json<Credentials>,
) -> ApiResult<Json<Value>> {
    let email = validate(&input)?;
    let id = insert_user(&state, &email, &input.password, false).await?;
    Ok(Json(json!({ "id": id })))
}

pub async fn delete_user(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    if id == admin.id {
        return Err(ApiError::bad_request("you cannot delete your own user"));
    }
    let accounts: Vec<i64> = sqlx::query_scalar("SELECT id FROM accounts WHERE user_id = ?")
        .bind(id)
        .fetch_all(&state.db)
        .await?;
    for account_id in &accounts {
        sync::stop(&state, *account_id).await;
    }
    let deleted = sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    for account_id in accounts {
        let _ = tokio::fs::remove_dir_all(state.config.data_dir.join("mail").join(account_id.to_string())).await;
    }
    Ok(Json(json!({ "ok": true })))
}

/// Actions a slide can be given, in the order they are offered.
pub(crate) const SWIPE_ACTIONS: [&str; 4] = ["read", "archive", "move", "trash"];

fn swipe_list(stored: &str) -> Vec<&str> {
    SWIPE_ACTIONS
        .into_iter()
        .filter(|action| stored.split(',').any(|s| s == *action))
        .collect()
}

pub async fn settings(State(state): State<AppState>, user: CurrentUser) -> ApiResult<Json<Value>> {
    let (left, right, weeks): (String, String, i64) =
        sqlx::query_as("SELECT swipe_left, swipe_right, auto_archive_weeks FROM users WHERE id = ?")
            .bind(user.id)
            .fetch_one(&state.db)
            .await?;
    Ok(Json(json!({
        "swipe_left": swipe_list(&left),
        "swipe_right": swipe_list(&right),
        "auto_archive_weeks": weeks,
    })))
}

#[derive(Deserialize)]
pub struct SettingsInput {
    swipe_left: Vec<String>,
    swipe_right: Vec<String>,
}

pub async fn update_settings(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(input): Json<SettingsInput>,
) -> ApiResult<Json<Value>> {
    let known = |list: &[String]| list.iter().all(|a| SWIPE_ACTIONS.contains(&a.as_str()));
    if !known(&input.swipe_left) || !known(&input.swipe_right) {
        return Err(ApiError::bad_request("unknown slide action"));
    }
    // Stored in the fixed offering order, without repeats.
    let (left, right) = (input.swipe_left.join(","), input.swipe_right.join(","));
    let (left, right) = (swipe_list(&left).join(","), swipe_list(&right).join(","));
    sqlx::query("UPDATE users SET swipe_left = ?, swipe_right = ? WHERE id = ?")
        .bind(&left)
        .bind(&right)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    Ok(Json(
        json!({ "swipe_left": swipe_list(&left), "swipe_right": swipe_list(&right) }),
    ))
}
