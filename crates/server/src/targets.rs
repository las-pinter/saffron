//! Portfolio target weights over HTTP: what share of the book each asset class and
//! region should hold.

use std::collections::BTreeMap;

use axum::{Json, extract::State};
use rust_decimal::Decimal;
use saffron_core::performance::{ASSET_CLASSES, REGIONS};
use saffron_db::{decimal, targets};
use serde::{Deserialize, Serialize};

use crate::{
    auth::extract::CurrentUser,
    error::{ApiError, ApiResult},
    portfolios::{self, guard::PortfolioAccess},
    request::JsonBody,
    state::AppState,
    users::record_audit,
};

/// Target weights as the API shows them, in percent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetsView {
    /// By asset class.
    pub asset_class: BTreeMap<String, String>,
    /// By region.
    pub region: BTreeMap<String, String>,
}

/// Body of `PUT /api/portfolios/{portfolio_id}/targets`; it replaces every target.
#[derive(Debug, Default, Deserialize)]
pub struct ReplaceRequest {
    #[serde(default)]
    asset_class: BTreeMap<String, String>,
    #[serde(default)]
    region: BTreeMap<String, String>,
}

impl From<targets::Targets> for TargetsView {
    fn from(targets: targets::Targets) -> Self {
        let text = |map: BTreeMap<String, Decimal>| {
            map.into_iter()
                .map(|(key, weight)| (key, decimal::to_text(weight)))
                .collect()
        };
        Self {
            asset_class: text(targets.asset_class),
            region: text(targets.region),
        }
    }
}

/// `GET /api/portfolios/{portfolio_id}/targets`
pub async fn show(
    State(state): State<AppState>,
    access: PortfolioAccess,
) -> ApiResult<Json<TargetsView>> {
    let targets = targets::get(&state.pool, access.0.portfolio.id).await?;
    Ok(Json(TargetsView::from(targets)))
}

/// `PUT /api/portfolios/{portfolio_id}/targets`
pub async fn replace(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    access: PortfolioAccess,
    JsonBody(request): JsonBody<ReplaceRequest>,
) -> ApiResult<Json<TargetsView>> {
    let targets = validate(request)?;
    let portfolio_id = access.0.portfolio.id;
    targets::replace(&state.pool, portfolio_id, &targets).await?;
    let detail = serde_json::json!({
        "asset_class": targets.asset_class.len(),
        "region": targets.region.len(),
    })
    .to_string();
    record_audit(
        &state,
        portfolios::entry(
            &session.user,
            &access.0,
            "portfolio.targets_replace",
            Some(&detail),
        ),
    )
    .await;
    Ok(Json(TargetsView::from(targets)))
}

/// Checks every key and weight, and that each dimension totals at most 100.
fn validate(request: ReplaceRequest) -> ApiResult<targets::Targets> {
    Ok(targets::Targets {
        asset_class: dimension(request.asset_class, &ASSET_CLASSES, "asset class")?,
        region: dimension(request.region, &REGIONS, "region")?,
    })
}

fn dimension(
    raw: BTreeMap<String, String>,
    allowed: &[&str],
    name: &str,
) -> ApiResult<BTreeMap<String, Decimal>> {
    let mut weights = BTreeMap::new();
    let mut total = Decimal::ZERO;
    for (key, text) in raw {
        let key = key.trim().to_ascii_lowercase();
        if !allowed.contains(&key.as_str()) {
            return Err(ApiError::bad_request(
                "invalid_key",
                format!("`{key}` is not a known {name}"),
            ));
        }
        let weight = decimal::from_text(text.trim()).map_err(|_| {
            ApiError::bad_request(
                "invalid_weight",
                format!("the {name} weight for `{key}` is not a decimal"),
            )
        })?;
        if weight < Decimal::ZERO || weight > Decimal::from(100) {
            return Err(ApiError::bad_request(
                "invalid_weight",
                format!("the {name} weight for `{key}` is not between 0 and 100"),
            ));
        }
        total += weight;
        weights.insert(key, weight);
    }
    if total > Decimal::from(100) {
        return Err(ApiError::bad_request(
            "weights_over_100",
            format!("the {name} weights total more than 100"),
        ));
    }
    Ok(weights)
}
