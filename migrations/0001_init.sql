CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    email         TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    is_admin      INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE sessions (
    token      TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL
);

CREATE TABLE accounts (
    id             INTEGER PRIMARY KEY,
    user_id        INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    label          TEXT NOT NULL,
    address        TEXT NOT NULL,
    display_name   TEXT NOT NULL DEFAULT '',
    imap_host      TEXT NOT NULL,
    imap_port      INTEGER NOT NULL,
    imap_security  TEXT NOT NULL,            -- tls | starttls | none
    imap_username  TEXT NOT NULL,
    smtp_host      TEXT NOT NULL,
    smtp_port      INTEGER NOT NULL,
    smtp_security  TEXT NOT NULL,            -- tls | starttls | none
    smtp_username  TEXT NOT NULL,
    password_enc   TEXT NOT NULL,
    inbox_folder   TEXT NOT NULL DEFAULT 'INBOX',
    junk_folder    TEXT NOT NULL DEFAULT '',
    sent_folder    TEXT NOT NULL DEFAULT '',
    append_sent    INTEGER NOT NULL DEFAULT 1,
    last_error     TEXT,
    last_sync_at   INTEGER,
    created_at     INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX accounts_user ON accounts(user_id);

CREATE TABLE folders (
    id          INTEGER PRIMARY KEY,
    account_id  INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    role        TEXT NOT NULL,               -- inbox | sent | junk
    uidvalidity INTEGER,
    UNIQUE (account_id, name)
);

CREATE TABLE senders (
    id           INTEGER PRIMARY KEY,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    address      TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    category     TEXT,                        -- important | feed | junk | NULL (screener)
    decided_at   INTEGER,
    created_at   INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (user_id, address)
);

CREATE TABLE threads (
    id        INTEGER PRIMARY KEY,
    user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    subject   TEXT NOT NULL DEFAULT '',
    sender_id INTEGER REFERENCES senders(id) ON DELETE SET NULL
);
CREATE INDEX threads_user ON threads(user_id);
CREATE INDEX threads_sender ON threads(sender_id);

CREATE TABLE messages (
    id              INTEGER PRIMARY KEY,
    user_id         INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    account_id      INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    folder_id       INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uid             INTEGER,                  -- NULL until the server copy has been seen by sync
    thread_id       INTEGER NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    sender_id       INTEGER REFERENCES senders(id) ON DELETE SET NULL,
    message_id      TEXT NOT NULL DEFAULT '',
    in_reply_to     TEXT NOT NULL DEFAULT '',
    refs            TEXT NOT NULL DEFAULT '',
    from_name       TEXT NOT NULL DEFAULT '',
    from_addr       TEXT NOT NULL DEFAULT '',
    to_addrs        TEXT NOT NULL DEFAULT '[]', -- JSON [{name,address}]
    cc_addrs        TEXT NOT NULL DEFAULT '[]',
    subject         TEXT NOT NULL DEFAULT '',
    date            INTEGER NOT NULL,
    snippet         TEXT NOT NULL DEFAULT '',
    seen            INTEGER NOT NULL DEFAULT 0,
    is_outgoing     INTEGER NOT NULL DEFAULT 0,
    has_attachments INTEGER NOT NULL DEFAULT 0,
    body_text       TEXT NOT NULL DEFAULT '',
    body_html       TEXT NOT NULL DEFAULT '',
    raw_path        TEXT NOT NULL DEFAULT ''
);
CREATE UNIQUE INDEX messages_folder_uid ON messages(folder_id, uid) WHERE uid IS NOT NULL;
CREATE INDEX messages_thread ON messages(thread_id, date);
CREATE INDEX messages_sender ON messages(sender_id);
CREATE INDEX messages_msgid ON messages(user_id, message_id);
CREATE INDEX messages_user_date ON messages(user_id, date);

CREATE TABLE attachments (
    id         INTEGER PRIMARY KEY,
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    idx        INTEGER NOT NULL,
    filename   TEXT NOT NULL,
    mime       TEXT NOT NULL,
    size       INTEGER NOT NULL,
    content_id TEXT,
    inline     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX attachments_message ON attachments(message_id);

CREATE TABLE drafts (
    id              INTEGER PRIMARY KEY,
    user_id         INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    account_id      INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
    kind            TEXT NOT NULL DEFAULT 'new',  -- new | reply | reply_all | forward
    source_message  INTEGER REFERENCES messages(id) ON DELETE SET NULL,
    to_addrs        TEXT NOT NULL DEFAULT '',
    cc_addrs        TEXT NOT NULL DEFAULT '',
    bcc_addrs       TEXT NOT NULL DEFAULT '',
    subject         TEXT NOT NULL DEFAULT '',
    body_html       TEXT NOT NULL DEFAULT '',
    forward_attachments INTEGER NOT NULL DEFAULT 0,
    include_quote   INTEGER NOT NULL DEFAULT 1,
    updated_at      INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE draft_attachments (
    id       INTEGER PRIMARY KEY,
    draft_id INTEGER NOT NULL REFERENCES drafts(id) ON DELETE CASCADE,
    filename TEXT NOT NULL,
    mime     TEXT NOT NULL,
    size     INTEGER NOT NULL,
    path     TEXT NOT NULL
);

CREATE VIRTUAL TABLE messages_fts USING fts5(
    subject, from_name, from_addr, to_addrs, body_text,
    content='messages', content_rowid='id'
);
CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, subject, from_name, from_addr, to_addrs, body_text)
    VALUES (new.id, new.subject, new.from_name, new.from_addr, new.to_addrs, new.body_text);
END;
CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, subject, from_name, from_addr, to_addrs, body_text)
    VALUES ('delete', old.id, old.subject, old.from_name, old.from_addr, old.to_addrs, old.body_text);
END;
