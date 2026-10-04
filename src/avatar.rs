//! Sender pictures from Gravatar (a person's own picture, by address) and BIMI (a brand's logo,
//! published in the sending domain's DNS). Results, including "nothing found", are cached.
//!
//! Both lookups talk to third parties: Gravatar sees a hash of the sender's address, the DNS
//! resolver sees the sender's domain, and a BIMI logo is fetched from wherever the domain points.
//! `EMSCREEN_AVATARS=off` turns all of it off.

use std::{
    net::{IpAddr, SocketAddr},
    sync::OnceLock,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use hickory_resolver::{TokioResolver, proto::rr::RData};
use sha2::{Digest, Sha256};

use crate::{
    mail::preview,
    state::{AppState, now},
};

const FOUND_TTL: i64 = 30 * 24 * 3600;
const MISSING_TTL: i64 = 3 * 24 * 3600;
const TIMEOUT: Duration = Duration::from_secs(6);
const MAX_RASTER: usize = 512 * 1024;
const MAX_SVG: usize = 256 * 1024;

pub struct Avatar {
    pub mime: String,
    pub data: Vec<u8>,
}

/// rustls needs one process-wide crypto provider; reqwest is built without its own.
pub fn install_crypto() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
    });
}

fn resolver() -> Result<&'static TokioResolver> {
    static RESOLVER: OnceLock<Option<TokioResolver>> = OnceLock::new();
    RESOLVER
        .get_or_init(|| {
            TokioResolver::builder_tokio()
                .ok()
                .and_then(|builder| builder.build().ok())
        })
        .as_ref()
        .ok_or_else(|| anyhow!("no DNS resolver available"))
}

/// Picture for a sender address: Gravatar first (it belongs to the person), then the BIMI logo
/// of the address's domain.
pub async fn lookup(state: &AppState, address: &str) -> Result<Option<Avatar>> {
    let address = address.trim().to_lowercase();
    let Some((_, domain)) = address.rsplit_once('@') else {
        return Ok(None);
    };
    let domain = domain.to_string();

    let gravatar_key = format!("g:{address}");
    let bimi_key = format!("b:{domain}");
    for (key, source) in [
        (gravatar_key, Source::Gravatar(address.clone())),
        (bimi_key, Source::Bimi(domain)),
    ] {
        if let Some(cached) = cached(state, &key).await? {
            match cached {
                Some(avatar) => return Ok(Some(avatar)),
                None => continue, // known to be missing; try the next source
            }
        }
        // One lookup at a time per slot, and check again: another request may have filled the cache.
        let _slot = state.avatar_slots.acquire().await?;
        if let Some(cached) = cached(state, &key).await? {
            match cached {
                Some(avatar) => return Ok(Some(avatar)),
                None => continue,
            }
        }
        let fetched = match &source {
            Source::Gravatar(address) => gravatar(address).await,
            Source::Bimi(domain) => bimi(domain).await,
        };
        let found = match fetched {
            Ok(found) => found,
            Err(e) => {
                // A failed lookup is cached like "nothing found", so a broken source is not hammered.
                tracing::debug!("avatar lookup {key} failed: {e:#}");
                None
            }
        };
        store(state, &key, found.as_ref()).await?;
        if found.is_some() {
            return Ok(found);
        }
    }
    Ok(None)
}

enum Source {
    Gravatar(String),
    Bimi(String),
}

/// `None`: not cached or expired. `Some(None)`: cached as missing. `Some(Some(_))`: cached picture.
async fn cached(state: &AppState, key: &str) -> Result<Option<Option<Avatar>>> {
    let row: Option<(Option<String>, Option<Vec<u8>>, i64)> =
        sqlx::query_as("SELECT mime, data, fetched_at FROM avatar_cache WHERE key = ?")
            .bind(key)
            .fetch_optional(&state.db)
            .await?;
    Ok(match row {
        Some((Some(mime), Some(data), fetched_at)) if now() - fetched_at < FOUND_TTL => {
            Some(Some(Avatar { mime, data }))
        }
        Some((None, _, fetched_at)) if now() - fetched_at < MISSING_TTL => Some(None),
        _ => None,
    })
}

async fn store(state: &AppState, key: &str, avatar: Option<&Avatar>) -> Result<()> {
    sqlx::query(
        "INSERT INTO avatar_cache (key, mime, data, fetched_at) VALUES (?, ?, ?, ?)
         ON CONFLICT (key) DO UPDATE SET mime = excluded.mime, data = excluded.data, fetched_at = excluded.fetched_at",
    )
    .bind(key)
    .bind(avatar.map(|a| a.mime.as_str()))
    .bind(avatar.map(|a| a.data.as_slice()))
    .bind(now())
    .execute(&state.db)
    .await?;
    Ok(())
}

pub fn gravatar_url(address: &str) -> String {
    let hash: String = Sha256::digest(address.trim().to_lowercase().as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    // d=404: answer "not found" instead of a generated placeholder.
    format!("https://gravatar.com/avatar/{hash}?s=128&d=404")
}

async fn gravatar(address: &str) -> Result<Option<Avatar>> {
    install_crypto();
    let client = reqwest::Client::builder().timeout(TIMEOUT).build()?;
    let response = client.get(gravatar_url(address)).send().await?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let data = response.bytes().await?.to_vec();
    if data.len() > MAX_RASTER {
        bail!("picture too large");
    }
    // Trust the bytes, not the header: only raster formats the browser shows as plain images.
    let mime = preview::raster_mime(&data).ok_or_else(|| anyhow!("not an image"))?;
    Ok(Some(Avatar {
        mime: mime.to_string(),
        data,
    }))
}

/// The logo address from a BIMI record such as `v=BIMI1; l=https://example.com/logo.svg; a=`.
pub fn bimi_logo(record: &str) -> Option<String> {
    let mut tags = record.split(';').filter_map(|tag| tag.trim().split_once('='));
    let mut version_ok = false;
    let mut logo = None;
    for (name, value) in &mut tags {
        match name.trim().to_lowercase().as_str() {
            "v" => version_ok = value.trim().eq_ignore_ascii_case("BIMI1"),
            "l" => logo = Some(value.trim().to_string()),
            _ => {}
        }
    }
    logo.filter(|l| version_ok && l.starts_with("https://"))
}

/// Names to ask for a domain's record: the domain itself, then its parent of two labels
/// (mail.example.com falls back to example.com).
fn bimi_names(domain: &str) -> Vec<String> {
    let labels: Vec<&str> = domain.split('.').filter(|l| !l.is_empty()).collect();
    let mut names = vec![format!("default._bimi.{domain}.")];
    if labels.len() > 2 {
        names.push(format!("default._bimi.{}.", labels[labels.len() - 2..].join(".")));
    }
    names
}

async fn bimi(domain: &str) -> Result<Option<Avatar>> {
    let resolver = resolver()?;
    let mut logo = None;
    for name in bimi_names(domain) {
        let Ok(Ok(answer)) = tokio::time::timeout(TIMEOUT, resolver.txt_lookup(name)).await else {
            continue;
        };
        logo = answer.answers().iter().find_map(|record| match &record.data {
            // A TXT record's value may be split into several strings; they belong together.
            RData::TXT(txt) => {
                let joined: Vec<u8> = txt.txt_data.iter().flat_map(|part| part.iter().copied()).collect();
                bimi_logo(&String::from_utf8_lossy(&joined))
            }
            _ => None,
        });
        if logo.is_some() {
            break;
        }
    }
    let Some(logo) = logo else {
        return Ok(None);
    };
    let data = fetch_public(&logo, MAX_SVG).await?;
    let text = std::str::from_utf8(&data).context("logo is not text")?;
    if !svg_is_plain(text) {
        bail!("logo is not a plain SVG");
    }
    Ok(Some(Avatar {
        mime: "image/svg+xml".to_string(),
        data,
    }))
}

/// Whether an address is out on the internet: not this machine, not a private or link-local network.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                || (a == 100 && (64..128).contains(&b)) // carrier-grade NAT
                || a == 0)
        }
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public(IpAddr::V4(v4)),
            None => {
                let first = v6.segments()[0];
                !(v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_multicast()
                    || (first & 0xfe00) == 0xfc00 // unique local
                    || (first & 0xffc0) == 0xfe80) // link-local
            }
        },
    }
}

/// Fetches an https address that a stranger's DNS record named. The host must resolve only to
/// public addresses, the connection is pinned to the address that was checked, and redirects
/// are not followed, so the record cannot steer this server at something on the local network.
async fn fetch_public(url: &str, max_bytes: usize) -> Result<Vec<u8>> {
    match fetch_step(url, max_bytes).await? {
        Fetched::Body(data) => Ok(data),
        Fetched::Elsewhere(_) => bail!("redirected"),
    }
}

/// A picture from a web address the user gave. Like `fetch_public`, but follows a few
/// redirects, each checked the same way as the first address.
pub async fn fetch_picture(url: &str, max_bytes: usize) -> Result<Vec<u8>> {
    let mut url = url.to_string();
    for _ in 0..4 {
        match fetch_step(&url, max_bytes).await? {
            Fetched::Body(data) => return Ok(data),
            Fetched::Elsewhere(next) => url = next,
        }
    }
    bail!("too many redirects")
}

enum Fetched {
    Body(Vec<u8>),
    /// The server points to another address.
    Elsewhere(String),
}

async fn fetch_step(url: &str, max_bytes: usize) -> Result<Fetched> {
    let parsed = reqwest::Url::parse(url)?;
    if parsed.scheme() != "https" {
        bail!("not https");
    }
    let host = parsed.host_str().ok_or_else(|| anyhow!("no host"))?.to_string();
    let port = parsed.port().unwrap_or(443);
    let addresses: Vec<SocketAddr> = tokio::time::timeout(TIMEOUT, tokio::net::lookup_host((host.as_str(), port)))
        .await??
        .collect();
    if addresses.is_empty() || !addresses.iter().all(|a| is_public(a.ip())) {
        bail!("host does not resolve to a public address");
    }
    install_crypto();
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .resolve(&host, addresses[0])
        .build()?;
    let mut response = client.get(parsed.clone()).send().await?;
    if response.status().is_redirection() {
        let target = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| anyhow!("redirect without a target"))?;
        return Ok(Fetched::Elsewhere(parsed.join(target)?.to_string()));
    }
    if !response.status().is_success() {
        bail!("status {}", response.status());
    }
    let mut data = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        data.extend_from_slice(&chunk);
        if data.len() > max_bytes {
            bail!("too large");
        }
    }
    Ok(Fetched::Body(data))
}

/// BIMI logos are a restricted SVG profile without scripting. Anything that could run or load
/// something is refused; the response is additionally served sandboxed.
pub fn svg_is_plain(svg: &str) -> bool {
    let lower = svg.to_lowercase();
    if !lower.contains("<svg") {
        return false;
    }
    let forbidden = [
        "<script",
        "javascript:",
        "<foreignobject",
        "<iframe",
        "<object",
        "<embed",
        "<!entity",
    ];
    if forbidden.iter().any(|needle| lower.contains(needle)) {
        return false;
    }
    // Event handler attributes: " onload=", " onclick =", ...
    let bytes = lower.as_bytes();
    for (i, window) in bytes.windows(3).enumerate() {
        if window[0].is_ascii_whitespace() && &window[1..] == b"on" {
            let rest = &lower[i + 3..];
            let name_len = rest.bytes().take_while(u8::is_ascii_lowercase).count();
            if name_len > 0 && rest[name_len..].trim_start().starts_with('=') {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gravatar_address_is_hashed_lowercase() {
        let url = gravatar_url("  Anna@Example.COM ");
        assert_eq!(url, gravatar_url("anna@example.com"));
        assert!(url.starts_with("https://gravatar.com/avatar/"));
        assert!(url.ends_with("?s=128&d=404"));
        assert!(!url.contains("anna"));
        let hash = url
            .trim_start_matches("https://gravatar.com/avatar/")
            .trim_end_matches("?s=128&d=404");
        assert!(
            hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()),
            "sha-256 in hex"
        );
    }

    #[test]
    fn bimi_record_parsing() {
        assert_eq!(
            bimi_logo("v=BIMI1; l=https://example.com/logo.svg; a=https://example.com/vmc.pem").as_deref(),
            Some("https://example.com/logo.svg")
        );
        assert_eq!(
            bimi_logo("v=BIMI1;l=https://x.example/l.svg").as_deref(),
            Some("https://x.example/l.svg")
        );
        assert_eq!(
            bimi_logo("v=BIMI1; l=; a="),
            None,
            "record that declines to publish a logo"
        );
        assert_eq!(bimi_logo("v=BIMI1; l=http://example.com/logo.svg"), None, "not https");
        assert_eq!(bimi_logo("v=spf1 include:example.com"), None);
        assert_eq!(bimi_logo("l=https://example.com/logo.svg"), None, "no version");
    }

    #[test]
    fn bimi_asks_the_domain_then_its_parent() {
        assert_eq!(bimi_names("example.com"), ["default._bimi.example.com."]);
        assert_eq!(
            bimi_names("mail.news.example.com"),
            ["default._bimi.mail.news.example.com.", "default._bimi.example.com."]
        );
    }

    #[test]
    fn only_public_addresses_are_fetched() {
        for private in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.1.1",
            "172.16.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(!is_public(private.parse().unwrap()), "{private}");
        }
        for public in ["93.184.216.34", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public(public.parse().unwrap()), "{public}");
        }
    }

    #[test]
    fn svg_with_anything_active_is_refused() {
        assert!(svg_is_plain(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle r="5"/></svg>"#
        ));
        assert!(svg_is_plain(
            "<?xml version=\"1.0\"?><svg><title>Only one</title><path d=\"M0 0\"/></svg>"
        ));
        for bad in [
            "<svg><script>alert(1)</script></svg>",
            r#"<svg onload="x()"></svg>"#,
            "<svg\nONLOAD = \"x()\"></svg>",
            r#"<svg><a href="javascript:x()">a</a></svg>"#,
            "<svg><foreignObject><p>html</p></foreignObject></svg>",
            "<!DOCTYPE svg [<!ENTITY x \"y\">]><svg/>",
            "<html><body>not an svg</body></html>",
        ] {
            assert!(!svg_is_plain(bad), "{bad}");
        }
    }
}
