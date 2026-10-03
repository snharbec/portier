-- Searches a user named and keeps in the side bar.
CREATE TABLE saved_searches (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    query      TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX saved_searches_user ON saved_searches(user_id);
