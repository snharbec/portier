mod accounts;
mod auth;
mod compose;
mod mail;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{delete, get, post, put},
};

use crate::state::AppState;

const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me", get(auth::me))
        .route("/register", post(auth::register))
        .route("/login", post(auth::login))
        .route("/logout", post(auth::logout))
        .route("/password", post(auth::change_password))
        .route("/users", get(auth::list_users).post(auth::create_user))
        .route("/users/{id}", delete(auth::delete_user))
        .route("/accounts", get(accounts::list).post(accounts::create))
        .route("/accounts/test", post(accounts::test))
        .route("/accounts/{id}", put(accounts::update).delete(accounts::delete))
        .route("/screener", get(mail::screener))
        .route("/senders", get(mail::senders))
        .route("/senders/{id}/category", post(mail::set_category))
        .route("/contacts", get(mail::contacts))
        .route("/counts", get(mail::counts))
        .route("/threads", get(mail::threads))
        .route("/threads/{id}", get(mail::thread))
        .route("/threads/{id}/seen", post(mail::mark_seen))
        .route("/feed", get(mail::feed))
        .route("/messages/{id}", get(mail::message))
        .route("/messages/{id}/attachments/{idx}", get(mail::attachment))
        .route("/messages/{id}/cid/{cid}", get(mail::inline_image))
        .route("/search", get(mail::search))
        .route("/events", get(mail::events))
        .route("/drafts", get(compose::list).post(compose::create))
        .route(
            "/drafts/{id}",
            get(compose::get).put(compose::update).delete(compose::delete),
        )
        .route(
            "/drafts/{id}/attachments",
            post(compose::upload).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)),
        )
        .route(
            "/drafts/{id}/attachments/{attachment_id}",
            delete(compose::delete_attachment),
        )
        .route("/drafts/{id}/send", post(compose::send))
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode, header},
    };
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use crate::{config::Config, crypto, state::AppState};

    async fn test_app() -> (Router, AppState) {
        let data_dir = std::env::temp_dir().join(format!("emscreen-test-{}", crypto::random_token()));
        std::fs::create_dir_all(&data_dir).unwrap();
        let db = crate::open_database(&format!("sqlite://{}", data_dir.join("test.db").display()))
            .await
            .unwrap();
        let config = Config {
            bind: "127.0.0.1:0".parse().unwrap(),
            data_dir,
            master_key: crypto::random_bytes(),
            open_registration: false,
            sync_max_per_folder: 100,
        };
        let state = AppState::new(db, config);
        (crate::app(state.clone()), state)
    }

    /// Sends a request and returns status, session cookie (if one was set) and JSON body.
    async fn call(
        app: &Router,
        method: &str,
        path: &str,
        cookie: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Option<String>, Value) {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let request = match body {
            Some(json) => request
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json.to_string()))
                .unwrap(),
            None => request.body(Body::empty()).unwrap(),
        };
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(str::to_string);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, cookie, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// Gives `user_id` an account with one inbound message; returns (thread, message, sender).
    async fn seed_mail(state: &AppState, user_id: i64) -> (i64, i64, i64) {
        let db = &state.db;
        let account: i64 = sqlx::query_scalar(
            "INSERT INTO accounts (user_id, label, address, imap_host, imap_port, imap_security, imap_username,
                 smtp_host, smtp_port, smtp_security, smtp_username, password_enc)
             VALUES (?, 'A', 'owner@example.org', 'h', 1, 'none', 'u', 'h', 1, 'none', 'u', 'x') RETURNING id",
        )
        .bind(user_id)
        .fetch_one(db)
        .await
        .unwrap();
        let folder: i64 = sqlx::query_scalar(
            "INSERT INTO folders (account_id, name, role) VALUES (?, 'INBOX', 'inbox') RETURNING id",
        )
        .bind(account)
        .fetch_one(db)
        .await
        .unwrap();
        let sender: i64 =
            sqlx::query_scalar("INSERT INTO senders (user_id, address) VALUES (?, 'anna@example.com') RETURNING id")
                .bind(user_id)
                .fetch_one(db)
                .await
                .unwrap();
        let thread: i64 = sqlx::query_scalar(
            "INSERT INTO threads (user_id, subject, sender_id) VALUES (?, 'Secret', ?) RETURNING id",
        )
        .bind(user_id)
        .bind(sender)
        .fetch_one(db)
        .await
        .unwrap();
        let message: i64 = sqlx::query_scalar(
            "INSERT INTO messages (user_id, account_id, folder_id, uid, thread_id, sender_id, message_id, from_addr,
                 subject, date, body_text)
             VALUES (?, ?, ?, 1, ?, ?, 'm1@example.com', 'anna@example.com', 'Secret', 1, 'confidential words')
             RETURNING id",
        )
        .bind(user_id)
        .bind(account)
        .bind(folder)
        .bind(thread)
        .bind(sender)
        .fetch_one(db)
        .await
        .unwrap();
        (thread, message, sender)
    }

    #[tokio::test]
    async fn first_user_is_admin_and_registration_then_closes() {
        let (app, _state) = test_app().await;
        let creds = json!({ "email": "admin@example.org", "password": "password1" });

        let (status, _, body) = call(&app, "GET", "/api/me", None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["setup_needed"], true);

        let (status, cookie, _) = call(&app, "POST", "/api/register", None, Some(creds.clone())).await;
        assert_eq!(status, StatusCode::OK);
        let (_, _, me) = call(&app, "GET", "/api/me", cookie.as_deref(), None).await;
        assert_eq!(me["user"]["is_admin"], true);

        let other = json!({ "email": "eve@example.org", "password": "password2" });
        let (status, _, _) = call(&app, "POST", "/api/register", None, Some(other)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let wrong = json!({ "email": "admin@example.org", "password": "wrong-password" });
        let (status, cookie, _) = call(&app, "POST", "/api/login", None, Some(wrong)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(cookie.is_none());
        let (status, cookie, _) = call(&app, "POST", "/api/login", None, Some(creds)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(cookie.is_some());
    }

    #[tokio::test]
    async fn anonymous_requests_are_rejected() {
        let (app, _state) = test_app().await;
        for path in [
            "/api/accounts",
            "/api/screener",
            "/api/threads?box=important",
            "/api/feed",
            "/api/drafts",
            "/api/users",
        ] {
            let (status, _, _) = call(&app, "GET", path, None, None).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        }
    }

    #[tokio::test]
    async fn users_cannot_see_or_change_each_others_mail() {
        let (app, state) = test_app().await;
        let admin = json!({ "email": "admin@example.org", "password": "password1" });
        let (_, admin_cookie, _) = call(&app, "POST", "/api/register", None, Some(admin)).await;
        let admin_cookie = admin_cookie.unwrap();
        let bob = json!({ "email": "bob@example.org", "password": "password2" });
        let (status, _, _) = call(&app, "POST", "/api/users", Some(&admin_cookie), Some(bob.clone())).await;
        assert_eq!(status, StatusCode::OK);
        let (_, bob_cookie, _) = call(&app, "POST", "/api/login", None, Some(bob)).await;
        let bob_cookie = bob_cookie.unwrap();

        let (thread, message, sender) = seed_mail(&state, 1).await;

        // The owner sees the mail.
        let (_, _, screener) = call(&app, "GET", "/api/screener", Some(&admin_cookie), None).await;
        assert_eq!(screener.as_array().unwrap().len(), 1);
        let (status, _, _) = call(
            &app,
            "GET",
            &format!("/api/threads/{thread}"),
            Some(&admin_cookie),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, _, hits) = call(&app, "GET", "/api/search?q=confidential", Some(&admin_cookie), None).await;
        assert_eq!(hits.as_array().unwrap().len(), 1);

        // Bob sees nothing of it.
        for path in [
            "/api/accounts",
            "/api/screener",
            "/api/threads?box=important",
            "/api/feed",
            "/api/senders",
            "/api/search?q=confidential",
        ] {
            let (status, _, body) = call(&app, "GET", path, Some(&bob_cookie), None).await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert_eq!(body, json!([]), "{path}");
        }
        for path in [
            format!("/api/threads/{thread}"),
            format!("/api/messages/{message}"),
            format!("/api/messages/{message}/attachments/0"),
        ] {
            let (status, _, _) = call(&app, "GET", &path, Some(&bob_cookie), None).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        }

        // Nor can Bob act on it.
        let junk = json!({ "category": "junk" });
        let (status, _, _) = call(
            &app,
            "POST",
            &format!("/api/senders/{sender}/category"),
            Some(&bob_cookie),
            Some(junk),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let reply = json!({ "kind": "reply", "source_message": message });
        let (status, _, _) = call(&app, "POST", "/api/drafts", Some(&bob_cookie), Some(reply)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _, _) = call(&app, "DELETE", "/api/accounts/1", Some(&bob_cookie), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _, _) = call(&app, "GET", "/api/users", Some(&bob_cookie), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let category: Option<String> = sqlx::query_scalar("SELECT category FROM senders WHERE id = ?")
            .bind(sender)
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(category, None);
    }

    #[tokio::test]
    async fn classifying_moves_threads_between_views() {
        let (app, state) = test_app().await;
        let admin = json!({ "email": "admin@example.org", "password": "password1" });
        let (_, cookie, _) = call(&app, "POST", "/api/register", None, Some(admin)).await;
        let cookie = cookie.unwrap();
        let (_, _, sender) = seed_mail(&state, 1).await;
        let count = |body: Value| body.as_array().unwrap().len();
        let path = format!("/api/senders/{sender}/category");

        let (_, _, important) = call(&app, "GET", "/api/threads?box=important", Some(&cookie), None).await;
        assert_eq!(count(important), 0, "unscreened mail stays out of Important");

        call(
            &app,
            "POST",
            &path,
            Some(&cookie),
            Some(json!({ "category": "important" })),
        )
        .await;
        let (_, _, important) = call(&app, "GET", "/api/threads?box=important", Some(&cookie), None).await;
        let (_, _, screener) = call(&app, "GET", "/api/screener", Some(&cookie), None).await;
        assert_eq!((count(important), count(screener)), (1, 0));

        call(&app, "POST", &path, Some(&cookie), Some(json!({ "category": "feed" }))).await;
        let (_, _, important) = call(&app, "GET", "/api/threads?box=important", Some(&cookie), None).await;
        let (_, _, feed) = call(&app, "GET", "/api/feed", Some(&cookie), None).await;
        assert_eq!((count(important), count(feed)), (0, 1));

        let (status, _, _) = call(&app, "POST", &path, Some(&cookie), Some(json!({ "category": "bogus" }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
