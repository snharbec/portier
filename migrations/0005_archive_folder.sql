-- Folder that "archive" moves mail to and that the Archive view shows.
-- Empty until found on the server or set by the user.
ALTER TABLE accounts ADD COLUMN archive_folder TEXT NOT NULL DEFAULT '';
