-- Folder on the mail server that delayed conversations wait in, out of the inbox, until they
-- return. Empty: delayed mail stays where it is on the server.
ALTER TABLE accounts ADD COLUMN delayed_folder TEXT NOT NULL DEFAULT '';
