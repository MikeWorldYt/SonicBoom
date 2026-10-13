//! Same-origin static assets (JavaScript/CSS) embedded in the binary.
//!
//! Keeping all script and style in these files allows a strict
//! Content-Security-Policy with no `'unsafe-inline'`.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

const INDEX_JS: &[u8] = include_bytes!("../../static/index.js");
const INDEX_CSS: &[u8] = include_bytes!("../../static/index.css");
const ADMIN_CSS: &[u8] = include_bytes!("../../static/admin.css");

/// Short content hash (FNV-1a, hex) used to version asset URLs.
/// A new hash means new bytes, so versioned URLs are immutable.
fn content_version(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn asset_body(file: &str) -> Option<(&'static str, &'static [u8])> {
    match file {
        "index.js" => Some(("text/javascript; charset=utf-8", INDEX_JS)),
        "index.css" => Some(("text/css; charset=utf-8", INDEX_CSS)),
        "admin.css" => Some(("text/css; charset=utf-8", ADMIN_CSS)),
        "setup.js" => Some((
            "application/javascript; charset=utf-8",
            include_bytes!("../../static/setup.js").as_slice(),
        )),
        "setup.css" => Some((
            "text/css; charset=utf-8",
            include_bytes!("../../static/setup.css").as_slice(),
        )),
        _ => None,
    }
}

/// Versioned URL for an asset (`/static/index.js?v=<hash>`).
/// Rendered pages use this so a redeploy can never serve stale
/// cached JS/CSS against new HTML again.
pub fn asset_url(file: &str) -> String {
    match asset_body(file) {
        Some((_, body)) => format!("/static/{file}?v={}", content_version(body)),
        None => format!("/static/{file}"),
    }
}

pub async fn get_static(
    Path(file): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let Some((content_type, body)) = asset_body(&file) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Versioned URLs are immutable; bare URLs revalidate every load
    // so a page referencing them can never go stale for long.
    let cache_control = if query.contains_key("v") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, cache_control),
        ],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, routing::get};
    use tower::ServiceExt;

    fn app() -> Router {
        Router::new().route("/static/{file}", get(get_static))
    }

    async fn fetch(path: &str) -> Response {
        app()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn serves_known_assets_with_content_types() {
        let response = fetch("/static/index.js").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .contains("javascript")
        );
        let response = fetch("/static/index.css").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .contains("css")
        );
        let response = fetch("/static/admin.css").await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn unknown_assets_are_404_without_traversal() {
        for path in ["/static/nope.js", "/static/../secret", "/static/"] {
            let response = fetch(path).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        }
    }

    #[test]
    fn embedded_js_has_no_inline_document_write() {
        // The asset allowlist is fixed at compile time; traversal can only 404.
        let js = std::str::from_utf8(INDEX_JS).unwrap();
        assert!(js.contains("modelReady"));
    }

    #[test]
    fn asset_versions_change_with_content() {
        assert_eq!(content_version(b"abc"), content_version(b"abc"));
        assert_ne!(content_version(b"abc"), content_version(b"abd"));
        for file in [
            "index.js",
            "index.css",
            "admin.css",
            "setup.js",
            "setup.css",
        ] {
            let url = asset_url(file);
            assert!(url.starts_with(&format!("/static/{file}?v=")), "{url}");
            assert_eq!(url.len(), format!("/static/{file}?v=").len() + 16);
        }
        assert_eq!(asset_url("nope.js"), "/static/nope.js");
    }

    #[tokio::test]
    async fn versioned_assets_are_immutable_and_bare_revalidate() {
        let url = asset_url("index.js");
        let query = url.split('?').nth(1).unwrap();
        let response = fetch(&format!("/static/index.js?{query}")).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers()[header::CACHE_CONTROL]
                .to_str()
                .unwrap()
                .contains("immutable")
        );
        let response = fetch("/static/index.js").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CACHE_CONTROL].to_str().unwrap(),
            "no-cache"
        );
    }
}
