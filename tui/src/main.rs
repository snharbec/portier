//! Portier in the terminal: the same lists and mails as the web client, from the same server.

mod api;
mod app;
mod text;
mod ui;

use std::{
    io::{IsTerminal, Write},
    path::PathBuf,
};

use anyhow::{Context, Result, anyhow, bail};
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use serde::{Deserialize, Serialize};

use crate::{api::Client, app::App};

const USAGE: &str = "portier-tui: Portier in the terminal

Usage: portier-tui [--url ADDRESS] [--logout]

  --url ADDRESS   Address of the Portier server, e.g. http://127.0.0.1:8080.
                  Also taken from PORTIER_URL; asked for on the first start.
  --logout        Forget the stored session and exit.

For scripts and tests:
  --print WxH     Draw one screen of that size as text and exit.
  --script KEYS   Keys to press before that, e.g. 'jj<enter><space>'.
  PORTIER_EMAIL, PORTIER_PASSWORD sign in without asking.

The session is kept in ~/.config/portier/session.json (PORTIER_CONFIG_DIR changes the folder).";

/// What is remembered between starts: where the server is, and the session there.
#[derive(Serialize, Deserialize)]
struct Stored {
    url: String,
    session: String,
}

fn config_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("PORTIER_CONFIG_DIR") {
        return Ok(PathBuf::from(dir));
    }
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(dir).join("portier"));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set; set PORTIER_CONFIG_DIR"))?;
    Ok(PathBuf::from(home).join(".config").join("portier"))
}

fn load_stored() -> Option<Stored> {
    let text = std::fs::read_to_string(config_dir().ok()?.join("session.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// The session is as good as a password for this server: readable by the owner only.
fn store(stored: &Stored) -> Result<()> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("session.json");
    std::fs::write(&path, serde_json::to_string_pretty(stored)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn ask(question: &str) -> Result<String> {
    print!("{question}");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(answer.trim().to_string())
}

/// A signed-in client: from the stored session when it still holds, else by signing in.
async fn sign_in(url: Option<String>) -> Result<(Client, String)> {
    let stored = load_stored();
    let url = url
        .or_else(|| std::env::var("PORTIER_URL").ok())
        .or_else(|| stored.as_ref().map(|s| s.url.clone()));
    if let (Some(stored), Some(url)) = (&stored, &url)
        && stored.url.trim_end_matches('/') == url.trim_end_matches('/')
    {
        let client = Client::with_session(url, &stored.session)?;
        if let Some(user) = client.me().await? {
            return Ok((client, user.email));
        }
    }

    let from_env = (
        std::env::var("PORTIER_EMAIL").ok(),
        std::env::var("PORTIER_PASSWORD").ok(),
    );
    let url = match url {
        Some(url) => url,
        None if std::io::stdin().is_terminal() => {
            let answer = ask("Address of the Portier server [http://127.0.0.1:8080]: ")?;
            if answer.is_empty() {
                "http://127.0.0.1:8080".to_string()
            } else {
                answer
            }
        }
        None => bail!("no server address: give --url or set PORTIER_URL"),
    };
    let (email, password) = match from_env {
        (Some(email), Some(password)) => (email, password),
        _ if std::io::stdin().is_terminal() => {
            println!("Sign in to {url}");
            let email = ask("Email: ")?;
            let password = rpassword::prompt_password("Password: ")?;
            (email, password)
        }
        _ => bail!("not signed in: set PORTIER_EMAIL and PORTIER_PASSWORD, or start portier-tui in a terminal once"),
    };
    let client = Client::login(&url, &email, &password).await?;
    store(&Stored {
        url: client.base.clone(),
        session: client.session.clone(),
    })?;
    let user = client.me().await?.map(|user| user.email).unwrap_or(email);
    Ok((client, user))
}

/// One screen as plain text, for scripts and tests.
fn print_screen(app: &mut App, width: u16, height: u16) -> Result<()> {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))?;
    // Drawn twice: the first pass measures the mail area, which paging and scrolling use.
    terminal.draw(|frame| ui::draw(frame, app))?;
    terminal.draw(|frame| ui::draw(frame, app))?;
    let buffer = terminal.backend().buffer();
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            line.push_str(buffer[(x, y)].symbol());
        }
        println!("{}", line.trim_end());
    }
    Ok(())
}

async fn interactive(app: &mut App, client: Client) -> Result<()> {
    let (changed_tx, mut changed) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(client.watch(changed_tx));
    // The terminal's keys are read on a thread of their own: reading blocks.
    let (keys_tx, mut keys) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        while let Ok(event) = event::read() {
            if keys_tx.send(event).is_err() {
                break;
            }
        }
    });

    let mut terminal = ratatui::init();
    let result: Result<()> = async {
        while !app.quit {
            terminal.draw(|frame| ui::draw(frame, app))?;
            tokio::select! {
                Some(event) = keys.recv() => {
                    if let Event::Key(key) = event
                        && key.kind != KeyEventKind::Release
                    {
                        app.key(key).await?;
                    }
                }
                Some(()) = changed.recv() => {
                    // Several changes in a row are one reason to look again.
                    while changed.try_recv().is_ok() {}
                    if let Err(error) = app.refresh().await {
                        if error.is::<api::SignedOut>() {
                            return Err(error);
                        }
                        app.status = format!("{error:#}");
                    }
                }
            }
        }
        Ok(())
    }
    .await;
    ratatui::restore();
    result
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut url = None;
    let mut print = None;
    let mut script = String::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| anyhow!("{name} needs a value"));
        match arg.as_str() {
            "--url" => url = Some(value("--url")?),
            "--script" => script = value("--script")?,
            "--print" => {
                let size = value("--print")?;
                let (w, h) = size.split_once('x').context("--print takes a size like 100x30")?;
                print = Some((w.parse::<u16>()?, h.parse::<u16>()?));
            }
            "--logout" => {
                let _ = std::fs::remove_file(config_dir()?.join("session.json"));
                println!("Signed out: the stored session is forgotten.");
                return Ok(());
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => bail!("unknown option {other}\n\n{USAGE}"),
        }
    }

    let (client, user) = sign_in(url).await?;
    let mut app = App::new(client.clone(), user).await?;
    if let Some((width, height)) = print {
        // Keys are pressed one screen at a time: paging needs to know how tall the mail is.
        for key in split_keys(&script) {
            print_screen_silently(&mut app, width, height)?;
            app.script(&key).await?;
        }
        return print_screen(&mut app, width, height);
    }
    if !std::io::stdout().is_terminal() {
        bail!("portier-tui needs a terminal (or --print WxH to write one screen as text)");
    }
    interactive(&mut app, client).await
}

/// `'jj<enter>'` as `["j", "j", "<enter>"]`.
fn split_keys(script: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut rest = script;
    while let Some(c) = rest.chars().next() {
        let len = match rest.strip_prefix('<').and_then(|r| r.find('>')) {
            Some(end) => end + 2,
            None => c.len_utf8(),
        };
        keys.push(rest[..len].to_string());
        rest = &rest[len..];
    }
    keys
}

/// Draws without printing, so the app knows the sizes the next key works with.
fn print_screen_silently(app: &mut App, width: u16, height: u16) -> Result<()> {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))?;
    terminal.draw(|frame| ui::draw(frame, app))?;
    Ok(())
}
