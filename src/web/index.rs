use axum::{
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

use crate::{AppState, tts::ModelStatus};

// Load the HTML template at compile time
const TEMPLATE: &str = include_str!("../../templates/index.html");

/// Liveness: the process is alive. Always 200 when the server can respond.
/// Used by container `HEALTHCHECK` / load-balancer liveness probes.
pub async fn get_health() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

/// Readiness: the model is loaded and the server can synthesize.
/// Used by load-balancer readiness probes and orchestrators.
pub async fn get_ready(State(state): State<AppState>) -> impl IntoResponse {
    let status = state.model_status.read().await;
    match &*status {
        ModelStatus::Ready(_) => (StatusCode::OK, "ready"),
        ModelStatus::Downloading { .. } => (StatusCode::SERVICE_UNAVAILABLE, "model downloading"),
        ModelStatus::Loading => (StatusCode::SERVICE_UNAVAILABLE, "model loading"),
        ModelStatus::Idle => (StatusCode::SERVICE_UNAVAILABLE, "model idle"),
        ModelStatus::Failed(_) => (StatusCode::SERVICE_UNAVAILABLE, "model failed to load"),
    }
}

pub async fn get_index(State(state): State<AppState>) -> Response {
    if !state.config.get().await.setup_complete {
        return axum::response::Redirect::to("/setup").into_response();
    }
    // Collect available voices
    let voices: Vec<String> = {
        let status = state.model_status.read().await;
        match &*status {
            ModelStatus::Ready(handle) => {
                let mut names: Vec<String> = handle.voice_styles.keys().cloned().collect();
                names.sort();
                names
            }
            _ => vec![],
        }
    };

    let status_msg = {
        let status = state.model_status.read().await;
        match &*status {
            ModelStatus::Idle => "Preparing model...".to_string(),
            ModelStatus::Downloading { progress } => {
                format!("Downloading model... ({:.0}%)", progress * 100.0)
            }
            ModelStatus::Loading => "Loading model...".to_string(),
            ModelStatus::Ready(_) => String::new(),
            // Never render internal load errors into the page.
            ModelStatus::Failed(_) => "Model load failed. Check server logs.".to_string(),
        }
    };

    let model_ready = !voices.is_empty();

    let voice_options: String = voice_options_html(&voices);

    let provider_rows = provider_rows_html(&state).await;

    let html = TEMPLATE
        .replace("PROVIDERS_ROWS", &provider_rows)
        .replace("STATUS_MSG", &status_msg)
        .replace(
            "STATUS_HIDDEN",
            if status_msg.is_empty() { "hidden" } else { "" },
        )
        .replace(
            "VOICE_OPTIONS",
            if voice_options.is_empty() {
                "<option value=\"\">No voices available</option>"
            } else {
                &voice_options
            },
        )
        .replace("VOICE_DISABLED", if !model_ready { "disabled" } else { "" })
        .replace("BTN_DISABLED", if !model_ready { "disabled" } else { "" })
        .replace("MODEL_READY_JS", if model_ready { "true" } else { "false" });
    let html = versioned_asset_urls(&html);

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

/// Rewrite bare `/static/…` URLs to content-hashed versions so a
/// redeploy can never serve stale cached JS/CSS against new HTML.
/// Extracted as a pure function so it is unit-testable without `AppState`.
fn versioned_asset_urls(html: &str) -> String {
    html.replace(
        "/static/index.js",
        &super::static_files::asset_url("index.js"),
    )
    .replace(
        "/static/index.css",
        &super::static_files::asset_url("index.css"),
    )
}

/// Render voice names as `<option>` elements. Model metadata is still
/// data: names are escaped for both the attribute and text contexts so a
/// hostile or surprising voice name can never break out of the markup.
fn voice_options_html(voices: &[String]) -> String {
    voices
        .iter()
        .map(|v| {
            let escaped = html_escape(v);
            format!(r#"<option value="{escaped}">{escaped}</option>"#)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Server-rendered provider summary (public metadata only: no secrets,
/// no reference transcripts, no audio bytes). Rendered as status cards
/// (not a table) so long model lists and detail strings stay readable.
/// This page never implies an unavailable model can synthesize.
async fn provider_rows_html(state: &AppState) -> String {
    let config = state.config.get().await;
    let handle = {
        let status = state.model_status.read().await;
        match &*status {
            ModelStatus::Ready(handle) => Some(std::sync::Arc::clone(handle)),
            _ => None,
        }
    };
    let registry = crate::api::providers::build_registry(&config, handle, &state.providers);
    let mut cards = Vec::new();
    for status in registry.statuses() {
        cards.push(provider_card_html(
            &status.provider_id,
            status.state.as_str(),
            status.effective_device.as_deref().unwrap_or("—"),
            &registry
                .get(&status.provider_id)
                .map(|p| p.capabilities().models.join(", "))
                .unwrap_or_default(),
            status.active_model.as_deref().unwrap_or(""),
            status.detail.as_deref().unwrap_or(""),
        ));
    }
    cards.join("\n")
}

/// One provider status card. All interpolated strings are escaped for
/// both element and attribute contexts so provider metadata can never
/// break out of the markup.
/// Badge class for a provider lifecycle state.
fn provider_badge_class(state: &str) -> &'static str {
    match state {
        "ready" => "badge-ready",
        "loading" => "badge-loading",
        "disabled" => "badge-disabled",
        _ => "badge-not_installed",
    }
}

/// Human-readable label for a provider lifecycle state (`not_installed` → `not installed`).
fn provider_state_label(state: &str) -> String {
    state.replace('_', " ")
}

/// One escaped card paragraph; the shared wrapper keeps every
/// optional provider line in a single component.
fn card_para(class: &str, body: String) -> String {
    format!("<p class=\"{class}\">{body}</p>")
}

/// One provider status card. All interpolated strings are escaped for
/// both element and attribute contexts so provider metadata can never
/// break out of the markup.
fn provider_card_html(
    provider_id: &str,
    state: &str,
    device: &str,
    models: &str,
    active_model: &str,
    detail: &str,
) -> String {
    let mut card = format!(
        "<div class=\"provider\"><div class=\"provider-top\"><span class=\"provider-name\">{}</span> \
         <span class=\"badge {}\">{}</span> \
         <span class=\"device-chip\">{}</span></div>",
        html_escape(provider_id),
        provider_badge_class(state),
        html_escape(&provider_state_label(state)),
        html_escape(device),
    );
    if !models.is_empty() {
        card.push_str(&card_para(
            "provider-models",
            format!("<span>Models:</span> {}", html_escape(models)),
        ));
    }
    if !active_model.is_empty() {
        card.push_str(&card_para(
            "provider-active",
            format!("Active model: <code>{}</code>", html_escape(active_model)),
        ));
    }
    if !detail.is_empty() {
        card.push_str(&card_para("provider-detail", html_escape(detail)));
    }
    card.push_str("</div>");
    card
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::{
        TEMPLATE, card_para, provider_badge_class, provider_card_html, provider_state_label,
        versioned_asset_urls, voice_options_html,
    };

    #[test]
    fn index_template_has_no_inline_script_or_style() {
        assert!(!TEMPLATE.contains("<script>"), "inline script found");
        assert!(!TEMPLATE.contains("<style>"), "inline style found");
        assert!(!TEMPLATE.contains("style="), "inline style attribute found");
        assert!(TEMPLATE.contains(r#"<script src="/static/index.js" defer></script>"#));
        assert!(TEMPLATE.contains(r#"<link rel="stylesheet" href="/static/index.css">"#));
        assert!(TEMPLATE.contains("data-model-ready="));
    }

    #[test]
    fn index_template_has_config_browser_hooks() {
        for id in [
            "configSearch",
            "configCategory",
            "configGroups",
            "configRefresh",
            "configReload",
            "configValidate",
            "configNotice",
            "provider-list",
            "charCount",
        ] {
            assert!(TEMPLATE.contains(id), "template missing #{id}");
        }
        assert!(
            !TEMPLATE.contains("<table>"),
            "provider table should be cards, not a table"
        );
    }

    #[test]
    fn language_options_live_in_js_not_in_template() {
        // The language list exists once (LANGUAGES in index.js); the
        // template only carries the empty select plus a pointer comment.
        assert!(TEMPLATE.contains(r#"<select id="lang""#));
        assert!(!TEMPLATE.contains(r#"<option value="ko">"#));
        assert!(!TEMPLATE.contains(r#"<option value="et">"#));
        assert!(TEMPLATE.contains("LANGUAGES const in index.js"));
    }

    #[test]
    fn provider_badge_classes_cover_every_state() {
        assert_eq!(provider_badge_class("ready"), "badge-ready");
        assert_eq!(provider_badge_class("loading"), "badge-loading");
        assert_eq!(provider_badge_class("disabled"), "badge-disabled");
        // Unknown/future states fall back to the warning badge, never unstyled.
        assert_eq!(provider_badge_class("not_installed"), "badge-not_installed");
        assert_eq!(provider_badge_class("error"), "badge-not_installed");
        assert_eq!(provider_state_label("not_installed"), "not installed");
    }

    #[test]
    fn card_paragraph_wraps_body_in_one_component() {
        assert_eq!(
            card_para("provider-detail", "hi".to_string()),
            r#"<p class="provider-detail">hi</p>"#
        );
    }

    #[test]
    fn rendered_assets_are_content_versioned() {
        let html = versioned_asset_urls(TEMPLATE);
        assert!(!html.contains(r#"src="/static/index.js""#), "{html}");
        assert!(!html.contains(r#"href="/static/index.css""#), "{html}");
        assert!(html.contains("/static/index.js?v="), "{html}");
        assert!(html.contains("/static/index.css?v="), "{html}");
    }

    #[test]
    fn provider_cards_render_status_badges() {
        let html = provider_card_html(
            "qwen3",
            "disabled",
            "—",
            "qwen3-tts-0.6b-base",
            "",
            "provider disabled in configuration",
        );
        assert!(html.contains("provider-name"), "{html}");
        assert!(html.contains("badge-disabled"), "{html}");
        assert!(html.contains("qwen3-tts-0.6b-base"), "{html}");
        assert!(!html.contains("<table"), "{html}");
        assert!(!html.contains("<tr"), "{html}");
    }

    #[test]
    fn provider_cards_escape_hostile_metadata() {
        let html = provider_card_html(
            "\"><script>alert(1)</script>",
            "not_installed",
            "cpu",
            "a<b",
            "m\"odel",
            "d&t",
        );
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("&quot;&gt;&lt;script&gt;"), "{html}");
        assert!(html.contains("a&lt;b"), "{html}");
        assert!(html.contains("m&quot;odel"), "{html}");
        assert!(html.contains("d&amp;t"), "{html}");
    }

    #[test]
    fn hostile_voice_names_are_escaped() {
        let voices = vec![
            "\"><script>alert(1)</script>".to_string(),
            "A&B".to_string(),
            "a<b>c".to_string(),
            "quote\"test".to_string(),
            "apostrophe'test".to_string(),
            "M1".to_string(),
        ];
        let html = voice_options_html(&voices);
        assert!(
            !html.contains("<script>"),
            "voice markup became executable: {html}"
        );
        assert!(!html.contains("\"><script>"), " breakout: {html}");
        assert!(
            html.contains("&quot;&gt;&lt;script&gt;"),
            "missing escape: {html}"
        );
        assert!(html.contains("A&amp;B"), "missing escape: {html}");
        assert!(html.contains("a&lt;b&gt;c"), "missing escape: {html}");
        assert!(html.contains("quote&quot;test"), "missing escape: {html}");
        assert!(
            html.contains("apostrophe&#39;test"),
            "missing escape: {html}"
        );
        assert!(html.contains(r#"<option value="M1">M1</option>"#));
    }
}
