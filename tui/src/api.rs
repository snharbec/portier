//! The server's JSON API as the terminal client uses it. The types mirror
//! `web/src/lib/api.ts`; fields the terminal has no use for are left out.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

/// The session cookie still carries the app's earlier name.
const COOKIE: &str = "emscreen_session";

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Counts {
    pub screener: i64,
    pub unread_important: i64,
    pub unread_flagged: i64,
    pub unread_feed: i64,
    pub unread_junk: i64,
    pub unread_delayed: i64,
    pub unread_archive: i64,
    pub unread_trash: i64,
    pub drafts: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ThreadSummary {
    pub id: i64,
    pub subject: String,
    pub count: i64,
    pub unread: i64,
    pub date: i64,
    pub snippet: String,
    pub from_name: String,
    pub from_addr: String,
    pub is_outgoing: bool,
    pub account_id: i64,
    pub sender_name: Option<String>,
    pub sender_address: Option<String>,
    pub snoozed_until: Option<i64>,
    #[serde(default)]
    pub note: String,
    /// For a search result: the list it is found in. Lists leave it empty.
    #[serde(default)]
    pub tag: String,
}

/// One mail the search found.
#[derive(Debug, Clone, Deserialize)]
pub struct SearchHit {
    /// The mail itself; `thread_id` is its conversation.
    pub id: i64,
    pub thread_id: i64,
    pub account_id: i64,
    pub subject: String,
    pub from_name: String,
    pub from_addr: String,
    pub date: i64,
    pub seen: bool,
    pub excerpt: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub place: String,
}

impl SearchHit {
    /// The side bar's name of the list the mail is found in.
    pub fn place_name(&self) -> &'static str {
        match self.place.as_str() {
            "home" => "Home",
            "flagged" => "Important",
            "delayed" => "Delayed",
            "feed" => "Nice to know",
            "screener" => "Screener",
            "archive" => "Archive",
            "sent" => "Sent",
            "junk" => "Junk",
            "trash" => "Trash",
            _ => "",
        }
    }

    /// As a row of a mail list: the conversation, shown by this mail.
    pub fn row(self) -> ThreadSummary {
        ThreadSummary {
            id: self.thread_id,
            tag: self.place_name().to_string(),
            subject: self.subject,
            count: 1,
            unread: i64::from(!self.seen),
            date: self.date,
            snippet: self.excerpt,
            from_name: self.from_name,
            from_addr: self.from_addr,
            is_outgoing: false,
            account_id: self.account_id,
            sender_name: None,
            sender_address: None,
            snoozed_until: None,
            note: self.note,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SavedSearch {
    pub id: i64,
    pub name: String,
    pub query: String,
    /// Unseen mails the search finds right now.
    pub unread: i64,
}

impl ThreadSummary {
    /// The person a row is about: the sender, or for your own mail the one you wrote to.
    pub fn who(&self) -> String {
        if self.is_outgoing && self.sender_address.is_some() {
            display_name(self.sender_name.as_deref(), self.sender_address.as_deref())
        } else {
            display_name(Some(&self.from_name), Some(&self.from_addr))
        }
    }
}

pub fn display_name(name: Option<&str>, address: Option<&str>) -> String {
    match (
        name.map(str::trim).filter(|n| !n.is_empty()),
        address.filter(|a| !a.is_empty()),
    ) {
        (Some(name), _) => name.to_string(),
        (None, Some(address)) => address.to_string(),
        (None, None) => "Unknown sender".to_string(),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Addr {
    pub name: String,
    pub address: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Attachment {
    /// Its number within the mail, by which it is fetched.
    #[serde(default)]
    pub idx: i64,
    pub filename: String,
    pub size: i64,
    /// image | pdf | office | other
    #[serde(default)]
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    pub id: i64,
    #[serde(default)]
    pub subject: String,
    pub from: Addr,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub date: i64,
    pub seen: bool,
    pub is_outgoing: bool,
    pub body_text: String,
    pub body_html: String,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SenderHead {
    pub address: String,
    pub display_name: String,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Thread {
    pub id: i64,
    /// Some received mail of it is still in the inbox, so it can be archived or delayed.
    pub can_archive: bool,
    /// Some mail of it is not in the Trash yet.
    pub can_trash: bool,
    /// Some mail of it is in the Trash and can be moved back.
    pub can_restore: bool,
    /// It is in the Important list.
    pub important: bool,
    pub subject: String,
    pub snoozed_until: Option<i64>,
    #[serde(default)]
    pub note: String,
    pub sender: Option<SenderHead>,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScreenerEntry {
    /// The sender, for deciding where their mail goes.
    pub id: i64,
    /// Their newest conversation, to read before deciding.
    pub thread_id: i64,
    pub address: String,
    pub display_name: String,
    pub count: i64,
    pub subject: String,
    pub date: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Draft {
    pub id: i64,
    pub to_addrs: String,
    pub subject: String,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub email: String,
}

#[derive(Debug, Deserialize)]
struct Me {
    user: Option<User>,
}

/// The server said no to the session: log in again.
#[derive(Debug)]
pub struct SignedOut;

impl std::fmt::Display for SignedOut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the session has ended; start portier-tui again to sign in")
    }
}

impl std::error::Error for SignedOut {}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    /// Address of the server without a trailing slash, e.g. `http://127.0.0.1:8080`.
    pub base: String,
    /// Value of the session cookie.
    pub session: String,
}

impl Client {
    fn http() -> Result<reqwest::Client> {
        // rustls needs one process-wide crypto provider; reqwest is built without its own.
        let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
        Ok(reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .build()?)
    }

    pub fn with_session(base: &str, session: &str) -> Result<Self> {
        Ok(Self {
            http: Self::http()?,
            base: base.trim_end_matches('/').to_string(),
            session: session.to_string(),
        })
    }

    pub async fn login(base: &str, email: &str, password: &str) -> Result<Self> {
        let base = base.trim_end_matches('/').to_string();
        let http = Self::http()?;
        let response = http
            .post(format!("{base}/api/login"))
            .json(&json!({ "email": email, "password": password }))
            .send()
            .await
            .with_context(|| format!("cannot reach {base}"))?;
        if !response.status().is_success() {
            bail!("{}", error_text(response).await);
        }
        let session = response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.split(';').next())
            .find_map(|pair| pair.trim().strip_prefix(&format!("{COOKIE}=")).map(str::to_string))
            .ok_or_else(|| anyhow!("the server did not start a session"))?;
        Ok(Self { http, base, session })
    }

    /// Who is signed in, or `None` when the stored session is no longer valid.
    pub async fn me(&self) -> Result<Option<User>> {
        Ok(self.get::<Me>("/me").await?.user)
    }

    fn cookie(&self) -> String {
        format!("{COOKIE}={}", self.session)
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let response = self
            .http
            .get(format!("{}/api{path}", self.base))
            .header(reqwest::header::COOKIE, self.cookie())
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .with_context(|| format!("cannot reach {}", self.base))?;
        Self::answer(response).await
    }

    pub async fn post<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T> {
        let response = self
            .http
            .post(format!("{}/api{path}", self.base))
            .header(reqwest::header::COOKIE, self.cookie())
            .timeout(Duration::from_secs(30))
            .json(&body)
            .send()
            .await
            .with_context(|| format!("cannot reach {}", self.base))?;
        Self::answer(response).await
    }

    pub async fn put<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T> {
        let response = self
            .http
            .put(format!("{}/api{path}", self.base))
            .header(reqwest::header::COOKIE, self.cookie())
            .timeout(Duration::from_secs(30))
            .json(&body)
            .send()
            .await
            .with_context(|| format!("cannot reach {}", self.base))?;
        Self::answer(response).await
    }

    pub async fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let response = self
            .http
            .delete(format!("{}/api{path}", self.base))
            .header(reqwest::header::COOKIE, self.cookie())
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .with_context(|| format!("cannot reach {}", self.base))?;
        Self::answer(response).await
    }

    /// The bytes behind an address of the API: an attachment, a sender's picture.
    pub async fn bytes(&self, path: &str) -> Result<Vec<u8>> {
        let response = self
            .http
            .get(format!("{}/api{path}", self.base))
            .header(reqwest::header::COOKIE, self.cookie())
            .timeout(Duration::from_secs(120))
            .send()
            .await
            .with_context(|| format!("cannot reach {}", self.base))?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(SignedOut.into());
        }
        if !response.status().is_success() {
            bail!("{}", error_text(response).await);
        }
        Ok(response.bytes().await?.to_vec())
    }

    /// Sends a file of this computer as an attachment of a draft.
    pub async fn upload<T: DeserializeOwned>(&self, path: &str, file: &str) -> Result<T> {
        // "~/…" as the shell would read it.
        let file = match (file.strip_prefix("~/"), std::env::var_os("HOME")) {
            (Some(rest), Some(home)) => std::path::PathBuf::from(home).join(rest),
            _ => std::path::PathBuf::from(file),
        };
        let data = std::fs::read(&file).with_context(|| format!("cannot read {}", file.display()))?;
        let name = file
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "attachment".into());
        let form = reqwest::multipart::Form::new().part("file", reqwest::multipart::Part::bytes(data).file_name(name));
        let response = self
            .http
            .post(format!("{}/api{path}", self.base))
            .header(reqwest::header::COOKIE, self.cookie())
            .timeout(Duration::from_secs(120))
            .multipart(form)
            .send()
            .await
            .with_context(|| format!("cannot reach {}", self.base))?;
        Self::answer(response).await
    }

    async fn answer<T: DeserializeOwned>(response: reqwest::Response) -> Result<T> {
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(SignedOut.into());
        }
        if !response.status().is_success() {
            bail!("{}", error_text(response).await);
        }
        Ok(response.json().await?)
    }

    /// Listens to the server's change events for as long as `changed` has a receiver, and
    /// sends a notice for each (true: a mail could not be sent). Reconnects after a pause when the connection drops.
    pub async fn watch(self, changed: tokio::sync::mpsc::UnboundedSender<bool>) {
        loop {
            let request = self
                .http
                .get(format!("{}/api/events", self.base))
                .header(reqwest::header::COOKIE, self.cookie())
                .send()
                .await;
            if let Ok(mut response) = request
                && response.status().is_success()
            {
                while let Ok(Some(chunk)) = response.chunk().await {
                    // Every event is a reason to look again. One of them also has to be told:
                    // a mail that could not be sent.
                    let failed = chunk.windows(11).any(|w| w == b"send_failed");
                    if chunk.windows(5).any(|w| w == b"data:") && changed.send(failed).is_err() {
                        return;
                    }
                }
            }
            if changed.is_closed() {
                return;
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }
}

/// The server's own words for a refused request (`{"error": "…"}`), else the status.
async fn error_text(response: reqwest::Response) -> String {
    let status = response.status();
    match response.json::<Value>().await {
        Ok(body) if body["error"].is_string() => body["error"].as_str().unwrap_or_default().to_string(),
        _ => format!("the server answered {status}"),
    }
}
