//! Portfolios over HTTP: ownership, the role a user holds, and the access rule on
//! every portfolio route.

mod common;

use axum::http::{Method, StatusCode};
use common::portfolio_steps::{
    Household, call, create, error, household, portfolio, portfolio_routes, share,
};
use serde_json::{Value, json};

#[tokio::test]
async fn a_new_portfolio_belongs_to_its_creator() {
    let Household { app, admin, .. } = household().await;

    let reply = create(&app, &admin, json!({ "name": "Main" })).await;

    assert_eq!(reply.status, StatusCode::CREATED);
    let body = reply.json();
    assert_eq!(
        (
            &body["name"],
            &body["base_currency"],
            &body["role"],
            &body["owner"]["username"],
            &body["archived_at"],
        ),
        (
            &json!("Main"),
            &json!("EUR"),
            &json!("owner"),
            &json!("admin"),
            &Value::Null
        )
    );
}

#[tokio::test]
async fn a_base_currency_is_stored_in_upper_case() {
    let Household { app, admin, .. } = household().await;

    let reply = create(
        &app,
        &admin,
        json!({ "name": "US", "base_currency": "usd" }),
    )
    .await;

    assert_eq!(reply.json()["base_currency"], json!("USD"));
}

#[tokio::test]
async fn a_base_currency_must_be_three_letters() {
    let Household { app, admin, .. } = household().await;

    let reply = create(
        &app,
        &admin,
        json!({ "name": "Bad", "base_currency": "EURO" }),
    )
    .await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_REQUEST, json!("invalid_currency"))
    );
}

#[tokio::test]
async fn a_portfolio_name_must_not_be_blank() {
    let Household { app, admin, .. } = household().await;

    let reply = create(&app, &admin, json!({ "name": "   " })).await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_REQUEST, json!("invalid_name"))
    );
}

#[tokio::test]
async fn an_owner_cannot_reuse_a_name() {
    let Household { app, admin, .. } = household().await;
    portfolio(&app, &admin, "Main").await;

    let reply = create(&app, &admin, json!({ "name": "Main" })).await;

    assert_eq!(error(&reply), (StatusCode::CONFLICT, json!("name_taken")));
}

#[tokio::test]
async fn another_user_gets_404_on_an_unshared_portfolio() {
    let Household { app, admin, member } = household().await;
    let id = portfolio(&app, &admin, "Private").await;

    let reply = app
        .get(&format!("/api/portfolios/{id}"))
        .cookie(&member)
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn every_portfolio_route_answers_404_to_a_user_without_access() {
    let Household { app, admin, member } = household().await;
    let id = portfolio(&app, &admin, "Private").await;

    for route in portfolio_routes(&id) {
        let reply = call(&app, &member, &route).await;

        assert_eq!(
            reply.status,
            StatusCode::NOT_FOUND,
            "{} {}",
            route.0,
            route.1
        );
    }
}

#[tokio::test]
async fn an_unknown_portfolio_is_404_for_everyone() {
    let Household { app, admin, .. } = household().await;
    portfolio(&app, &admin, "Main").await;

    let reply = app.get("/api/portfolios/999").cookie(&admin).send().await;

    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_viewer_reads_the_portfolio() {
    let Household { app, admin, member } = household().await;
    let id = portfolio(&app, &admin, "Shared").await;
    share(&app, &admin, &id, "member", "viewer").await;

    let reply = app
        .get(&format!("/api/portfolios/{id}"))
        .cookie(&member)
        .send()
        .await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.json()["role"], json!("viewer"));
}

#[tokio::test]
async fn a_viewer_gets_403_on_any_write() {
    let Household { app, admin, member } = household().await;
    let id = portfolio(&app, &admin, "Shared").await;
    share(&app, &admin, &id, "member", "viewer").await;

    for route in portfolio_routes(&id) {
        if route.0 == Method::GET {
            continue;
        }

        let reply = call(&app, &member, &route).await;

        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "{} {}",
            route.0,
            route.1
        );
    }
}

#[tokio::test]
async fn an_editor_cannot_do_what_only_the_owner_may() {
    let Household { app, admin, member } = household().await;
    let id = portfolio(&app, &admin, "Shared").await;
    share(&app, &admin, &id, "member", "editor").await;

    for route in portfolio_routes(&id).into_iter().skip(1) {
        let reply = call(&app, &member, &route).await;

        assert_eq!(
            error(&reply),
            (StatusCode::FORBIDDEN, json!("insufficient_role")),
            "{} {}",
            route.0,
            route.1
        );
    }
}

#[tokio::test]
async fn an_administrator_does_not_see_other_users_portfolios() {
    let Household { app, admin, member } = household().await;
    let id = portfolio(&app, &member, "Mine").await;

    let one = app
        .get(&format!("/api/portfolios/{id}"))
        .cookie(&admin)
        .send()
        .await;
    let listed = app
        .get("/api/portfolios")
        .cookie(&admin)
        .send()
        .await
        .json();

    assert_eq!(one.status, StatusCode::NOT_FOUND);
    assert_eq!(listed, json!([]));
}

#[tokio::test]
async fn the_owner_renames_a_portfolio() {
    let Household { app, admin, .. } = household().await;
    let id = portfolio(&app, &admin, "Main").await;

    let reply = app
        .call(Method::PATCH, &format!("/api/portfolios/{id}"))
        .cookie(&admin)
        .json(json!({ "name": "Long term" }))
        .send()
        .await;

    assert_eq!(reply.json()["name"], json!("Long term"));
}

#[tokio::test]
async fn archiving_and_restoring_set_and_clear_the_archive_time() {
    let Household { app, admin, .. } = household().await;
    let id = portfolio(&app, &admin, "Main").await;

    let archived = app
        .post(&format!("/api/portfolios/{id}/archive"))
        .cookie(&admin)
        .send()
        .await
        .json();
    let restored = app
        .post(&format!("/api/portfolios/{id}/unarchive"))
        .cookie(&admin)
        .send()
        .await
        .json();

    assert_eq!(
        archived["archived_at"],
        json!(saffron_db::time::to_text(app.state.clock.now()))
    );
    assert_eq!(restored["archived_at"], Value::Null);
}

#[tokio::test]
async fn a_portfolio_id_must_be_a_number() {
    let Household { app, admin, .. } = household().await;

    let reply = app.get("/api/portfolios/main").cookie(&admin).send().await;

    assert_eq!(
        error(&reply),
        (StatusCode::BAD_REQUEST, json!("invalid_path"))
    );
}

#[tokio::test]
async fn portfolios_need_a_session() {
    let Household { app, admin, .. } = household().await;
    let id = portfolio(&app, &admin, "Main").await;

    let list = app.get("/api/portfolios").send().await;
    let one = app.get(&format!("/api/portfolios/{id}")).send().await;

    assert_eq!(
        (list.status, one.status),
        (StatusCode::UNAUTHORIZED, StatusCode::UNAUTHORIZED)
    );
}

#[tokio::test]
async fn archiving_an_archived_portfolio_is_audited_once() {
    let Household { app, admin, .. } = household().await;
    let id = portfolio(&app, &admin, "Main").await;
    let path = format!("/api/portfolios/{id}/archive");
    app.post(&path).cookie(&admin).send().await;

    let again = app.post(&path).cookie(&admin).send().await;

    let archives: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'portfolio.archive'")
            .fetch_one(&app.state.pool)
            .await
            .expect("audit");
    assert_eq!((again.status, archives), (StatusCode::OK, 1));
}
