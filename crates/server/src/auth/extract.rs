//! Request extractors: the signed-in user and the client address.

use std::{convert::Infallible, net::SocketAddr};

use axum::{
    extract::{ConnectInfo, FromRequestParts, OptionalFromRequestParts},
    http::{header::COOKIE, request::Parts},
};

use super::{
    client, cookie,
    session::{self, LiveSession},
};
use crate::{error::ApiError, state::AppState};

/// The signed-in user; rejects the request with 401 without a live session.
#[derive(Debug, Clone)]
pub struct CurrentUser(pub LiveSession);

/// A signed-in administrator; rejects with 401 without a session and 403 for a
/// user who is not an administrator.
#[derive(Debug, Clone)]
pub struct AdminUser(pub LiveSession);

/// The client address, used to throttle failed sign-ins per client. Forwarding
/// headers count only from trusted proxies; see [`client`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientSource(pub String);

async fn live_session(parts: &Parts, state: &AppState) -> Result<Option<LiveSession>, ApiError> {
    let Some(token) = parts
        .headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|header| cookie::token(&state.origin, header))
    else {
        return Ok(None);
    };
    session::resolve(&state.pool, token, state.clock.now()).await
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        live_session(parts, state)
            .await?
            .map(Self)
            .ok_or_else(ApiError::unauthenticated)
    }
}

impl OptionalFromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Option<Self>, ApiError> {
        Ok(live_session(parts, state).await?.map(Self))
    }
}

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let CurrentUser(session) =
            <CurrentUser as FromRequestParts<AppState>>::from_request_parts(parts, state).await?;
        if !session.user.is_admin {
            return Err(ApiError::forbidden(
                "admin_required",
                "only an administrator may do this",
            ));
        }
        Ok(Self(session))
    }
}

impl FromRequestParts<AppState> for ClientSource {
    type Rejection = Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> impl Future<Output = Result<Self, Infallible>> + Send {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip());
        // A proxy may add its own header line instead of extending the client's, so
        // every line counts, in order.
        let forwarded_for = parts
            .headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect::<Vec<_>>()
            .join(",");
        let forwarded_for = Some(forwarded_for.as_str()).filter(|value| !value.is_empty());
        let source = client::client_address(peer, forwarded_for, &state.trusted_proxies);
        std::future::ready(Ok(Self(source)))
    }
}
