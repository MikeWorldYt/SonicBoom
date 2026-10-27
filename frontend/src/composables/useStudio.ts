import { computed, inject, onMounted, onUnmounted, ref, shallowRef, watch, type InjectionKey } from 'vue';
import { samplingQuery, samplingErrors, type SamplingDraft } from '../features/speech/generationSchemas';
export { samplingQuery } from '../features/speech/generationSchemas';
import { isCancelledError } from '@tanstack/vue-query';
import { createResourceCache } from '../app/queryClient';
import { createHttpClient, ApiError } from '../services/http';
import { createApi, type Api } from '../services/api';
import { effectiveLocale, localeChoices, showLocaleChoice } from '../features/speech/pronunciation';
import { modelLanguages } from '../services/providerLanguages';
import { activatePocketModel as activatePocketBundle } from '../services/pocketActivation';
import type { HomeData } from '../types';
import type { NormalizationMode, SpeechPronunciation, ServerInfo, ProviderStatus, Capabilities, Voice, BuiltinVoice, ConfigSnapshot, ConfigStatus, Schema, Json, JsonObject, AudioDevice } from '../services/contracts';
import { errorMessage, type Message } from '../i18n';
export function getPath(object: JsonObject | undefined, path: string): Json | undefined {
  return path.split('.').reduce<Json | undefined>((value, key) => value && typeof value === 'object' && !Array.isArray(value) ? value[key] : undefined, object);
}
export function nestedChange(path: string, value: Json): JsonObject {
  return path.split('.').reverse().reduce<Json>((result, key) => ({ [key]: result }), value) as JsonObject;
}
/** Fast background cadence: provider/job-facing status only. */
export const STUDIO_FAST_MS = 5000;
/** Slow background cadence: low-frequency metadata (voice list, snapshots). */
export const STUDIO_SLOW_EVERY = 6;
/** Decides whether the slow metadata refresh is due on a fast tick. */
export function slowRefreshDue(tick: number): boolean {
  return tick % STUDIO_SLOW_EVERY === 0;
}

/** Qwen3-TTS advanced generation settings (all optional, string-backed). */
export type SamplingState = SamplingDraft;
export const DEFAULT_SAMPLING: SamplingState = {
  preset: 'balanced', seed: '', quality: '',
  talkerTemperature: '', talkerTopK: '', talkerTopP: '', talkerRepetitionPenalty: '',
  predictorTemperature: '', predictorTopK: '', predictorTopP: '', predictorRepetitionPenalty: '',
};
const samplingStorageKey = 'sonicboom.sampling';
function loadSampling(): SamplingState {
  try {
    const saved = localStorage.getItem(samplingStorageKey);
    if (saved) {
      const value: unknown = JSON.parse(saved);
      if (value && typeof value === 'object') return Object.fromEntries(Object.entries(DEFAULT_SAMPLING).map(([key, fallback]) => [key, typeof (value as Record<string, unknown>)[key] === 'string' ? (value as Record<string, string>)[key] : fallback])) as unknown as SamplingState;
    }
  } catch { /* Storage can be unavailable in restricted contexts. */ }
  return { ...DEFAULT_SAMPLING };
}
function persistSampling(sampling: SamplingState) {
  try { localStorage.setItem(samplingStorageKey, JSON.stringify(sampling)); } catch { /* session-only */ }
}
export function createStudio(data: HomeData, overrides?: { api?: Api }) {
  const token = ref('');
  const scopeEpoch = ref(0);
  const info = shallowRef<ServerInfo | null>(null);
  const providers = shallowRef<ProviderStatus[]>(data.providers.map(entry => entry.status));
  const capabilities = shallowRef<Capabilities[]>([]);
  const diagnostics = shallowRef<unknown>(null);
  const builtinVoices = shallowRef<BuiltinVoice[]>(data.voices.map(id => ({ id, name: id })));
  const voices = shallowRef<Voice[]>([]);
  const config = shallowRef<ConfigSnapshot | null>(null);
  const status = shallowRef<ConfigStatus | null>(null);
  const schema = shallowRef<Schema>({});
  const devices = shallowRef<AudioDevice[]>([]);
  const errors = ref<Record<string, Message | undefined>>({});
  const loading = ref(true);
  const model = ref('supertonic-3');
  const voice = ref(data.voices[0] || '');
  const language = ref('en');
  const text = ref('');
  const preview = ref(0);
  const sampling = ref<SamplingState>(loadSampling());
  watch(sampling, value => persistSampling({ ...value }), { deep: true });
  const samplingIssues = computed(() => samplingErrors(sampling.value, info.value?.synthesis?.sampling));
  const samplingParams = computed(() => samplingQuery(sampling.value, info.value?.synthesis?.sampling));
  function resetSampling() { sampling.value = { ...DEFAULT_SAMPLING }; }
  // Pronunciation (text normalization). The speech language is the form's language,
  // never the UI language, and every locale comes from /api/info.
  const normalizationInfo = computed(() => info.value?.normalization);
  const normalizationAvailable = computed(() => !!normalizationInfo.value);
  const normalizationChoice = ref<NormalizationMode | null>(null);
  const normalizationMode = computed<NormalizationMode>({
    get: () => normalizationChoice.value ?? normalizationInfo.value?.default_mode ?? 'auto',
    set: value => { normalizationChoice.value = value; },
  });
  const pronunciationLocale = ref('');
  const normalizationLanguage = computed(() => normalizationInfo.value?.languages.find(entry => entry.language === language.value));
  const pronunciationLocales = computed(() => localeChoices(normalizationLanguage.value));
  const showPronunciationLocale = computed(() => showLocaleChoice(normalizationLanguage.value, normalizationMode.value));
  /** Query fields for synthesis; undefined when the server predates normalization (legacy behavior). */
  const pronunciation = computed<SpeechPronunciation | undefined>(() => normalizationInfo.value
    ? { normalization: normalizationMode.value, locale: effectiveLocale(normalizationLanguage.value, pronunciationLocale.value, normalizationMode.value) }
    : undefined);
  watch(pronunciationLocales, options => { if (pronunciationLocale.value && !options.includes(pronunciationLocale.value)) pronunciationLocale.value = ''; });
  let session = new AbortController();
  const cache = createResourceCache();
  const api = overrides?.api ?? createApi(createHttpClient(() => token.value, () => info.value?.auth.required ?? true, () => session.signal));
  let disposed = false;
  let refreshing = false;
  let queued = false;
  let slowQueued = false;
  let tick = 0;
  // Generation guard: responses from an older token (or a disposed studio)
  // never overwrite newer state. Bumped on token change and disposal.
  let generation = 0;
  let authScope: string | null = null;
  // Last model status that triggered a built-in voices fetch. Immutable
  // voices are re-fetched only when readiness changes, not every tick.
  let lastVoicesStatus: string | null = null;
  async function load(area: string, action: () => Promise<void>) {
    const gen = generation;
    try { await action(); if (gen !== generation || disposed) return; errors.value[area] = undefined; }
    catch (error) { if (gen !== generation || disposed || isCancelledError(error)) return; errors.value[area] = errorMessage(error); }
  }
  async function refreshInfo() {
    const gen = generation;
    await load('info', async () => {
      const result = await cache.read(gen, 'info', signal => api.info(signal));
      if (gen !== generation || disposed) return;
      const nextAuth = `${result.auth.mode}:${result.auth.required}`;
      if (authScope !== null && authScope !== nextAuth) {
        authScope = nextAuth; info.value = result; invalidateSession(); void refresh(); return;
      }
      authScope = nextAuth;
      info.value = result;
      // Built-in voices are immutable for a model state: refetch only when
      // readiness changes (first load, or a ready transition), never on
      // every ready-state tick.
      if (result.model.status === 'ready' && lastVoicesStatus !== 'ready') {
        const voices = await cache.read(gen, `builtin-${result.model.status}`, signal => api.builtinVoices(signal));
        if (gen !== generation || disposed) return;
        builtinVoices.value = voices;
      }
      lastVoicesStatus = result.model.status;
    });
  }
  async function refreshProviders() {
    const gen = generation;
    await load('providers', async () => { const result = await cache.read(gen, 'providers', signal => api.providers(signal)); if (gen !== generation || disposed) return; providers.value = result.providers; capabilities.value = result.capabilities; diagnostics.value = result.diagnostics; });
  }
  async function refreshVoices(force = true) { const gen = generation; await load('voices', async () => { if (force) await cache.invalidate('voices', true); const result = await cache.read(gen, 'voices', signal => api.voices(signal)); if (gen !== generation || disposed) return; voices.value = result; }); }
  async function refreshDevices() {
    const gen = generation;
    await load('devices', async () => {
      const value = await cache.read(gen, 'devices', signal => api.audioDevices(signal), 30000);
      if (gen === generation && !disposed) devices.value = value;
    });
  }
  async function refreshConfig(force = false) {
    const gen = generation;
    await load('settings', async () => {
      const result = await cache.read(gen, 'config-status', signal => api.configStatus(signal));
      if (gen !== generation || disposed) return;
      status.value = result;
      if (force || !config.value || result.revision !== config.value.revision || !Object.keys(schema.value).length) {
        if (force || result.revision !== config.value?.revision) await cache.invalidate('config');
        const [snapshot, definitions] = await Promise.all([
          cache.read(gen, 'config', signal => api.config(signal), force ? 0 : 1000),
          cache.read(gen, 'schema', signal => api.schema(signal), Infinity),
        ]);
        if (gen !== generation || disposed) return;
        config.value = snapshot; schema.value = definitions;
      }
    });
  }
  async function refreshFast() {
    // Frequently changing provider/job status only.
    await Promise.all([refreshInfo(), refreshProviders(), refreshConfig()]);
  }
  async function refreshSlow() {
    // Low-frequency metadata: voice list plus the conditional snapshot.
    await Promise.all([refreshVoices(false), refreshDevices()]);
  }
  async function refresh(forceSlow = true) {
    if (refreshing) { queued = true; if (forceSlow) slowQueued = true; return; }
    refreshing = true;
    loading.value = true;
    const slow = forceSlow || slowQueued;
    slowQueued = false;
    try { await Promise.all(slow ? [refreshFast(), refreshSlow()] : [refreshFast()]); }
    finally { refreshing = false; loading.value = false; if (queued && !disposed) { queued = false; void refresh(slowQueued); } }
  }
  async function saveSetting(path: string, value: Json) {
    try { await api.patchConfig(nestedChange(path, value), config.value?.revision); }
    catch (error) {
      if (error instanceof ApiError && error.status === 409) {
        await refreshConfig(true);
        throw Object.assign(error, { localizedMessage: { key: 'Settings changed on the server. Your draft is preserved. Review the current value before saving again.' } });
      }
      throw error;
    }
    await refreshConfig(true);
    if (errors.value.settings) throw Object.assign(new Error(), { localizedMessage: errors.value.settings });
  }
  async function toggleProvider(provider: ProviderStatus) {
    const path = `providers.${provider.provider_id.replaceAll('-', '_')}.enabled`;
    const snapshot = await api.config();
    try { await api.patchConfig(nestedChange(path, !provider.enabled), snapshot.revision); }
    catch (error) {
      if (!(error instanceof ApiError) || error.status !== 409) throw error;
      const latest = await api.config();
      await api.patchConfig(nestedChange(path, !provider.enabled), latest.revision);
    }
    await Promise.all([refreshConfig(true), refreshProviders()]);
  }
  async function activatePocketModel(model: string) {
    const supported = capabilities.value.find(cap => cap.provider_id === 'pocket-tts')?.models || [];
    await activatePocketBundle(api, model, supported);
    await Promise.all([refreshConfig(true), refreshProviders()]);
    if (errors.value.settings) throw Object.assign(new Error(), { localizedMessage: errors.value.settings });
    if (errors.value.providers) throw Object.assign(new Error(), { localizedMessage: errors.value.providers });
  }
  const models = computed(() => [
    { id: 'supertonic-3', provider: 'supertonic', cloned: false, languages: capabilities.value.find(cap => cap.provider_id === 'supertonic')?.languages || ['en'], ready: info.value ? info.value.model.status === 'ready' : data.modelReady },
    ...capabilities.value.filter(cap => cap.requires_reference_audio).flatMap(cap => cap.models.map(id => ({ id, provider: cap.provider_id, cloned: true, languages: modelLanguages(cap, id), ready: providers.value.some(provider => provider.provider_id === cap.provider_id && provider.state === 'ready' && (cap.provider_id !== 'pocket-tts' || provider.active_model === id)) }))),
  ]);
  const selectedModel = computed(() => models.value.find(item => item.id === model.value) || models.value[0]);
  function profileCompatible(profile: Voice) {
    const provider = providers.value.find(item => item.provider_id === profile.provider);
    const cap = capabilities.value.find(item => item.provider_id === profile.provider);
    return !(provider?.model_revision && profile.model_revision && provider.model_revision !== profile.model_revision)
      && (!cap || cap.models.includes(profile.model) && cap.clone_modes.includes(profile.clone_mode) && cap.languages.includes(profile.language));
  }
  const availableVoices = computed(() => selectedModel.value.cloned ? voices.value.filter(item => item.model === model.value && profileCompatible(item)).map(item => ({ id: item.id, name: item.name, detail: `${item.provider} · ${item.language}`, cloned: true })) : builtinVoices.value.map(item => ({ ...item, detail: 'Built-in voice', cloned: false })));
  watch(availableVoices, value => { if (!value.some(item => item.id === voice.value)) voice.value = value[0]?.id || ''; });
  watch(voice, value => { const profile = voices.value.find(item => item.id === value); if (profile) language.value = profile.language; });
  watch(selectedModel, value => { if (value.languages.length && !value.languages.includes(language.value)) language.value = value.languages[0]; });
  function invalidateSession() {
    generation++; scopeEpoch.value = generation;
    session.abort(); session = new AbortController(); cache.clear();
    voices.value = []; config.value = null; status.value = null; schema.value = {};
    providers.value = []; capabilities.value = []; devices.value = []; errors.value = {};
    lastVoicesStatus = null;
  }
  watch(token, () => { invalidateSession(); void refresh(); }, { flush: 'sync' });
  function dispose() { disposed = true; generation++; session.abort(); cache.clear(); clearInterval(timer); }
  function selectProfile(profile: Voice, previewAudio = false) { model.value = profile.model; voice.value = profile.id; language.value = profile.language; if (previewAudio) preview.value++; }
  onMounted(() => {
    void refresh();
  });
  // Fast status ticks every 5s while visible; slow metadata every 6th tick.
  // In-flight refreshes are deduped, stale token generations are dropped,
  // and unmount clears the timer so no requests or rejections outlive it.
  const timer = setInterval(() => {
    if (disposed || typeof document !== 'undefined' && document.hidden) return;
    tick++;
    void refresh(slowRefreshDue(tick));
  }, STUDIO_FAST_MS);
  onUnmounted(dispose);
  return { api, token, scopeEpoch, info, providers, capabilities, diagnostics, builtinVoices, voices, config, status, schema, devices, errors, loading, model, voice, language, text, preview, sampling, samplingIssues, samplingParams, resetSampling, normalizationAvailable, normalizationMode, pronunciationLocale, normalizationLanguage, pronunciationLocales, showPronunciationLocale, pronunciation, models, selectedModel, availableVoices, refresh, refreshConfig, refreshProviders, refreshVoices, saveSetting, toggleProvider, activatePocketModel, profileCompatible, selectProfile, dispose };
}
export type Studio = ReturnType<typeof createStudio>;
export const studioKey: InjectionKey<Studio> = Symbol('SonicBoom studio');
export function useStudio() { const studio = inject(studioKey); if (!studio) throw new Error('Studio context is missing'); return studio; }
