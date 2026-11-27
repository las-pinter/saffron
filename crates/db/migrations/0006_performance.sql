-- Performance: instrument classification, a portfolio benchmark, and targets.
--
-- Classification is nullable: when an instrument has none, the API derives it
-- from the instrument kind and the primary listing's exchange suffix.

ALTER TABLE instruments ADD COLUMN asset_class TEXT;
ALTER TABLE instruments ADD COLUMN region TEXT;

-- The instrument a portfolio compares itself against; NULL means the default.
ALTER TABLE portfolios ADD COLUMN benchmark_instrument_id INTEGER
    REFERENCES instruments (id) ON DELETE SET NULL;

-- Target weights in percent, one row per key and dimension.
CREATE TABLE portfolio_targets (
    portfolio_id INTEGER NOT NULL REFERENCES portfolios (id) ON DELETE CASCADE,
    dimension    TEXT    NOT NULL CHECK (dimension IN ('asset_class', 'region')),
    key          TEXT    NOT NULL,
    weight       TEXT    NOT NULL,
    PRIMARY KEY (portfolio_id, dimension, key)
) STRICT, WITHOUT ROWID;
