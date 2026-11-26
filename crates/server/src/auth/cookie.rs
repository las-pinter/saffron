//! The session cookie.

use super::origin::PublicOrigin;

/// Cookie name on an HTTPS origin. The `__Host-` prefix makes the browser refuse
/// the cookie without `Secure` and `Path=/`, or with a `Domain`.
pub const HTTPS_NAME: &str = "__Host-saffron_session";
/// Cookie name on the localhost origin, where plain HTTP cannot carry `Secure`.
pub const LOCALHOST_NAME: &str = "saffron_session";

/// The cookie name for `origin`.
#[must_use]
pub fn name(origin: &PublicOrigin) -> &'static str {
    if origin.is_https() {
        HTTPS_NAME
    } else {
        LOCALHOST_NAME
    }
}

/// The `Set-Cookie` value that starts a session lasting `max_age_secs`.
#[must_use]
pub fn set(origin: &PublicOrigin, token: &str, max_age_secs: i64) -> String {
    format!(
        "{}={token}; Path=/; HttpOnly{}; SameSite=Lax; Max-Age={max_age_secs}",
        name(origin),
        secure(origin)
    )
}

/// The `Set-Cookie` value that removes the session cookie.
#[must_use]
pub fn clear(origin: &PublicOrigin) -> String {
    format!(
        "{}=; Path=/; HttpOnly{}; SameSite=Lax; Max-Age=0",
        name(origin),
        secure(origin)
    )
}

/// The session token in a `Cookie` header.
///
/// Only the origin's own cookie name counts: on HTTPS, honoring the plain name would
/// let a sibling host plant a `Domain` cookie that shadows the `__Host-` one.
#[must_use]
pub fn token<'a>(origin: &PublicOrigin, header: &'a str) -> Option<&'a str> {
    let expected = name(origin);
    header
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, value)| *name == expected && !value.is_empty())
        .map(|(_, value)| value)
}

fn secure(origin: &PublicOrigin) -> &'static str {
    if origin.is_https() { "; Secure" } else { "" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn https() -> PublicOrigin {
        PublicOrigin::parse("https://example.test").expect("origin")
    }

    fn localhost() -> PublicOrigin {
        PublicOrigin::parse("http://localhost:8640").expect("origin")
    }

    #[test]
    fn https_cookie_is_host_prefixed_and_secure() {
        assert_eq!(
            set(&https(), "tok", 60),
            "__Host-saffron_session=tok; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=60"
        );
    }

    #[test]
    fn localhost_cookie_has_no_secure_flag() {
        assert_eq!(
            set(&localhost(), "tok", 60),
            "saffron_session=tok; Path=/; HttpOnly; SameSite=Lax; Max-Age=60"
        );
    }

    #[test]
    fn clearing_expires_the_cookie_now() {
        assert_eq!(
            clear(&https()),
            "__Host-saffron_session=; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=0"
        );
    }

    #[test]
    fn token_is_read_among_other_cookies() {
        assert_eq!(
            token(&https(), "a=b; __Host-saffron_session=tok; c=d"),
            Some("tok")
        );
    }

    #[test]
    fn https_ignores_the_unprefixed_name() {
        assert_eq!(token(&https(), "saffron_session=planted"), None);
    }

    #[test]
    fn an_empty_value_is_no_token() {
        assert_eq!(token(&localhost(), "saffron_session="), None);
    }
}
