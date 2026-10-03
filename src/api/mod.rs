mod accounts;
mod auth;
pub(crate) mod autoarchive;
mod bulk;
mod compose;
mod files;
mod mail;
mod saved;

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
        .route("/settings", get(auth::settings).put(auth::update_settings))
        .route("/settings/auto-archive", put(autoarchive::update))
        .route("/settings/auto-archive/preview", get(autoarchive::preview))
        .route("/users", get(auth::list_users).post(auth::create_user))
        .route("/users/{id}", delete(auth::delete_user))
        .route("/accounts", get(accounts::list).post(accounts::create))
        .route("/accounts/test", post(accounts::test))
        .route("/accounts/{id}", put(accounts::update).delete(accounts::delete))
        .route("/accounts/{id}/folders", get(accounts::folders))
        .route("/mail/actions", post(bulk::apply))
        .route("/screener", get(mail::screener))
        .route("/senders", get(mail::senders))
        .route("/senders/{id}/category", post(mail::set_category))
        .route("/senders/{id}/images", post(mail::set_images))
        .route("/contacts", get(mail::contacts))
        .route("/avatar", get(mail::avatar))
        .route("/counts", get(mail::counts))
        .route("/threads", get(mail::threads))
        .route("/threads/{id}", get(mail::thread))
        .route("/threads/{id}/seen", post(mail::mark_seen))
        .route("/feed", get(mail::feed))
        .route("/messages/{id}", get(mail::message))
        .route("/messages/{id}/attachments/{idx}", get(mail::attachment))
        .route("/messages/{id}/cid/{cid}", get(mail::inline_image))
        .route("/attachments", get(files::list))
        .route("/messages/{id}/attachments/{idx}/thumb", get(files::thumb))
        .route("/messages/{id}/attachments/{idx}/view", get(files::view))
        .route("/messages/{id}/attachments/{idx}/pdf", get(files::pdf))
        .route("/search", get(mail::search))
        .route("/searches", get(saved::list).post(saved::create))
        .route("/searches/{id}", put(saved::update).delete(saved::delete))
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
        test_app_with(false).await
    }

    /// `avatars`: whether sender pictures may be looked up (tests never reach the internet:
    /// they only use what is already in the cache).
    async fn test_app_with(avatars: bool) -> (Router, AppState) {
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
            soffice: None,
            avatars,
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

        // Remembering "show images" is per sender and only for the owner.
        let images = format!("/api/senders/{sender}/images");
        let show = json!({ "show": true });
        let (status, _, _) = call(&app, "POST", &images, Some(&bob_cookie), Some(show.clone())).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let thread_path = format!("/api/threads/{thread}");
        let (_, _, before) = call(&app, "GET", &thread_path, Some(&admin_cookie), None).await;
        assert_eq!(before["messages"][0]["show_images"], false);
        let (status, _, _) = call(&app, "POST", &images, Some(&admin_cookie), Some(show)).await;
        assert_eq!(status, StatusCode::OK);
        let (_, _, after) = call(&app, "GET", &thread_path, Some(&admin_cookie), None).await;
        assert_eq!(after["messages"][0]["show_images"], true);
        let hide = json!({ "show": false });
        call(&app, "POST", &images, Some(&admin_cookie), Some(hide)).await;
        let (_, _, reverted) = call(&app, "GET", &thread_path, Some(&admin_cookie), None).await;
        assert_eq!(reverted["messages"][0]["show_images"], false);

        let category: Option<String> = sqlx::query_scalar("SELECT category FROM senders WHERE id = ?")
            .bind(sender)
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(category, None);
    }

    #[tokio::test]
    async fn attachments_page_lists_recent_files_from_accepted_senders_only() {
        use crate::mail::smtp::{self, Outgoing, OutgoingAttachment};

        let (app, state) = test_app().await;
        let admin = json!({ "email": "admin@example.org", "password": "password1" });
        let (_, cookie, _) = call(&app, "POST", "/api/register", None, Some(admin)).await;
        let cookie = cookie.unwrap();
        let bob = json!({ "email": "bob@example.org", "password": "password2" });
        call(&app, "POST", "/api/users", Some(&cookie), Some(bob.clone())).await;
        let (_, bob_cookie, _) = call(&app, "POST", "/api/login", None, Some(bob)).await;
        let bob_cookie = bob_cookie.unwrap();

        // seed_mail's message is from 1970: far outside the four weeks.
        let (_, old_message, anna) = seed_mail(&state, 1).await;
        let db = &state.db;
        sqlx::query("UPDATE senders SET category = 'important' WHERE id = ?")
            .bind(anna)
            .execute(db)
            .await
            .unwrap();
        let junk: i64 = sqlx::query_scalar(
            "INSERT INTO senders (user_id, address, category) VALUES (1, 'spam@example.com', 'junk') RETURNING id",
        )
        .fetch_one(db)
        .await
        .unwrap();
        let waiting: i64 =
            sqlx::query_scalar("INSERT INTO senders (user_id, address) VALUES (1, 'new@example.com') RETURNING id")
                .fetch_one(db)
                .await
                .unwrap();

        let mut recent = Vec::new();
        for (n, sender) in [anna, junk, waiting].into_iter().enumerate() {
            let id: i64 = sqlx::query_scalar(
                "INSERT INTO messages (user_id, account_id, folder_id, uid, thread_id, sender_id, message_id,
                     from_addr, subject, date)
                 VALUES (1, 1, 1, ?, 1, ?, ?, 'x@example.com', 'Files', ?) RETURNING id",
            )
            .bind(10 + n as i64)
            .bind(sender)
            .bind(format!("recent{n}@example.com"))
            .bind(crate::state::now() - 3600)
            .fetch_one(db)
            .await
            .unwrap();
            recent.push(id);
        }
        let attach = async |message: i64, idx: i64, name: &str, inline: bool| {
            sqlx::query(
                "INSERT INTO attachments (message_id, idx, filename, mime, size, content_id, inline)
                 VALUES (?, ?, ?, 'image/png', 10, ?, ?)",
            )
            .bind(message)
            .bind(idx)
            .bind(name)
            .bind(inline.then_some("cid1"))
            .bind(inline)
            .execute(db)
            .await
            .unwrap();
        };
        attach(recent[0], 0, "photo.png", false).await;
        attach(recent[0], 1, "inline-logo.png", true).await;
        attach(recent[1], 0, "junk.png", false).await;
        attach(recent[2], 0, "unscreened.png", false).await;
        attach(old_message, 0, "old.png", false).await;

        // A real raw message behind photo.png, so previews can be made from it.
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbImage::from_pixel(900, 300, image::Rgb([10, 120, 200]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let raw = smtp::build(Outgoing {
            from: "anna@example.com".parse().unwrap(),
            to: smtp::parse_recipients("owner@example.org").unwrap(),
            cc: vec![],
            bcc: vec![],
            subject: "Files".into(),
            html: "<p>see attached</p>".into(),
            in_reply_to: None,
            references: vec![],
            attachments: vec![OutgoingAttachment {
                filename: "photo.png".into(),
                mime: "image/png".into(),
                data: png.into_inner(),
            }],
        })
        .unwrap()
        .formatted();
        let path = state.raw_path(1, recent[0]);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, raw).unwrap();

        let (status, _, body) = call(&app, "GET", "/api/attachments", Some(&cookie), None).await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<&str> = body["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["filename"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["photo.png"]);
        assert_eq!(body["files"][0]["kind"], "image");
        assert_eq!(body["office_previews"], false);

        let base = format!("/api/messages/{}/attachments/0", recent[0]);
        let (status, _, _) = call(&app, "GET", &format!("{base}/thumb"), Some(&cookie), None).await;
        assert_eq!(status, StatusCode::OK);
        let cached = state.preview_dir(1, recent[0]).join("0.jpg");
        let thumb = image::load_from_memory(&std::fs::read(cached).unwrap()).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (480, 160));
        let (status, _, _) = call(&app, "GET", &format!("{base}/view"), Some(&cookie), None).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _, _) = call(&app, "GET", &format!("{base}/pdf"), Some(&cookie), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // Another user gets none of it.
        let (_, _, body) = call(&app, "GET", "/api/attachments", Some(&bob_cookie), None).await;
        assert_eq!(body["files"], json!([]));
        for suffix in ["thumb", "view", "pdf"] {
            let (status, _, _) = call(&app, "GET", &format!("{base}/{suffix}"), Some(&bob_cookie), None).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{suffix}");
        }
    }

    #[tokio::test]
    async fn bulk_actions_change_only_the_users_own_mail() {
        let (app, state) = test_app().await;
        let admin = json!({ "email": "admin@example.org", "password": "password1" });
        let (_, cookie, _) = call(&app, "POST", "/api/register", None, Some(admin)).await;
        let cookie = cookie.unwrap();
        let bob = json!({ "email": "bob@example.org", "password": "password2" });
        call(&app, "POST", "/api/users", Some(&cookie), Some(bob.clone())).await;
        let (_, bob_cookie, _) = call(&app, "POST", "/api/login", None, Some(bob)).await;
        let bob_cookie = bob_cookie.unwrap();

        let (thread, message, _) = seed_mail(&state, 1).await;
        let db = &state.db;
        let seen = async || -> bool {
            sqlx::query_scalar("SELECT seen FROM messages WHERE id = ?")
                .bind(message)
                .fetch_one(db)
                .await
                .unwrap()
        };
        let remaining = async || -> i64 {
            sqlx::query_scalar("SELECT COUNT(*) FROM messages")
                .fetch_one(db)
                .await
                .unwrap()
        };
        let act = async |who: &str, body: Value| call(&app, "POST", "/api/mail/actions", Some(who), Some(body)).await;

        // Another user's selection of this mail does nothing.
        let (status, _, body) = act(&bob_cookie, json!({ "action": "read", "thread_ids": [thread] })).await;
        assert_eq!((status, body["affected"].as_i64()), (StatusCode::OK, Some(0)));
        assert!(!seen().await);
        act(&bob_cookie, json!({ "action": "trash", "message_ids": [message] })).await;
        assert_eq!(remaining().await, 1);

        // Read and unread, by conversation and by single mail.
        let (_, _, body) = act(&cookie, json!({ "action": "read", "thread_ids": [thread] })).await;
        assert_eq!(body["affected"], 1);
        assert!(seen().await);
        act(&cookie, json!({ "action": "unread", "message_ids": [message] })).await;
        assert!(!seen().await);

        // No Trash folder known: refuse, and keep the mail.
        let (status, _, body) = act(&cookie, json!({ "action": "trash", "thread_ids": [thread] })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().contains("No Trash folder"));
        assert_eq!(remaining().await, 1);

        // Moving needs a folder and the account the mail belongs to.
        let (status, _, _) = act(
            &cookie,
            json!({ "action": "move", "thread_ids": [thread], "account_id": 1 }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let wrong_account = json!({ "action": "move", "thread_ids": [thread], "account_id": 99, "folder": "Archive" });
        let (status, _, _) = act(&cookie, wrong_account).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _, _) = act(&cookie, json!({ "action": "explode", "thread_ids": [thread] })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(remaining().await, 1);

        // With a Trash folder the mail moves into the mirrored Trash: out of every other list and
        // out of search, into the Trash list. (A mail server that accepts the connection and then
        // says nothing keeps the server-side move pending, so this can be checked without a race.)
        let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        sqlx::query(
            "UPDATE accounts SET trash_folder = 'Trash', imap_host = '127.0.0.1', imap_port = ?, password_enc = ?",
        )
        .bind(silent.local_addr().unwrap().port())
        .bind(crypto::encrypt(&state.config.master_key, "secret").unwrap())
        .execute(db)
        .await
        .unwrap();
        sqlx::query("UPDATE senders SET category = 'important'")
            .execute(db)
            .await
            .unwrap();
        let listed = async |path: &str| -> usize {
            call(&app, "GET", path, Some(&cookie), None)
                .await
                .2
                .as_array()
                .unwrap()
                .len()
        };
        assert_eq!(
            (
                listed("/api/threads?box=important").await,
                listed("/api/threads?box=trash").await
            ),
            (1, 0)
        );
        assert_eq!(listed("/api/search?q=confidential").await, 1);

        let (status, _, body) = act(&cookie, json!({ "action": "trash", "thread_ids": [thread] })).await;
        assert_eq!((status, body["affected"].as_i64()), (StatusCode::OK, Some(1)));
        assert_eq!(remaining().await, 1, "kept locally, in the Trash folder");
        let role: String =
            sqlx::query_scalar("SELECT f.role FROM messages m JOIN folders f ON f.id = m.folder_id WHERE m.id = ?")
                .bind(message)
                .fetch_one(db)
                .await
                .unwrap();
        assert_eq!(role, "trash");
        assert_eq!(
            (
                listed("/api/threads?box=important").await,
                listed("/api/threads?box=trash").await
            ),
            (0, 1)
        );
        assert_eq!(listed("/api/feed?box=trash").await, 1);
        assert_eq!(listed("/api/feed?box=important").await, 0);
        assert_eq!(
            listed("/api/search?q=confidential").await,
            0,
            "the Trash is not searched"
        );
        let (_, _, detail) = call(&app, "GET", &format!("/api/threads/{thread}"), Some(&cookie), None).await;
        assert_eq!(
            (detail["can_trash"].as_bool(), detail["can_archive"].as_bool()),
            (Some(false), Some(false))
        );
        // Trashing what is in the Trash already does nothing.
        let (_, _, body) = act(&cookie, json!({ "action": "trash", "thread_ids": [thread] })).await;
        assert_eq!(body["affected"], 0);

        // While the mail is still on its way to the Trash on the server it cannot be taken out.
        assert_eq!(detail["can_restore"], false);
        let (status, _, _) = act(&cookie, json!({ "action": "untrash", "thread_ids": [thread] })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        // The next sync finds it in the Trash folder; then it can go back to the Inbox list.
        sqlx::query("UPDATE messages SET uid = 77 WHERE id = ?")
            .bind(message)
            .execute(db)
            .await
            .unwrap();
        let (_, _, detail) = call(&app, "GET", &format!("/api/threads/{thread}"), Some(&cookie), None).await;
        assert_eq!(detail["can_restore"], true);
        let (_, _, body) = act(&cookie, json!({ "action": "untrash", "thread_ids": [thread] })).await;
        assert_eq!(body["affected"], 1);
        assert_eq!(
            (
                listed("/api/threads?box=important").await,
                listed("/api/threads?box=trash").await
            ),
            (1, 0)
        );
        let (_, _, detail) = call(&app, "GET", &format!("/api/threads/{thread}"), Some(&cookie), None).await;
        assert_eq!(
            (detail["can_trash"].as_bool(), detail["can_restore"].as_bool()),
            (Some(true), Some(false))
        );
        // Untrashing what is not in the Trash is refused.
        let (status, _, _) = act(&cookie, json!({ "action": "untrash", "thread_ids": [thread] })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn slide_actions_are_stored_per_user() {
        let (app, _state) = test_app().await;
        let admin = json!({ "email": "admin@example.org", "password": "password1" });
        let (_, cookie, _) = call(&app, "POST", "/api/register", None, Some(admin)).await;
        let cookie = cookie.unwrap();
        let bob = json!({ "email": "bob@example.org", "password": "password2" });
        call(&app, "POST", "/api/users", Some(&cookie), Some(bob.clone())).await;
        let (_, bob_cookie, _) = call(&app, "POST", "/api/login", None, Some(bob)).await;
        let bob_cookie = bob_cookie.unwrap();

        let (_, _, defaults) = call(&app, "GET", "/api/settings", Some(&cookie), None).await;
        assert_eq!(
            defaults,
            json!({ "swipe_left": ["trash"], "swipe_right": ["read"], "auto_archive_weeks": 0 })
        );

        // Order and repeats in the request do not matter; an empty list turns a direction off.
        let change = json!({ "swipe_left": ["trash", "move", "trash"], "swipe_right": [] });
        let (status, _, saved) = call(&app, "PUT", "/api/settings", Some(&cookie), Some(change)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            (&saved["swipe_left"], &saved["swipe_right"]),
            (&json!(["move", "trash"]), &json!([]))
        );
        let (_, _, again) = call(&app, "GET", "/api/settings", Some(&cookie), None).await;
        assert_eq!(
            (&again["swipe_left"], &again["swipe_right"]),
            (&saved["swipe_left"], &saved["swipe_right"])
        );

        let bad = json!({ "swipe_left": ["explode"], "swipe_right": [] });
        let (status, _, _) = call(&app, "PUT", "/api/settings", Some(&cookie), Some(bad)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (_, _, bobs) = call(&app, "GET", "/api/settings", Some(&bob_cookie), None).await;
        assert_eq!(bobs, defaults);
        let (status, _, _) = call(&app, "GET", "/api/settings", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn search_filters_narrow_the_results() {
        let (app, state) = test_app().await;
        let admin = json!({ "email": "admin@example.org", "password": "password1" });
        let (_, cookie, _) = call(&app, "POST", "/api/register", None, Some(admin)).await;
        let cookie = cookie.unwrap();
        // One mail: from anna@example.com, subject "Secret", body "confidential words", dated 1970.
        seed_mail(&state, 1).await;

        // (query as typed, URL-encoded by hand, expected hits)
        let cases = [
            ("confidential", "confidential", 1),
            ("from:anna", "from:anna", 1),
            ("from:ANNA@EXAMPLE", "from:ANNA@EXAMPLE", 1),
            ("from:bob", "from:bob", 0),
            ("confidential from:anna", "confidential%20from:anna", 1),
            ("confidential from:bob", "confidential%20from:bob", 0),
            ("subject:secr", "subject:secr", 1),
            ("title:secr", "title:secr", 1),
            ("subject:words", "subject:words", 0),
            ("attachment:false", "attachment:false", 1),
            ("attachment:true", "attachment:true", 0),
            ("received:..1971/01/01", "received:..1971/01/01", 1),
            ("received:01.01.1969..01.01.1971", "received:01.01.1969..01.01.1971", 1),
            ("received:2026/01/01..", "received:2026/01/01..", 0),
            ("received:last month", "received:last%20month", 0),
            ("from:100% (wildcards are literal)", "from:100%25", 0),
            ("from:_nna (wildcards are literal)", "from:_nna", 0),
            // The same filter twice is "either"; different filters are "both".
            ("from:bob from:anna", "from:bob%20from:anna", 1),
            ("from:bob from:carl", "from:bob%20from:carl", 0),
            ("subject:nothing title:secr", "subject:nothing%20title:secr", 1),
            ("from:anna subject:nothing", "from:anna%20subject:nothing", 0),
            (
                "from:bob from:anna subject:nothing subject:secr",
                "from:bob%20from:anna%20subject:nothing%20subject:secr",
                1,
            ),
            (
                "received:2026/01/01.. received:..1971/01/01",
                "received:2026/01/01..%20received:..1971/01/01",
                1,
            ),
            (
                "received:2026/01/01.. received:2025/01/01",
                "received:2026/01/01..%20received:2025/01/01",
                0,
            ),
            (
                "attachment:true attachment:false",
                "attachment:true%20attachment:false",
                1,
            ),
            (
                "confidential from:bob from:anna",
                "confidential%20from:bob%20from:anna",
                1,
            ),
        ];
        for (typed, encoded, expected) in cases {
            let path = format!("/api/search?q={encoded}");
            let (status, _, hits) = call(&app, "GET", &path, Some(&cookie), None).await;
            assert_eq!(status, StatusCode::OK, "{typed}");
            assert_eq!(hits.as_array().unwrap().len(), expected, "{typed}");
        }

        let (status, _, body) = call(&app, "GET", "/api/search?q=received:sometime", Some(&cookie), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().contains("received:sometime"));
    }

    #[tokio::test]
    async fn archiving_moves_a_conversation_to_the_archive_list() {
        let (app, state) = test_app().await;
        let admin = json!({ "email": "admin@example.org", "password": "password1" });
        let (_, cookie, _) = call(&app, "POST", "/api/register", None, Some(admin)).await;
        let cookie = cookie.unwrap();
        let (thread, message, sender) = seed_mail(&state, 1).await;
        let db = &state.db;
        sqlx::query("UPDATE senders SET category = 'important' WHERE id = ?")
            .bind(sender)
            .execute(db)
            .await
            .unwrap();
        let listed = async |mailbox: &str| -> usize {
            let path = format!("/api/threads?box={mailbox}");
            let (_, _, body) = call(&app, "GET", &path, Some(&cookie), None).await;
            body.as_array().unwrap().len()
        };
        let role = async || -> String {
            sqlx::query_scalar("SELECT f.role FROM messages m JOIN folders f ON f.id = m.folder_id WHERE m.id = ?")
                .bind(message)
                .fetch_one(db)
                .await
                .unwrap()
        };
        let archive = json!({ "action": "archive", "thread_ids": [thread] });
        assert_eq!((listed("important").await, listed("archive").await), (1, 0));

        // No Archive folder known: refuse and change nothing.
        let (status, _, body) = call(&app, "POST", "/api/mail/actions", Some(&cookie), Some(archive.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().contains("No Archive folder"));
        assert_eq!(role().await, "inbox");

        // A mail server that accepts the connection and then says nothing: the move stays pending,
        // so what the lists show right after archiving can be checked without a race.
        let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        sqlx::query(
            "UPDATE accounts SET archive_folder = 'Archive', imap_host = '127.0.0.1', imap_port = ?, password_enc = ?",
        )
        .bind(silent.local_addr().unwrap().port())
        .bind(crypto::encrypt(&state.config.master_key, "secret").unwrap())
        .execute(db)
        .await
        .unwrap();
        let (status, _, body) = call(&app, "POST", "/api/mail/actions", Some(&cookie), Some(archive.clone())).await;
        assert_eq!((status, body["affected"].as_i64()), (StatusCode::OK, Some(1)));
        assert_eq!(role().await, "archive");
        assert_eq!((listed("important").await, listed("archive").await), (0, 1));
        let (_, _, page) = call(&app, "GET", "/api/feed?box=archive", Some(&cookie), None).await;
        assert_eq!(page.as_array().unwrap().len(), 1);
        let (_, _, page) = call(&app, "GET", "/api/feed?box=important", Some(&cookie), None).await;
        assert_eq!(page.as_array().unwrap().len(), 0);
        // Archiving again is a no-op: the mail is no longer in the inbox.
        let (_, _, body) = call(&app, "POST", "/api/mail/actions", Some(&cookie), Some(archive)).await;
        assert_eq!(body["affected"], 0);
    }

    #[tokio::test]
    async fn archiving_is_undone_when_the_mail_server_cannot_be_reached() {
        let (app, state) = test_app().await;
        let admin = json!({ "email": "admin@example.org", "password": "password1" });
        let (_, cookie, _) = call(&app, "POST", "/api/register", None, Some(admin)).await;
        let cookie = cookie.unwrap();
        let (thread, message, _) = seed_mail(&state, 1).await;
        let db = &state.db;
        // A port nothing listens on: the connection is refused at once.
        let closed = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            listener.local_addr().unwrap().port()
        };
        sqlx::query(
            "UPDATE accounts SET archive_folder = 'Archive', imap_host = '127.0.0.1', imap_port = ?, password_enc = ?",
        )
        .bind(closed)
        .bind(crypto::encrypt(&state.config.master_key, "secret").unwrap())
        .execute(db)
        .await
        .unwrap();
        let archive = json!({ "action": "archive", "thread_ids": [thread] });
        let (status, _, _) = call(&app, "POST", "/api/mail/actions", Some(&cookie), Some(archive)).await;
        assert_eq!(status, StatusCode::OK);

        let mut state_now = (String::new(), None::<i64>);
        for _ in 0..100 {
            state_now = sqlx::query_as(
                "SELECT f.role, m.uid FROM messages m JOIN folders f ON f.id = m.folder_id WHERE m.id = ?",
            )
            .bind(message)
            .fetch_one(db)
            .await
            .unwrap();
            if state_now.0 == "inbox" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert_eq!(
            state_now,
            ("inbox".to_string(), Some(1)),
            "mail is back in the inbox with its UID"
        );
    }

    #[tokio::test]
    async fn saved_searches_are_per_user_and_count_unread_matches() {
        let (app, state) = test_app().await;
        let admin = json!({ "email": "admin@example.org", "password": "password1" });
        let (_, cookie, _) = call(&app, "POST", "/api/register", None, Some(admin)).await;
        let cookie = cookie.unwrap();
        let bob = json!({ "email": "bob@example.org", "password": "password2" });
        call(&app, "POST", "/api/users", Some(&cookie), Some(bob.clone())).await;
        let (_, bob_cookie, _) = call(&app, "POST", "/api/login", None, Some(bob)).await;
        let bob_cookie = bob_cookie.unwrap();
        // One unread mail from anna@example.com with subject "Secret".
        let (thread, _, _) = seed_mail(&state, 1).await;

        let save = async |who: &str, name: &str, query: &str| {
            let body = json!({ "name": name, "query": query });
            call(&app, "POST", "/api/searches", Some(who), Some(body)).await
        };
        let (status, _, saved) = save(&cookie, "  From Anna ", "from:anna").await;
        assert_eq!(status, StatusCode::OK);
        let id = saved["id"].as_i64().unwrap();
        save(&cookie, "Nobody", "from:nobody").await;
        save(&cookie, "words", "confidential subject:secret").await;

        // Refused: no name, nothing to search for, a date that cannot be read.
        for (name, query) in [("", "from:anna"), ("Empty", "   "), ("Bad date", "received:sometime")] {
            let (status, _, _) = save(&cookie, name, query).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{name:?} {query:?}");
        }

        // Listed by name, each with the unread mails it finds.
        let (_, _, list) = call(&app, "GET", "/api/searches", Some(&cookie), None).await;
        let summary: Vec<(&str, &str, i64)> = list
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                (
                    s["name"].as_str().unwrap(),
                    s["query"].as_str().unwrap(),
                    s["unread"].as_i64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("From Anna", "from:anna", 1),
                ("Nobody", "from:nobody", 0),
                ("words", "confidential subject:secret", 1)
            ]
        );

        // Reading the mail brings the counts down.
        let read = json!({ "action": "read", "thread_ids": [thread] });
        call(&app, "POST", "/api/mail/actions", Some(&cookie), Some(read)).await;
        let (_, _, list) = call(&app, "GET", "/api/searches", Some(&cookie), None).await;
        assert!(list.as_array().unwrap().iter().all(|s| s["unread"] == 0));

        // Another user sees none of them and cannot change them.
        let (_, _, bobs) = call(&app, "GET", "/api/searches", Some(&bob_cookie), None).await;
        assert_eq!(bobs, json!([]));
        let path = format!("/api/searches/{id}");
        let rename = json!({ "name": "Mine now", "query": "from:anna" });
        let (status, _, _) = call(&app, "PUT", &path, Some(&bob_cookie), Some(rename.clone())).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _, _) = call(&app, "DELETE", &path, Some(&bob_cookie), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // The owner can rename and remove.
        let (status, _, _) = call(&app, "PUT", &path, Some(&cookie), Some(rename)).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _, _) = call(&app, "DELETE", &path, Some(&cookie), None).await;
        assert_eq!(status, StatusCode::OK);
        let (_, _, list) = call(&app, "GET", "/api/searches", Some(&cookie), None).await;
        let names: Vec<&str> = list
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["Nobody", "words"]);
    }

    #[tokio::test]
    async fn important_and_delay_take_a_conversation_out_of_the_inbox() {
        let (app, state) = test_app().await;
        let admin = json!({ "email": "admin@example.org", "password": "password1" });
        let (_, cookie, _) = call(&app, "POST", "/api/register", None, Some(admin)).await;
        let cookie = cookie.unwrap();
        let bob = json!({ "email": "bob@example.org", "password": "password2" });
        call(&app, "POST", "/api/users", Some(&cookie), Some(bob.clone())).await;
        let (_, bob_cookie, _) = call(&app, "POST", "/api/login", None, Some(bob)).await;
        let bob_cookie = bob_cookie.unwrap();
        let (thread, message, sender) = seed_mail(&state, 1).await;
        let db = &state.db;
        sqlx::query("UPDATE senders SET category = 'important' WHERE id = ?")
            .bind(sender)
            .execute(db)
            .await
            .unwrap();

        // (inbox, important, delayed) list lengths
        let lists = async || -> (usize, usize, usize) {
            let mut lengths = Vec::new();
            for mailbox in ["important", "flagged", "delayed"] {
                let path = format!("/api/threads?box={mailbox}");
                let (_, _, body) = call(&app, "GET", &path, Some(&cookie), None).await;
                lengths.push(body.as_array().unwrap().len());
            }
            (lengths[0], lengths[1], lengths[2])
        };
        let act = async |who: &str, body: Value| call(&app, "POST", "/api/mail/actions", Some(who), Some(body)).await;
        let counts = async || call(&app, "GET", "/api/counts", Some(&cookie), None).await.2;
        assert_eq!(lists().await, (1, 0, 0));

        // Another user cannot move it.
        let (_, _, body) = act(&bob_cookie, json!({ "action": "important", "thread_ids": [thread] })).await;
        assert_eq!(body["affected"], 0);
        let (_, _, body) = act(
            &bob_cookie,
            json!({ "action": "delay", "thread_ids": [thread], "days": 1 }),
        )
        .await;
        assert_eq!(body["affected"], 0);
        assert_eq!(lists().await, (1, 0, 0));

        // Important: leaves the inbox, and the unread count goes with it.
        act(&cookie, json!({ "action": "important", "thread_ids": [thread] })).await;
        assert_eq!(lists().await, (0, 1, 0));
        let c = counts().await;
        assert_eq!(
            (c["unread_important"].as_i64(), c["unread_flagged"].as_i64()),
            (Some(0), Some(1))
        );
        let (_, _, detail) = call(&app, "GET", &format!("/api/threads/{thread}"), Some(&cookie), None).await;
        assert_eq!(detail["important"], true);
        let (_, _, page) = call(&app, "GET", "/api/feed?box=flagged", Some(&cookie), None).await;
        assert_eq!(page.as_array().unwrap().len(), 1);
        // And back, here by single mail as from a search result.
        act(&cookie, json!({ "action": "unimportant", "message_ids": [message] })).await;
        assert_eq!(lists().await, (1, 0, 0));

        // Delay: only the offered numbers of days.
        let (status, _, _) = act(&cookie, json!({ "action": "delay", "thread_ids": [thread], "days": 5 })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _, _) = act(&cookie, json!({ "action": "delay", "thread_ids": [thread] })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        act(&cookie, json!({ "action": "read", "thread_ids": [thread] })).await;
        act(&cookie, json!({ "action": "delay", "thread_ids": [thread], "days": 2 })).await;
        assert_eq!(lists().await, (0, 0, 1));
        assert_eq!(counts().await["delayed"], 1);
        let until: i64 = sqlx::query_scalar("SELECT snoozed_until FROM threads WHERE id = ?")
            .bind(thread)
            .fetch_one(db)
            .await
            .unwrap();
        let ahead = until - crate::state::now();
        assert!(ahead > 86_400 && ahead <= 3 * 86_400, "returns in {ahead} s");
        let (_, _, detail) = call(&app, "GET", &format!("/api/threads/{thread}"), Some(&cookie), None).await;
        assert_eq!(detail["snoozed_until"], until);

        // Brought back early: in the inbox again, still read.
        act(&cookie, json!({ "action": "undelay", "thread_ids": [thread] })).await;
        assert_eq!(lists().await, (1, 0, 0));
        assert_eq!(counts().await["unread_important"], 0);

        // The delay runs out: back in the inbox, unread, marked as returned.
        act(&cookie, json!({ "action": "delay", "thread_ids": [thread], "days": 1 })).await;
        assert_eq!(crate::mail::delay::wake_due(&state).await.unwrap(), 0, "not due yet");
        sqlx::query("UPDATE threads SET snoozed_until = ? WHERE id = ?")
            .bind(crate::state::now() - 5)
            .bind(thread)
            .execute(db)
            .await
            .unwrap();
        assert_eq!(crate::mail::delay::wake_due(&state).await.unwrap(), 1);
        assert_eq!(lists().await, (1, 0, 0));
        assert_eq!(counts().await["unread_important"], 1);
        let (snoozed, returned): (Option<i64>, Option<i64>) =
            sqlx::query_as("SELECT snoozed_until, returned_at FROM threads WHERE id = ?")
                .bind(thread)
                .fetch_one(db)
                .await
                .unwrap();
        assert!(snoozed.is_none() && returned.is_some());
        assert_eq!(
            crate::mail::delay::wake_due(&state).await.unwrap(),
            0,
            "returned only once"
        );
    }

    #[tokio::test]
    async fn sender_pictures_come_from_the_cache_and_only_for_known_senders() {
        let (app, state) = test_app_with(true).await;
        let admin = json!({ "email": "admin@example.org", "password": "password1" });
        let (_, cookie, _) = call(&app, "POST", "/api/register", None, Some(admin)).await;
        let cookie = cookie.unwrap();
        // A sender anna@example.com the user has mail from.
        seed_mail(&state, 1).await;
        let db = &state.db;
        let cache = async |key: &str, mime: Option<&str>, data: Option<&[u8]>| {
            sqlx::query("INSERT OR REPLACE INTO avatar_cache (key, mime, data, fetched_at) VALUES (?, ?, ?, ?)")
                .bind(key)
                .bind(mime)
                .bind(data)
                .bind(crate::state::now())
                .execute(db)
                .await
                .unwrap();
        };
        // Status, content type, whether the sandboxing policy is set, body.
        let get = async |address: &str, who: Option<&str>| {
            let mut request = Request::builder().uri(format!("/api/avatar?address={address}"));
            if let Some(cookie) = who {
                request = request.header(header::COOKIE, cookie);
            }
            let response = app.clone().oneshot(request.body(Body::empty()).unwrap()).await.unwrap();
            let status = response.status();
            let header_text = |name: header::HeaderName| {
                response
                    .headers()
                    .get(name)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .to_string()
            };
            let content_type = header_text(header::CONTENT_TYPE);
            let sandboxed = header_text(header::CONTENT_SECURITY_POLICY).contains("sandbox");
            let body = response.into_body().collect().await.unwrap().to_bytes().to_vec();
            (status, content_type, sandboxed, body)
        };

        // Nothing found at either source (both cached as missing): initials are used.
        cache("g:anna@example.com", None, None).await;
        cache("b:example.com", None, None).await;
        assert_eq!(get("anna@example.com", Some(&cookie)).await.0, StatusCode::NOT_FOUND);

        // The domain's BIMI logo: served as SVG, sandboxed.
        let logo = br#"<svg xmlns="http://www.w3.org/2000/svg"><circle r="4"/></svg>"#;
        cache("b:example.com", Some("image/svg+xml"), Some(logo)).await;
        let (status, content_type, sandboxed, body) = get("Anna@Example.com", Some(&cookie)).await;
        assert_eq!(
            (status, content_type.as_str(), sandboxed),
            (StatusCode::OK, "image/svg+xml", true)
        );
        assert_eq!(body, logo);

        // The person's own Gravatar wins over the domain's logo.
        cache("g:anna@example.com", Some("image/png"), Some(b"png-bytes")).await;
        let (status, content_type, _, body) = get("anna@example.com", Some(&cookie)).await;
        assert_eq!(
            (status, content_type.as_str(), body.as_slice()),
            (StatusCode::OK, "image/png", &b"png-bytes"[..])
        );

        // An address the user has no mail from is not looked up, cached or not.
        cache("g:stranger@example.com", Some("image/png"), Some(b"x")).await;
        assert_eq!(
            get("stranger@example.com", Some(&cookie)).await.0,
            StatusCode::NOT_FOUND
        );
        // Not without logging in.
        assert_eq!(get("anna@example.com", None).await.0, StatusCode::UNAUTHORIZED);

        // Switched off: no pictures at all.
        let (app_off, state_off) = test_app().await;
        let admin = json!({ "email": "admin@example.org", "password": "password1" });
        let (_, cookie_off, _) = call(&app_off, "POST", "/api/register", None, Some(admin)).await;
        seed_mail(&state_off, 1).await;
        sqlx::query("INSERT INTO avatar_cache (key, mime, data, fetched_at) VALUES ('g:anna@example.com', 'image/png', x'00', ?)")
            .bind(crate::state::now())
            .execute(&state_off.db)
            .await
            .unwrap();
        let request = Request::builder()
            .uri("/api/avatar?address=anna@example.com")
            .header(header::COOKIE, cookie_off.unwrap())
            .body(Body::empty())
            .unwrap();
        assert_eq!(app_off.oneshot(request).await.unwrap().status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn automatic_archive_takes_only_old_seen_inbox_conversations() {
        let (app, state) = test_app().await;
        let admin = json!({ "email": "admin@example.org", "password": "password1" });
        let (_, cookie, _) = call(&app, "POST", "/api/register", None, Some(admin)).await;
        let cookie = cookie.unwrap();
        // Account 1 with inbox folder 1, sender anna, and one very old unread mail.
        let (_, _, anna) = seed_mail(&state, 1).await;
        let db = &state.db;
        // Server moves stay pending: the mail server accepts the connection and says nothing.
        let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        sqlx::query(
            "UPDATE accounts SET archive_folder = 'Archive', imap_host = '127.0.0.1', imap_port = ?, password_enc = ?",
        )
        .bind(silent.local_addr().unwrap().port())
        .bind(crypto::encrypt(&state.config.master_key, "secret").unwrap())
        .execute(db)
        .await
        .unwrap();
        sqlx::query("UPDATE senders SET category = 'important' WHERE id = ?")
            .bind(anna)
            .execute(db)
            .await
            .unwrap();
        let news: i64 = sqlx::query_scalar(
            "INSERT INTO senders (user_id, address, category) VALUES (1, 'news@example.com', 'feed') RETURNING id",
        )
        .fetch_one(db)
        .await
        .unwrap();

        let week = 7 * 24 * 3600;
        let now = crate::state::now();
        // (subject, sender, age in weeks, seen, flagged, delayed)
        let mails = [
            ("old seen", anna, 10, true, false, false),
            ("old unseen", anna, 10, false, false, false),
            ("old important", anna, 10, true, true, false),
            ("old delayed", anna, 10, true, false, true),
            ("recent seen", anna, 1, true, false, false),
            ("old newsletter", news, 10, true, false, false),
        ];
        for (uid, (subject, sender, weeks, seen, flagged, delayed)) in mails.iter().enumerate() {
            let thread: i64 = sqlx::query_scalar(
                "INSERT INTO threads (user_id, subject, sender_id, snoozed_until) VALUES (1, ?, ?, ?) RETURNING id",
            )
            .bind(subject)
            .bind(sender)
            .bind(delayed.then_some(now + week))
            .fetch_one(db)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO messages (user_id, account_id, folder_id, uid, thread_id, sender_id, message_id,
                     from_addr, subject, date, seen, flagged)
                 VALUES (1, 1, 1, ?, ?, ?, ?, 'x@example.com', ?, ?, ?, ?)",
            )
            .bind(100 + uid as i64)
            .bind(thread)
            .bind(sender)
            .bind(format!("auto{uid}@example.com"))
            .bind(subject)
            .bind(now - weeks * week)
            .bind(seen)
            .bind(flagged)
            .execute(db)
            .await
            .unwrap();
        }
        let archived = async || -> Vec<String> {
            sqlx::query_scalar(
                "SELECT m.subject FROM messages m JOIN folders f ON f.id = m.folder_id
                 WHERE f.role = 'archive' ORDER BY m.subject",
            )
            .fetch_all(db)
            .await
            .unwrap()
        };
        let preview = async |weeks: i64| {
            let path = format!("/api/settings/auto-archive/preview?weeks={weeks}");
            call(&app, "GET", &path, Some(&cookie), None).await
        };

        // Off by default, and the preview says what a setting would do without doing it.
        let (_, _, settings) = call(&app, "GET", "/api/settings", Some(&cookie), None).await;
        assert_eq!(settings["auto_archive_weeks"], 0);
        assert_eq!(preview(4).await.2["count"], 1);
        assert_eq!(preview(20).await.2["count"], 0, "nothing seen is that old");
        assert_eq!(preview(0).await.2["count"], 0);
        assert_eq!(preview(-1).await.0, StatusCode::BAD_REQUEST);
        assert_eq!(preview(600).await.0, StatusCode::BAD_REQUEST);
        assert!(archived().await.is_empty());

        // Turning it on archives what is due at once: only the old, seen Inbox conversation.
        let (status, _, body) = call(
            &app,
            "PUT",
            "/api/settings/auto-archive",
            Some(&cookie),
            Some(json!({ "weeks": 4 })),
        )
        .await;
        assert_eq!((status, body["archived"].as_i64()), (StatusCode::OK, Some(1)));
        assert_eq!(archived().await, ["old seen"]);
        let (_, _, settings) = call(&app, "GET", "/api/settings", Some(&cookie), None).await;
        assert_eq!(settings["auto_archive_weeks"], 4);
        assert_eq!(preview(4).await.2["count"], 0, "nothing left to do");

        // A later run takes what has become due since: the recent mail, once it is old enough.
        sqlx::query("UPDATE messages SET date = ? WHERE subject = 'recent seen'")
            .bind(now - 5 * week)
            .execute(db)
            .await
            .unwrap();
        assert_eq!(crate::api::autoarchive::run_for_user(&state, 1, 4).await.unwrap(), 1);
        assert_eq!(archived().await, ["old seen", "recent seen"]);

        // Off again: nothing more is archived, whatever its age.
        let (status, _, body) = call(
            &app,
            "PUT",
            "/api/settings/auto-archive",
            Some(&cookie),
            Some(json!({ "weeks": 0 })),
        )
        .await;
        assert_eq!((status, body["archived"].as_i64()), (StatusCode::OK, Some(0)));
        sqlx::query("UPDATE messages SET seen = 1 WHERE subject = 'old unseen'")
            .execute(db)
            .await
            .unwrap();
        assert_eq!(crate::api::autoarchive::run_for_user(&state, 1, 0).await.unwrap(), 0);
        assert_eq!(archived().await, ["old seen", "recent seen"]);
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
        // Nice to know is a conversation list as well, and each list can be read on one page.
        let (_, _, feed_list) = call(&app, "GET", "/api/threads?box=feed", Some(&cookie), None).await;
        assert_eq!(count(feed_list), 1);
        for (mailbox, expected) in [("feed", 1), ("important", 0), ("junk", 0), ("sent", 0)] {
            let path = format!("/api/feed?box={mailbox}");
            let (status, _, page) = call(&app, "GET", &path, Some(&cookie), None).await;
            assert_eq!((status, count(page)), (StatusCode::OK, expected), "{mailbox}");
        }
        let (status, _, _) = call(&app, "GET", "/api/feed?box=bogus", Some(&cookie), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _, _) = call(&app, "POST", &path, Some(&cookie), Some(json!({ "category": "bogus" }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
