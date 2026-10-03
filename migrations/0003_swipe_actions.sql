-- What sliding a mail left or right does, per user: comma-separated list of read, move, trash.
-- One action is performed on release; several are offered as buttons; empty turns the direction off.
ALTER TABLE users ADD COLUMN swipe_left TEXT NOT NULL DEFAULT 'trash';
ALTER TABLE users ADD COLUMN swipe_right TEXT NOT NULL DEFAULT 'read';
