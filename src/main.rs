mod api;
mod assets;
mod auth;
mod avatar;
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
        .nest("/api", api::router())
        .fallback(assets::serve)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
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
    let db = open_database(&format!("sqlite://{}", db_path.display())).await?;
    sqlx::query("DELETE FROM sessions WHERE expires_at < ?")
        .bind(state::now())
        .execute(&db)
        .await?;
    // What was waiting for its time to undo when the server last stopped never happened:
    // a draft about to be sent is a draft again, and mail on its way out of Portier is
    // forgotten here, so the next sync shows it where it still is on the mail server.
    sqlx::query("UPDATE drafts SET sending_at = NULL").execute(&db).await?;
    sqlx::query("DELETE FROM messages WHERE folder_id IN (SELECT id FROM folders WHERE role = 'limbo')")
        .execute(&db)
        .await?;

    // `portier reset-password EMAIL`: for a user who is locked out. Run on the server itself,
    // where whoever can run it could read the database anyway.
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (None, _) => {}
        (Some("reset-password"), Some(email)) => return reset_password(&db, &email).await,
        _ => anyhow::bail!(
            "usage: portier                        run the server\n       portier reset-password EMAIL   give a user a new password"
        ),
    }

    let bind = config.bind;
    let state = AppState::new(db, config);
    mail::sync::start_all(&state).await?;
    tokio::spawn(mail::delay::run(state.clone()));
    tokio::spawn(api::autoarchive::run(state.clone()));

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
