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
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let bind = std::env::var("EMSCREEN_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8080".into())
            .parse()
            .context("EMSCREEN_BIND is not a valid socket address")?;
        let data_dir = PathBuf::from(std::env::var("EMSCREEN_DATA_DIR").unwrap_or_else(|_| "data".into()));
        std::fs::create_dir_all(&data_dir).context("cannot create data directory")?;
        let master_key = load_master_key(&data_dir)?;
        let open_registration = matches!(
            std::env::var("EMSCREEN_OPEN_REGISTRATION").as_deref(),
            Ok("1") | Ok("true")
        );
        let sync_max_per_folder = std::env::var("EMSCREEN_SYNC_MAX_PER_FOLDER")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5000);
        Ok(Self {
            bind,
            data_dir,
            master_key,
            open_registration,
            sync_max_per_folder,
        })
    }
}

/// Key comes from `EMSCREEN_MASTER_KEY` (base64, 32 bytes) or from
/// `<data_dir>/master.key`, which is generated on first run.
fn load_master_key(data_dir: &std::path::Path) -> Result<[u8; 32]> {
    let encoded = match std::env::var("EMSCREEN_MASTER_KEY") {
        Ok(v) => v,
        Err(_) => {
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
