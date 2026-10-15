//! Vite's hashed production assets are compiled into the server binary.
use axum::{
    extract::Path,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
include!(concat!(env!("OUT_DIR"), "/frontend_assets.rs"));

pub async fn get_static(Path(file): Path<String>) -> Response {
    let Some((mime, body)) = asset_body(&file) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        body,
    )
        .into_response()
}

/// Development serves TypeScript through Vite's same-origin localhost proxy.
/// Release binaries always use embedded production assets.
pub fn dev_mode() -> bool {
    cfg!(debug_assertions) && std::env::var("SONICBOOM_FRONTEND_DEV").as_deref() == Ok("1")
}

pub fn page(data: serde_json::Value) -> String {
    let template = if dev_mode() {
        include_str!("../../frontend/index.html").replace(
            "</head>",
            "<script type=\"module\" src=\"/@vite/client\"></script></head>",
        )
    } else {
        include_str!(concat!(env!("OUT_DIR"), "/frontend.html")).to_owned()
    };
    // JSON lives in a quoted data attribute, never executable inline JavaScript.
    let data = data
        .to_string()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;");
    template.replace("SONICBOOM_PAGE_DATA", &data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, routing::get};
    use tower::ServiceExt;

    #[test]
    fn hostile_voice_names_are_escaped() {
        let html = page(
            serde_json::json!({"page": "home", "voices": ["\"><script>alert(1)</script>", "A&B"]}),
        );
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("A&amp;B"));
        assert!(!html.contains("unsafe-eval"));
    }

    #[tokio::test]
    async fn embedded_document_assets_are_available_and_immutable() {
        let html = include_str!(concat!(env!("OUT_DIR"), "/frontend.html"));
        let app = Router::new().route("/assets/{*file}", get(get_static));
        let mut count = 0;
        for part in html.split('"').filter(|part| part.starts_with("/assets/")) {
            let response = app
                .clone()
                .oneshot(Request::get(part).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{part}");
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                "public, max-age=31536000, immutable"
            );
            let mime = response.headers()[header::CONTENT_TYPE].to_str().unwrap();
            assert!(mime.contains(if part.ends_with(".js") {
                "javascript"
            } else {
                "css"
            }));
            count += 1;
        }
        assert!(count >= 2, "Missing JS/CSS references in production HTML");
        let response = app
            .oneshot(
                Request::get("/assets/missing.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
