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
