-- Backups of the whole installation that the server writes by itself, once a day.
CREATE TABLE backup_settings (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    dir        TEXT NOT NULL DEFAULT '',   -- folder the backups go to; empty: none are written
    last_at    INTEGER,
    last_file  TEXT,
    last_error TEXT
);
INSERT INTO backup_settings (id) VALUES (1);
