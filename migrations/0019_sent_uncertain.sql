-- When a mail is handed to the SMTP server, `sent_at` records that it may be out. It is cleared
-- again when the server definitely refused the mail. A row that still carries it after a restart
-- is one whose sending was interrupted, so the mail is not offered for sending again by itself.
ALTER TABLE drafts ADD COLUMN sent_at INTEGER;
