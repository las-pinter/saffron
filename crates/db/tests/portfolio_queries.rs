//! Portfolio queries: ownership, the role a user holds, and sharing.

use jiff::Timestamp;
use saffron_db::{
    is_unique_violation, open_in_memory,
    portfolios::{self, NewPortfolio, Role, ShareRole},
    users::{self, NewUser},
};
use sqlx::SqlitePool;

fn now() -> Timestamp {
    "2026-11-26T12:00:00Z".parse().expect("timestamp")
}

async fn db() -> SqlitePool {
    open_in_memory().await.expect("in-memory database")
}

async fn user(pool: &SqlitePool, username: &str) -> i64 {
    let user = NewUser {
        username,
        display_name: username,
        is_admin: false,
        password_hash: Some("$argon2id$placeholder"),
    };
    users::create(pool, user, now()).await.expect("create user")
}

async fn portfolio(pool: &SqlitePool, owner_id: i64, name: &str) -> i64 {
    let new = NewPortfolio {
        owner_id,
        name,
        base_currency: "EUR",
    };
    portfolios::create(pool, new, now())
        .await
        .expect("create portfolio")
}

async fn role(pool: &SqlitePool, user_id: i64, portfolio_id: i64) -> Option<Role> {
    portfolios::access(pool, user_id, portfolio_id)
        .await
        .expect("access")
        .map(|access| access.role)
}

#[tokio::test]
async fn the_owner_has_the_owner_role() {
    let pool = db().await;
    let owner = user(&pool, "owner").await;
    let id = portfolio(&pool, owner, "Main").await;

    let access = portfolios::access(&pool, owner, id)
        .await
        .expect("access")
        .expect("visible");

    assert_eq!(
        (
            access.role,
            access.portfolio.name.as_str(),
            access.portfolio.base_currency.as_str()
        ),
        (Role::Owner, "Main", "EUR")
    );
}

#[tokio::test]
async fn another_user_has_no_access_until_shared() {
    let pool = db().await;
    let owner = user(&pool, "owner").await;
    let other = user(&pool, "other").await;
    let id = portfolio(&pool, owner, "Main").await;

    assert_eq!(role(&pool, other, id).await, None);
}

#[tokio::test]
async fn a_share_gives_its_role() {
    let pool = db().await;
    let owner = user(&pool, "owner").await;
    let viewer = user(&pool, "viewer").await;
    let editor = user(&pool, "editor").await;
    let id = portfolio(&pool, owner, "Main").await;
    portfolios::share(&pool, id, viewer, ShareRole::Viewer, now())
        .await
        .expect("share");
    portfolios::share(&pool, id, editor, ShareRole::Editor, now())
        .await
        .expect("share");

    let roles = (role(&pool, viewer, id).await, role(&pool, editor, id).await);

    assert_eq!(roles, (Some(Role::Viewer), Some(Role::Editor)));
}

#[tokio::test]
async fn sharing_again_changes_the_role() {
    let pool = db().await;
    let owner = user(&pool, "owner").await;
    let other = user(&pool, "other").await;
    let id = portfolio(&pool, owner, "Main").await;
    portfolios::share(&pool, id, other, ShareRole::Viewer, now())
        .await
        .expect("share");

    portfolios::share(&pool, id, other, ShareRole::Editor, now())
        .await
        .expect("share again");

    assert_eq!(role(&pool, other, id).await, Some(Role::Editor));
    assert_eq!(
        portfolios::shares(&pool, id).await.expect("shares").len(),
        1
    );
}

#[tokio::test]
async fn a_share_of_one_portfolio_gives_nothing_on_another() {
    let pool = db().await;
    let owner = user(&pool, "owner").await;
    let other = user(&pool, "other").await;
    let shared = portfolio(&pool, owner, "Shared").await;
    let private = portfolio(&pool, owner, "Private").await;
    portfolios::share(&pool, shared, other, ShareRole::Editor, now())
        .await
        .expect("share");

    assert_eq!(role(&pool, other, private).await, None);
}

#[tokio::test]
async fn an_unknown_portfolio_has_no_access() {
    let pool = db().await;
    let owner = user(&pool, "owner").await;
    portfolio(&pool, owner, "Main").await;

    assert_eq!(role(&pool, owner, 404).await, None);
}

#[tokio::test]
async fn a_user_sees_owned_and_shared_portfolios_by_name() {
    let pool = db().await;
    let me = user(&pool, "me").await;
    let other = user(&pool, "other").await;
    portfolio(&pool, me, "Zeta").await;
    let shared = portfolio(&pool, other, "Alpha").await;
    portfolio(&pool, other, "Hidden").await;
    portfolios::share(&pool, shared, me, ShareRole::Viewer, now())
        .await
        .expect("share");

    let visible = portfolios::visible_to(&pool, me).await.expect("visible");

    let seen: Vec<_> = visible
        .iter()
        .map(|access| {
            (
                access.portfolio.name.as_str(),
                access.role,
                access.owner_username.as_str(),
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            ("Alpha", Role::Viewer, "other"),
            ("Zeta", Role::Owner, "me")
        ]
    );
}

#[tokio::test]
async fn an_owner_cannot_reuse_a_portfolio_name() {
    let pool = db().await;
    let owner = user(&pool, "owner").await;
    portfolio(&pool, owner, "Main").await;
    let second = portfolio(&pool, owner, "Second").await;

    let error = portfolios::rename(&pool, second, "Main")
        .await
        .expect_err("taken name");

    assert!(is_unique_violation(&error), "{error}");
}

#[tokio::test]
async fn two_owners_may_use_the_same_name() {
    let pool = db().await;
    let first = user(&pool, "first").await;
    let second = user(&pool, "second").await;
    portfolio(&pool, first, "Main").await;

    let created = portfolios::create(
        &pool,
        NewPortfolio {
            owner_id: second,
            name: "Main",
            base_currency: "EUR",
        },
        now(),
    )
    .await;

    assert!(created.is_ok(), "{created:?}");
}

#[tokio::test]
async fn archiving_records_the_time_and_restoring_clears_it() {
    let pool = db().await;
    let owner = user(&pool, "owner").await;
    let id = portfolio(&pool, owner, "Main").await;
    portfolios::set_archived(&pool, id, Some(now()))
        .await
        .expect("archive");
    let archived = portfolios::access(&pool, owner, id)
        .await
        .expect("access")
        .expect("visible")
        .portfolio
        .archived_at;

    portfolios::set_archived(&pool, id, None)
        .await
        .expect("restore");

    let restored = portfolios::access(&pool, owner, id)
        .await
        .expect("access")
        .expect("visible")
        .portfolio
        .archived_at;
    assert_eq!(
        (archived.as_deref(), restored),
        (Some("2026-11-26T12:00:00Z"), None)
    );
}

#[tokio::test]
async fn shares_are_listed_with_their_users() {
    let pool = db().await;
    let owner = user(&pool, "owner").await;
    let bob = user(&pool, "bob").await;
    let alice = user(&pool, "alice").await;
    let id = portfolio(&pool, owner, "Main").await;
    portfolios::share(&pool, id, bob, ShareRole::Editor, now())
        .await
        .expect("share");
    portfolios::share(&pool, id, alice, ShareRole::Viewer, now())
        .await
        .expect("share");

    let listed = portfolios::shares(&pool, id).await.expect("shares");

    let seen: Vec<_> = listed
        .iter()
        .map(|share| (share.user_id, share.username.as_str(), share.role))
        .collect();
    assert_eq!(
        seen,
        [(alice, "alice", Role::Viewer), (bob, "bob", Role::Editor)]
    );
}

#[tokio::test]
async fn unsharing_removes_access() {
    let pool = db().await;
    let owner = user(&pool, "owner").await;
    let other = user(&pool, "other").await;
    let id = portfolio(&pool, owner, "Main").await;
    portfolios::share(&pool, id, other, ShareRole::Viewer, now())
        .await
        .expect("share");

    let removed = (
        portfolios::unshare(&pool, id, other)
            .await
            .expect("unshare"),
        portfolios::unshare(&pool, id, other).await.expect("again"),
    );

    assert_eq!(removed, (true, false));
    assert_eq!(role(&pool, other, id).await, None);
}

#[tokio::test]
async fn roles_order_from_viewer_to_owner() {
    assert!(Role::Viewer < Role::Editor && Role::Editor < Role::Owner);
}

#[tokio::test]
async fn archiving_twice_keeps_the_first_time() {
    let pool = db().await;
    let owner = user(&pool, "owner").await;
    let id = portfolio(&pool, owner, "Main").await;
    portfolios::set_archived(&pool, id, Some(now()))
        .await
        .expect("archive");
    let later = "2026-11-27T12:00:00Z".parse().expect("timestamp");

    let changed = portfolios::set_archived(&pool, id, Some(later))
        .await
        .expect("archive again");

    let access = portfolios::access(&pool, owner, id)
        .await
        .expect("access")
        .expect("visible");
    assert_eq!(
        (changed, access.portfolio.archived_at.as_deref()),
        (false, Some("2026-11-26T12:00:00Z"))
    );
}

#[tokio::test]
async fn restoring_an_active_portfolio_changes_nothing() {
    let pool = db().await;
    let owner = user(&pool, "owner").await;
    let id = portfolio(&pool, owner, "Main").await;

    let changed = portfolios::set_archived(&pool, id, None)
        .await
        .expect("restore");

    assert!(!changed);
}
