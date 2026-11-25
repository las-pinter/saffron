-- Initial Saffron schema.
--
-- Conventions:
--   * STRICT tables, so SQLite enforces the declared column types.
--   * Decimal amounts and quantities are TEXT in canonical rust_decimal form
--     (never REAL), parsed and validated in Rust.
--   * Timestamps are RFC 3339 UTC TEXT; calendar dates are YYYY-MM-DD TEXT.
--   * Currencies are upper-case ISO 4217 codes.

CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    display_name  TEXT    NOT NULL,
    is_admin      INTEGER NOT NULL DEFAULT 0 CHECK (is_admin IN (0, 1)),
    -- Argon2id PHC string; NULL for passkey-only users.
    password_hash TEXT,
    -- Encrypted TOTP secret; NULL when TOTP is off.
    totp_secret   TEXT,
    created_at    TEXT    NOT NULL,
    disabled_at   TEXT
) STRICT;

CREATE TABLE passkeys (
    id            INTEGER PRIMARY KEY,
    user_id       INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    credential_id BLOB    NOT NULL UNIQUE,
    -- Serialized WebAuthn credential (public key and counter).
    credential    TEXT    NOT NULL,
    name          TEXT    NOT NULL,
    created_at    TEXT    NOT NULL,
    last_used_at  TEXT
) STRICT;

CREATE INDEX passkeys_user ON passkeys (user_id);

CREATE TABLE sessions (
    -- SHA-256 of the session token; the token itself is never stored.
    token_hash   BLOB    PRIMARY KEY,
    user_id      INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at   TEXT    NOT NULL,
    last_seen_at TEXT    NOT NULL,
    expires_at   TEXT    NOT NULL
) STRICT, WITHOUT ROWID;

CREATE INDEX sessions_user ON sessions (user_id);

CREATE TABLE portfolios (
    id            INTEGER PRIMARY KEY,
    owner_id      INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name          TEXT    NOT NULL,
    base_currency TEXT    NOT NULL
        CHECK (length(base_currency) = 3 AND base_currency = upper(base_currency)),
    created_at    TEXT    NOT NULL,
    archived_at   TEXT,
    UNIQUE (owner_id, name)
) STRICT;

CREATE TABLE portfolio_shares (
    portfolio_id INTEGER NOT NULL REFERENCES portfolios (id) ON DELETE CASCADE,
    user_id      INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role         TEXT    NOT NULL CHECK (role IN ('viewer', 'editor')),
    created_at   TEXT    NOT NULL,
    PRIMARY KEY (portfolio_id, user_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX portfolio_shares_user ON portfolio_shares (user_id);

CREATE TABLE accounts (
    id           INTEGER PRIMARY KEY,
    portfolio_id INTEGER NOT NULL REFERENCES portfolios (id) ON DELETE CASCADE,
    name         TEXT    NOT NULL,
    kind         TEXT    NOT NULL
        CHECK (kind IN ('book_entry', 'equity_savings', 'fund', 'savings', 'cash', 'other')),
    currency     TEXT    NOT NULL CHECK (length(currency) = 3 AND currency = upper(currency)),
    institution  TEXT,
    created_at   TEXT    NOT NULL,
    closed_at    TEXT,
    UNIQUE (portfolio_id, name)
) STRICT;

-- Instruments and market data are shared by all users.
CREATE TABLE instruments (
    id         INTEGER PRIMARY KEY,
    kind       TEXT    NOT NULL CHECK (kind IN ('stock', 'etf', 'fund', 'bond', 'crypto', 'other')),
    name       TEXT    NOT NULL,
    isin       TEXT    UNIQUE CHECK (isin IS NULL OR (length(isin) = 12 AND isin = upper(isin))),
    currency   TEXT    NOT NULL CHECK (length(currency) = 3 AND currency = upper(currency)),
    created_at TEXT    NOT NULL
) STRICT;

-- How a price provider names an instrument (Yahoo `NOKIA.HE`, FT `FI0009000681:EUR`).
CREATE TABLE instrument_listings (
    id            INTEGER PRIMARY KEY,
    instrument_id INTEGER NOT NULL REFERENCES instruments (id) ON DELETE CASCADE,
    provider      TEXT    NOT NULL,
    symbol        TEXT    NOT NULL,
    exchange      TEXT,
    currency      TEXT    NOT NULL CHECK (length(currency) = 3 AND currency = upper(currency)),
    is_primary    INTEGER NOT NULL DEFAULT 0 CHECK (is_primary IN (0, 1)),
    UNIQUE (provider, symbol)
) STRICT;

CREATE INDEX instrument_listings_instrument ON instrument_listings (instrument_id);
CREATE UNIQUE INDEX instrument_listings_one_primary
    ON instrument_listings (instrument_id) WHERE is_primary = 1;

CREATE TABLE transactions (
    id            INTEGER PRIMARY KEY,
    account_id    INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    instrument_id INTEGER REFERENCES instruments (id) ON DELETE RESTRICT,
    kind          TEXT    NOT NULL CHECK (kind IN (
        'buy', 'sell', 'dividend', 'fee', 'deposit', 'withdrawal', 'interest',
        'split', 'transfer_in', 'transfer_out'
    )),
    trade_date    TEXT    NOT NULL,
    -- Units bought, sold, or transferred; for a split, the new-per-old ratio.
    quantity      TEXT,
    -- Price per unit in `currency`.
    price         TEXT,
    -- Cash amount of the transaction in `currency`, before fees.
    amount        TEXT,
    fee           TEXT    NOT NULL DEFAULT '0',
    currency      TEXT    NOT NULL CHECK (length(currency) = 3 AND currency = upper(currency)),
    -- Units of the account currency per unit of `currency` on the trade date,
    -- when the two differ.
    fx_rate       TEXT,
    note          TEXT,
    -- Stable key of an imported row, so a re-import creates no duplicate.
    import_key    TEXT    UNIQUE,
    created_by    INTEGER REFERENCES users (id) ON DELETE SET NULL,
    created_at    TEXT    NOT NULL,
    updated_at    TEXT    NOT NULL,
    CHECK (CASE
        WHEN kind IN ('buy', 'sell', 'dividend', 'split', 'transfer_in', 'transfer_out')
            THEN instrument_id IS NOT NULL
        WHEN kind IN ('deposit', 'withdrawal', 'interest')
            THEN instrument_id IS NULL
        ELSE 1
    END)
) STRICT;

CREATE INDEX transactions_account_date ON transactions (account_id, trade_date);
CREATE INDEX transactions_instrument ON transactions (instrument_id);

-- One price per instrument and day; intraday quotes overwrite the day's row.
CREATE TABLE prices (
    instrument_id INTEGER NOT NULL REFERENCES instruments (id) ON DELETE CASCADE,
    date          TEXT    NOT NULL,
    close         TEXT    NOT NULL,
    currency      TEXT    NOT NULL CHECK (length(currency) = 3 AND currency = upper(currency)),
    source        TEXT    NOT NULL,
    -- When the provider says the price was valid.
    as_of         TEXT    NOT NULL,
    PRIMARY KEY (instrument_id, date)
) STRICT, WITHOUT ROWID;

-- Reference rates as units of `currency` per one EUR (the ECB convention).
CREATE TABLE fx_rates (
    date     TEXT NOT NULL,
    currency TEXT NOT NULL CHECK (length(currency) = 3 AND currency = upper(currency)),
    per_eur  TEXT NOT NULL,
    source   TEXT NOT NULL,
    PRIMARY KEY (date, currency)
) STRICT, WITHOUT ROWID;

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE user_settings (
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    key     TEXT    NOT NULL,
    value   TEXT    NOT NULL,
    PRIMARY KEY (user_id, key)
) STRICT, WITHOUT ROWID;

CREATE TABLE audit_log (
    id      INTEGER PRIMARY KEY,
    at      TEXT    NOT NULL,
    user_id INTEGER REFERENCES users (id) ON DELETE SET NULL,
    action  TEXT    NOT NULL,
    target  TEXT,
    detail  TEXT
) STRICT;

CREATE INDEX audit_log_at ON audit_log (at);
