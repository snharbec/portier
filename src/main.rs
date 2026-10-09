mod api;
mod assets;
mod auth;
mod avatar;
mod backup;
mod config;
mod crypto;
mod error;
mod mail;
mod models;
mod search;
mod state;
mod undo;

use std::str::FromStr;

use anyhow::{Context, Result};
use axum::Router;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use tower_http::trace::TraceLayer;

use crate::{config::Config, state::AppState};

pub async fn open_database(url: &str) -> Result<sqlx::SqlitePool> {
    let options = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(10));
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?;
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}

pub fn app(state: AppState) -> Router {
    Router::new()
        .nest(
            "/api",
            api::router().layer(axum::middleware::from_fn_with_state(state.clone(), api::require_token)),
        )
        .fallback(assets::serve)
        .layer(axum::middleware::from_fn_with_state(state.clone(), security_headers))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Headers on every answer: the app is a signed-in place, so a browser is told not to sniff types,
/// not to send the address on as a referrer, and to keep to HTTPS when this is served over it.
/// A page's own frame policy is set where the mail frame is built.
async fn security_headers(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::X_CONTENT_TYPE_OPTIONS,
        axum::http::HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        axum::http::header::REFERRER_POLICY,
        axum::http::HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        axum::http::header::HeaderName::from_static("x-frame-options"),
        axum::http::HeaderValue::from_static("DENY"),
    );
    // Only meaningful when the browser reached this over HTTPS, which it did when this process
    // serves TLS or the deployment says a proxy does.
    if state.config.tls.is_some() || state.config.cookie_secure {
        headers.insert(
            axum::http::header::STRICT_TRANSPORT_SECURITY,
            axum::http::HeaderValue::from_static("max-age=31536000"),
        );
    }
    response
}

/// What a starting server clears up before it does anything else.
///
/// What was waiting for its time to undo when the server last stopped never happened: a draft
/// about to be sent is a draft again, and mail on its way out of Portier is forgotten here, so the
/// next sync shows it where it still is on the mail server. A draft that carries `sent_at` was
/// interrupted *after* its mail may have reached the mail server: it is a draft again, but it is
/// told apart and never sent again by itself, so a crash cannot deliver the same mail twice.
pub async fn startup_bookkeeping(db: &sqlx::SqlitePool) -> Result<()> {
    sqlx::query(
        "UPDATE drafts SET sending_at = NULL, last_error = CASE
             WHEN sent_at IS NOT NULL THEN
                 'Sending was interrupted. This mail may already have gone out: check Sent before sending it again.'
             ELSE last_error END",
    )
    .execute(db)
    .await?;
    sqlx::query("DELETE FROM messages WHERE folder_id IN (SELECT id FROM folders WHERE role = 'limbo')")
        .execute(db)
        .await?;
    Ok(())
}

/// Gives a user a new random password, prints it, and ends the user's sessions.
async fn reset_password(db: &sqlx::SqlitePool, email: &str) -> Result<()> {
    let email = email.trim().to_lowercase();
    let user: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(&email)
        .fetch_optional(db)
        .await?;
    let Some(user) = user else {
        anyhow::bail!("there is no user {email}");
    };
    // 16 characters of a random token: long enough to stand until the user picks their own.
    let password: String = crypto::random_token().chars().take(16).collect();
    let hash = auth::hash_password(&password).map_err(|_| anyhow::anyhow!("the password could not be hashed"))?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(hash)
        .bind(user)
        .execute(db)
        .await?;
    let ended = sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(user)
        .execute(db)
        .await?
        .rows_affected();
    println!("New password for {email}: {password}");
    println!("Sessions ended: {ended}. Sign in and choose your own password under Settings, Your password.");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "portier=info,tower_http=warn".into()),
        )
        .init();

    avatar::install_crypto();
    let config = Config::from_env()?;
    let db_path = config.data_dir.join("emscreen.db");

    // Commands for whoever runs the server; run on the server itself, where whoever can run
    // them could read the data folder anyway.
    const USAGE: &str = "usage: portier                              run the server
       portier reset-password EMAIL         give a user a new password
       portier backup [FILE]                write a backup of the whole installation
       portier restore FILE [--replace]     put a backup in place (server stopped)";
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] | ["reset-password", _] | ["backup"] | ["backup", _] => {}
        ["restore", file] | ["restore", file, "--replace"] | ["restore", "--replace", file] => {
            // Restoring under a running server would pull the database from under it.
            let running = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                tokio::net::TcpStream::connect(config.bind),
            )
            .await;
            if matches!(running, Ok(Ok(_))) {
                anyhow::bail!("something answers at {}: stop the Portier server first", config.bind);
            }
            let aside = backup::restore(
                &config.data_dir,
                std::path::Path::new(file),
                args.contains(&"--replace"),
            )?;
            println!("Restored into {}.", config.data_dir.display());
            if aside.exists() {
                println!("What was there before is in {}.", aside.display());
            }
            if config::setting("MASTER_KEY").is_some() {
                println!(
                    "PORTIER_MASTER_KEY is set: it must be the key the backup was made with, or the mail passwords cannot be read."
                );
            }
            return Ok(());
        }
        _ => anyhow::bail!(USAGE),
    }

    let db = open_database(&format!("sqlite://{}", db_path.display())).await?;
    match args.as_slice() {
        ["reset-password", email] => return reset_password(&db, email).await,
        ["backup", rest @ ..] => {
            let file = rest
                .first()
                .map_or_else(|| backup::file_name(true), |name| name.to_string());
            let size = backup::write(&db, &config.data_dir, std::path::Path::new(&file)).await?;
            println!("Backup written: {file} ({:.1} MB)", size as f64 / 1e6);
            println!("It holds all mail and the key to the mail passwords: keep it as safe as the server.");
            return Ok(());
        }
        _ => {}
    }

    sqlx::query("DELETE FROM sessions WHERE expires_at < ?")
        .bind(state::now())
        .execute(&db)
        .await?;
    startup_bookkeeping(&db).await?;

    let bind = config.bind;
    let state = AppState::new(db, config);
    mail::sync::start_all(&state).await?;
    tokio::spawn(mail::delay::run(state.clone()));
    tokio::spawn(api::autoarchive::run(state.clone()));
    tokio::spawn(mail::summary::run(state.clone()));
    tokio::spawn(backup::run(state.clone()));

    let Some((cert, key)) = state.config.tls.clone() else {
        if !bind.ip().is_loopback() {
            tracing::warn!(
                "listening on {bind} without HTTPS: passwords and mail cross the network unprotected. \
                 Set PORTIER_TLS_CERT and PORTIER_TLS_KEY, or keep it behind a proxy that adds HTTPS."
            );
        }
        let listener = tokio::net::TcpListener::bind(bind).await?;
        tracing::info!("Portier listening on http://{bind}");
        axum::serve(listener, app(state)).await?;
        return Ok(());
    };

    let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert, &key)
        .await
        .with_context(|| {
            format!(
                "cannot read the certificate {} and key {}",
                cert.display(),
                key.display()
            )
        })?;
    // Certificates are renewed every few months: the files are read again twice a day, so a
    // renewed one is picked up without a restart.
    let renewed = tls.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(12 * 3600)).await;
            if let Err(e) = renewed.reload_from_pem_file(&cert, &key).await {
                tracing::warn!("the certificate could not be read again, keeping the old one: {e}");
            }
        }
    });
    tracing::info!("Portier listening on https://{bind}");
    axum_server::bind_rustls(bind, tls)
        .serve(app(state).into_make_service())
        .await?;
    Ok(())
}
