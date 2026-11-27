-- Drop the unused transactions.import_key column.
--
-- Imports match duplicates by field, so the column never held a value. SQLite
-- cannot drop a column that carries a UNIQUE constraint, so the table is
-- rebuilt without it.

CREATE TABLE transactions_new (
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

INSERT INTO transactions_new
    (id, account_id, instrument_id, kind, trade_date, quantity, price, amount, fee,
     currency, fx_rate, note, created_by, created_at, updated_at)
SELECT id, account_id, instrument_id, kind, trade_date, quantity, price, amount, fee,
       currency, fx_rate, note, created_by, created_at, updated_at
FROM transactions;

DROP TABLE transactions;
ALTER TABLE transactions_new RENAME TO transactions;

CREATE INDEX transactions_account_date ON transactions (account_id, trade_date);
CREATE INDEX transactions_instrument ON transactions (instrument_id);
