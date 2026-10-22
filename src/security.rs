use axum::{
    body::Body,
    extract::State,
    http::{HeaderValue, Request, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

use crate::config::ConfigManager;

/// Content-Security-Policy for HTML pages.
///
/// `{nonce}` is replaced with a fresh random nonce per response. It lets
/// exactly one audited inline style load (the audio player renders its
/// waveform inside a shadow root with an inline stylesheet) without opening
/// `style-src` to `'unsafe-inline'`.
pub const CSP_TEMPLATE: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'nonce-{nonce}'; img-src 'self' data:; media-src 'self' blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'; object-src 'none'";

/// Development keeps `'unsafe-inline'` for styles: the Vite client injects
/// stylesheets at runtime.
const DEV_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; media-src 'self' blob:; connect-src 'self' ws://localhost:5173; frame-ancestors 'none'; base-uri 'none'; form-action 'self'; object-src 'none'";

/// Per-response CSP nonce, shared with the page so the frontend can mark the
/// one inline style it owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CspNonce(String);

impl CspNonce {
    /// 128 bits of randomness, hex encoded: a valid CSP nonce value.
    pub fn new() -> Self {
        use rand::RngCore;
        let mut bytes = [0u8; 16];
        rand::rng().fill_bytes(&mut bytes);
        CspNonce(hex::encode(bytes))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for CspNonce {
    fn default() -> Self {
        Self::new()
    }
}

/// Add defense-in-depth security headers to responses.
///
/// - `X-Content-Type-Options: nosniff` on everything.
/// - `Referrer-Policy: same-origin` keeps URLs private across origins while
///   preserving browser Origin headers for same-origin admin form posts.
/// - `X-Frame-Options: DENY` plus CSP `frame-ancestors 'none'` on HTML pages
///   to prevent clickjacking of the admin UI.
/// - A strict CSP on HTML pages. All JavaScript and CSS live in same-origin
///   static assets, so no `'unsafe-inline'` is needed; the single inline
///   style the audio player owns is unlocked with a per-response nonce. The
///   TTS demo page plays audio from a blob URL, hence `media-src 'self' blob:`.
pub async fn security_headers(
    State(config): State<Arc<ConfigManager>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let nonce = CspNonce::new();
    request.extensions_mut().insert(nonce.clone());
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    // HSTS is opt-in: only enable when the deployment is known-HTTPS
    // (direct TLS or a trusted TLS-terminating proxy for all traffic).
    // Read live: the header follows hot-reloaded configuration.
    if config.get().await.security.enable_hsts {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=63072000; includeSubDomains"),
        );
    }
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );

    let is_html = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.contains("text/html"));
    if is_html {
        headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
        let policy = if crate::web::static_files::dev_mode() {
            DEV_CSP.to_owned()
        } else {
            CSP_TEMPLATE.replace("{nonce}", nonce.as_str())
        };
        if let Ok(value) = HeaderValue::from_str(&policy) {
            headers.insert(header::CONTENT_SECURITY_POLICY, value);
        }
    }
    response
}

/// Reject browser cross-origin mutations and DNS rebinding in local mode.
pub async fn request_origin_guard(
    State(config): State<Arc<ConfigManager>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let cfg = config.get().await;
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok());
    if cfg.server.auth_mode == crate::config::AuthMode::Local {
        let valid_host = host
            .and_then(|host| format!("http://{host}").parse::<axum::http::Uri>().ok())
            .and_then(|uri| uri.host().map(str::to_string))
            .is_some_and(|host| {
                host.eq_ignore_ascii_case("localhost")
                    || host
                        .trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            });
        if !valid_host {
            return axum::http::StatusCode::FORBIDDEN.into_response();
        }
    }
    if !matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
    ) {
        let cross_site = request
            .headers()
            .get("sec-fetch-site")
            .is_some_and(|v| v == "cross-site");
        let foreign_origin = request.headers().get(header::ORIGIN).is_some_and(|origin| {
            origin
                .to_str()
                .ok()
                .and_then(|origin| origin.parse::<axum::http::Uri>().ok())
                .and_then(|origin| origin.authority().map(|a| a.as_str().to_string()))
                .as_deref()
                != host
        });
        if cross_site || foreign_origin {
            return axum::http::StatusCode::FORBIDDEN.into_response();
        }
    }
    next.run(request).await
}

/// Prevent caching of sensitive admin responses (auth state, one-time token
/// display, auth redirects). Applied as a layer over the whole admin router.
pub async fn no_store_cache(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert("pragma", HeaderValue::from_static("no-cache"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        middleware,
        routing::get,
    };
    use tower::ServiceExt;

    use crate::config::AppConfig;

    async fn body_text(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn test_config(hsts: bool) -> Arc<ConfigManager> {
        let mut config = AppConfig::default();
        config.security.enable_hsts = hsts;
        ConfigManager::in_memory(config)
    }

    fn app() -> Router {
        app_with_hsts(false)
    }

    fn app_with_hsts(hsts: bool) -> Router {
        Router::new()
            .route(
                "/html",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                        "<html></html>",
                    )
                }),
            )
            .route(
                "/audio",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "audio/ogg; codecs=opus")],
                        vec![0u8, 1u8],
                    )
                }),
            )
            .layer(middleware::from_fn_with_state(
                test_config(hsts),
                security_headers,
            ))
    }

    #[tokio::test]
    async fn html_responses_carry_strict_headers() {
        let response = app()
            .oneshot(Request::get("/html").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers();
        assert_eq!(
            headers.get(header::X_CONTENT_TYPE_OPTIONS).unwrap(),
            "nosniff"
        );
        assert_eq!(headers.get(header::REFERRER_POLICY).unwrap(), "same-origin");
        assert_eq!(headers.get(header::X_FRAME_OPTIONS).unwrap(), "DENY");
        let csp = headers
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(csp.contains("frame-ancestors 'none'"), "csp: {csp}");
        assert!(!csp.contains("'unsafe-inline'"), "csp: {csp}");
        assert!(csp.contains("script-src 'self'"), "csp: {csp}");
        assert!(csp.contains("style-src 'self'"), "csp: {csp}");
        assert!(csp.contains("base-uri 'none'"), "csp: {csp}");
        assert!(csp.contains("object-src 'none'"), "csp: {csp}");
        assert!(csp.contains("media-src 'self' blob:"), "csp: {csp}");
        assert_eq!(body_text(response).await, "<html></html>");
    }

    /// The inline style the audio player owns is unlocked per response, so the
    /// policy stays strict while the one audited stylesheet still loads.
    #[tokio::test]
    async fn html_responses_carry_a_fresh_nonce() {
        let nonce_of = |csp: &str| {
            csp.split("'nonce-")
                .nth(1)
                .and_then(|rest| rest.split('\'').next())
                .unwrap_or_default()
                .to_string()
        };
        let first = app()
            .oneshot(Request::get("/html").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let first_csp = first
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            first_csp.contains("style-src 'self' 'nonce-"),
            "csp: {first_csp}"
        );
        assert!(!first_csp.contains("'unsafe-inline'"), "csp: {first_csp}");
        let first_nonce = nonce_of(&first_csp);
        assert_eq!(first_nonce.len(), 32, "nonce: {first_nonce}");
        assert!(first_nonce.chars().all(|c| c.is_ascii_hexdigit()));
        let second = app()
            .oneshot(Request::get("/html").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let second_csp = second
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_ne!(first_nonce, nonce_of(&second_csp), "nonces must be fresh");
    }

    #[tokio::test]
    async fn hsts_is_opt_in() {
        let response = app()
            .oneshot(Request::get("/html").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(
            response
                .headers()
                .get(header::STRICT_TRANSPORT_SECURITY)
                .is_none()
        );
        let response = app_with_hsts(true)
            .oneshot(Request::get("/html").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response
                .headers()
                .get(header::STRICT_TRANSPORT_SECURITY)
                .unwrap(),
            "max-age=63072000; includeSubDomains"
        );
    }

    #[tokio::test]
    async fn no_store_middleware_marks_responses() {
        let app = Router::new()
            .route("/sensitive", get(|| async { "secret" }))
            .layer(middleware::from_fn(no_store_cache));
        let response = app
            .oneshot(Request::get("/sensitive").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        assert_eq!(response.headers().get("pragma").unwrap(), "no-cache");
    }

    #[tokio::test]
    async fn non_html_responses_skip_framing_headers() {
        let response = app()
            .oneshot(Request::get("/audio").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .unwrap(),
            "nosniff"
        );
        assert!(response.headers().get(header::X_FRAME_OPTIONS).is_none());
    }
}
