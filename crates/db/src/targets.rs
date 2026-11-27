//! Portfolio target weights: what share of the book each asset class and region
//! should hold, in percent.

use std::collections::BTreeMap;

use rust_decimal::Decimal;
use sqlx::SqlitePool;

use crate::decimal;

/// The dimension of asset-class targets.
pub const ASSET_CLASS: &str = "asset_class";
/// The dimension of region targets.
pub const REGION: &str = "region";

/// Target weights by key, in percent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Targets {
    /// By asset class.
    pub asset_class: BTreeMap<String, Decimal>,
    /// By region.
    pub region: BTreeMap<String, Decimal>,
}

/// The targets of `portfolio_id`.
pub async fn get(pool: &SqlitePool, portfolio_id: i64) -> Result<Targets, sqlx::Error> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT dimension, key, weight FROM portfolio_targets
         WHERE portfolio_id = ? ORDER BY dimension, key",
    )
    .bind(portfolio_id)
    .fetch_all(pool)
    .await?;
    let mut targets = Targets::default();
    for (dimension, key, weight) in rows {
        let weight = decimal::from_text(&weight)
            .map_err(|error| sqlx::Error::Decode(error.to_string().into()))?;
        let map = match dimension.as_str() {
            ASSET_CLASS => &mut targets.asset_class,
            REGION => &mut targets.region,
            other => {
                return Err(sqlx::Error::Decode(
                    format!("unknown target dimension `{other}`").into(),
                ));
            }
        };
        map.insert(key, weight);
    }
    Ok(targets)
}

/// Replaces every target of `portfolio_id` in one transaction.
pub async fn replace(
    pool: &SqlitePool,
    portfolio_id: i64,
    targets: &Targets,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM portfolio_targets WHERE portfolio_id = ?")
        .bind(portfolio_id)
        .execute(&mut *tx)
        .await?;
    for (dimension, map) in [
        (ASSET_CLASS, &targets.asset_class),
        (REGION, &targets.region),
    ] {
        for (key, weight) in map {
            sqlx::query(
                "INSERT INTO portfolio_targets (portfolio_id, dimension, key, weight)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(portfolio_id)
            .bind(dimension)
            .bind(key)
            .bind(decimal::to_text(*weight))
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await
}
