-- Texts a user keeps for mail written again and again; put into a draft when writing.
CREATE TABLE mail_templates (
    id        INTEGER PRIMARY KEY,
    user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name      TEXT NOT NULL COLLATE NOCASE,
    -- Formatted text as the editor makes it; may hold {first name}, {name} and {date}.
    body_html TEXT NOT NULL,
    UNIQUE (user_id, name)
);
