//! Authentication: first-run setup, password sign-in, sessions, and throttling.

pub mod ceremony;
pub mod client;
pub mod cookie;
pub mod extract;
pub mod gate;
pub mod origin;
pub mod passkey_login;
pub mod passkeys;
pub mod password;
pub mod routes;
pub mod session;
pub mod setup;
pub mod throttle;
pub mod token;
pub mod totp;
