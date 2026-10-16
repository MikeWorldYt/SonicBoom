//! Bounded, authenticated option sources for isolated plugin clients.
use crate::{auth::AuthenticatedToken, error::AppError};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header},
};
use serde::Serialize;
use sonicboom::tts::providers::ProviderStatusState;

#[derive(Serialize)]
pub struct OptionItem {
    id: String,
    name: String,
    disabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<String>,
}
fn headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    headers
}
pub async fn providers(
    _token: AuthenticatedToken,
    State(state): State<crate::AppState>,
) -> (HeaderMap, Json<Vec<OptionItem>>) {
    let registry = super::providers::build_registry_for_state(&state).await;
    let items = registry
        .statuses()
        .into_iter()
        .filter(|s| s.enabled)
        .map(|s| {
            let label = match s.provider_id.as_str() {
                "supertonic" => "Supertonic",
                "pocket-tts" => "Pocket TTS",
                "qwen3-gguf" => "Qwen3-TTS GGUF",
                _ => "Unknown provider",
            };
            let ready = s.state == ProviderStatusState::Ready;
            OptionItem {
                id: s.provider_id,
                name: if ready {
                    label.into()
                } else {
                    format!("{label} ({})", s.state.as_str())
                },
                disabled: !ready,
                state: Some(s.state.as_str().into()),
            }
        })
        .collect();
    (headers(), Json(items))
}
pub async fn voices(
    token: AuthenticatedToken,
    State(state): State<crate::AppState>,
) -> Result<(HeaderMap, Json<Vec<OptionItem>>), AppError> {
    let registry = super::providers::build_registry_for_state(&state).await;
    let config = state.config.get().await;
    let owner = token.rate_limit_key();
    let profiles = super::voices::with_store(&state, move |store| {
        Ok(store
            .list()
            .into_iter()
            .filter(|p| p.owner_token_hash.as_deref() == Some(owner.as_str()))
            .cloned()
            .collect::<Vec<_>>())
    })
    .await?;
    let mut items = Vec::new();
    if let Some(handle) = super::providers::supertonic_handle(&state).await {
        for name in handle.voice_styles.keys() {
            items.push(OptionItem {
                id: format!("supertonic:{name}"),
                name: name.clone(),
                disabled: false,
                state: None,
            });
        }
    }
    for profile in profiles {
        let Some(provider) = registry.get(&profile.provider) else {
            continue;
        };
        let status = provider.status();
        let capabilities = provider.capabilities();
        if !status.enabled
            || status.state != ProviderStatusState::Ready
            || status.active_model.as_deref() != Some(profile.model.as_str())
            || profile.model_revision
                != super::voices::config_revision_for(&config, &profile.provider)
            || !capabilities.models.contains(&profile.model)
            || !capabilities.languages.contains(&profile.language)
            || !capabilities.clone_modes.contains(&profile.clone_mode)
        {
            continue;
        }
        items.push(OptionItem {
            id: format!("{}:{}", profile.provider, profile.id),
            name: profile.name.chars().take(128).collect(),
            disabled: false,
            state: None,
        });
    }
    items.sort_by(|a, b| a.id.cmp(&b.id));
    items.truncate(500);
    Ok((headers(), Json(items)))
}
