//! Instruments: finding a listing at a price provider and adding it to Saffron.
//! Instruments are shared by every user, so any signed-in user may look one up
//! and add it.

use axum::{Json, extract::State, http::StatusCode};
use saffron_core::{
    instrument::Isin,
    performance::{ASSET_CLASSES, REGIONS},
};
use saffron_db::{
    audit, instruments,
    instruments::{Instrument, NewInstrument, NewListing},
    is_unique_violation,
};
use saffron_market::{Listing, MarketError, yahoo};
use serde::{Deserialize, Serialize};

use crate::{
    auth::extract::CurrentUser,
    error::{ApiError, ApiResult},
    performance,
    request::{JsonBody, PathParam, QueryParam},
    state::AppState,
    users::record_audit,
};

/// Longest lookup text, in characters.
pub const MAX_QUERY_CHARS: usize = 100;
/// Longest provider symbol, in characters.
pub const MAX_SYMBOL_CHARS: usize = 32;

/// Query of `GET /api/instruments/lookup`.
#[derive(Debug, Deserialize)]
pub struct LookupQuery {
    q: String,
}

/// The answer to a lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LookupView {
    /// The query read as an ISIN, when it is one; pass it on when adding a result.
    pub isin: Option<String>,
    /// Listings that match, best first.
    pub results: Vec<LookupHit>,
}

/// One listing found by a lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LookupHit {
    /// The provider, such as `yahoo`.
    pub provider: String,
    /// The provider's symbol.
    pub symbol: String,
    /// The instrument's name.
    pub name: String,
    /// `stock`, `etf`, `fund`, `bond`, `crypto`, or `other`.
    pub kind: String,
    /// The exchange as the provider shows it.
    pub exchange: Option<String>,
    /// The instrument, when Saffron already has this listing.
    pub instrument_id: Option<i64>,
}

/// Body of `POST /api/instruments`.
#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    symbol: String,
    #[serde(default)]
    isin: Option<String>,
}

/// An instrument as the API shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstrumentView {
    /// Row id.
    pub id: i64,
    /// `stock`, `etf`, `fund`, `bond`, `crypto`, or `other`.
    pub kind: String,
    /// Its name.
    pub name: String,
    /// Its ISIN, when known.
    pub isin: Option<String>,
    /// The ISO 4217 currency it is valued in.
    pub currency: String,
    /// Its asset class, stored or derived from its kind.
    pub asset_class: String,
    /// Its region, stored or derived from its primary listing.
    pub region: String,
    /// When it was added.
    pub created_at: String,
    /// Where providers list it, primary first.
    pub listings: Vec<ListingView>,
    /// Its latest price; `None` until one is known.
    #[serde(default)]
    pub latest_price: Option<crate::prices::PriceView>,
}

/// A listing as the API shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListingView {
    /// The provider.
    pub provider: String,
    /// The provider's symbol.
    pub symbol: String,
    /// The exchange as the provider shows it.
    pub exchange: Option<String>,
    /// The currency the provider quotes prices in.
    pub currency: String,
    /// Whether prices come from this listing first.
    pub is_primary: bool,
}

/// `instrument` as the API shows it, with its latest price.
pub async fn view(state: &AppState, instrument: Instrument) -> ApiResult<InstrumentView> {
    let latest_price = crate::prices::latest_view(state, &instrument).await?;
    Ok(InstrumentView {
        latest_price,
        ..InstrumentView::from(instrument)
    })
}

impl From<Instrument> for InstrumentView {
    fn from(instrument: Instrument) -> Self {
        let symbol = instrument
            .listings
            .iter()
            .find(|listing| listing.is_primary)
            .or_else(|| instrument.listings.first())
            .map(|listing| listing.symbol.as_str());
        let asset_class = instrument
            .asset_class
            .clone()
            .unwrap_or_else(|| performance::default_asset_class(instrument.kind).to_owned());
        let region = instrument
            .region
            .clone()
            .unwrap_or_else(|| performance::default_region(symbol).to_owned());
        Self {
            latest_price: None,
            id: instrument.id,
            kind: instrument.kind.as_str().to_owned(),
            name: instrument.name,
            isin: instrument.isin,
            currency: instrument.currency,
            asset_class,
            region,
            created_at: instrument.created_at,
            listings: instrument
                .listings
                .into_iter()
                .map(|listing| ListingView {
                    provider: listing.provider,
                    symbol: listing.symbol,
                    exchange: listing.exchange,
                    currency: listing.currency,
                    is_primary: listing.is_primary,
                })
                .collect(),
        }
    }
}

/// `GET /api/instruments/lookup?q=`: listings matching an ISIN, a symbol, or a name.
pub async fn lookup(
    State(state): State<AppState>,
    _: CurrentUser,
    QueryParam(query): QueryParam<LookupQuery>,
) -> ApiResult<Json<LookupView>> {
    let text = validate_query(&query.q)?;
    let hits = match state.market.yahoo.search(text).await {
        Ok(hits) => hits,
        Err(MarketError::NotFound) => Vec::new(),
        Err(error) => return Err(provider_failure(&error)),
    };
    let symbols: Vec<&str> = hits.iter().map(|hit| hit.symbol.as_str()).collect();
    let known = instruments::known_symbols(&state.pool, yahoo::PROVIDER, &symbols).await?;
    let results = hits
        .into_iter()
        .map(|hit| LookupHit {
            instrument_id: known
                .iter()
                .find(|(symbol, _)| *symbol == hit.symbol)
                .map(|(_, id)| *id),
            provider: yahoo::PROVIDER.to_owned(),
            kind: hit.kind.as_str().to_owned(),
            symbol: hit.symbol,
            name: hit.name,
            exchange: hit.exchange,
        })
        .collect();
    Ok(Json(LookupView {
        isin: Isin::parse(text).map(|isin| isin.as_str().to_owned()),
        results,
    }))
}

/// `POST /api/instruments`: adds the Yahoo listing `symbol`. Answers 201 with a new
/// instrument; 200 when the listing is already known, or when `isin` names an
/// instrument Saffron has, which then gains the listing. Each listing keeps the
/// currency it is quoted in, so a listing may differ from its instrument's currency.
/// For a listing Saffron already has, `isin` is ignored: the stored instrument is
/// returned as it is.
pub async fn create(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    JsonBody(request): JsonBody<CreateRequest>,
) -> ApiResult<(StatusCode, Json<InstrumentView>)> {
    let symbol = validate_symbol(&request.symbol)?;
    let isin = request.isin.as_deref().map(validate_isin).transpose()?;
    if let Some(id) = instruments::id_by_listing(&state.pool, yahoo::PROVIDER, symbol).await? {
        return found(&state, StatusCode::OK, id).await;
    }
    if let Some(isin) = &isin {
        confirm_isin(&state, isin, symbol).await?;
    }
    let listing = state
        .market
        .yahoo
        .details(symbol)
        .await
        .map_err(|error| match error {
            MarketError::NotFound => {
                ApiError::bad_request("unknown_symbol", "Yahoo Finance does not know this symbol")
            }
            MarketError::Unsupported(kind) => ApiError::bad_request(
                "unsupported_kind",
                format!("Yahoo Finance lists this as {kind}, which Saffron does not hold"),
            ),
            error => provider_failure(&error),
        })?;
    let (stored, id) = store(&state, &listing, isin.as_ref()).await?;
    let action = match stored {
        Stored::Created => Some("instrument.create"),
        Stored::Joined => Some("instrument.add_listing"),
        Stored::Known => None,
    };
    let status = if stored == Stored::Created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    let reply = found(&state, status, id).await?;
    if let Some(action) = action {
        let detail = serde_json::json!({ "symbol": listing.symbol }).to_string();
        let entry = audit::Entry {
            user_id: Some(session.user.id),
            action,
            target: Some(&reply.1.name),
            detail: Some(&detail),
        };
        record_audit(&state, entry).await;
    }
    Ok(reply)
}

/// What [`store`] did with a listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stored {
    /// Made a new instrument for it.
    Created,
    /// Added it to the instrument with the same ISIN.
    Joined,
    /// Found it already stored by a parallel request.
    Known,
}

/// `GET /api/instruments/{id}`
pub async fn show(
    State(state): State<AppState>,
    _: CurrentUser,
    PathParam(id): PathParam<i64>,
) -> ApiResult<Json<InstrumentView>> {
    let instrument = instruments::get(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(view(&state, instrument).await?))
}

/// Body of `PUT /api/instruments/{id}/classification`.
#[derive(Debug, Deserialize)]
pub struct ClassificationRequest {
    asset_class: String,
    region: String,
}

/// `PUT /api/instruments/{id}/classification`: sets the asset class and region used
/// by the allocation. Instruments are shared, so any signed-in user may classify
/// one, as with a manual price; every call is audited, which keeps a change
/// traceable to its author.
pub async fn classify(
    State(state): State<AppState>,
    CurrentUser(session): CurrentUser,
    PathParam(id): PathParam<i64>,
    JsonBody(request): JsonBody<ClassificationRequest>,
) -> ApiResult<Json<InstrumentView>> {
    let asset_class = validate_choice(&request.asset_class, &ASSET_CLASSES, "asset class")?;
    let region = validate_choice(&request.region, &REGIONS, "region")?;
    if !instruments::set_classification(&state.pool, id, &asset_class, &region).await? {
        return Err(ApiError::not_found());
    }
    let instrument = instruments::get(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::internal("a stored instrument cannot be read back"))?;
    let view = view(&state, instrument).await?;
    let detail = serde_json::json!({ "asset_class": asset_class, "region": region }).to_string();
    let entry = audit::Entry {
        user_id: Some(session.user.id),
        action: "instrument.classify",
        target: Some(&view.name),
        detail: Some(&detail),
    };
    record_audit(&state, entry).await;
    Ok(Json(view))
}

/// Stores `listing`: on the instrument with `isin` when Saffron has one, or as a
/// new instrument. A parallel request may store it first; that is not an error.
async fn store(
    state: &AppState,
    listing: &Listing,
    isin: Option<&Isin>,
) -> ApiResult<(Stored, i64)> {
    let new_listing = NewListing {
        provider: yahoo::PROVIDER,
        symbol: &listing.symbol,
        exchange: listing.exchange.as_deref(),
        currency: &listing.quote_currency,
        timezone: listing.timezone.as_deref(),
    };
    let existing = match isin {
        Some(isin) => instruments::id_by_isin(&state.pool, isin.as_str()).await?,
        None => None,
    };
    let stored = if let Some(id) = existing {
        instruments::add_listing(&state.pool, id, new_listing)
            .await
            .map(|()| (Stored::Joined, id))
    } else {
        let new = NewInstrument {
            kind: listing.kind,
            name: &listing.name,
            isin: isin.map(Isin::as_str),
            currency: &listing.currency,
        };
        instruments::create(&state.pool, new, new_listing, state.clock.now())
            .await
            .map(|id| (Stored::Created, id))
    };
    match stored {
        Ok(stored) => Ok(stored),
        Err(error) if is_unique_violation(&error) => {
            let id =
                instruments::id_by_listing(&state.pool, yahoo::PROVIDER, &listing.symbol).await?;
            let id = match (id, isin) {
                (Some(id), _) => Some(id),
                (None, Some(isin)) => instruments::id_by_isin(&state.pool, isin.as_str()).await?,
                (None, None) => None,
            };
            id.map(|id| (Stored::Known, id))
                .ok_or_else(|| ApiError::internal(error))
        }
        Err(error) => Err(error.into()),
    }
}

/// Refuses an ISIN under which Yahoo does not list `symbol`, so an instrument
/// never carries another instrument's ISIN.
async fn confirm_isin(state: &AppState, isin: &Isin, symbol: &str) -> ApiResult<()> {
    let hits = match state.market.yahoo.search(isin.as_str()).await {
        Ok(hits) => hits,
        Err(MarketError::NotFound) => Vec::new(),
        Err(error) => return Err(provider_failure(&error)),
    };
    if hits
        .iter()
        .any(|hit| hit.symbol.eq_ignore_ascii_case(symbol))
    {
        return Ok(());
    }
    Err(ApiError::bad_request(
        "isin_mismatch",
        "Yahoo Finance does not list this symbol under that ISIN",
    ))
}

async fn found(
    state: &AppState,
    status: StatusCode,
    id: i64,
) -> ApiResult<(StatusCode, Json<InstrumentView>)> {
    let instrument = instruments::get(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::internal("a stored instrument cannot be read back"))?;
    Ok((status, Json(view(state, instrument).await?)))
}

fn provider_failure(error: &MarketError) -> ApiError {
    tracing::warn!(%error, provider = yahoo::PROVIDER, "market data request failed");
    match error {
        MarketError::BadResponse(_) => ApiError::new(
            StatusCode::BAD_GATEWAY,
            "provider_error",
            "Yahoo Finance answered in an unexpected form",
        ),
        _ => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_unavailable",
            "Yahoo Finance cannot be reached now; try again later",
        ),
    }
}

fn validate_query(raw: &str) -> ApiResult<&str> {
    let text = raw.trim();
    if text.is_empty()
        || text.chars().count() > MAX_QUERY_CHARS
        || text.chars().any(char::is_control)
    {
        return Err(ApiError::bad_request(
            "invalid_query",
            format!("a lookup has 1 to {MAX_QUERY_CHARS} characters"),
        ));
    }
    Ok(text)
}

fn validate_symbol(raw: &str) -> ApiResult<&str> {
    // Yahoo symbols use letters, digits, and `.-^=_`. Requiring a letter or digit
    // also keeps `.` and `..` from changing the request path.
    let symbol = raw.trim();
    let allowed = |c: char| c.is_ascii_alphanumeric() || ".-^=_".contains(c);
    if symbol.chars().count() > MAX_SYMBOL_CHARS
        || !symbol.chars().all(allowed)
        || !symbol.chars().any(|c| c.is_ascii_alphanumeric())
    {
        return Err(ApiError::bad_request(
            "invalid_symbol",
            format!(
                "a symbol has 1 to {MAX_SYMBOL_CHARS} letters, digits, or the characters . - ^ = _"
            ),
        ));
    }
    Ok(symbol)
}

fn validate_isin(raw: &str) -> ApiResult<Isin> {
    Isin::parse(raw).ok_or_else(|| {
        ApiError::bad_request(
            "invalid_isin",
            "an ISIN has 12 characters and a valid check digit",
        )
    })
}

/// A classification value: trimmed, lower case, and one of `allowed`.
fn validate_choice(raw: &str, allowed: &[&str], name: &str) -> ApiResult<String> {
    let value = raw.trim().to_ascii_lowercase();
    if !allowed.contains(&value.as_str()) {
        return Err(ApiError::bad_request(
            "invalid_classification",
            format!("`{value}` is not a known {name}"),
        ));
    }
    Ok(value)
}
