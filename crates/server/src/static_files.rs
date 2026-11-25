//! The embedded web application.
//!
//! `just build` writes the Vite bundle to `web/dist` before the Rust build, so release
//! binaries embed the UI. In debug builds `rust-embed` reads the files from disk, and
//! `allow_missing` keeps the crate compiling when the bundle has not been built yet.

use axum::{
    body::Body,
    http::{StatusCode, Uri, header},
    response::Response,
};
use rust_embed::RustEmbed;

const INDEX: &str = "index.html";

#[derive(RustEmbed)]
#[folder = "../../web/dist/"]
#[allow_missing = true]
struct Assets;

/// Serves a bundled asset, or `index.html` for client-side routes.
#[allow(clippy::unused_async)]
pub async fn spa(uri: Uri) -> Response<Body> {
    if Assets::get(INDEX).is_none() {
        return not_built();
    }
    match spa_path(uri.path(), |candidate| Assets::get(candidate).is_some()) {
        Some(path) => asset_response(path),
        None => plain_response(StatusCode::NOT_FOUND, "not found\n"),
    }
}

/// Chooses the file to serve for a request path. `index.html` answers the root and every
/// unknown route, so the client-side router owns them. A missing file (a path whose last
/// segment has an extension, such as a stale `assets/app-1a2b.js`) gets no fallback:
/// answering it with HTML would surface as a script or style error instead of a 404.
fn spa_path(requested: &str, exists: impl Fn(&str) -> bool) -> Option<&str> {
    let requested = requested.trim_start_matches('/');
    if !requested.is_empty() && exists(requested) {
        return Some(requested);
    }
    if looks_like_file(requested) {
        return None;
    }
    exists(INDEX).then_some(INDEX)
}

fn looks_like_file(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|segment| segment.contains('.'))
}

fn asset_response(path: &str) -> Response<Body> {
    let Some(asset) = Assets::get(path) else {
        return plain_response(StatusCode::NOT_FOUND, "not found\n");
    };
    let mime = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, cache_control(path))
        .body(Body::from(asset.data.into_owned()))
        .unwrap_or_else(|error| {
            tracing::error!(%error, path, "cannot build the asset response");
            plain_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error\n")
        })
}

/// Vite content-hashes the files under `assets/`, so only they may be cached forever.
fn cache_control(path: &str) -> &'static str {
    if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

fn not_built() -> Response<Body> {
    plain_response(
        StatusCode::NOT_FOUND,
        "web UI not built; run `just build` or use the Vite dev server\n",
    )
}

fn plain_response(status: StatusCode, text: &'static str) -> Response<Body> {
    let mut response = Response::new(Body::from(text));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spa_path_serves_index_for_root() {
        let path = spa_path("/", |p| p == INDEX);

        assert_eq!(path, Some(INDEX));
    }

    #[test]
    fn spa_path_serves_existing_asset() {
        let path = spa_path("/assets/app.js", |p| p == "assets/app.js" || p == INDEX);

        assert_eq!(path, Some("assets/app.js"));
    }

    #[test]
    fn spa_path_falls_back_to_index_for_client_route() {
        let path = spa_path("/holdings/nokia", |p| p == INDEX);

        assert_eq!(path, Some(INDEX));
    }

    #[test]
    fn spa_path_is_none_for_missing_file() {
        let path = spa_path("/assets/old-1a2b.js", |p| p == INDEX);

        assert_eq!(path, None);
    }

    #[test]
    fn spa_path_falls_back_when_only_an_inner_segment_has_a_dot() {
        let path = spa_path("/instruments/NOKIA.HE/lots", |p| p == INDEX);

        assert_eq!(path, Some(INDEX));
    }

    #[test]
    fn spa_path_is_none_when_bundle_missing() {
        let path = spa_path("/holdings", |_| false);

        assert_eq!(path, None);
    }

    #[test]
    fn cache_control_is_immutable_only_for_hashed_assets() {
        assert_eq!(
            cache_control("assets/app-1a2b.js"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(cache_control("index.html"), "no-cache");
    }
}
