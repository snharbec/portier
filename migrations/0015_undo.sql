-- A draft that is about to be sent waits a few seconds, during which sending can be undone.
-- While it waits it is no draft to edit; if sending fails afterwards, the reason is kept with it.
ALTER TABLE drafts ADD COLUMN sending_at INTEGER;
ALTER TABLE drafts ADD COLUMN last_error TEXT;
