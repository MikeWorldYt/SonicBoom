//! Session-protected page data; Vue renders the admin interface.
use crate::{auth::token::Token, security::CspNonce, web::static_files::page};
use serde_json::json;

pub fn login_page(error: Option<&str>, nonce: &CspNonce) -> String {
    page(json!({ "page": "login", "error": error }), nonce)
}
pub fn error_page(message: &str, nonce: &CspNonce) -> String {
    page(json!({ "page": "error", "error": message }), nonce)
}
pub fn admin_page(
    tokens: &[Token],
    csrf_token: &str,
    new_token: Option<&str>,
    nonce: &CspNonce,
) -> String {
    let tokens: Vec<_> = tokens.iter().map(|token| json!({
        "id": token.id, "fingerprint": token.fingerprint(),
        "created": token.created_at.format("%Y-%m-%d %H:%M UTC").to_string(),
        "expires": token.expires_at.map(|e| e.format("%Y-%m-%d %H:%M UTC").to_string()).unwrap_or_else(|| "Never".into()),
        "status": if token.revoked { "Revoked" } else if token.is_valid() { "Active" } else { "Expired" },
        "revoked": token.revoked,
    })).collect();
    page(
        json!({ "page": "admin", "tokens": tokens, "csrf": csrf_token, "newToken": new_token }),
        nonce,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::token::hash_token_value;

    fn nonce() -> CspNonce {
        CspNonce::new()
    }

    #[test]
    fn admin_page_never_renders_raw_tokens() {
        let raw = "super-secret-bearer-value";
        let token = Token::new(hash_token_value(raw), None);
        let html = admin_page(&[token], "csrf", None, &nonce());
        assert!(!html.contains(raw));
        assert!(html.contains("fingerprint"));
    }

    #[test]
    fn admin_page_carries_csrf_and_one_time_token() {
        let html = admin_page(&[], "csrf-abc", Some("one-time-value"), &nonce());
        assert!(html.contains("csrf-abc"));
        assert!(html.contains("one-time-value"));
        assert!(!admin_page(&[], "csrf-abc", None, &nonce()).contains("one-time-value"));
        let component = include_str!("../../frontend/src/pages/Admin.vue");
        assert_eq!(component.matches("name=\"csrf_token\"").count(), 3);
        assert!(component.contains("Copy this token now. It cannot be displayed again."));
    }

    #[test]
    fn admin_errors_remain_data() {
        let html = error_page("\"><script>alert(1)</script>", &nonce());
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;script&gt;"));
    }
}
