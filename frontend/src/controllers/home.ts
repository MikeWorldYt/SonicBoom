import type { ProviderCardData } from '../types';
import { element } from '../dom';
export function startHome(modelReady: boolean, providers: ProviderCardData[]) {
// SonicBoom home page (CSP: no inline script, so everything lives here).
//
// Layout of this file:
//   1. Constants — the single source of truth for endpoints, setting types,
//      apply strategies, categories, and languages (all frozen).
//   2. Component builders — tiny DOM factories shared by every section.
//   3. TTS synthesis.
//   4. Configuration browser.
//
'use strict';

// --- 1. Constants -----------------------------------------------------------

// Backend state is exposed via data attributes (CSP has no unsafe-inline).
const MODEL_READY = modelReady;

const App = Object.freeze({
  modelReloadMs: 5000,
  configPollMs: 8000,
});

const Endpoints = Object.freeze({
  info: '/api/info',
  tts: '/api/tts',
  config: '/api/config',
  configSchema: '/api/config/schema',
  configStatus: '/api/config/status',
  audioDevices: '/api/audio/devices',
  configAction: action => `/api/config/${action}`,
  configOpen: '/api/config/open',
  providers: '/api/providers',
  voices: '/api/voices',
  voicePreview: id => `/api/voices/${encodeURIComponent(id)}/preview`,
});

const SettingType = Object.freeze({
  String: 'string',
  Integer: 'integer',
  Number: 'number',
  Boolean: 'boolean',
  Array: 'array',
  Object: 'object',
});

const ApplyStrategy = Object.freeze({
  Hot: 'hot',
  RestartServer: 'restart_server',
  RestartSubsystem: 'restart_subsystem',
  ReloadModel: 'reload_model',
  RestartProcess: 'restart_process',
});

const APPLY_META = Object.freeze({
  [ApplyStrategy.Hot]: { label: 'Live' },
  [ApplyStrategy.RestartServer]: { label: 'Restarts server' },
  [ApplyStrategy.RestartSubsystem]: { label: 'Restarts worker' },
  [ApplyStrategy.ReloadModel]: { label: 'Reloads model' },
  [ApplyStrategy.RestartProcess]: { label: 'Needs restart' },
});

const CATEGORIES = Object.freeze({
  server: 'Server',
  admin: 'Admin',
  audio: 'Audio playback',
  model: 'Model',
  tts: 'TTS limits',
  inference: 'Inference',
  rate_limit: 'Rate limiting',
  logging: 'Logging',
  paths: 'Paths',
  security: 'Security',
  providers: 'Voice providers',
  voices: 'Voices',
});

// Canonical language list for the synthesis form. The HTML carries an
// empty <select>; options are rendered from here so the list exists once.
const LANGUAGES = Object.freeze([
  { code: 'en', label: 'English' },
  { code: 'ko', label: 'Korean' },
  { code: 'ja', label: 'Japanese' },
  { code: 'zh', label: 'Chinese' },
  { code: 'es', label: 'Spanish' },
  { code: 'pt', label: 'Portuguese' },
  { code: 'fr', label: 'French' },
  { code: 'de', label: 'German' },
  { code: 'it', label: 'Italian' },
  { code: 'ru', label: 'Russian' },
  { code: 'ar', label: 'Arabic' },
  { code: 'hi', label: 'Hindi' },
  { code: 'nl', label: 'Dutch' },
  { code: 'pl', label: 'Polish' },
  { code: 'tr', label: 'Turkish' },
  { code: 'vi', label: 'Vietnamese' },
  { code: 'id', label: 'Indonesian' },
  { code: 'th', label: 'Thai' },
  { code: 'sv', label: 'Swedish' },
  { code: 'da', label: 'Danish' },
  { code: 'fi', label: 'Finnish' },
  { code: 'el', label: 'Greek' },
  { code: 'cs', label: 'Czech' },
  { code: 'sk', label: 'Slovak' },
  { code: 'sl', label: 'Slovenian' },
  { code: 'hr', label: 'Croatian' },
  { code: 'ro', label: 'Romanian' },
  { code: 'hu', label: 'Hungarian' },
  { code: 'bg', label: 'Bulgarian' },
  { code: 'uk', label: 'Ukrainian' },
  { code: 'lt', label: 'Lithuanian' },
  { code: 'lv', label: 'Latvian' },
  { code: 'et', label: 'Estonian' },
]);

const VOLUME_SETTING = 'audio.volume';

// --- 2. Component builders --------------------------------------------------

/** Create an element with an optional class, text, and attributes. */
function el(tag, { cls = '', text = '', attrs = {} } = {}) {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text) node.textContent = text;
  for (const [key, value] of Object.entries(attrs)) {
    node.setAttribute(key, value);
  }
  return node;
}

/** One <option> for a <select>. */
function optionEl(value: string, label = value) {
  return el('option', { text: label ?? value, attrs: { value } });
}

/** Fill a <select> with values, preserving the current value first. */
function populateSelect(select, values, current) {
  select.innerHTML = '';
  const seen = new Set();
  for (const value of [current, ...values]) {
    if (!value || seen.has(value)) continue;
    seen.add(value);
    select.append(optionEl(value));
  }
  select.value = current;
}

/** Apply badge for a schema `apply` strategy (falls back to the raw key). */
function applyBadge(apply) {
  const meta = APPLY_META[apply];
  const badge = el('span', {
    cls: `apply apply-${apply}`,
    text: meta ? meta.label : apply,
  });
  badge.title = `Changing this ${apply.replace(/_/g, ' ')}`;
  return badge;
}

/** Small primary action button. */
function actionButton(label) {
  const button = el('button', { cls: 'btn-small', text: label });
  button.type = 'button';
  return button;
}

/** Message line inside a setting row (`ok` / `error` / idle). */
function rowMessage() {
  return el('p', { cls: 'row-msg' });
}

function setRowMessage(msg, text, kind) {
  msg.textContent = text;
  msg.className = kind ? `row-msg ${kind}` : 'row-msg';
}

/** Boolean toggle. Returns { node, getValue, isClean }. */
function switchControl({ label, checked, onEdit }) {
  const wrapper = el('label', { cls: 'switch' });
  const input = el('input', { attrs: { type: 'checkbox', 'aria-label': label } });
  input.checked = checked;
  const state = el('span', { cls: 'switch-state', text: checked ? 'On' : 'Off' });
  input.addEventListener('change', () => {
    state.textContent = input.checked ? 'On' : 'Off';
    onEdit();
  });
  wrapper.append(input, state);
  return {
    node: wrapper,
    getValue: () => input.checked,
    isClean: () => input.checked === checked,
  };
}

/** Dropdown from an explicit value list. Returns { node, getValue, isClean }. */
function enumControl({ label, values, current, onEdit }) {
  const select = el('select', { attrs: { 'aria-label': label } });
  populateSelect(select, values, current);
  select.addEventListener('change', onEdit);
  return {
    node: select,
    getValue: () => select.value,
    isClean: () => select.value === current,
  };
}

/** Audio-device dropdown; device names load lazily and upgrade the list. */
function audioDeviceControl({ label, current, onEdit }) {
  const select = el('select', { attrs: { 'aria-label': label } });
  populateSelect(select, ['default'], current);
  select.addEventListener('change', onEdit);
  loadAudioDevices().then(devices => {
    const previous = select.value;
    select.innerHTML = '';
    const seen = new Set();
    for (const value of [previous, 'default', ...devices.map(d => d.id)]) {
      if (!value || seen.has(value)) continue;
      seen.add(value);
      const found = devices.find(d => d.id === value);
      const text = found && found.name && found.name !== value
        ? `${found.name} (${value})`
        : value;
      select.append(optionEl(value, text));
    }
    select.value = previous;
  });
  return {
    node: select,
    getValue: () => select.value,
    isClean: () => select.value === current,
  };
}

/** Volume slider with a live numeric readout. */
function volumeControl({ label, current, onEdit }) {
  const wrap = el('div', { cls: 'range-wrap' });
  const input = el('input', {
    attrs: { type: 'range', min: '0', max: '1', step: '0.01', 'aria-label': label },
  });
  input.value = current ?? 1;
  const readout = el('span', { cls: 'range-val', text: Number(input.value).toFixed(2) });
  input.addEventListener('input', () => {
    readout.textContent = Number(input.value).toFixed(2);
    onEdit();
  });
  wrap.append(input, readout);
  return {
    node: wrap,
    getValue: () => Number(input.value),
    isClean: () => Number(input.value) === Number(current),
  };
}

/** Plain text / number input. Returns { node, getValue, isClean }. */
function fieldControl({ label, meta, current, onEdit }) {
  const input = el('input', { attrs: { 'aria-label': label } });
  if (meta.type === SettingType.Integer || meta.type === SettingType.Number) {
    input.type = 'number';
    input.step = meta.type === SettingType.Integer ? '1' : 'any';
    if (meta.min !== undefined && meta.min !== null) input.min = String(meta.min);
    if (meta.max !== undefined && meta.max !== null) input.max = String(meta.max);
  } else {
    input.type = 'text';
    if (meta.type === SettingType.Array) input.placeholder = '["entry"]';
  }
  input.value = formatForInput(current);
  input.addEventListener('input', onEdit);
  return {
    node: input,
    getValue: () => input.value,
    isClean: () => input.value === formatForInput(current),
  };
}

/** Pick the right control for a schema entry. */
function settingControl(path, meta, title, current, onEdit) {
  const base = { label: title, current, onEdit };
  if (meta.type === SettingType.Boolean) {
    return switchControl({ ...base, checked: current === true });
  }
  if (meta.allowed_values && meta.allowed_values.length > 0) {
    return enumControl({ ...base, values: meta.allowed_values, current: formatForInput(current) });
  }
  if (meta.allowed_values_source === 'audio_devices') {
    return audioDeviceControl({ ...base, current: formatForInput(current) });
  }
  if (path === VOLUME_SETTING) {
    return volumeControl(base);
  }
  return fieldControl({ ...base, meta });
}

// --- DOM cache --------------------------------------------------------------

const tokenGroup = element('tokenGroup');
const apiTokenInput = element('apiToken');
const textInput = element('text');
const charCount = element('charCount');
const voiceSelect = element('voice');
const langSelect = element('lang');
const generateBtn = element('btn');
const statusDiv = element('status');
const audioPlayer = element('player');

const configGroupsEl = element('configGroups');
const configSearch = element('configSearch');
const configCategory = element('configCategory');
const configNotice = element('configNotice');
const configError = element('configError');

// Server info discovered from GET /api/info. When the
// server runs in local (loopback) mode no API token is
// required, so the token field is hidden (spec §32).
let serverInfo = null;

// Store active audio object URL to prevent memory leaks
let currentAudioUrl = null;

let configSnapshot = null;
let configSchema: Record<string, any> = {};
let audioDevicesCache = null;

// --- Shared helpers ---------------------------------------------------------

// Centralized API client: attaches the Bearer token only
// when the server actually requires one (spec §32).
async function apiFetch(path, options: RequestInit = {}) {
  const headers = new Headers(options.headers || {});
  const apiToken = apiTokenInput.value.trim();
  if (serverInfo && serverInfo.auth && serverInfo.auth.required && apiToken) {
    headers.set('Authorization', `Bearer ${apiToken}`);
  }
  return fetch(path, { ...options, headers });
}

async function configRequest(path: string, options: RequestInit = {}) {
  const response = await apiFetch(path, options);
  const body = await response.json();
  if (!response.ok) throw new Error(body.message || body.error || `HTTP ${response.status}`);
  return body;
}

function setNotice(kind, text) {
  configNotice.textContent = text;
  configNotice.className = kind || '';
}

function showProblems(problems) {
  if (problems.length > 0) {
    configError.textContent = problems.join('\n');
    configError.hidden = false;
  } else {
    configError.textContent = '';
    configError.hidden = true;
  }
}

// --- Providers (live status cards) ------------------------------------------

const PROVIDER_BADGES = Object.freeze({
  ready: 'badge-ready',
  loading: 'badge-loading',
  disabled: 'badge-disabled',
});

function providerBadgeClass(state) {
  return PROVIDER_BADGES[state] || 'badge-not_installed';
}

/**
 * One voice-engine status card, shared by initial page data and live updates.
 * Text is assigned (never parsed as HTML), so provider metadata is safe.
 */
function providerCard(status, models) {
  const card = el('div', { cls: 'provider' });
  const top = el('div', { cls: 'provider-top' });
  top.append(
    el('span', { cls: 'provider-name', text: status.provider_id }),
    el('span', {
      cls: `badge ${providerBadgeClass(status.state)}`,
      text: String(status.state).replace(/_/g, ' '),
    }),
    el('span', { cls: 'device-chip', text: status.effective_device || '—' }),
  );
  card.append(top);
  if (models) {
    const line = el('p', { cls: 'provider-models' });
    line.append(el('span', { text: 'Models: ' }), document.createTextNode(models));
    card.append(line);
  }
  if (status.active_model) {
    const line = el('p', { cls: 'provider-active' });
    line.append(document.createTextNode('Active model: '), el('code', { text: status.active_model }));
    card.append(line);
  }
  if (status.detail) {
    card.append(el('p', { cls: 'provider-detail', text: status.detail }));
  }
  const togglePath = providerTogglePath(status.provider_id);
  if (togglePath) {
    const actions = el('div', { cls: 'provider-actions' });
    const toggle = el('button', {
      cls: `${status.enabled ? 'btn-secondary' : 'btn-small'} provider-toggle`,
      text: status.enabled ? 'Disable' : 'Enable',
    });
    toggle.type = 'button';
    toggle.dataset.path = togglePath;
    toggle.dataset.enabled = status.enabled ? 'true' : 'false';
    actions.append(toggle);
    card.append(actions);
  }
  return card;
}

/**
 * Config path behind a provider's enable toggle, derived from the live
 * schema (`providers.<id>.enabled`). Unknown ids and the built-in engine
 * have none, so their cards render without a toggle.
 */
function providerTogglePath(providerId) {
  const path = `providers.${providerId.replace(/-/g, '_')}.enabled`;
  return Object.hasOwn(configSchema, path) ? path : null;
}

async function patchConfig(changes) {
  return configRequest(Endpoints.config, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ expected_revision: configSnapshot.revision, changes }),
  });
}

/** Flip a provider's enabled flag via the config API (persisted, hot-apply,
 *  no file edits or restarts). Enabling kicks off the model download;
 *  progress shows on the card through the regular status polling. */
async function setProviderEnabled(path, enabled, button) {
  const original = button.textContent;
  button.disabled = true;
  button.textContent = enabled ? 'Starting…' : 'Stopping…';
  try {
    configSnapshot = await configRequest(Endpoints.config);
    const changes = {};
    setNestedValue(changes, path, enabled);
    try {
      await patchConfig(changes);
    } catch (error) {
      if (!/conflict/i.test(error.message)) throw error;
      configSnapshot = await configRequest(Endpoints.config);
      await patchConfig(changes);
    }
    providerSignature = null;
    await refreshConfiguration();
    await refreshProviders();
    setNotice('ok', `${path} ${enabled ? 'enabled — the engine will download and start.' : 'disabled.'}`);
  } catch (error) {
    setNotice('error', error.message);
  } finally {
    button.disabled = false;
    button.textContent = original;
  }
}

let providerSignature = null;

/** Re-render engine cards when provider status changes; keeps the
 *  last successful status cards visible on errors or when idle. */
async function refreshProviders() {
  const list = document.querySelector('.provider-list');
  if (!list) return;
  try {
    const body = await configRequest(Endpoints.providers);
    const diagnostics = element('inferenceDiagnostics');
    if (diagnostics) diagnostics.textContent = JSON.stringify(body.diagnostics || {}, null, 2);
    const signature = JSON.stringify(body.providers || []);
    if (signature === providerSignature) return;
    providerSignature = signature;
    const modelsById = new Map(
      (body.capabilities || [])
        .filter(Boolean)
        .map(c => [c.provider_id, (c.models || []).join(', ')]),
    );
    list.innerHTML = '';
    for (const status of body.providers || []) {
      list.append(providerCard(status, modelsById.get(status.provider_id) || ''));
    }
    lastProviders = body;
    await renderModelOptions();
    refreshCloneForm();
  } catch {
    // Network or auth failure: retain the last successful status cards.
  }
}

// --- Model, voice, and language selection -----------------------------------
// Supertonic aliases (tts-1, tts-1-hd) synthesize byte-identically on one
// engine, so the form offers a single Supertonic entry; every *ready*
// cloning engine adds its own models, voiced by your uploaded profiles.

const SUPERTONIC_PROVIDER = 'supertonic';
const SUPERTONIC_MODEL = 'supertonic-3';

const modelSelect = element('model');
const modelHint = element('modelHint');

let builtinVoiceOptions = [];
let voiceProfiles = [];
let lastProviders = null;

function snapshotBuiltinVoices() {
  builtinVoiceOptions = [...voiceSelect.options].map(o => ({ value: o.value, text: o.textContent }));
}

/** Models of providers reporting `ready`, excluding the Supertonic engine. */
function readyCloningModels() {
  if (!lastProviders) return [];
  const caps = new Map<string, any>(
    (lastProviders.capabilities || []).filter(Boolean).map(c => [c.provider_id, c]),
  );
  const out = [];
  for (const status of lastProviders.providers || []) {
    if (status.state !== 'ready' || status.provider_id === SUPERTONIC_PROVIDER) continue;
    const capability = caps.get(status.provider_id);
    for (const model of capability?.models || []) {
      out.push({ model, provider: status.provider_id, languages: capability?.languages || [] });
    }
  }
  return out;
}

function selectedModelMeta() {
  const model = modelSelect.value || SUPERTONIC_MODEL;
  if (model === SUPERTONIC_MODEL) return { kind: 'supertonic', model };
  const found = readyCloningModels().find(m => m.model === model);
  if (!found) return { kind: 'supertonic', model: SUPERTONIC_MODEL };
  return { kind: 'cloning', ...found };
}

async function renderModelOptions() {
  const previous = modelSelect.value || SUPERTONIC_MODEL;
  const options = [optionEl(SUPERTONIC_MODEL, `${SUPERTONIC_MODEL} (built-in voices)`)];
  for (const { model, provider } of readyCloningModels()) {
    options.push(optionEl(model, `${model} (${provider})`));
  }
  modelSelect.innerHTML = '';
  for (const option of options) modelSelect.append(option);
  modelSelect.value = options.some(o => o.value === previous) ? previous : SUPERTONIC_MODEL;
  if (modelSelect.value !== previous) await syncVoicesToModel();
}

/** Rebuild the voice and language dropdowns for the selected model:
 *  built-in styles for Supertonic, your uploaded profiles (plus the
 *  provider's languages) for cloning engines. */
async function syncVoicesToModel() {
  const meta = selectedModelMeta();
  if (meta.kind === 'supertonic') {
    voiceSelect.innerHTML = '';
    for (const { value, text } of builtinVoiceOptions) voiceSelect.append(optionEl(value, text));
    populateLanguages(null, 'en');
    modelHint.textContent = '';
    return;
  }
  modelHint.textContent = `Cloned voice: synthesizes with your uploaded reference for ${meta.model}.`;
  const needToken = serverInfo?.auth?.required && !apiTokenInput.value.trim();
  let profiles = [];
  if (!needToken) {
    try {
      const body = await configRequest(Endpoints.voices);
      profiles = body.data || [];
    } catch {
      profiles = [];
    }
  }
  voiceProfiles = profiles;
  const matches = profiles.filter(p => p.model === meta.model);
  voiceSelect.innerHTML = '';
  if (matches.length === 0) {
    const empty = optionEl('', needToken ? 'Enter your API token to load voices' : 'No voice profiles for this model yet');
    empty.disabled = true;
    voiceSelect.append(empty);
    setStatus(
      needToken
        ? 'Enter your API token to load voice profiles.'
        : 'No voice profiles for this model yet — create one with POST /api/voices/clones.',
      true,
    );
  } else {
    const seen = new Set();
    for (const profile of matches) {
      let text = profile.name;
      if (seen.has(profile.name)) text = `${profile.name} (${String(profile.id).slice(0, 8)})`;
      seen.add(profile.name);
      voiceSelect.append(optionEl(profile.id, text));
    }
    setStatus('');
  }
  populateLanguages(meta.languages.length > 0 ? meta.languages : null, matches[0]?.language);
}

// --- Voice cloning ----------------------------------------------------------
// Create and manage cloning voice profiles in-page: the same endpoints the
// API exposes (GET/POST/DELETE /api/voices), driven by live capabilities
// so models, languages, and modes are never hardcoded per provider.

const voicesList = element('voicesList');
const cloneForm = element('cloneForm');
const cloneName = element('cloneName');
const cloneProvider = element('cloneProvider');
const cloneModel = element('cloneModel');
const cloneLang = element('cloneLang');
const cloneMode = element('cloneMode');
const cloneTranscriptGroup = element('cloneTranscriptGroup');
const cloneTranscript = element('cloneTranscript');
const cloneConsent = element('cloneConsent');
const cloneFile = element('cloneFile');
const cloneSubmit = element('cloneSubmit');
const cloneStatus = element('cloneStatus');

function setCloneStatus(text, kind) {
  cloneStatus.textContent = text;
  cloneStatus.className = kind || '';
}

async function apiErrorMessage(resp) {
  try {
    const body = await resp.json();
    if (body && body.message) return body.message;
  } catch {
    // Fall through to the status line below.
  }
  return `HTTP ${resp.status}`;
}

/** Cloning-capable engines from live capabilities (never hardcoded). */
function cloningCapabilities() {
  if (!lastProviders) return [];
  return (lastProviders.capabilities || []).filter(c => c && c.requires_reference_audio);
}

function cloneCapability() {
  return cloningCapabilities().find(c => c.provider_id === cloneProvider.value) || null;
}

function refreshCloneForm() {
  const ids = cloningCapabilities().map(c => c.provider_id);
  const previous = cloneProvider.value;
  cloneProvider.innerHTML = '';
  if (ids.length === 0) cloneProvider.append(optionEl('', 'No cloning engines available'));
  for (const id of ids) cloneProvider.append(optionEl(id));
  if (ids.includes(previous)) cloneProvider.value = previous;
  refreshCloneDependents();
}

function refreshCloneDependents() {
  const cap = cloneCapability();
  const prevModel = cloneModel.value;
  cloneModel.innerHTML = '';
  for (const model of cap?.models || []) cloneModel.append(optionEl(model));
  if ([...cloneModel.options].some(o => o.value === prevModel)) cloneModel.value = prevModel;
  const prevLang = cloneLang.value;
  cloneLang.innerHTML = '';
  for (const code of cap?.languages || []) {
    const known = LANGUAGES.find(l => l.code === code);
    cloneLang.append(optionEl(code, known ? known.label : code));
  }
  if ([...cloneLang.options].some(o => o.value === prevLang)) cloneLang.value = prevLang;
  const prevMode = cloneMode.value;
  cloneMode.innerHTML = '';
  for (const mode of cap?.clone_modes?.length > 0 ? cap.clone_modes : ['icl']) {
    cloneMode.append(optionEl(mode));
  }
  if ([...cloneMode.options].some(o => o.value === prevMode)) cloneMode.value = prevMode;
  updateTranscriptVisibility();
}

function updateTranscriptVisibility() {
  const icl = cloneMode.value === 'icl';
  cloneTranscriptGroup.hidden = !icl;
  if (!icl) cloneTranscript.value = '';
}

async function refreshVoiceList() {
  voicesList.innerHTML = '';
  let profiles = [];
  try {
    const body = await configRequest(Endpoints.voices);
    profiles = body.data || [];
  } catch {
    voicesList.append(el('p', { cls: 'muted', text: 'Enter your API token to see your voices.' }));
    return;
  }
  if (profiles.length === 0) {
    voicesList.append(el('p', { cls: 'muted', text: 'No cloned voices yet — upload a reference below.' }));
    return;
  }
  for (const profile of profiles) voicesList.append(voiceRow(profile));
}

function voiceRow(profile) {
  const row = el('div', { cls: 'voice-row' });
  row.dataset.id = profile.id;
  const main = el('div');
  main.append(
    el('div', { cls: 'voice-name', text: profile.name }),
    el('div', {
      cls: 'voice-meta',
      text: `${profile.provider} · ${profile.model} · ${profile.language} · ${profile.clone_mode} · ${(profile.reference_duration_secs || 0).toFixed(1)}s reference · ${profile.reference_sample_rate || '?'} Hz`,
    }),
  );
  const actions = el('div', { cls: 'voice-actions' });
  const use = el('button', { cls: 'btn-inline', text: 'Use', attrs: { type: 'button', 'data-action': 'use' } });
  const preview = el('button', { cls: 'btn-inline', text: 'Preview', attrs: { type: 'button', 'data-action': 'preview' } });
  const del = el('button', { cls: 'btn-inline', text: 'Delete', attrs: { type: 'button', 'data-action': 'delete' } });
  actions.append(use, preview, del);
  row.append(main, actions);
  return row;
}

/** A WAV file starts with RIFF....WAVE; catches renamed MP3s before upload. */
async function isWavFile(file) {
  try {
    const head = new Uint8Array(await file.slice(0, 12).arrayBuffer());
    const tag = o => String.fromCharCode(head[o], head[o + 1], head[o + 2], head[o + 3]);
    return tag(0) === 'RIFF' && tag(8) === 'WAVE';
  } catch {
    return false;
  }
}

voicesList.addEventListener('click', async event => {
  const button = (event.target as Element).closest<HTMLButtonElement>('button[data-action]');
  if (!button || button.disabled) return;
  const id = button.closest<HTMLButtonElement>('.voice-row')?.dataset.id;
  if (!id) return;
  if (button.dataset.action === 'delete') {
    if (!confirm('Delete this cloned voice? Its reference audio is removed too.')) return;
    button.disabled = true;
    try {
      const resp = await apiFetch(`${Endpoints.voices}/${encodeURIComponent(id)}`, { method: 'DELETE' });
      if (!resp.ok) throw new Error(await apiErrorMessage(resp));
      await refreshVoiceList();
      await syncVoicesToModel();
    } catch (error) {
      setCloneStatus(error.message, 'error');
    } finally {
      button.disabled = false;
    }
    return;
  }
  try {
    const body = await configRequest(Endpoints.voices);
    const profile = (body.data || []).find(p => String(p.id) === id);
    if (!profile) throw new Error('Voice profile is no longer available.');
    if (![...modelSelect.options].some(o => o.value === profile.model)) {
      throw new Error(`Model ${profile.model} is not ready — enable its engine first.`);
    }
    modelSelect.value = profile.model;
    await syncVoicesToModel();
    voiceSelect.value = profile.id;
    voiceSelect.dispatchEvent(new Event('change'));
    element('tts-heading').scrollIntoView({ behavior: 'smooth', block: 'start' });
    if (button.dataset.action === 'preview') {
      await synthesize({ previewText: 'Hi there. This is a voice preview.' });
      return;
    }
    setStatus(`Using cloned voice “${profile.name}”.`, false);
  } catch (error) {
    setCloneStatus(error.message, 'error');
  }
});

cloneProvider.addEventListener('change', refreshCloneDependents);
cloneMode.addEventListener('change', updateTranscriptVisibility);

cloneForm.addEventListener('submit', async event => {
  event.preventDefault();
  setCloneStatus('', '');
  const name = cloneName.value.trim();
  const provider = cloneProvider.value;
  const model = cloneModel.value;
  const language = cloneLang.value;
  const mode = cloneMode.value || 'icl';
  const transcript = cloneTranscript.value.trim();
  const consent = cloneConsent.value.trim();
  const file = cloneFile.files[0];
  try {
    if (!provider) throw new Error('No cloning engine is available right now.');
    if (name.length < 1 || name.length > 64) throw new Error('Name must be 1–64 characters.');
    if (!model) throw new Error('Pick a model.');
    if (!language) throw new Error('Pick a language.');
    if (mode === 'icl' && !transcript) throw new Error('ICL mode needs the exact reference transcript.');
    if (!consent) throw new Error('Consent basis is required (e.g. owner).');
    if (!file) throw new Error('Choose a WAV reference recording.');
    if (file.size === 0) throw new Error('The selected file is empty.');
    if (!await isWavFile(file)) throw new Error('The selected file is not a WAV recording.');
    const params = new URLSearchParams({
      name, provider, model, language, mode, consent_basis: consent,
    });
    if (mode === 'icl') params.set('transcript', transcript);
    cloneSubmit.disabled = true;
    cloneSubmit.textContent = 'Cloning…';
    const resp = await apiFetch(`${Endpoints.voices}/clones?${params.toString()}`, {
      method: 'POST',
      headers: { 'Content-Type': 'audio/wav' },
      body: file,
    });
    if (!resp.ok) throw new Error(await apiErrorMessage(resp));
    const profile = await resp.json();
    cloneForm.reset();
    refreshCloneDependents();
    await refreshVoiceList();
    setCloneStatus(`Reference profile “${profile.name}” saved. Generate a short preview to validate voice conditioning.`, 'ok');
    if ([...modelSelect.options].some(o => o.value === profile.model)) {
      modelSelect.value = profile.model;
      await syncVoicesToModel();
      voiceSelect.value = profile.id;
      voiceSelect.dispatchEvent(new Event('change'));
    }
  } catch (error) {
    setCloneStatus(error.message, 'error');
  } finally {
    cloneSubmit.disabled = false;
    cloneSubmit.textContent = 'Clone voice';
  }
});

// --- 3. TTS synthesis -------------------------------------------------------

function populateLanguages(allowed = null, preferred = 'en') {
  const list = allowed && allowed.length > 0
    ? LANGUAGES.filter(l => allowed.includes(l.code))
    : [...LANGUAGES];
  langSelect.innerHTML = '';
  for (const { code, label } of list) {
    langSelect.append(optionEl(code, label));
  }
  langSelect.value = list.some(l => l.code === preferred) ? preferred : (list[0]?.code ?? 'en');
}

async function loadServerInfo() {
  try {
    const resp = await apiFetch(Endpoints.info);
    if (!resp.ok) {
      throw new Error(`HTTP ${resp.status}`);
    }
    serverInfo = await resp.json();
  } catch (e) {
    console.warn('Could not load server info:', e);
    serverInfo = null;
  }
  if (serverInfo && serverInfo.auth && !serverInfo.auth.required) {
    tokenGroup.hidden = true;
  } else {
    tokenGroup.hidden = false;
  }
}

// Initialize Event Listeners
const providerList = document.querySelector('.provider-list');
for (const provider of providers) providerList?.append(providerCard(provider.status, provider.models));
snapshotBuiltinVoices();
populateLanguages();
loadServerInfo().then(() => {
  refreshConfiguration();
  refreshProviders();
  refreshVoiceList();
});
modelSelect.addEventListener('change', () => void syncVoicesToModel());
voiceSelect.addEventListener('change', () => {
  const meta = selectedModelMeta();
  if (meta.kind !== 'cloning') return;
  const profile = voiceProfiles.find(p => String(p.id) === voiceSelect.value);
  if (profile?.language) {
    populateLanguages(meta.languages.length > 0 ? meta.languages : null, profile.language);
  }
});
element('modelRefresh').addEventListener('click', async event => {
  const button = event.currentTarget as HTMLButtonElement;
  button.disabled = true;
  try {
    providerSignature = null;
    await refreshProviders();
    await syncVoicesToModel();
    await refreshVoiceList();
    if (!statusDiv.classList.contains('error')) {
      setStatus('Engines, voices, and languages refreshed.', false);
    }
  } finally {
    button.disabled = false;
  }
});
// One delegated listener covers the server-rendered cards and every
// live re-render: toggles are identified by their data attributes.
document.querySelector('.provider-list').addEventListener('click', event => {
  const button = (event.target as Element).closest<HTMLButtonElement>('.provider-toggle');
  if (!button || button.disabled) return;
  void setProviderEnabled(button.dataset.path, button.dataset.enabled !== 'true', button);
});
generateBtn.addEventListener('click', () => synthesize());
textInput.addEventListener('input', updateCharCount);
updateCharCount();

function updateCharCount() {
  const n = textInput.value.length;
  charCount.textContent = `${n} character${n === 1 ? '' : 's'}`;
}

let activeJobId = null;
const cancelGeneration = element('cancelGeneration');
cancelGeneration.addEventListener('click', async () => {
  if (!activeJobId) return;
  cancelGeneration.disabled = true;
  try {
    const response = await apiFetch(`/api/tts/jobs/${encodeURIComponent(activeJobId)}/cancel`, { method: 'POST' });
    if (!response.ok) throw new Error(`Cancellation failed (HTTP ${response.status})`);
    setStatus('Cancellation requested. Waiting for native computation to stop…');
  } catch (error) { setStatus(error.message, true); cancelGeneration.disabled = false; }
});

async function watchSynthesisJob(id) {
  const endpoint = `/api/tts/jobs/${encodeURIComponent(id)}`;
  let failures = 0;
  const terminal = snapshot => {
    const seconds = Math.floor(snapshot.elapsed_secs || 0);
    const context = snapshot.context;
    const identity = context ? `${context.model} (${context.provider}) · ${context.voice} · ${context.effective_device} · ` : '';
    setStatus(`${identity}${snapshot.stage}${snapshot.cancellation_requested || snapshot.deadline_exceeded ? ' (stopping)' : ''} · ${seconds}s elapsed${snapshot.frames ? ` · ${snapshot.frames} generated frames` : ''}${context?.effective_device === 'cpu' && context?.provider === 'qwen3' ? ' · CPU inference can be slow.' : ''}`);
    if (snapshot.stage === 'completed') return true;
    if (snapshot.stage === 'failed' || snapshot.stage === 'cancelled') {
      const error: Error & { terminalJob?: boolean } = new Error(snapshot.stage === 'failed' ? snapshot.error || 'Inference failed.' : 'Generation cancelled or its deadline expired.');
      error.terminalJob = true; throw error;
    }
    return false;
  };
  while (failures < 5) {
    // Reconnection always fetches a current snapshot; missed SSE events cannot
    // strand the UI. No percentage or remaining-time estimate is invented.
    let statusResponse;
    try { statusResponse = await apiFetch(endpoint, { signal: AbortSignal.timeout(15000) }); }
    catch (_) { failures++; await new Promise(resolve => setTimeout(resolve, 1000)); continue; }
    if (!statusResponse.ok) {
      const error: Error & { terminalJob?: boolean } = new Error(`Job status unavailable (HTTP ${statusResponse.status}).`);
      error.terminalJob = statusResponse.status === 404;
      throw error;
    }
    if (terminal(await statusResponse.json())) return;
    let response;
    try { response = await apiFetch(`${endpoint}/events`, { signal: AbortSignal.timeout(30000) }); }
    catch (_) { failures++; await new Promise(resolve => setTimeout(resolve, 1000)); continue; }
    if (!response.ok || !response.body) { failures++; await new Promise(resolve => setTimeout(resolve, 1000)); continue; }
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let pending = '';
    try {
      while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        pending += decoder.decode(value, { stream: true }).replace(/\r/g, '');
        let boundary;
        while ((boundary = pending.indexOf('\n\n')) >= 0) {
          const event = pending.slice(0, boundary); pending = pending.slice(boundary + 2);
          const data = event.split('\n').filter(line => line.startsWith('data:')).map(line => line.slice(5).trim()).join('\n');
          if (data) { failures = 0; if (terminal(JSON.parse(data))) return; }
        }
        if (pending.length > 65536) throw new Error('Invalid progress response.');
      }
    } catch (error) {
      if (error.terminalJob) throw error;
      failures++;
    } finally { await reader.cancel().catch(() => {}); reader.releaseLock(); }
    await new Promise(resolve => setTimeout(resolve, 1000));
  }
  throw new Error('Progress connection lost. The job may still be running; reconnect to check its status.');
}

async function synthesize(options: { previewText?: string } = {}) {
  if (generateBtn.disabled) return;
  const text = typeof options.previewText === 'string' ? options.previewText : textInput.value.trim();
  if (!text && !activeJobId) {
    setStatus('Please enter some text.', true);
    return;
  }

  if (serverInfo && serverInfo.auth && serverInfo.auth.required) {
    const apiToken = apiTokenInput.value.trim();
    if (!apiToken) {
      setStatus('Please enter your API token.', true);
      return;
    }
  }

  const voice = voiceSelect.value;
  const lang = langSelect.value;
  const meta = selectedModelMeta();

  // Set UI Loading State
  generateBtn.disabled = true;
  generateBtn.classList.add('loading');
  generateBtn.textContent = 'Generating…';
  const providerStatus = (lastProviders?.providers || []).find(p => p.active_model === modelSelect.value);
  const selectedDevice = providerStatus?.effective_device || 'unknown';
  setStatus(`${modelSelect.value} · ${voice} · ${selectedDevice}${selectedDevice === 'cpu' && meta.kind === 'cloning' ? ' · CPU inference may be slow; no completion time estimate is available.' : ''}`);

  let terminalReached = false;
  try {
    if (meta.kind === 'cloning' && !voice && !activeJobId) {
      throw new Error('Create a voice profile for this model first.');
    }
    const params = new URLSearchParams({ lang, format: 'wav' });
    if (meta.kind === 'cloning') params.set('profile', voice);
    else params.set('voice', voice);
    let job = { id: activeJobId };
    if (!activeJobId) {
      const created = await apiFetch(`/api/tts/jobs?${params}`, {
        method: 'POST', headers: { 'Content-Type': 'text/plain' }, body: text,
      });
      if (!created.ok) throw new Error(`HTTP ${created.status}: ${await created.text()}`);
      job = await created.json();
      activeJobId = job.id;
    }
    cancelGeneration.hidden = false;
    cancelGeneration.disabled = false;
    await watchSynthesisJob(job.id);
    terminalReached = true;
    const resp = await apiFetch(`/api/tts/jobs/${encodeURIComponent(job.id)}/audio`);

    if (!resp.ok) {
      const msg = await resp.text().catch(() => resp.statusText);
      throw new Error(`HTTP ${resp.status}: ${msg}`);
    }

    const blob = await resp.blob();

    // Revoke previous audio session URL to free up browser memory
    if (currentAudioUrl) {
      URL.revokeObjectURL(currentAudioUrl);
    }

    currentAudioUrl = URL.createObjectURL(blob);
    audioPlayer.src = currentAudioUrl;
    audioPlayer.style.display = 'block';
    const download = element('downloadAudio');
    download.href = currentAudioUrl; download.hidden = false;

    // Play naturally, catching browser auto-play prevention errors
    audioPlayer.play().catch(err => {
      console.log("Audio waiting for user interaction to play: ", err);
    });

    setStatus('Completed. Audio is ready to play or download.');
  } catch (e) {
    terminalReached ||= Boolean(e.terminalJob);
    setStatus(e.message + (activeJobId && !terminalReached ? ' Use Resume monitoring or Cancel generation.' : ''), true);
  } finally {
    if (terminalReached) activeJobId = null;
    cancelGeneration.hidden = !activeJobId;
    cancelGeneration.disabled = false;
    // Reset UI State
    generateBtn.disabled = false;
    generateBtn.classList.remove('loading');
    generateBtn.textContent = activeJobId ? 'Resume monitoring' : 'Generate speech';
  }
}

function setStatus(msg: string, isError = false) {
  statusDiv.textContent = msg;
  statusDiv.className = isError ? 'error' : '';
}

// Refresh periodically if model is not ready yet
const reloadTimer = !MODEL_READY ? setTimeout(() => location.reload(), App.modelReloadMs) : undefined;

// --- 4. Configuration browser -----------------------------------------------

configSearch.addEventListener('input', renderConfigGroups);
configCategory.addEventListener('change', renderConfigGroups);

function humanize(key) {
  return key
    .replace(/_/g, ' ')
    .replace(/\b\w/g, c => c.toUpperCase());
}

function categoryOf(path) {
  const head = path.split('.')[0];
  return Object.hasOwn(CATEGORIES, head) ? head : 'other';
}

function categoryLabel(key) {
  return CATEGORIES[key] || humanize(key);
}

function getConfiguredValue(path) {
  if (!configSnapshot) return undefined;
  return path.split('.').reduce((object, key) => object?.[key], configSnapshot.config);
}

function setNestedValue(root, path, value) {
  const parts = path.split('.');
  let target = root;
  for (const part of parts.slice(0, -1)) {
    target = target[part] = {};
  }
  target[parts.at(-1)] = value;
}

function parseSettingInput(meta, raw) {
  const type = meta.type;
  if (meta.nullable && (raw === '' || raw === 'null')) return null;
  if (type === SettingType.Boolean) return raw === true || raw === 'true';
  if (type === SettingType.Integer || type === SettingType.Number) {
    if (raw === '' || raw === null || raw === undefined) throw new Error('Enter a number');
    const value = Number(raw);
    if (!Number.isFinite(value)) throw new Error('Enter a valid number');
    if (type === SettingType.Integer && !Number.isInteger(value)) throw new Error('Enter a whole number');
    return value;
  }
  if (type === SettingType.Array || type === SettingType.Object) {
    try {
      return JSON.parse(raw);
    } catch {
      throw new Error(type === SettingType.Array ? 'Enter a JSON array, e.g. ["a"]' : 'Enter valid JSON');
    }
  }
  return raw;
}

function formatForInput(value) {
  if (value === null || value === undefined) return '';
  if (Array.isArray(value) || (typeof value === 'object' && value !== null)) return JSON.stringify(value);
  return String(value);
}

async function loadAudioDevices() {
  if (audioDevicesCache) return audioDevicesCache;
  try {
    const list = await configRequest(Endpoints.audioDevices);
    audioDevicesCache = list.devices || [];
  } catch {
    audioDevicesCache = [];
  }
  return audioDevicesCache;
}

function buildCategoryOptions() {
  const present = new Set(
    Object.keys(configSchema).filter(p => !configSchema[p].secret).map(categoryOf),
  );
  configCategory.innerHTML = '';
  configCategory.append(optionEl('all', 'All categories'));
  for (const key of [...present].sort((a, b) => categoryLabel(a).localeCompare(categoryLabel(b)))) {
    configCategory.append(optionEl(key, categoryLabel(key)));
  }
}

function matchesFilter(path, meta, title) {
  const q = configSearch.value.trim().toLowerCase();
  if (q && !(path.toLowerCase().includes(q) || title.toLowerCase().includes(q) || meta.description.toLowerCase().includes(q))) {
    return false;
  }
  const cat = configCategory.value || 'all';
  if (cat !== 'all' && categoryOf(path) !== cat) return false;
  return true;
}

function hintFor(meta) {
  const hints = [];
  const hasMin = meta.min !== undefined && meta.min !== null;
  const hasMax = meta.max !== undefined && meta.max !== null;
  if (meta.type === SettingType.Integer || meta.type === SettingType.Number) {
    if (hasMin && hasMax) hints.push(`Range ${meta.min} – ${meta.max}`);
    else if (hasMin || hasMax) hints.push(`Allowed: ${[hasMin ? `≥ ${meta.min}` : '', hasMax ? `≤ ${meta.max}` : ''].filter(Boolean).join(', ')}`);
  }
  if (meta.nullable) hints.push('Empty clears the value');
  if (meta.type === SettingType.Array) hints.push('JSON array');
  return hints.join(' · ');
}

function groupSection(category, entries) {
  entries.sort((a, b) => a.path.localeCompare(b.path));
  const section = el('section', { cls: 'cfg-group' });
  const heading = el('h3', { text: categoryLabel(category) });
  heading.append(el('span', {
    cls: 'count',
    text: `${entries.length} setting${entries.length === 1 ? '' : 's'}`,
  }));
  section.append(heading);
  for (const entry of entries) {
    section.append(settingRow(entry));
  }
  return section;
}

function renderConfigGroups() {
  if (!configSnapshot || Object.keys(configSchema).length === 0) return;
  const groups = new Map();
  for (const [path, meta] of Object.entries(configSchema)) {
    if (meta.secret) continue;
    const title = humanize(path.split('.').at(-1));
    if (!matchesFilter(path, meta, title)) continue;
    const cat = categoryOf(path);
    if (!groups.has(cat)) groups.set(cat, []);
    groups.get(cat).push({ path, meta, title });
  }

  configGroupsEl.innerHTML = '';
  if (groups.size === 0) {
    configGroupsEl.append(el('div', { cls: 'empty-result', text: 'No settings match this filter.' }));
    return;
  }

  const ordered = [...groups.entries()]
    .sort((a, b) => categoryLabel(a[0]).localeCompare(categoryLabel(b[0])));
  for (const [cat, entries] of ordered) {
    configGroupsEl.append(groupSection(cat, entries));
  }
}

function settingRow({ path, meta, title }) {
  const row = el('div', { cls: 'setting', attrs: { 'data-path': path } });

  const head = el('div', { cls: 'setting-head' });
  const titleWrap = el('div');
  titleWrap.append(
    el('div', { cls: 'setting-title', text: title }),
    el('div', { cls: 'setting-path', text: path }),
  );
  head.append(titleWrap, applyBadge(meta.apply));
  row.append(head);

  row.append(el('p', { cls: 'setting-desc', text: meta.description }));

  const control = el('div', { cls: 'setting-control' });
  const markDirty = () => row.classList.toggle('dirty', !field.isClean());
  const field = settingControl(path, meta, title, getConfiguredValue(path), markDirty);
  const save = actionButton('Save');
  control.append(field.node, save);
  row.append(control);

  const hint = hintFor(meta);
  if (hint) row.append(el('p', { cls: 'field-hint', text: hint }));

  const msg = rowMessage();
  row.append(msg);

  save.addEventListener('click', async () => {
    setRowMessage(msg, '', null);
    save.disabled = true;
    try {
      const value = parseSettingInput(meta, field.getValue());
      const changes = {};
      setNestedValue(changes, path, value);
      await configRequest(Endpoints.config, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ expected_revision: configSnapshot.revision, changes }),
      });
      row.classList.remove('dirty');
      setRowMessage(msg, 'Saved.', 'ok');
      await refreshConfiguration();
    } catch (error) {
      setRowMessage(msg, error.message, 'error');
    } finally {
      save.disabled = false;
    }
  });

  return row;
}

async function refreshConfiguration() {
  const loading = document.getElementById('configLoading');
  try {
    const status = await configRequest(Endpoints.configStatus);
    element('configPath').textContent = status.config_path || '';
    element('configOpen').hidden = serverInfo?.mode !== 'desktop';
    element('configRevision').textContent = `Revision ${status.revision}`;
    element('statWatcher').textContent = status.watcher || '—';
    element('statReload').textContent = status.last_reload || '—';
    element('statPending').textContent = (status.pending_applies || []).join(', ') || 'none';
    showProblems([
      status.last_error,
      ...(status.apply_failures || []).map(f => `${f.path}: ${f.error} (active: ${JSON.stringify(f.active)})`),
    ].filter(Boolean));
    const previous = configSnapshot;
    if (!configSnapshot || configSnapshot.revision !== status.revision) {
      configSnapshot = await configRequest(Endpoints.config);
      if (Object.keys(configSchema).length === 0) {
        const schema = await configRequest(Endpoints.configSchema);
        // The schema endpoint returns either a flat map or { settings: [...] }.
        configSchema = Array.isArray(schema.settings)
          ? Object.fromEntries(schema.settings.map(s => [s.path, s]))
          : schema;
        buildCategoryOptions();
        // Provider toggles are schema-gated: cards rendered before the
        // schema arrived have none, so force one re-render to add them.
        providerSignature = null;
        await refreshProviders();
      }
      if (previous) setNotice('ok', 'Configuration changed — list refreshed.');
      renderConfigGroups();
    } else if (configSnapshot && configGroupsEl.querySelector('#configLoading')) {
      renderConfigGroups();
    }
    if (loading) loading.remove();
  } catch (error) {
    if (loading) loading.textContent = 'Configuration needs an API token to load.';
    showProblems([error.message]);
  }
}

function bindConfigAction(buttonId, action, onSuccess) {
  element(buttonId).addEventListener('click', async () => {
    setNotice('', '');
    try {
      await configRequest(Endpoints.configAction(action), { method: 'POST' });
      await onSuccess();
    } catch (error) {
      setNotice('error', error.message);
    }
  });
}

apiTokenInput.addEventListener('change', refreshConfiguration);
element('configRefresh').addEventListener('click', async () => {
  configSchema = {};
  configCategory.innerHTML = '';
  await refreshConfiguration();
});
bindConfigAction('configReload', 'reload', async () => {
  setNotice('ok', 'Reloaded from disk.');
  await refreshConfiguration();
});
bindConfigAction('configValidate', 'validate', async () => {
  setNotice('ok', 'Configuration file is valid.');
});

element('configOpen').addEventListener('click', async () => {
  try { await configRequest(Endpoints.configOpen, { method: 'POST' }); }
  catch (error) {
    setNotice('error', error.message);
  }
});

const configTimer = setInterval(async () => {
  await loadServerInfo();
  await refreshConfiguration();
  await refreshProviders();
}, App.configPollMs);
return () => {
  clearInterval(configTimer);
  clearTimeout(reloadTimer);
  if (currentAudioUrl) URL.revokeObjectURL(currentAudioUrl);
};

}
