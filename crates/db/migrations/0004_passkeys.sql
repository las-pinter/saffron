-- Passkey sign-in. The passkeys table exists since 0001.

-- Random 16-byte WebAuthn user handle, set at the first passkey registration. A
-- discoverable sign-in names the user only by this handle.
ALTER TABLE users ADD COLUMN webauthn_id BLOB;
CREATE UNIQUE INDEX users_webauthn_id ON users (webauthn_id);
