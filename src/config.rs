use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::STANDARD as B64};

#[derive(Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    pub master_key: [u8; 32],
    pub open_registration: bool,
    /// Newest messages kept per folder; older mail is not downloaded.
    pub sync_max_per_folder: usize,
    /// LibreOffice binary used to preview Office attachments, if there is one.
    pub soffice: Option<PathBuf>,
    /// Look up sender pictures at Gravatar and through BIMI.
    pub avatars: bool,
    /// Certificate chain and private key (PEM files) to serve HTTPS with; without them
    /// the server speaks plain HTTP.
    pub tls: Option<(PathBuf, PathBuf)>,
    /// Whether the session cookie is marked `Secure`. On by itself behind in-process TLS, and
    /// settable for the deployment the README recommends: HTTPS added by a reverse proxy in front.
    pub cookie_secure: bool,
}

/// A setting from the environment: `PORTIER_<name>`, or `EMSCREEN_<name>` as installations
/// from before the app was renamed have it. The new name wins when both are set.
pub fn setting(name: &str) -> Option<String> {
    std::env::var(format!("PORTIER_{name}"))
        .or_else(|_| std::env::var(format!("EMSCREEN_{name}")))
        .ok()
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let bind = setting("BIND")
            .unwrap_or_else(|| "127.0.0.1:8080".into())
            .parse()
            .context("PORTIER_BIND is not a valid socket address")?;
        let data_dir = PathBuf::from(setting("DATA_DIR").unwrap_or_else(|| "data".into()));
        std::fs::create_dir_all(&data_dir).context("cannot create data directory")?;
        let master_key = load_master_key(&data_dir)?;
        let open_registration = matches!(setting("OPEN_REGISTRATION").as_deref(), Some("1") | Some("true"));
        let sync_max_per_folder = setting("SYNC_MAX_PER_FOLDER")
            .and_then(|v| v.parse().ok())
            .unwrap_or(5000);
        let tls = match (setting("TLS_CERT"), setting("TLS_KEY")) {
            (Some(cert), Some(key)) => Some((PathBuf::from(cert), PathBuf::from(key))),
            (None, None) => None,
            _ => anyhow::bail!("PORTIER_TLS_CERT and PORTIER_TLS_KEY have to be set together"),
        };
        Ok(Self {
            bind,
            tls,
            data_dir,
            master_key,
            open_registration,
            sync_max_per_folder,
            soffice: crate::mail::preview::find_soffice(),
            avatars: setting("AVATARS").as_deref() != Some("off"),
            // The session cookie is marked Secure by itself when this process speaks HTTPS; behind
            // a proxy that adds HTTPS it has to be asked for, since nothing here can see it.
            cookie_secure: setting("COOKIE_SECURE").is_none_or(|v| !matches!(v.as_str(), "0" | "off" | "false")),
        })
    }
}

/// Key comes from `PORTIER_MASTER_KEY` (base64, 32 bytes) or from
/// `<data_dir>/master.key`, which is generated on first run.
fn load_master_key(data_dir: &std::path::Path) -> Result<[u8; 32]> {
    let encoded = match setting("MASTER_KEY") {
        Some(v) => v,
        None => {
            let path = data_dir.join("master.key");
            if !path.exists() {
                let mut key = [0u8; 32];
                getrandom::fill(&mut key).map_err(|e| anyhow::anyhow!("random source failed: {e}"))?;
                std::fs::write(&path, B64.encode(key))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
                }
            }
            std::fs::read_to_string(&path)?
        }
    };
    let bytes = B64.decode(encoded.trim()).context("master key is not valid base64")?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("master key must be exactly 32 bytes"))
}

#[cfg(test)]
mod tests {
    use super::setting;

    #[test]
    fn settings_take_the_new_name_and_fall_back_to_the_old_one() {
        // Names no other test or the app itself reads: the environment is shared by all tests.
        // SAFETY: nothing else touches these three variables.
        unsafe {
            std::env::set_var("EMSCREEN_TEST_ONLY_OLD", "old");
            std::env::set_var("EMSCREEN_TEST_BOTH", "old");
            std::env::set_var("PORTIER_TEST_BOTH", "new");
        }
        assert_eq!(setting("TEST_ONLY_OLD").as_deref(), Some("old"));
        assert_eq!(setting("TEST_BOTH").as_deref(), Some("new"));
        assert_eq!(setting("TEST_NEITHER"), None);
    }
}
