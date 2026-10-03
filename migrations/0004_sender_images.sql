-- Remote images in mail from this sender are loaded without asking.
ALTER TABLE senders ADD COLUMN show_images INTEGER NOT NULL DEFAULT 0;
