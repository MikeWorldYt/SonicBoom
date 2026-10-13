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
const MODEL_READY = document.body.dataset.modelReady === 'true';

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
function optionEl(value, label) {
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

const tokenGroup = document.getElementById('tokenGroup');
const apiTokenInput = document.getElementById('apiToken');
const textInput = document.getElementById('text');
const charCount = document.getElementById('charCount');
const voiceSelect = document.getElementById('voice');
const langSelect = document.getElementById('lang');
const generateBtn = document.getElementById('btn');
const statusDiv = document.getElementById('status');
const audioPlayer = document.getElementById('player');

const configGroupsEl = document.getElementById('configGroups');
const configSearch = document.getElementById('configSearch');
const configCategory = document.getElementById('configCategory');
const configNotice = document.getElementById('configNotice');
const configError = document.getElementById('configError');

// Server info discovered from GET /api/info. When the
// server runs in local (loopback) mode no API token is
// required, so the token field is hidden (spec §32).
let serverInfo = null;

// Store active audio object URL to prevent memory leaks
let currentAudioUrl = null;

let configSnapshot = null;
let configSchema = {};
let audioDevicesCache = null;

// --- Shared helpers ---------------------------------------------------------

// Centralized API client: attaches the Bearer token only
// when the server actually requires one (spec §32).
async function apiFetch(path, options = {}) {
  const headers = new Headers(options.headers || {});
  const apiToken = apiTokenInput.value.trim();
  if (serverInfo && serverInfo.auth && serverInfo.auth.required && apiToken) {
    headers.set('Authorization', `Bearer ${apiToken}`);
  }
  return fetch(path, { ...options, headers });
}

async function configRequest(path, options) {
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
 * One voice-engine status card. Mirrors the server-rendered markup in
 * `provider_card_html` so first paint and live updates look identical.
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
  return card;
}

let providerSignature = null;

/** Re-render engine cards when provider status changes; keeps the
 *  server-rendered first paint untouched on errors or when idle. */
async function refreshProviders() {
  const list = document.querySelector('.provider-list');
  if (!list) return;
  try {
    const body = await configRequest(Endpoints.providers);
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
  } catch {
    // Network or auth failure: leave the server-rendered cards in place.
  }
}

// --- 3. TTS synthesis -------------------------------------------------------

function populateLanguages() {
  langSelect.innerHTML = '';
  for (const { code, label } of LANGUAGES) {
    langSelect.append(optionEl(code, label));
  }
  langSelect.value = 'en';
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
populateLanguages();
loadServerInfo().then(() => {
  refreshConfiguration();
  refreshProviders();
});
generateBtn.addEventListener('click', synthesize);
textInput.addEventListener('input', updateCharCount);
updateCharCount();

function updateCharCount() {
  const n = textInput.value.length;
  charCount.textContent = `${n} character${n === 1 ? '' : 's'}`;
}

async function synthesize() {
  const text = textInput.value.trim();
  if (!text) {
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

  // Set UI Loading State
  generateBtn.disabled = true;
  generateBtn.classList.add('loading');
  generateBtn.textContent = 'Generating…';
  setStatus('');

  try {
    const params = new URLSearchParams({ voice, lang });
    const resp = await apiFetch(`${Endpoints.tts}?${params.toString()}`, {
      method: 'POST',
      headers: {
        'Content-Type': 'text/plain',
      },
      body: text,
    });

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

    // Play naturally, catching browser auto-play prevention errors
    audioPlayer.play().catch(err => {
      console.log("Audio waiting for user interaction to play: ", err);
    });

    setStatus('');
  } catch (e) {
    setStatus(e.message, true);
  } finally {
    // Reset UI State
    generateBtn.disabled = false;
    generateBtn.classList.remove('loading');
    generateBtn.textContent = 'Generate speech';
  }
}

function setStatus(msg, isError) {
  statusDiv.textContent = msg;
  statusDiv.className = isError ? 'error' : '';
}

// Refresh periodically if model is not ready yet
if (!MODEL_READY) {
  setTimeout(() => location.reload(), App.modelReloadMs);
}

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
    document.getElementById('configPath').textContent = status.config_path || '';
    document.getElementById('configOpen').hidden = serverInfo?.mode !== 'desktop';
    document.getElementById('configRevision').textContent = `Revision ${status.revision}`;
    document.getElementById('statWatcher').textContent = status.watcher || '—';
    document.getElementById('statReload').textContent = status.last_reload || '—';
    document.getElementById('statPending').textContent = (status.pending_applies || []).join(', ') || 'none';
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
  document.getElementById(buttonId).addEventListener('click', async () => {
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
document.getElementById('configRefresh').addEventListener('click', async () => {
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

document.getElementById('configOpen').addEventListener('click', async () => {
  try { await configRequest(Endpoints.configOpen, { method: 'POST' }); }
  catch (error) {
    setNotice('error', error.message);
  }
});

setInterval(async () => {
  await loadServerInfo();
  await refreshConfiguration();
  await refreshProviders();
}, App.configPollMs);
