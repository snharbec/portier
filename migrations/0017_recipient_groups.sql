-- Groups of recipients a user set up: a name that stands for several addresses when writing.
CREATE TABLE recipient_groups (
    id      INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name    TEXT NOT NULL COLLATE NOCASE,
    -- The addresses, separated by ", ": as they are put into the recipient field.
    members TEXT NOT NULL,
    UNIQUE (user_id, name)
);
