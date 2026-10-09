-- Failed sign-ins per address, so that guessing a password is slowed down and cannot be done
-- without limit. Old rows are dropped whenever a new one is written.
CREATE TABLE login_failures (
    address TEXT NOT NULL,
    at      INTEGER NOT NULL
);
CREATE INDEX login_failures_address ON login_failures(address, at);
