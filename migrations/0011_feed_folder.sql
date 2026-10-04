-- Folder on the mail server that mail from "Nice to know" senders is moved to, out of the inbox.
-- Empty: such mail stays in the inbox.
ALTER TABLE accounts ADD COLUMN feed_folder TEXT NOT NULL DEFAULT '';
