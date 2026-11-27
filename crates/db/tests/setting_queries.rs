//! Installation settings.

use saffron_db::{open_in_memory, settings};

#[tokio::test]
async fn an_unset_key_has_no_value() {
    let pool = open_in_memory().await.expect("database");

    assert_eq!(
        settings::get(&pool, "prices.ft_fallback")
            .await
            .expect("get"),
        None
    );
}

#[tokio::test]
async fn a_value_is_replaced_by_the_next() {
    let pool = open_in_memory().await.expect("database");
    settings::set(&pool, "prices.ft_fallback", "on")
        .await
        .expect("set");

    settings::set(&pool, "prices.ft_fallback", "off")
        .await
        .expect("set again");

    assert_eq!(
        settings::get(&pool, "prices.ft_fallback")
            .await
            .expect("get")
            .as_deref(),
        Some("off")
    );
}
