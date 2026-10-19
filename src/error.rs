use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// Client-facing request identifier, logged server-side with the full error
/// so operators can correlate reports without leaking internals.
fn correlation_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Field issues contain bounds/counts, never transcript or audio contents.
#[derive(Debug, serde::Serialize)]
pub struct FieldIssue {
    pub field: &'static str,
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub received: Option<serde_json::Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("invalid input: {message}")]
    Validation {
        message: String,
        fields: Vec<FieldIssue>,
    },
    #[error("not found: {0}")]
    NotFound(String),
    /// The request conflicts with existing state. For synthesis jobs this is
    /// an idempotency key reused with a different request fingerprint.
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("unsupported media type: {0}")]
    UnsupportedMediaType(String),
    #[error("payload too large: {0}")]
    PayloadTooLarge(String),
    #[error("too many requests")]
    TooManyRequests(String),
    /// Token-bucket rejection. Same `429` JSON shape as [`AppError::TooManyRequests`]
    /// plus `Retry-After` / `X-RateLimit-*` headers computed from live limiter
    /// state. Inference saturation and playback-queue overflow keep using
    /// [`AppError::TooManyRequests`] (no rate-limit headers), so operators can
    /// tell the three `429` sources apart by message and header presence:
    /// - `RateLimited`: "Rate limit exceeded..." + `Retry-After`
    /// - inference saturated: "Server is busy..." (no headers)
    /// - playback queue full: "Playback queue is full..." (no headers)
    #[error("too many requests")]
    RateLimited {
        message: String,
        retry_after_secs: u64,
        limit: u32,
        remaining: u32,
        reset_secs: u64,
    },
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
    #[error("internal server error: {0}")]
    Internal(String),
}

impl AppError {
    /// Internal errors carry a server-side detail that is logged but never
    /// sent to the client.
    pub fn field(
        field: &'static str,
        code: &'static str,
        message: impl Into<String>,
        expected: Option<serde_json::Value>,
        received: Option<serde_json::Value>,
    ) -> Self {
        let message = message.into();
        Self::Validation {
            message: message.clone(),
            fields: vec![FieldIssue {
                field,
                code,
                message,
                expected,
                received,
            }],
        }
    }

    pub fn internal(detail: impl Into<String>) -> Self {
        AppError::Internal(detail.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let request_id = correlation_id();
        let (status, error_code, message) = match &self {
            AppError::Validation { message, .. } => {
                (StatusCode::BAD_REQUEST, "bad_request", message.clone())
            }
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "bad_request", msg.clone()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, "not_found", msg.clone()),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, "conflict", msg.clone()),
            AppError::UnsupportedMediaType(msg) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_media_type",
                msg.clone(),
            ),
            AppError::PayloadTooLarge(msg) => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                msg.clone(),
            ),
            AppError::TooManyRequests(msg) => (
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_requests",
                msg.clone(),
            ),
            AppError::RateLimited { message, .. } => (
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_requests",
                message.clone(),
            ),
            AppError::ServiceUnavailable(msg) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "service_unavailable",
                msg.clone(),
            ),
            AppError::Internal(detail) => {
                // Log the detail server-side; the client only gets a stable
                // message plus a correlation id.
                tracing::error!(request_id = %request_id, detail = %detail, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_server_error",
                    "An internal error occurred.".to_string(),
                )
            }
        };

        let mut body = json!({
            "error": error_code,
            "code": error_code,
            "message": message,
            "status": status.as_u16(),
            "request_id": request_id,
        });
        if let AppError::Validation { fields, .. } = &self {
            body["fields"] = json!(fields);
        }
        if let AppError::RateLimited {
            retry_after_secs, ..
        } = &self
        {
            body["retry_after_secs"] = json!(retry_after_secs.max(&1));
        }

        let mut response = (status, Json(body)).into_response();
        // Token-bucket rejections carry live retry metadata; the other 429
        // sources (inference/queue saturation) intentionally have no headers.
        if let AppError::RateLimited {
            retry_after_secs,
            limit,
            remaining,
            reset_secs,
            ..
        } = &self
        {
            let headers = response.headers_mut();
            headers.insert(
                axum::http::header::RETRY_AFTER,
                axum::http::HeaderValue::from_str(&(*retry_after_secs).max(1).to_string())
                    .unwrap_or_else(|_| axum::http::HeaderValue::from_static("1")),
            );
            for (name, value) in [
                ("x-ratelimit-limit", limit.to_string()),
                ("x-ratelimit-remaining", remaining.to_string()),
                ("x-ratelimit-reset", (*reset_secs).max(1).to_string()),
            ] {
                if let (Ok(name), Ok(value)) = (
                    axum::http::HeaderName::from_bytes(name.as_bytes()),
                    axum::http::HeaderValue::from_str(&value),
                ) {
                    headers.insert(name, value);
                }
            }
        }
        response
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::internal(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    async fn body_json(response: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn field_errors_preserve_legacy_code_and_expose_only_bounds_and_counts() {
        let response = AppError::field(
            "transcript",
            "too_long",
            "Transcript exceeds its bound",
            Some(json!(10000)),
            Some(json!(10001)),
        )
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = body_json(response).await;
        assert_eq!(body["error"], "bad_request");
        assert_eq!(body["code"], "bad_request");
        assert_eq!(body["fields"][0]["expected"], 10000);
        assert_eq!(body["fields"][0]["received"], 10001);
        assert!(body["request_id"].as_str().is_some());
    }

    #[tokio::test]
    async fn internal_errors_do_not_leak_details() {
        let secret_path = "/srv/secret/models/broken.onnx";
        let response = AppError::internal(format!("failed to open {secret_path}")).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = body_json(response).await;
        let rendered = body.to_string();
        assert!(
            !rendered.contains(secret_path),
            "internal detail leaked: {rendered}"
        );
        assert_eq!(body["error"], "internal_server_error");
        assert!(body.get("request_id").is_some());
    }

    #[tokio::test]
    async fn rate_limit_maps_to_429() {
        let response = AppError::TooManyRequests("slow down".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn rate_limited_carries_retry_headers() {
        let response = AppError::RateLimited {
            message: "Rate limit exceeded. Try again later.".to_string(),
            retry_after_secs: 7,
            limit: 300,
            remaining: 0,
            reset_secs: 42,
        }
        .into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let headers = response.headers();
        assert_eq!(headers.get("retry-after").unwrap(), "7");
        assert_eq!(headers.get("x-ratelimit-limit").unwrap(), "300");
        assert_eq!(headers.get("x-ratelimit-remaining").unwrap(), "0");
        assert_eq!(headers.get("x-ratelimit-reset").unwrap(), "42");
        let body = body_json(response).await;
        assert_eq!(body["error"], "too_many_requests");
        assert_eq!(body["status"], 429);
        assert!(body.get("request_id").is_some());
    }

    #[tokio::test]
    async fn saturation_429s_carry_no_rate_limit_headers() {
        for error in [
            AppError::TooManyRequests(
                "Server is busy. Too many pending inference requests.".to_string(),
            ),
            AppError::TooManyRequests("Playback queue is full. Try again later.".to_string()),
        ] {
            let response = error.into_response();
            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
            assert!(
                response.headers().get("retry-after").is_none(),
                "saturation 429s must stay distinguishable from rate-limit 429s"
            );
        }
    }
}
