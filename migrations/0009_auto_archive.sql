-- Read conversations in the Inbox whose last mail is older than this many weeks are archived
-- automatically. 0 turns it off.
ALTER TABLE users ADD COLUMN auto_archive_weeks INTEGER NOT NULL DEFAULT 0;
