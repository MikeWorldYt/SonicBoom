export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type JsonObject = { [key: string]: Json };
export interface ServerInfo { version: string; mode: string; setup_complete: boolean; auth: { mode: string; required: boolean }; features: { playback: boolean; audio_devices: boolean; hot_reload: boolean }; config_revision: number; model: { status: string } }
export interface ProviderStatus { provider_id: string; state: string; enabled: boolean; effective_device?: string | null; active_model?: string | null; detail?: string | null }
export interface Capabilities { provider_id: string; models: string[]; languages: string[]; devices: string[]; clone_modes: string[]; requires_reference_audio: boolean; requires_reference_transcript: boolean; sample_rate?: number }
export interface ProvidersResponse { providers: ProviderStatus[]; capabilities: Capabilities[]; diagnostics?: unknown }
export interface Voice { id: string; name: string; provider: string; model: string; language: string; clone_mode: string; reference_duration_secs?: number; reference_sample_rate?: number; warnings?: string[]; defects?: string[]; guidance?: string[] }
export interface BuiltinVoice { id: string; name: string }
export interface ConfigSnapshot { revision: number; config: JsonObject }
export type ApplyStrategy = 'hot' | 'restart_server' | 'restart_subsystem' | 'reload_model' | 'restart_process';
export interface SettingSchema { path: string; description: string; type: 'string' | 'integer' | 'number' | 'boolean' | 'array' | 'object'; apply: ApplyStrategy; min?: number; max?: number; allowed_values?: string[]; nullable: boolean; secret: boolean; allowed_values_source?: string }
export type Schema = Record<string, SettingSchema>;
export interface ConfigStatus { revision: number; config_path: string; watcher?: string; last_reload?: string; last_error?: string; pending_applies?: string[]; apply_failures?: { path: string; error: string; active: Json }[] }
export interface AudioDevice { id: string; name: string }
export interface QualityIssue { kind: string; severity: 'info' | 'warning' | 'error'; detail: string; confidence?: number | null }
export interface AudioHealth { sample_count: number; sample_rate: number; duration_secs: number; non_finite_samples: number; peak: number; rms: number; dc_offset: number; clipped_samples: number; clipping_fraction: number; near_silent_fraction: number; max_discontinuity: number; impulsive_samples: number; chunk_boundary_peaks: number }
export interface EchoAnalysis { detected: boolean; asr_required: boolean; matched_words: number; candidate_words: number; at_start: boolean; confidence?: number | null; evidence: string[] }
export interface QualityReport { quality_status: 'ok' | 'warning' | 'failed'; issues: QualityIssue[]; audio?: AudioHealth; echo?: EchoAnalysis; retries: number; mode: string }
export interface Job { id?: string; stage: string; elapsed_secs?: number; audio_secs?: number; rtf?: number; frames?: number; cancellation_requested?: boolean; deadline_exceeded?: boolean; error?: string; quality?: QualityReport; context?: { provider: string; model: string; voice: string; effective_device: string; seed?: string } }
export interface CloneInput { name: string; provider: string; model: string; language: string; mode: string; consent_basis: string; transcript?: string }
export interface SetupStatus { model: { cache_dir: string }; server: { port: number } }
export interface SetupInput { usage_mode: string; audio_device: string; inference_steps: number; model_cache_dir: string | null; bind: string | null; port: number | null; auth_mode: string | null; admin_username: string; admin_password: string }
export interface SetupResult { api_url: string; audio_device: string; auth: string; api_token?: string }
