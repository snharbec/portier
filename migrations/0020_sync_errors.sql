-- Mail the server lists but this program cannot read: remembered by UID per folder, so the same
-- bytes are not fetched again on every sync pass. When a folder goes away, so do its reminders.
CREATE TABLE sync_errors (
    folder_id INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uid       INTEGER NOT NULL,
    at        INTEGER NOT NULL,
    PRIMARY KEY (folder_id, uid)
);
