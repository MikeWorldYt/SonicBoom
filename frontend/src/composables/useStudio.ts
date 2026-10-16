import { computed, inject, onMounted, onUnmounted, ref, shallowRef, watch, type InjectionKey } from 'vue';
import { createHttpClient, ApiError } from '../services/http';
import { createApi } from '../services/api';
import { modelLanguages } from '../services/providerLanguages';
import { activatePocketModel as activatePocketBundle } from '../services/pocketActivation';
import type { HomeData } from '../types';
import type { ServerInfo, ProviderStatus, Capabilities, Voice, BuiltinVoice, ConfigSnapshot, ConfigStatus, Schema, Json, JsonObject, AudioDevice } from '../services/contracts';
import { errorMessage, type Message } from '../i18n';
export function getPath(object: JsonObject | undefined, path: string): Json | undefined {
  return path.split('.').reduce<Json | undefined>((value, key) => value && typeof value === 'object' && !Array.isArray(value) ? value[key] : undefined, object);
}
export function nestedChange(path: string, value: Json): JsonObject {
  return path.split('.').reverse().reduce<Json>((result, key) => ({ [key]: result }), value) as JsonObject;
}
export function createStudio(data: HomeData) {
  const token = ref('');
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
  const api = createApi(createHttpClient(() => token.value, () => info.value?.auth.required ?? true));
  let disposed = false;
  let refreshing = false;
  let queued = false;
  async function load(area: string, action: () => Promise<void>) {
    try { await action(); errors.value[area] = undefined; }
    catch (error) { if (!disposed) errors.value[area] = errorMessage(error); }
  }
  async function refreshInfo() {
    await load('info', async () => {
      const result = await api.info();
      if (disposed) return;
      info.value = result;
      if (result.model.status === 'ready') builtinVoices.value = await api.builtinVoices();
    });
  }
  async function refreshProviders() {
    await load('providers', async () => { const result = await api.providers(); if (disposed) return; providers.value = result.providers; capabilities.value = result.capabilities; diagnostics.value = result.diagnostics; });
  }
  async function refreshVoices() { await load('voices', async () => { const result = await api.voices(); if (!disposed) voices.value = result; }); }
  async function refreshConfig(force = false) {
    await load('settings', async () => {
      const result = await api.configStatus();
      if (disposed) return;
      status.value = result;
      if (force || !config.value || result.revision !== config.value.revision || !Object.keys(schema.value).length) {
        const [snapshot, definitions] = await Promise.all([api.config(), api.schema()]);
        if (!disposed) { config.value = snapshot; schema.value = definitions; }
      }
    });
  }
  async function refresh() {
    if (refreshing) { queued = true; return; }
    refreshing = true;
    loading.value = true;
    try { await Promise.all([refreshInfo(), refreshProviders(), refreshVoices(), refreshConfig()]); }
    finally { refreshing = false; loading.value = false; if (queued && !disposed) { queued = false; void refresh(); } }
  }
  async function saveSetting(path: string, value: Json) {
    await api.patchConfig(nestedChange(path, value), config.value?.revision);
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
    { id: 'supertonic-3', provider: 'supertonic', cloned: false, languages: [], ready: info.value ? info.value.model.status === 'ready' : data.modelReady },
    ...capabilities.value.filter(cap => cap.requires_reference_audio).flatMap(cap => cap.models.map(id => ({ id, provider: cap.provider_id, cloned: true, languages: modelLanguages(cap, id), ready: providers.value.some(provider => provider.provider_id === cap.provider_id && provider.state === 'ready' && (cap.provider_id !== 'pocket-tts' || provider.active_model === id)) }))),
  ]);
  const selectedModel = computed(() => models.value.find(item => item.id === model.value) || models.value[0]);
  const availableVoices = computed(() => selectedModel.value.cloned ? voices.value.filter(item => item.model === model.value).map(item => ({ id: item.id, name: item.name, detail: `${item.provider} · ${item.language}`, cloned: true })) : builtinVoices.value.map(item => ({ ...item, detail: 'Built-in voice', cloned: false })));
  watch(availableVoices, value => { if (!value.some(item => item.id === voice.value)) voice.value = value[0]?.id || ''; });
  watch(voice, value => { const profile = voices.value.find(item => item.id === value); if (profile) language.value = profile.language; });
  watch(selectedModel, value => { if (value.languages.length && !value.languages.includes(language.value)) language.value = value.languages[0]; });
  watch(token, () => void refresh());
  function selectProfile(profile: Voice, previewAudio = false) { model.value = profile.model; voice.value = profile.id; language.value = profile.language; if (previewAudio) preview.value++; }
  onMounted(() => {
    void refresh();
    void api.audioDevices().then(value => { if (!disposed) devices.value = value; }).catch(() => {});
  });
  const timer = setInterval(() => { if (!disposed && !document.hidden) void refresh(); }, 5000);
  onUnmounted(() => { disposed = true; clearInterval(timer); });
  return { api, token, info, providers, capabilities, diagnostics, builtinVoices, voices, config, status, schema, devices, errors, loading, model, voice, language, text, preview, models, selectedModel, availableVoices, refresh, refreshConfig, refreshProviders, refreshVoices, saveSetting, toggleProvider, activatePocketModel, selectProfile };
}
export type Studio = ReturnType<typeof createStudio>;
export const studioKey: InjectionKey<Studio> = Symbol('SonicBoom studio');
export function useStudio() { const studio = inject(studioKey); if (!studio) throw new Error('Studio context is missing'); return studio; }
