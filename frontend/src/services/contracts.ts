export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type JsonObject = { [key: string]: Json };
export interface NumericBounds { min: number; max: number; integer?: boolean; exclusive_min?: boolean }
export interface SynthesisConstraints {
  max_text_chars: number; max_body_bytes: number;
  sampling: { provider: string; seed_max: string; temperature: NumericBounds; top_k: NumericBounds; top_p: NumericBounds; repetition_penalty: NumericBounds };
}
export interface ServerInfo { synthesis?: SynthesisConstraints; normalization?: NormalizationInfo; version: string; mode: string; setup_complete: boolean; auth: { mode: string; required: boolean }; features: { playback: boolean; audio_devices: boolean; hot_reload: boolean }; config_revision: number; model: { status: string } }
export interface BackendDiagnostics { compiled_backends: string[]; available_runtime_backends: string[]; vulkan_gpu_name?: string | null; requested_device: string; effective_device?: string | null; model_loading_status: string; vulkan_initialization_error?: string | null; cpu_fallback_reason?: string | null; status: string }
export interface ProviderStatus { model_revision?: string; backend_diagnostics?: BackendDiagnostics; provider_id: string; state: string; enabled: boolean; effective_device?: string | null; active_model?: string | null; detail?: string | null }
export interface Capabilities { provider_id: string; models: string[]; languages: string[]; devices: string[]; clone_modes: string[]; requires_reference_audio: boolean; requires_reference_transcript: boolean; sample_rate?: number }
export interface ProvidersResponse { providers: ProviderStatus[]; capabilities: Capabilities[]; diagnostics?: unknown }
export interface Voice { model_revision?: string; id: string; name: string; provider: string; model: string; language: string; clone_mode: string; created_at?: string; updated_at?: string; reference_duration_secs?: number; reference_sample_rate?: number; warnings?: string[]; defects?: string[]; guidance?: string[] }
export interface ReferenceHealth { duration_secs: number; source_channels: number; source_sample_rate: number; peak: number; rms: number; clipping_fraction: number; low_amplitude_fraction: number; non_finite_samples: number }
export interface VoiceCreateResult extends Voice { revision?: number; reference_health: ReferenceHealth; warnings: string[]; defects: string[]; guidance: string[] }
export interface BuiltinVoice { id: string; name: string }
export interface ConfigSnapshot { revision: number; config: JsonObject }
export type ApplyStrategy = 'hot' | 'restart_server' | 'restart_subsystem' | 'reload_model' | 'restart_process';
export interface SettingSchema { path: string; description: string; type: 'string' | 'integer' | 'number' | 'boolean' | 'array' | 'object'; apply: ApplyStrategy; min?: number; max?: number; allowed_values?: string[]; nullable: boolean; secret: boolean; allowed_values_source?: string }
export type Schema = Record<string, SettingSchema>;
export interface ConfigStatus { pending_restart?: boolean; valid?: boolean; revision: number; config_path: string; watcher?: string; last_reload?: string; last_error?: string; pending_applies?: string[]; apply_failures?: { path: string; error: string; active: Json }[] }
export interface AudioDevice { id: string; name: string }
export interface QualityIssue { kind: string; severity: 'info' | 'warning' | 'error'; detail: string; confidence?: number | null }
export interface AudioHealth { sample_count: number; sample_rate: number; duration_secs: number; non_finite_samples: number; peak: number; rms: number; dc_offset: number; clipped_samples: number; clipping_fraction: number; near_silent_fraction: number; max_discontinuity: number; impulsive_samples: number; chunk_boundary_peaks: number }
export interface EchoAnalysis { detected: boolean; asr_required: boolean; matched_words: number; candidate_words: number; at_start: boolean; confidence?: number | null; evidence: string[] }
export interface QualityReport { quality_status: 'ok' | 'warning' | 'failed'; issues: QualityIssue[]; audio?: AudioHealth; echo?: EchoAnalysis; retries: number; mode: string }
export interface Job { id?: string; stage: string; elapsed_secs?: number; audio_secs?: number; rtf?: number; frames?: number; cancellation_requested?: boolean; deadline_exceeded?: boolean; error?: string; quality?: QualityReport; context?: { provider: string; model: string; voice: string; effective_device: string; seed?: string } }
export interface CloneInput { expected_revision?: number; name: string; provider: string; model: string; language: string; mode: string; consent_basis: string; transcript?: string }
export interface SetupConstraints { version: number; password_min_chars: number; rejected_passwords: string[]; max_inference_steps: number; auth_modes: string[]; allow_insecure_remote: boolean }
export interface SetupStatus { constraints?: SetupConstraints; model: { cache_dir: string }; server: { port: number } }
export interface SetupInput { usage_mode: string; audio_device: string; inference_steps: number; model_cache_dir: string | null; bind: string | null; port: number | null; auth_mode: string | null; admin_username: string; admin_password: string }
export interface SetupResult { api_url: string; audio_device: string; auth: string; api_token?: string }

export interface VoiceConstraints {
  revision?: number;
  version: number; max_upload_bytes: number; min_sample_rate: number; max_sample_rate: number;
  max_channels: number; name_max_chars: number; transcript_max_chars: number;
  providers: Record<string, { duration_seconds: [number, number]; clone_modes: string[]; models: string[]; languages: string[]; transcript_required_for: string[] }>;
}

/** Pronunciation (text normalization) mode: `auto` converts numerals, dates, etc.; `off` speaks the text as typed. */
export type NormalizationMode = 'auto' | 'off';
export type CoverageLevel = 'full' | 'partial' | 'none';
/** One speech language in the `normalization` block of `GET /api/info`. */
export interface NormalizationLanguage { language: string; locales: string[]; default_locale: string; rules: CoverageLevel; features: Record<string, CoverageLevel> }
export interface NormalizationInfo { ruleset_version: string; default_mode: NormalizationMode; languages: NormalizationLanguage[] }
/** Optional synthesis query fields. Absent fields are never sent, so legacy requests are unchanged. */
export interface SpeechPronunciation { normalization?: NormalizationMode; locale?: string }
export interface NormalizeRequest { text: string; language: string; locale?: string; mode?: NormalizationMode; model?: string }
/** `start`/`end` are UTF-8 byte offsets into the original text: display `before`/`after`, never slice with them. */
export interface NormalizationChange { kind: string; before: string; after: string; start: number; end: number; confidence: string }
export interface NormalizationWarning { code: string; before: string; start: number; end: number }
export interface NormalizePreview { original: string; spoken: string; language: string; locale: string; mode: NormalizationMode; normalizer_version: string; changes: NormalizationChange[]; warnings: NormalizationWarning[]; coverage: Record<string, CoverageLevel> }
