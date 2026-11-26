-- Failed sign-in attempts and account locks.
--
-- Failures are recorded under the submitted username even when no such user
-- exists, so lockout behaves the same for real and unknown names.

CREATE TABLE login_failures (
    id       INTEGER PRIMARY KEY,
    username TEXT    NOT NULL COLLATE NOCASE,
    -- Peer address of the client, for the per-client brake.
    source   TEXT    NOT NULL,
    at       TEXT    NOT NULL
) STRICT;

CREATE INDEX login_failures_username_at ON login_failures (username, at);
CREATE INDEX login_failures_source_at ON login_failures (source, at);

CREATE TABLE login_locks (
    username     TEXT PRIMARY KEY COLLATE NOCASE,
    locked_until TEXT NOT NULL
) STRICT, WITHOUT ROWID;
