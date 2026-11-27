-- Price refresh: exchange time zones on listings and refresh times on prices.

-- The exchange's IANA time zone for a listing, such as `Europe/Helsinki`, as the
-- provider names it. The price schedule and the stale check read trading days and
-- hours in this zone; NULL when the provider names none.
ALTER TABLE instrument_listings ADD COLUMN timezone TEXT;

-- When a provider last confirmed a price, set on every refresh even when the price
-- is unchanged. Staleness itself is judged from `as_of` and the price's day in
-- `saffron_core::freshness`, not from this column.
ALTER TABLE prices ADD COLUMN checked_at TEXT;

-- When a price refresh was last attempted for a listing, whether or not it
-- produced a price. Retries are spaced from this, so a listing the provider has
-- nothing for is not requested on every tick.
ALTER TABLE instrument_listings ADD COLUMN last_attempt_at TEXT;

-- When a history backfill was last attempted. Kept apart from last_attempt_at so
-- fetching history never holds back the day's quote (and the reverse).
ALTER TABLE instrument_listings ADD COLUMN history_attempt_at TEXT;

-- When the listing's one-year history was successfully fetched, so it is asked
-- for once and never again. Backfill selects on this mark, not on having a price,
-- so a quote stored first never strands an instrument without its history.
ALTER TABLE instrument_listings ADD COLUMN history_done_at TEXT;

-- When the time zone lookup was last attempted, so a listing Yahoo names no zone
-- for is not asked on every tick.
ALTER TABLE instrument_listings ADD COLUMN timezone_attempt_at TEXT;
