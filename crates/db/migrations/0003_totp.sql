-- TOTP enrolment and replay protection.

-- Encrypted secret offered during enrolment, until a first code confirms it.
ALTER TABLE users ADD COLUMN totp_pending TEXT;
-- Last accepted 30-second step, so a code is never accepted twice.
ALTER TABLE users ADD COLUMN totp_last_step INTEGER;
