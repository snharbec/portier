-- Sender pictures fetched from Gravatar (key "g:<address>") and BIMI (key "b:<domain>").
-- A row without data records that nothing was found, so the lookup is not repeated at once.
CREATE TABLE avatar_cache (
    key        TEXT PRIMARY KEY,
    mime       TEXT,
    data       BLOB,
    fetched_at INTEGER NOT NULL
);
