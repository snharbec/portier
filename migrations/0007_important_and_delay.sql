-- Important: the server's \Flagged mark, mirrored per message.
ALTER TABLE messages ADD COLUMN flagged INTEGER NOT NULL DEFAULT 0;
CREATE INDEX messages_flagged ON messages(thread_id) WHERE flagged = 1;

-- Delay: a conversation is out of the lists until this moment (unix seconds), then returns unread.
ALTER TABLE threads ADD COLUMN snoozed_until INTEGER;
-- When a delayed conversation came back; sorts it to the top of the inbox.
ALTER TABLE threads ADD COLUMN returned_at INTEGER;
CREATE INDEX threads_snoozed ON threads(snoozed_until) WHERE snoozed_until IS NOT NULL;
