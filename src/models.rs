use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Account {
    pub id: i64,
    pub user_id: i64,
    pub address: String,
    pub display_name: String,
    pub imap_host: String,
    pub imap_port: i64,
    pub imap_security: String,
    pub imap_username: String,
    pub smtp_host: String,
    pub smtp_port: i64,
    pub smtp_security: String,
    pub smtp_username: String,
    pub password_enc: String,
    pub inbox_folder: String,
    pub junk_folder: String,
    pub sent_folder: String,
    pub trash_folder: String,
    pub append_sent: bool,
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Folder {
    pub id: i64,
    pub name: String,
    pub role: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Addr {
    pub name: String,
    pub address: String,
}

pub const CATEGORIES: [&str; 3] = ["important", "feed", "junk"];

/// Sent mail is kept under this folder name when the account has no Sent folder on the server.
pub const LOCAL_SENT: &str = "__local_sent";
