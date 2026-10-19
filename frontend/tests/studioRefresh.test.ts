import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  createStudio,
  slowRefreshDue,
  STUDIO_FAST_MS,
  STUDIO_SLOW_EVERY,
} from '../src/composables/useStudio.ts';
import type { Api } from '../src/services/api.ts';

const data = {
  page: 'home' as const,
  modelReady: true,
  voices: ['M1'],
  status: 'ready',
  providers: [],
};

const readyInfo = () => ({
  version: 'test',
  mode: 'server',
  setup_complete: true,
  auth: { mode: 'token', required: true },
  features: { playback: false, audio_devices: false, hot_reload: false },
  config_revision: 1,
  model: { status: 'ready' },
});

function countingApi(counts: Record<string, number>, modelStatus = 'ready') {
  const count =
    (name: string) =>
    async (...args: unknown[]) => {
      counts[name] = (counts[name] ?? 0) + 1;
      void args;
      switch (name) {
        case 'info':
          return readyInfoFor(modelStatus);
        case 'providers':
          return { providers: [], capabilities: [] };
        case 'builtinVoices':
          return [{ id: 'M1', name: 'M1' }];
        case 'voices':
          return [];
        case 'configStatus':
          return { revision: 3, config_path: '/tmp/config.toml' };
        case 'config':
          return { revision: 3, config: {} };
        case 'schema':
          return {};
        case 'audioDevices':
          return [];
        default:
          throw new Error(`unexpected api call: ${name}`);
      }
    };
  const readyInfoFor = (status: string) => ({ ...readyInfo(), model: { status } });
  return {
    info: count('info'),
    providers: count('providers'),
    builtinVoices: count('builtinVoices'),
    voices: count('voices'),
    configStatus: count('configStatus'),
    config: count('config'),
    schema: count('schema'),
    audioDevices: count('audioDevices'),
  } as unknown as Api;
}

test('slow metadata ticks every sixth fast tick', () => {
  assert.equal(STUDIO_FAST_MS, 5000);
  assert.equal(STUDIO_SLOW_EVERY, 6);
  for (const tick of [1, 2, 3, 4, 5, 7, 11]) assert.equal(slowRefreshDue(tick), false);
  for (const tick of [6, 12, 60]) assert.equal(slowRefreshDue(tick), true);
});

test('immutable built-in voices fetch once per readiness change, not every tick', async () => {
  const counts: Record<string, number> = {};
  const studio = createStudio(data, { api: countingApi(counts) });
  try {
    await studio.refresh();
    assert.equal(counts.info, 1);
    assert.equal(counts.builtinVoices, 1);
    await studio.refresh();
    await studio.refresh(false);
    assert.equal(counts.info, 3);
    assert.equal(
      counts.builtinVoices,
      1,
      'ready-state ticks must not refetch immutable voices',
    );
  } finally {
    studio.dispose();
  }
});

test('voice list refreshes on slow ticks and explicit calls only', async () => {
  const counts: Record<string, number> = {};
  const studio = createStudio(data, { api: countingApi(counts) });
  try {
    await studio.refresh();
    assert.equal(counts.voices, 1);
    await studio.refresh(false);
    await studio.refresh(false);
    assert.equal(counts.voices, 1, 'fast ticks must not refetch the voice list');
    // Clone create/delete flows refresh explicitly and keep working.
    await studio.refreshVoices();
    assert.equal(counts.voices, 2);
    assert.equal(studio.errors.value.voices, undefined);
  } finally {
    studio.dispose();
  }
});

test('stale generations never overwrite state and never reject', async () => {
  const counts: Record<string, number> = {};
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const base = countingApi(counts);
  const api = {
    ...(base as unknown as Record<string, unknown>),
    info: async () => {
      counts.info = (counts.info ?? 0) + 1;
      await gate;
      return readyInfo();
    },
  } as unknown as Api;
  const studio = createStudio(data, { api });
  try {
    const pending = studio.refresh();
    studio.dispose();
    release();
    await pending;
    assert.equal(studio.info.value, null);
    assert.deepEqual(studio.errors.value, {});
  } finally {
    studio.dispose();
    release();
  }
});

test('failed requests record errors without unhandled rejections', async () => {
  const counts: Record<string, number> = {};
  const base = countingApi(counts);
  const api = {
    ...(base as unknown as Record<string, unknown>),
    info: async () => {
      throw new Error('down');
    },
  } as unknown as Api;
  const studio = createStudio(data, { api });
  try {
    await studio.refresh();
    assert.notEqual(studio.errors.value.info, undefined);
  } finally {
    studio.dispose();
  }
});

test('concurrent config refreshes share status, snapshot and schema requests', async () => {
  const counts: Record<string, number> = {};
  const studio = createStudio(data, { api: countingApi(counts) });
  try {
    await Promise.all([studio.refreshConfig(), studio.refreshConfig()]);
    assert.equal(counts.configStatus, 1);
    assert.equal(counts.config, 1);
    assert.equal(counts.schema, 1);
  } finally { studio.dispose(); }
});

test('token change clears scoped state immediately and aborts late voice reads', async () => {
  const counts: Record<string, number> = {};
  let owner = 'old', oldSignal: AbortSignal | undefined, release!: (value: unknown[]) => void;
  const oldResult = new Promise<unknown[]>(resolve => { release = resolve; });
  const base = countingApi(counts);
  const api = { ...base, voices: async (signal?: AbortSignal) => {
    if (owner === 'old') { oldSignal = signal; return oldResult; }
    return [{ id: 'new-owner', name: 'New', provider: 'pocket-tts', model: 'pocket-tts-en', language: 'en', clone_mode: 'audio_prompt' }];
  } } as unknown as Api;
  const studio = createStudio(data, { api });
  try {
    const initial = studio.refresh();
    await new Promise(resolve => setTimeout(resolve, 0));
    owner = 'new'; studio.token.value = 'new-token';
    assert.deepEqual(studio.voices.value, []);
    assert.equal(oldSignal?.aborted, true);
    release([{ id: 'old-owner' }]);
    await initial;
    for (let i = 0; i < 10 && !studio.voices.value.length; i++) await new Promise(resolve => setTimeout(resolve, 0));
    assert.equal(studio.voices.value[0]?.id, 'new-owner');
  } finally { studio.dispose(); }
});

test('incompatible profile revisions are excluded from generation options', () => {
  const studio = createStudio(data, { api: countingApi({}) });
  try {
    studio.providers.value = [{ provider_id: 'pocket-tts', state: 'ready', enabled: true, active_model: 'pocket-tts-en', model_revision: 'new-revision' }];
    studio.capabilities.value = [{ provider_id: 'pocket-tts', models: ['pocket-tts-en'], languages: ['en'], devices: ['cpu'], clone_modes: ['audio_prompt'], requires_reference_audio: true, requires_reference_transcript: false }];
    const profile = { id: 'old-profile', name: 'Old', provider: 'pocket-tts', model: 'pocket-tts-en', language: 'en', clone_mode: 'audio_prompt', model_revision: 'old-revision' };
    studio.voices.value = [profile]; studio.model.value = 'pocket-tts-en';
    assert.equal(studio.profileCompatible(profile), false);
    assert.equal(studio.availableVoices.value.length, 0);
    studio.voices.value = [{ ...profile, model_revision: 'new-revision' }];
    assert.equal(studio.availableVoices.value.length, 1);
  } finally { studio.dispose(); }
});

test('hot authentication-mode changes invalidate cached owner-scoped resources', async () => {
  let mode = 'none';
  const base = countingApi({});
  const api = { ...base, info: async () => ({ ...readyInfo(), auth: { mode, required: mode === 'token' } }), voices: async () => mode === 'none' ? [{ id: 'local-owner', name: 'Local', provider: 'pocket-tts', model: 'pocket-tts-en', language: 'en', clone_mode: 'audio_prompt' }] : [] } as unknown as Api;
  const studio = createStudio(data, { api });
  try {
    await studio.refresh();
    assert.equal(studio.voices.value[0]?.id, 'local-owner');
    const epoch = studio.scopeEpoch.value;
    mode = 'token';
    await studio.refresh(false);
    assert.ok(studio.scopeEpoch.value > epoch);
    assert.deepEqual(studio.voices.value, []);
  } finally { studio.dispose(); }
});
