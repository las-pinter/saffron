//! Sign-in throttling: account lockout and a per-client brake.
//!
//! Five failures for one username within fifteen minutes lock that username for
//! fifteen minutes, even for the right password. Twenty failures from one client
//! address within fifteen minutes refuse that client until the oldest failure ages
//! out, which slows guessing across many usernames.

use jiff::{SignedDuration, Timestamp};
use saffron_db::login_throttle::{self, ThrottleError};
use sqlx::SqlitePool;

/// Failures count over this window.
pub const WINDOW: SignedDuration = SignedDuration::from_mins(15);
/// Failures for one username that lock it.
pub const ACCOUNT_MAX_FAILURES: i64 = 5;
/// How long a lock lasts.
pub const LOCK_DURATION: SignedDuration = SignedDuration::from_mins(15);
/// Failures from one client that stop it.
pub const SOURCE_MAX_FAILURES: i64 = 20;
/// Failure rows older than this are deleted.
const RETENTION: SignedDuration = SignedDuration::from_hours(24);

/// Whether a sign-in attempt may proceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Check the credentials.
    Allowed,
    /// The username is locked for `retry_after`.
    AccountLocked {
        /// Time until the lock ends.
        retry_after: SignedDuration,
    },
    /// The client failed too often; it may retry after `retry_after`.
    SourceThrottled {
        /// Time until the oldest failure in the window ages out.
        retry_after: SignedDuration,
    },
}

/// Decides whether `username` may try to sign in from `source` now.
pub async fn check(
    pool: &SqlitePool,
    username: &str,
    source: &str,
    now: Timestamp,
) -> Result<Verdict, ThrottleError> {
    if let Some(retry_after) = source_brake(pool, source, now).await? {
        return Ok(Verdict::SourceThrottled { retry_after });
    }
    if let Some(until) = login_throttle::locked_until(pool, username).await?
        && until > now
    {
        return Ok(Verdict::AccountLocked {
            retry_after: until - now,
        });
    }
    Ok(Verdict::Allowed)
}

/// How long `source` must wait when it failed too often, by any sign-in method.
pub async fn source_brake(
    pool: &SqlitePool,
    source: &str,
    now: Timestamp,
) -> Result<Option<SignedDuration>, ThrottleError> {
    let (failures, oldest) =
        login_throttle::failures_for_source(pool, source, now - WINDOW).await?;
    Ok(oldest
        .filter(|_| failures >= SOURCE_MAX_FAILURES)
        .map(|oldest| oldest + WINDOW - now))
}

/// The username a failure without one is recorded under. No account can have it,
/// so it counts toward the per-client brake only.
pub const NO_USERNAME: &str = "";

/// Records a failed sign-in that names no password account, such as a refused
/// passkey: it counts toward the per-client brake, never an account lockout.
pub async fn record_source_failure(
    pool: &SqlitePool,
    source: &str,
    now: Timestamp,
) -> Result<(), sqlx::Error> {
    login_throttle::record_failure(pool, NO_USERNAME, source, now).await?;
    login_throttle::prune_failures(pool, now - RETENTION).await
}

/// Records a failed attempt; returns `true` when this failure locked the username.
///
/// Failure rows stay after a lock, because the per-client brake counts them too.
/// The account count starts after the latest lock instead, so the failures that
/// caused a lock cannot re-lock the account on the first mistake after it ends.
pub async fn record_failure(
    pool: &SqlitePool,
    username: &str,
    source: &str,
    now: Timestamp,
) -> Result<bool, ThrottleError> {
    login_throttle::record_failure(pool, username, source, now).await?;
    login_throttle::prune_failures(pool, now - RETENTION).await?;
    login_throttle::prune_locks(pool, now).await?;
    let mut since = now - WINDOW;
    if let Some(until) = login_throttle::locked_until(pool, username).await? {
        let locked_at = until - LOCK_DURATION;
        since = since.max(locked_at + SignedDuration::from_secs(1));
    }
    let failures = login_throttle::failures_for_username(pool, username, since).await?;
    if failures < ACCOUNT_MAX_FAILURES {
        return Ok(false);
    }
    login_throttle::lock(pool, username, now + LOCK_DURATION).await?;
    Ok(true)
}

/// Forgets the failures and any expired lock of `username` after a sign-in.
pub async fn record_success(pool: &SqlitePool, username: &str) -> Result<(), sqlx::Error> {
    login_throttle::clear_failures(pool, username).await?;
    login_throttle::unlock(pool, username).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Timestamp {
        "2026-11-25T12:00:00Z".parse().expect("timestamp")
    }

    fn minutes(count: i64) -> SignedDuration {
        SignedDuration::from_mins(count)
    }

    async fn db() -> SqlitePool {
        saffron_db::open_in_memory().await.expect("database")
    }

    async fn fail(pool: &SqlitePool, username: &str, source: &str, at: Timestamp) -> bool {
        record_failure(pool, username, source, at)
            .await
            .expect("record failure")
    }

    #[tokio::test]
    async fn the_fifth_failure_locks_the_account() {
        let pool = db().await;
        let mut locked = Vec::new();
        for attempt in 0..5 {
            locked.push(fail(&pool, "las", "peer", now() + minutes(attempt)).await);
        }

        assert_eq!(locked, [false, false, false, false, true]);
    }

    #[tokio::test]
    async fn a_locked_account_is_refused_for_fifteen_minutes() {
        let pool = db().await;
        for _ in 0..5 {
            fail(&pool, "las", "peer", now()).await;
        }

        let verdict = check(&pool, "LAS", "other-peer", now() + minutes(14))
            .await
            .expect("check");

        assert_eq!(
            verdict,
            Verdict::AccountLocked {
                retry_after: minutes(1)
            }
        );
    }

    #[tokio::test]
    async fn the_lock_ends_after_fifteen_minutes() {
        let pool = db().await;
        for _ in 0..5 {
            fail(&pool, "las", "peer", now()).await;
        }

        let verdict = check(&pool, "las", "peer", now() + minutes(15))
            .await
            .expect("check");

        assert_eq!(verdict, Verdict::Allowed);
    }

    #[tokio::test]
    async fn failures_spread_over_more_than_the_window_do_not_lock() {
        let pool = db().await;
        let mut locked = false;
        for attempt in 0..5 {
            locked |= fail(&pool, "las", "peer", now() + minutes(attempt * 4)).await;
        }

        assert!(!locked);
    }

    #[tokio::test]
    async fn the_first_failure_after_a_lock_does_not_relock() {
        let pool = db().await;
        for _ in 0..5 {
            fail(&pool, "las", "peer", now()).await;
        }

        let relocked = fail(&pool, "las", "peer", now() + LOCK_DURATION).await;

        assert!(!relocked);
    }

    #[tokio::test]
    async fn success_clears_the_failure_count() {
        let pool = db().await;
        for _ in 0..4 {
            fail(&pool, "las", "peer", now()).await;
        }

        record_success(&pool, "las").await.expect("success");
        let locked = fail(&pool, "las", "peer", now()).await;

        assert!(!locked);
    }

    #[tokio::test]
    async fn a_client_failing_on_many_usernames_is_throttled() {
        let pool = db().await;
        for attempt in 0..20 {
            fail(&pool, &format!("user{attempt}"), "10.0.0.9", now()).await;
        }

        let verdict = check(&pool, "fresh", "10.0.0.9", now() + minutes(5))
            .await
            .expect("check");

        assert_eq!(
            verdict,
            Verdict::SourceThrottled {
                retry_after: minutes(10)
            }
        );
    }

    #[tokio::test]
    async fn a_client_locking_account_after_account_is_throttled() {
        let pool = db().await;
        for account in 0..4 {
            for _ in 0..5 {
                fail(&pool, &format!("user{account}"), "10.0.0.9", now()).await;
            }
        }

        let verdict = check(&pool, "fresh", "10.0.0.9", now())
            .await
            .expect("check");

        assert!(
            matches!(verdict, Verdict::SourceThrottled { .. }),
            "verdict: {verdict:?}"
        );
    }

    #[tokio::test]
    async fn another_client_is_not_throttled() {
        let pool = db().await;
        for attempt in 0..SOURCE_MAX_FAILURES {
            fail(&pool, &format!("user{attempt}"), "10.0.0.9", now()).await;
        }

        let verdict = check(&pool, "fresh", "10.0.0.10", now())
            .await
            .expect("check");

        assert_eq!(verdict, Verdict::Allowed);
    }
}
