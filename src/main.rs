mod api;
mod assets;
mod auth;
mod config;
mod crypto;
mod error;
mod mail;
mod models;
mod search;
mod state;

use std::str::FromStr;

use anyhow::Result;
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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "emscreen=info,tower_http=warn".into()),
        )
        .init();

    let config = Config::from_env()?;
    let db_path = config.data_dir.join("emscreen.db");
    let db = open_database(&format!("sqlite://{}", db_path.display())).await?;
    sqlx::query("DELETE FROM sessions WHERE expires_at < ?")
        .bind(state::now())
        .execute(&db)
        .await?;

    let bind = config.bind;
    let state = AppState::new(db, config);
    mail::sync::start_all(&state).await?;
    tokio::spawn(mail::delay::run(state.clone()));

    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("emscreen listening on http://{bind}");
    axum::serve(listener, app(state)).await?;
    Ok(())
}
