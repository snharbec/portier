//! Thin layer over async-imap: connecting, folder discovery and the few commands emscreen uses.

use std::{collections::HashSet, fmt::Debug, sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use async_imap::types::{Flag, NameAttribute};
use futures::TryStreamExt;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tokio_rustls::{
    TlsConnector,
    rustls::{ClientConfig, RootCertStore, pki_types::ServerName},
};

use crate::{crypto, models::Account};

pub trait ImapStream: AsyncRead + AsyncWrite + Unpin + Send + Debug {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Debug> ImapStream for T {}

pub type Session = async_imap::Session<Box<dyn ImapStream>>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug)]
pub struct ImapParams {
    pub host: String,
    pub port: u16,
    pub security: String,
    pub username: String,
    pub password: String,
}

impl ImapParams {
    pub fn for_account(account: &Account, master_key: &[u8; 32]) -> Result<Self> {
        Ok(Self {
            host: account.imap_host.clone(),
            port: account.imap_port as u16,
            security: account.imap_security.clone(),
            username: account.imap_username.clone(),
            password: crypto::decrypt(master_key, &account.password_enc)?,
        })
    }
}

fn tls_connector() -> TlsConnector {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

pub async fn connect(params: &ImapParams) -> Result<Session> {
    tokio::time::timeout(CONNECT_TIMEOUT, connect_inner(params))
        .await
        .map_err(|_| anyhow!("timed out connecting to {}:{}", params.host, params.port))?
}

async fn connect_inner(params: &ImapParams) -> Result<Session> {
    let tcp = TcpStream::connect((params.host.as_str(), params.port))
        .await
        .with_context(|| format!("cannot reach {}:{}", params.host, params.port))?;
    let server_name = ServerName::try_from(params.host.clone()).context("invalid IMAP host name")?;

    let client = match params.security.as_str() {
        "tls" => {
            let tls = tls_connector()
                .connect(server_name, tcp)
                .await
                .context("TLS handshake failed")?;
            let mut client = async_imap::Client::new(Box::new(tls) as Box<dyn ImapStream>);
            client
                .read_response()
                .await?
                .ok_or_else(|| anyhow!("no IMAP greeting"))?;
            client
        }
        "starttls" => {
            let mut plain = async_imap::Client::new(Box::new(tcp) as Box<dyn ImapStream>);
            plain
                .read_response()
                .await?
                .ok_or_else(|| anyhow!("no IMAP greeting"))?;
            plain
                .run_command_and_check_ok("STARTTLS", None)
                .await
                .context("STARTTLS refused")?;
            let tls = tls_connector()
                .connect(server_name, plain.into_inner())
                .await
                .context("TLS handshake failed")?;
            async_imap::Client::new(Box::new(tls) as Box<dyn ImapStream>)
        }
        "none" => {
            let mut client = async_imap::Client::new(Box::new(tcp) as Box<dyn ImapStream>);
            client
                .read_response()
                .await?
                .ok_or_else(|| anyhow!("no IMAP greeting"))?;
            client
        }
        other => bail!("unknown IMAP security mode {other}"),
    };

    client
        .login(&params.username, &params.password)
        .await
        .map_err(|(e, _)| anyhow!("IMAP login failed: {e}"))
}

#[derive(Debug, Default, Clone)]
pub struct SpecialFolders {
    pub junk: Option<String>,
    pub sent: Option<String>,
    pub trash: Option<String>,
    pub archive: Option<String>,
    pub all: Vec<String>,
}

/// Finds Junk, Sent, Trash and Archive by SPECIAL-USE attribute, falling back to common names.
pub async fn discover_folders(session: &mut Session) -> Result<SpecialFolders> {
    let names: Vec<_> = session.list(Some(""), Some("*")).await?.try_collect().await?;
    let mut found = SpecialFolders::default();
    for name in &names {
        if name.attributes().iter().any(|a| matches!(a, NameAttribute::NoSelect)) {
            continue;
        }
        found.all.push(name.name().to_string());
        for attr in name.attributes() {
            match attr {
                NameAttribute::Junk => found.junk = Some(name.name().to_string()),
                NameAttribute::Sent => found.sent = Some(name.name().to_string()),
                NameAttribute::Trash => found.trash = Some(name.name().to_string()),
                NameAttribute::Archive => found.archive = Some(name.name().to_string()),
                _ => {}
            }
        }
    }
    let by_name = |candidates: &[&str]| {
        found.all.iter().find(|n| {
            let leaf = n.rsplit(['/', '.']).next().unwrap_or(n).to_lowercase();
            candidates.contains(&leaf.as_str())
        })
    };
    if found.junk.is_none() {
        found.junk = by_name(&["junk", "spam", "junk e-mail", "junk email", "bulk mail"]).cloned();
    }
    if found.sent.is_none() {
        found.sent = by_name(&["sent", "sent items", "sent messages", "sent mail", "gesendet"]).cloned();
    }
    if found.trash.is_none() {
        found.trash = by_name(&["trash", "deleted items", "deleted messages", "bin", "papierkorb"]).cloned();
    }
    if found.archive.is_none() {
        found.archive = by_name(&["archive", "archives", "archiv", "archived"]).cloned();
    }
    Ok(found)
}

pub fn uid_set(uids: &[u32]) -> String {
    uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
}

pub struct Fetched {
    pub uid: u32,
    pub seen: bool,
    pub flagged: bool,
    pub body: Vec<u8>,
}

pub async fn fetch_full(session: &mut Session, uids: &[u32]) -> Result<Vec<Fetched>> {
    let fetches: Vec<_> = session
        .uid_fetch(uid_set(uids), "(UID FLAGS BODY.PEEK[])")
        .await?
        .try_collect()
        .await?;
    Ok(fetches
        .iter()
        .filter_map(|f| {
            Some(Fetched {
                uid: f.uid?,
                seen: f.flags().any(|flag| flag == Flag::Seen),
                flagged: f.flags().any(|flag| flag == Flag::Flagged),
                body: f.body()?.to_vec(),
            })
        })
        .collect())
}

/// Returns (uid, seen, flagged) for every message with UID >= `from_uid`.
pub async fn fetch_flags(session: &mut Session, from_uid: u32) -> Result<Vec<(u32, bool, bool)>> {
    let fetches: Vec<_> = session
        .uid_fetch(format!("{from_uid}:*"), "(UID FLAGS)")
        .await?
        .try_collect()
        .await?;
    Ok(fetches
        .iter()
        .filter_map(|f| {
            Some((
                f.uid?,
                f.flags().any(|flag| flag == Flag::Seen),
                f.flags().any(|flag| flag == Flag::Flagged),
            ))
        })
        .collect())
}

pub async fn all_uids(session: &mut Session) -> Result<HashSet<u32>> {
    Ok(session.uid_search("ALL").await?)
}

/// Moves messages out of the currently selected folder. Uses MOVE, or COPY + delete when the
/// server lacks it.
pub async fn move_uids(session: &mut Session, uids: &[u32], target: &str) -> Result<()> {
    if uids.is_empty() {
        return Ok(());
    }
    let set = uid_set(uids);
    let has_move = session.capabilities().await?.has_str("MOVE");
    if has_move {
        session.uid_mv(&set, target).await?;
    } else {
        session.uid_copy(&set, target).await?;
        session
            .uid_store(&set, "+FLAGS.SILENT (\\Deleted)")
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        session.expunge().await?.try_collect::<Vec<_>>().await?;
    }
    Ok(())
}

/// Sets or clears \\Flagged, the mark other mail programs show as a flag or star.
pub async fn set_flagged(session: &mut Session, uids: &[u32], flagged: bool) -> Result<()> {
    if uids.is_empty() {
        return Ok(());
    }
    let change = if flagged {
        "+FLAGS.SILENT (\\Flagged)"
    } else {
        "-FLAGS.SILENT (\\Flagged)"
    };
    session
        .uid_store(uid_set(uids), change)
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    Ok(())
}

pub async fn set_seen(session: &mut Session, uids: &[u32], seen: bool) -> Result<()> {
    if uids.is_empty() {
        return Ok(());
    }
    let change = if seen {
        "+FLAGS.SILENT (\\Seen)"
    } else {
        "-FLAGS.SILENT (\\Seen)"
    };
    session
        .uid_store(uid_set(uids), change)
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    Ok(())
}
