-- A picture the user chose for a sender, shown instead of the one looked up (Gravatar, BIMI).
-- Always stored as a small JPEG made by Email Screen, never the bytes as they came.
CREATE TABLE sender_pictures (
    sender_id  INTEGER PRIMARY KEY REFERENCES senders(id) ON DELETE CASCADE,
    data       BLOB NOT NULL,
    -- 'upload', or the web address the picture was taken from.
    source     TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
