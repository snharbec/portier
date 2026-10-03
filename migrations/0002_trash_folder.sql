-- Folder that "delete" moves mail to. Empty until found on the server or set by the user.
ALTER TABLE accounts ADD COLUMN trash_folder TEXT NOT NULL DEFAULT '';
