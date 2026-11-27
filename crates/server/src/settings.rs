//! Installation settings that administrators change.

use axum::{Json, extract::State};
use saffron_db::{audit, settings};
use serde::{Deserialize, Serialize};

use crate::{
    auth::extract::AdminUser,
    error::{ApiError, ApiResult},
    request::JsonBody,
    state::AppState,
    users::record_audit,
};

/// Whether FT markets supplies fund prices when Yahoo has none. Off until an
/// administrator turns it on, because FT's terms of use are not yet reviewed.
pub const FT_FALLBACK: &str = "prices.ft_fallback";

/// The settings as the API shows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsView {
    /// See [`FT_FALLBACK`].
    pub ft_fallback: bool,
}

/// Body of `PATCH /api/settings`; absent fields stay as they are.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsPatch {
    #[serde(default)]
    ft_fallback: Option<bool>,
}

/// Whether the FT fund fallback is on.
pub async fn ft_fallback(state: &AppState) -> ApiResult<bool> {
    flag(state, FT_FALLBACK).await
}

/// `GET /api/settings`
pub async fn show(State(state): State<AppState>, _: AdminUser) -> ApiResult<Json<SettingsView>> {
    Ok(Json(view(&state).await?))
}

/// `PATCH /api/settings`
pub async fn update(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    JsonBody(patch): JsonBody<SettingsPatch>,
) -> ApiResult<Json<SettingsView>> {
    if let Some(on) = patch.ft_fallback {
        settings::set(&state.pool, FT_FALLBACK, if on { "on" } else { "off" }).await?;
        let detail = serde_json::json!({ "ft_fallback": on }).to_string();
        let entry = audit::Entry {
            user_id: Some(admin.user.id),
            action: "settings.update",
            target: Some(FT_FALLBACK),
            detail: Some(&detail),
        };
        record_audit(&state, entry).await;
    }
    Ok(Json(view(&state).await?))
}

async fn view(state: &AppState) -> ApiResult<SettingsView> {
    Ok(SettingsView {
        ft_fallback: ft_fallback(state).await?,
    })
}

async fn flag(state: &AppState, key: &str) -> ApiResult<bool> {
    match settings::get(&state.pool, key).await?.as_deref() {
        None | Some("off") => Ok(false),
        Some("on") => Ok(true),
        Some(other) => Err(ApiError::internal(format!(
            "setting {key} holds {other:?}, not on or off"
        ))),
    }
}
