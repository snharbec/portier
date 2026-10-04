//! Backups of the whole installation: the database, the stored mail, the drafts' files and the
//! key that protects the mail passwords, in one `.tar.gz` file.
//!
//! A backup can be written while the server runs. Restoring replaces the data folder and is
//! done with the server stopped (`portier restore FILE`).

use std::{
    fs::File,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use sqlx::SqlitePool;

use crate::state::{AppState, now};

const DATABASE: &str = "emscreen.db";
const KEY: &str = "master.key";
const MANIFEST: &str = "portier-backup.json";
/// Folders of the data folder that belong to a backup.
const FOLDERS: [&str; 2] = ["mail", "drafts"];
/// Backups the server writes by itself are named like this; so many of them are kept.
const PREFIX: &str = "portier-";
const SUFFIX: &str = ".tar.gz";
pub const KEEP: usize = 7;

/// `portier-2026-10-04.tar.gz`, or with the time of day as well.
pub fn file_name(with_time: bool) -> String {
    let format = if with_time { "%Y-%m-%d-%H%M%S" } else { "%Y-%m-%d" };
    format!("{PREFIX}{}{SUFFIX}", chrono::Local::now().format(format))
}

/// Readable by the owner only: a backup holds every user's mail and the key to the mail passwords.
fn create_private(path: &Path) -> Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .with_context(|| format!("cannot write {}", path.display()))
}

/// Writes a backup to `target` and returns its size. Works while the server runs: the database
/// is copied in one consistent piece first.
pub async fn write(db: &SqlitePool, data_dir: &Path, target: &Path) -> Result<u64> {
    let work = data_dir.join("tmp").join(crate::crypto::random_token());
    tokio::fs::create_dir_all(&work).await?;
    let result = write_in(db, data_dir, target, &work).await;
    let _ = tokio::fs::remove_dir_all(&work).await;
    result
}

async fn write_in(db: &SqlitePool, data_dir: &Path, target: &Path, work: &Path) -> Result<u64> {
    let snapshot = work.join(DATABASE);
    let quoted = snapshot.to_string_lossy().replace('\'', "''");
    sqlx::query(sqlx::AssertSqlSafe(format!("VACUUM INTO '{quoted}'")))
        .execute(db)
        .await
        .context("cannot copy the database")?;

    let (data_dir, target) = (data_dir.to_path_buf(), target.to_path_buf());
    tokio::task::spawn_blocking(move || -> Result<u64> {
        // Written beside the target and renamed, so that a backup is either whole or not there.
        let partial = target.with_extension("partial");
        let mut archive = tar::Builder::new(GzEncoder::new(create_private(&partial)?, Compression::default()));
        let manifest = serde_json::json!({
            "portier_backup": 1,
            "version": env!("CARGO_PKG_VERSION"),
            "created_at": now(),
        })
        .to_string();
        let mut header = tar::Header::new_gnu();
        header.set_size(manifest.len() as u64);
        header.set_mode(0o600);
        header.set_mtime(now() as u64);
        header.set_cksum();
        archive.append_data(&mut header, MANIFEST, manifest.as_bytes())?;
        archive.append_path_with_name(&snapshot, DATABASE)?;
        let key = data_dir.join(KEY);
        if key.is_file() {
            archive.append_path_with_name(&key, KEY)?;
        }
        for folder in FOLDERS {
            add_folder(&mut archive, &data_dir.join(folder), Path::new(folder))?;
        }
        archive.into_inner()?.finish()?.sync_all()?;
        std::fs::rename(&partial, &target)?;
        Ok(std::fs::metadata(&target)?.len())
    })
    .await?
}

/// Adds the files of a folder, without the previews of attachments: those are made again on demand.
fn add_folder(archive: &mut tar::Builder<GzEncoder<File>>, dir: &Path, name: &Path) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        let (path, inside) = (entry.path(), name.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if path.extension().is_none_or(|extension| extension != "previews") {
                add_folder(archive, &path, &inside)?;
            }
        } else if kind.is_file() {
            // A mail deleted since the folder was listed is simply not in the backup.
            match archive.append_path_with_name(&path, &inside) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                other => other?,
            }
        }
    }
    Ok(())
}

/// Puts a backup in place of the data folder's content. What was there is moved into a folder
/// `before-restore-…` inside the data folder, not deleted. The server must not be running.
pub fn restore(data_dir: &Path, file: &Path, replace: bool) -> Result<PathBuf> {
    let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S").to_string();
    let fresh = data_dir.join(format!("restore-{stamp}.tmp"));
    std::fs::create_dir_all(&fresh)?;
    let unpacked = unpack(file, &fresh);
    if let Err(error) = unpacked {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(error);
    }

    let parts = [DATABASE, KEY, FOLDERS[0], FOLDERS[1]];
    let existing = data_dir.join(DATABASE).exists();
    if existing && !replace {
        let _ = std::fs::remove_dir_all(&fresh);
        bail!(
            "{} holds a database already. Add --replace to put the backup in its place; \
             what is there now is moved aside, not deleted.",
            data_dir.display()
        );
    }
    let aside = data_dir.join(format!("before-restore-{stamp}"));
    if existing {
        std::fs::create_dir_all(&aside)?;
    }
    // The database's side files belong to the old database only.
    for name in parts.iter().copied().chain(["emscreen.db-wal", "emscreen.db-shm"]) {
        let current = data_dir.join(name);
        if !current.exists() {
            continue;
        }
        if existing {
            std::fs::rename(&current, aside.join(name))?;
        } else if current.is_dir() {
            std::fs::remove_dir_all(&current)?;
        } else {
            // Only a key made a moment ago for a data folder that is otherwise empty.
            std::fs::remove_file(&current)?;
        }
    }
    for name in parts {
        let restored = fresh.join(name);
        if restored.exists() {
            std::fs::rename(&restored, data_dir.join(name))?;
        }
    }
    std::fs::remove_dir_all(&fresh)?;
    Ok(aside)
}

/// Unpacks a backup into an empty folder, taking only what a backup holds.
fn unpack(file: &Path, into: &Path) -> Result<()> {
    let source = File::open(file).with_context(|| format!("cannot read {}", file.display()))?;
    let mut archive = tar::Archive::new(GzDecoder::new(source));
    let mut manifest = false;
    for entry in archive.entries().context("this is not a Portier backup")? {
        let mut entry = entry.context("the backup is damaged")?;
        let path = entry.path()?.into_owned();
        let first = path
            .components()
            .next()
            .map(|part| part.as_os_str().to_string_lossy().into_owned());
        let known = matches!(first.as_deref(), Some(DATABASE | KEY | MANIFEST)) && path.components().count() == 1
            || first.as_deref().is_some_and(|name| FOLDERS.contains(&name));
        let plain = matches!(
            entry.header().entry_type(),
            tar::EntryType::Regular | tar::EntryType::Directory
        );
        if !known || !plain {
            bail!("this is not a Portier backup: it holds {}", path.display());
        }
        manifest |= first.as_deref() == Some(MANIFEST);
        // `unpack_in` refuses paths that would leave the folder.
        if !entry.unpack_in(into)? {
            bail!("this is not a Portier backup: it holds {}", path.display());
        }
    }
    if !manifest || !into.join(DATABASE).is_file() {
        bail!("this is not a Portier backup");
    }
    let _ = std::fs::remove_file(into.join(MANIFEST));
    Ok(())
}

// ---- Backups the server writes by itself ---------------------------------------------------

#[derive(sqlx::FromRow, serde::Serialize)]
pub struct Settings {
    pub dir: String,
    pub last_at: Option<i64>,
    pub last_file: Option<String>,
    pub last_error: Option<String>,
}

pub async fn settings(db: &SqlitePool) -> Result<Settings> {
    Ok(
        sqlx::query_as("SELECT dir, last_at, last_file, last_error FROM backup_settings WHERE id = 1")
            .fetch_one(db)
            .await?,
    )
}

/// The server's own backups in a folder, oldest first.
fn own_backups(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(PREFIX) && name.ends_with(SUFFIX))
        })
        .collect();
    // The date in the name sorts them.
    files.sort();
    files
}

/// Writes a backup into the folder that is set and deletes all but the newest [`KEEP`].
/// Records how it went. `Ok(None)`: no folder is set.
pub async fn write_to_folder(state: &AppState, with_time: bool) -> Result<Option<PathBuf>> {
    let dir = settings(&state.db).await?.dir;
    if dir.is_empty() {
        return Ok(None);
    }
    let dir = PathBuf::from(dir);
    let target = dir.join(file_name(with_time));
    let written = async {
        tokio::fs::create_dir_all(&dir)
            .await
            .with_context(|| format!("cannot create {}", dir.display()))?;
        write(&state.db, &state.config.data_dir, &target).await?;
        let files = own_backups(&dir);
        for old in &files[..files.len().saturating_sub(KEEP)] {
            let _ = std::fs::remove_file(old);
        }
        anyhow::Ok(())
    }
    .await;
    let (file, error) = match &written {
        Ok(()) => (Some(target.display().to_string()), None),
        Err(error) => (None, Some(format!("{error:#}"))),
    };
    sqlx::query(
        "UPDATE backup_settings SET last_error = ?1, last_at = CASE WHEN ?1 IS NULL THEN ?2 ELSE last_at END,
             last_file = COALESCE(?3, last_file) WHERE id = 1",
    )
    .bind(error)
    .bind(now())
    .bind(file)
    .execute(&state.db)
    .await?;
    written.map(|()| Some(target))
}

/// Once a day, when a folder is set: checked a few minutes after the start, then every hour,
/// and written when today's backup is not there yet.
pub async fn run(state: AppState) {
    tokio::time::sleep(Duration::from_secs(180)).await;
    loop {
        let due = match settings(&state.db).await {
            Ok(settings) if !settings.dir.is_empty() => !Path::new(&settings.dir).join(file_name(false)).exists(),
            _ => false,
        };
        if due && let Err(error) = write_to_folder(&state, false).await {
            tracing::warn!("backup failed: {error:#}");
        }
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_backup_is_written_while_running_and_restores_the_data_folder() {
        let root = std::env::temp_dir().join(format!("portier-backup-test-{}", crate::crypto::random_token()));
        let data = root.join("data");
        std::fs::create_dir_all(data.join("mail/1/7.previews")).unwrap();
        std::fs::write(data.join("mail/1/7.eml"), "Subject: kept\r\n\r\nhello").unwrap();
        std::fs::write(data.join("mail/1/7.previews/page.png"), "regenerable").unwrap();
        std::fs::write(data.join(KEY), "the key").unwrap();
        let db = crate::open_database(&format!("sqlite://{}", data.join(DATABASE).display()))
            .await
            .unwrap();
        sqlx::query("INSERT INTO users (email, password_hash) VALUES ('a@example.org', 'x')")
            .execute(&db)
            .await
            .unwrap();

        let file = root.join("backup.tar.gz");
        assert!(write(&db, &data, &file).await.unwrap() > 0);
        assert!(
            !data.join("tmp").read_dir().unwrap().any(|_| true),
            "nothing is left behind"
        );

        // Into an empty data folder.
        let empty = root.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        restore(&empty, &file, false).unwrap();
        assert_eq!(
            std::fs::read_to_string(empty.join("mail/1/7.eml")).unwrap(),
            "Subject: kept\r\n\r\nhello"
        );
        assert_eq!(std::fs::read_to_string(empty.join(KEY)).unwrap(), "the key");
        assert!(
            !empty.join("mail/1/7.previews").exists(),
            "previews are not part of a backup"
        );
        let restored = crate::open_database(&format!("sqlite://{}", empty.join(DATABASE).display()))
            .await
            .unwrap();
        let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&restored)
            .await
            .unwrap();
        assert_eq!(users, 1);

        // Over existing data: only when asked, and what was there is moved aside.
        sqlx::query("INSERT INTO users (email, password_hash) VALUES ('later@example.org', 'x')")
            .execute(&db)
            .await
            .unwrap();
        db.close().await;
        assert!(restore(&data, &file, false).is_err());
        let aside = restore(&data, &file, true).unwrap();
        assert!(aside.join(DATABASE).is_file() && aside.join("mail/1/7.eml").is_file());
        let again = crate::open_database(&format!("sqlite://{}", data.join(DATABASE).display()))
            .await
            .unwrap();
        let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&again)
            .await
            .unwrap();
        assert_eq!(users, 1, "the user added after the backup is gone");

        // Something that is not a backup is refused and changes nothing.
        let other = root.join("other.tar.gz");
        let mut archive = tar::Builder::new(GzEncoder::new(File::create(&other).unwrap(), Compression::default()));
        let mut header = tar::Header::new_gnu();
        header.set_size(1);
        header.set_cksum();
        archive.append_data(&mut header, "elsewhere.txt", &b"x"[..]).unwrap();
        archive.into_inner().unwrap().finish().unwrap();
        assert!(restore(&empty, &other, true).is_err());
        assert!(empty.join(DATABASE).is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_the_newest_own_backups_are_kept_in_order() {
        let dir = std::env::temp_dir().join(format!("portier-backups-{}", crate::crypto::random_token()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in [
            "portier-2026-10-02.tar.gz",
            "portier-2026-09-30.tar.gz",
            "notes.txt",
            "portier-2026-10-01.tar.gz",
        ] {
            std::fs::write(dir.join(name), "").unwrap();
        }
        let names: Vec<String> = own_backups(&dir)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "portier-2026-09-30.tar.gz",
                "portier-2026-10-01.tar.gz",
                "portier-2026-10-02.tar.gz"
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
