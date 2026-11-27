//! The daily ECB rate refresh: backfill, the daily file, and a day that must not
//! be lost.

mod common;

use axum::http::StatusCode;
use common::instrument_steps::market;
use jiff::{SignedDuration, civil::date};
use saffron_db::fx;
use saffron_server::refresh;

#[tokio::test]
async fn a_fresh_install_takes_the_archive_at_once() {
    let market = market().await;
    // 14:00 Helsinki, before the ECB's daily publication: the archive is already
    // published, so the first run does not wait.
    let tick = refresh::tick(&market.app.state).await.expect("tick");

    let converted = fx::convert(
        &market.app.state.pool,
        "100".parse().expect("decimal"),
        "USD",
        "EUR",
        date(2026, 11, 26),
    )
    .await
    .expect("convert")
    .expect("a rate")
    .round_dp(2);
    assert!(
        tick.fx > 300,
        "the whole history is stored on the first run ({})",
        tick.fx
    );
    assert_eq!(
        converted.to_string(),
        "91.12",
        "100 USD on the fixture date"
    );
}

#[tokio::test]
async fn the_ecb_is_asked_once_a_day() {
    let market = market().await;
    market.app.advance(SignedDuration::from_hours(4));
    refresh::tick(&market.app.state).await.expect("first tick");
    // Past the retry interval, so only the day mark can hold the next request back.
    market.app.advance(SignedDuration::from_hours(2));
    let asked = market.yahoo.requests();

    refresh::tick(&market.app.state).await.expect("second tick");

    assert_eq!(market.yahoo.requests() - asked, 0, "the day is done");
}

#[tokio::test]
async fn a_later_day_takes_the_daily_file() {
    let market = market().await;
    market.app.advance(SignedDuration::from_hours(4));
    refresh::tick(&market.app.state).await.expect("backfill");
    // Two days on, after publication: the archive is not needed again.
    market.app.advance(SignedDuration::from_hours(52));
    let before = market.yahoo.requests();

    let tick = refresh::tick(&market.app.state).await.expect("daily");

    assert!(
        tick.fx > 0 && market.yahoo.requests() - before == 1,
        "one daily file, {} rows",
        tick.fx
    );
}

#[tokio::test]
async fn a_failed_fetch_does_not_lose_the_day() {
    let market = market().await;
    market.app.advance(SignedDuration::from_hours(4));
    market.yahoo.fail_with(StatusCode::INTERNAL_SERVER_ERROR);
    let failed = refresh::tick(&market.app.state).await.expect("failed tick");
    assert_eq!(failed.fx, 0);

    // An hour later the ECB answers, and the day is still due.
    market
        .app
        .advance(SignedDuration::from_hours(1) + SignedDuration::from_mins(1));
    market.yahoo.recover();
    let recovered = refresh::tick(&market.app.state)
        .await
        .expect("recovered tick");

    let converted = fx::convert(
        &market.app.state.pool,
        "100".parse().expect("decimal"),
        "USD",
        "EUR",
        date(2026, 11, 26),
    )
    .await
    .expect("convert");
    assert!(
        recovered.fx > 300 && converted.is_some(),
        "the day was retried, not written off"
    );
}

#[tokio::test]
async fn the_same_currency_converts_without_any_rates() {
    let market = market().await;

    let converted = fx::convert(
        &market.app.state.pool,
        "42.5".parse().expect("decimal"),
        "EUR",
        "EUR",
        date(2026, 11, 26),
    )
    .await
    .expect("convert");

    assert_eq!(
        converted.map(|value| value.to_string()),
        Some("42.5".to_owned())
    );
}

#[tokio::test]
async fn a_missed_days_gap_is_repaired_from_the_archive() {
    let market = market().await;
    market.app.advance(SignedDuration::from_hours(4));
    refresh::tick(&market.app.state).await.expect("backfill");
    // Five days later: several publication days were missed while the server ran.
    market.app.advance(SignedDuration::from_hours(120));
    let before = market.yahoo.requests();

    let tick = refresh::tick(&market.app.state).await.expect("gap repair");

    assert!(
        tick.fx > 300 && market.yahoo.requests() - before == 1,
        "the archive repairs the gap in one request, not one daily file ({})",
        tick.fx
    );
}

#[tokio::test]
async fn a_stale_daily_file_is_not_taken_as_the_day_done() {
    let market = market().await;
    market.app.advance(SignedDuration::from_hours(4));
    refresh::tick(&market.app.state).await.expect("backfill");
    market.app.advance(SignedDuration::from_hours(52));
    refresh::tick(&market.app.state)
        .await
        .expect("stale daily file");
    // The fixture still holds the previous day, so the day is not done.
    market
        .app
        .advance(SignedDuration::from_hours(1) + SignedDuration::from_mins(1));
    let before = market.yahoo.requests();

    refresh::tick(&market.app.state).await.expect("retry");

    assert_eq!(
        market.yahoo.requests() - before,
        1,
        "an hour later the ECB is asked again"
    );
}

#[tokio::test]
async fn a_publication_day_is_not_written_off_by_a_lagging_archive() {
    let market = market().await;
    refresh::tick(&market.app.state).await.expect("backfill");
    // 06:00 in Brussels the next day: before publication, nothing is asked for.
    market.app.advance(SignedDuration::from_hours(42));
    let before = market.yahoo.requests();
    refresh::tick(&market.app.state)
        .await
        .expect("before publication");
    assert_eq!(market.yahoo.requests() - before, 0);

    // 17:00 Brussels; the archive still holds yesterday, so the day stays open and
    // is asked for again an hour later rather than written off.
    market.app.advance(SignedDuration::from_hours(10));
    refresh::tick(&market.app.state)
        .await
        .expect("lagging archive");
    market
        .app
        .advance(SignedDuration::from_hours(1) + SignedDuration::from_mins(1));
    let after = market.yahoo.requests();
    refresh::tick(&market.app.state).await.expect("retry");

    assert_eq!(market.yahoo.requests() - after, 1, "the day is still open");
}

#[tokio::test]
async fn a_first_backfill_on_a_publication_day_completes_the_day() {
    let market = market().await;
    // 17:00 in Brussels, after publication, on the fixture's own newest day.
    market.app.advance(SignedDuration::from_hours(28));
    refresh::tick(&market.app.state).await.expect("backfill");
    market.app.advance(SignedDuration::from_hours(2));
    let asked = market.yahoo.requests();

    refresh::tick(&market.app.state).await.expect("later tick");

    assert_eq!(
        market.yahoo.requests() - asked,
        0,
        "rates for today are already stored"
    );
}
