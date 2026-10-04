-- Summaries of new mail, written by a local language model (Ollama).
--
-- One installation has one model, set by its administrator: the server itself calls the address
-- given here, so it is not something each user may point anywhere.
CREATE TABLE ai_settings (
    id       INTEGER PRIMARY KEY CHECK (id = 1),
    url      TEXT NOT NULL DEFAULT '',     -- e.g. http://127.0.0.1:11434; empty: no summaries
    model    TEXT NOT NULL DEFAULT '',
    language TEXT NOT NULL DEFAULT 'English'
);
INSERT INTO ai_settings (id) VALUES (1);

-- What the model said about a mail and about its attachments. Made once per mail.
CREATE TABLE summaries (
    message_id  INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    text        TEXT NOT NULL DEFAULT '',
    attachments TEXT NOT NULL DEFAULT '[]',  -- JSON [{filename, text}]
    model       TEXT NOT NULL DEFAULT '',
    -- Why there is no summary, when the model could not make one for this mail.
    error       TEXT,
    created_at  INTEGER NOT NULL
);
