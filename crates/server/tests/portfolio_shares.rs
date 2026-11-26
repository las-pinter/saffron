//! Sharing portfolios over HTTP: sharing, changing a role, removing a share, and
//! leaving one.

mod common;

use axum::http::{Method, StatusCode};
use common::portfolio_steps::{Household, error, household, portfolio, share};
use serde_json::json;

#[tokio::test]
async fn a_shared_portfolio_is_listed_with_its_owner_and_role() {
    let Household { app, admin, member } = household().await;
    let id = portfolio(&app, &admin, "Shared").await;
    portfolio(&app, &admin, "Private").await;
    portfolio(&app, &member, "Mine").await;
    share(&app, &admin, &id, "member", "editor").await;

    let listed = app
        .get("/api/portfolios")
        .cookie(&member)
        .send()
        .await
        .json();

    let seen: Vec<_> = listed
        .as_array()
        .expect("list")
        .iter()
        .map(|item| {
            (
                item["name"].clone(),
                item["role"].clone(),
                item["owner"]["username"].clone(),
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            (json!("Mine"), json!("owner"), json!("member")),
            (json!("Shared"), json!("editor"), json!("admin")),
        ]
    );
}

#[tokio::test]
async fn sharing_with_an_unknown_user_is_refused() {
    let Household { app, admin, .. } = household().await;
    let id = portfolio(&app, &admin, "Main").await;

    let reply = share(&app, &admin, &id, "nobody", "viewer").await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_REQUEST, json!("unknown_user"))
    );
}

#[tokio::test]
async fn sharing_with_a_disabled_user_is_refused() {
    let Household { app, admin, .. } = household().await;
    let id = portfolio(&app, &admin, "Main").await;
    let other = app.create_user(&admin, "other").await;
    app.post(&format!("/api/users/{other}/disable"))
        .cookie(&admin)
        .send()
        .await;

    let reply = share(&app, &admin, &id, "other", "viewer").await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_REQUEST, json!("unknown_user"))
    );
}

#[tokio::test]
async fn sharing_with_the_owner_is_refused() {
    let Household { app, admin, .. } = household().await;
    let id = portfolio(&app, &admin, "Main").await;

    let reply = share(&app, &admin, &id, "admin", "editor").await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_REQUEST, json!("owner_share"))
    );
}

#[tokio::test]
async fn sharing_again_changes_the_role() {
    let Household { app, admin, member } = household().await;
    let id = portfolio(&app, &admin, "Shared").await;
    share(&app, &admin, &id, "member", "viewer").await;

    share(&app, &admin, &id, "member", "editor").await;

    let shares = app
        .get(&format!("/api/portfolios/{id}/shares"))
        .cookie(&admin)
        .send()
        .await
        .json();
    assert_eq!(shares[0]["role"], json!("editor"));
    let mine = app
        .get(&format!("/api/portfolios/{id}"))
        .cookie(&member)
        .send()
        .await
        .json();
    assert_eq!(mine["role"], json!("editor"));
}

#[tokio::test]
async fn removing_a_share_ends_access() {
    let Household { app, admin, member } = household().await;
    let id = portfolio(&app, &admin, "Shared").await;
    share(&app, &admin, &id, "member", "viewer").await;
    let shares = app
        .get(&format!("/api/portfolios/{id}/shares"))
        .cookie(&admin)
        .send()
        .await
        .json();
    let user_id = shares[0]["user_id"].clone();

    let reply = app
        .call(
            Method::DELETE,
            &format!("/api/portfolios/{id}/shares/{user_id}"),
        )
        .cookie(&admin)
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    let after = app
        .get(&format!("/api/portfolios/{id}"))
        .cookie(&member)
        .send()
        .await;
    assert_eq!(after.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_member_may_leave_a_shared_portfolio() {
    let Household { app, admin, member } = household().await;
    let id = portfolio(&app, &admin, "Shared").await;
    share(&app, &admin, &id, "member", "viewer").await;

    let reply = app
        .call(Method::DELETE, &format!("/api/memberships/{id}"))
        .cookie(&member)
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    let after = app
        .get(&format!("/api/portfolios/{id}"))
        .cookie(&member)
        .send()
        .await;
    assert_eq!(after.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_owner_cannot_leave_their_portfolio() {
    let Household { app, admin, .. } = household().await;
    let id = portfolio(&app, &admin, "Main").await;

    let reply = app
        .call(Method::DELETE, &format!("/api/memberships/{id}"))
        .cookie(&admin)
        .send()
        .await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_REQUEST, json!("owner_cannot_leave"))
    );
}

#[tokio::test]
async fn leaving_a_portfolio_never_shared_is_404() {
    let Household { app, admin, member } = household().await;
    let id = portfolio(&app, &admin, "Private").await;

    let reply = app
        .call(Method::DELETE, &format!("/api/memberships/{id}"))
        .cookie(&member)
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn sharing_is_audited() {
    let Household { app, admin, .. } = household().await;
    let id = portfolio(&app, &admin, "Main").await;

    share(&app, &admin, &id, "member", "viewer").await;

    let row: (String, String, String) =
        sqlx::query_as("SELECT action, target, detail FROM audit_log ORDER BY id DESC LIMIT 1")
            .fetch_one(&app.state.pool)
            .await
            .expect("audit");
    assert_eq!(
        row,
        (
            "portfolio.share".to_owned(),
            "Main".to_owned(),
            r#"{"role":"viewer","user":"member"}"#.to_owned()
        )
    );
}
