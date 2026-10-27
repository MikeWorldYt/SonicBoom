import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ref, shallowRef } from 'vue';
import { createApi } from '../../src/services/api';
import { ApiError } from '../../src/services/http';
import { infoSchema, normalizationInfoSchema, normalizePreviewSchema } from '../../src/services/responseSchemas';
import { useSynthesis } from '../../src/composables/useSynthesis';
import { usePronunciationPreview } from '../../src/composables/usePronunciationPreview';
import { createLatestRunner, effectiveLocale, isPreviewStale, previewKey, previewRequestBody, pronunciationQuery, showLocaleChoice } from '../../src/features/speech/pronunciation';
import type { NormalizationLanguage, NormalizationMode } from '../../src/services/contracts';
import type { Studio } from '../../src/composables/useStudio';

const infoBlock = {
  ruleset_version: 'tn-v1', default_mode: 'auto',
  languages: [
    { language: 'es', locales: ['es-ES', 'es-MX', 'es-PE'], default_locale: 'es-ES', rules: 'full', features: { cardinal_numbers: 'full', currency: 'partial', fractions: 'none' } },
    { language: 'fr', locales: ['fr-FR'], default_locale: 'fr-FR', rules: 'partial', features: {} },
  ],
};
const previewBody = {
  original: '26/10/2026', spoken: 'veintiséis de octubre de dos mil veintiséis', language: 'es', locale: 'es-PE', mode: 'auto', normalizer_version: 'tn-v1',
  changes: [{ kind: 'date', before: '26/10/2026', after: 'veintiséis de octubre de dos mil veintiséis', start: 0, end: 10, confidence: 'high' }],
  warnings: [{ code: 'ambiguous_date', before: '01/02/2026', start: 0, end: 10 }],
  coverage: { dates: 'full', currency: 'full' },
};
const baseInfo = {
  version: 'test', mode: 'server', setup_complete: true, auth: { mode: 'token', required: true },
  features: { playback: false, audio_devices: false, hot_reload: false }, config_revision: 1, model: { status: 'ready' },
};
const spanish: NormalizationLanguage = { language: 'es', locales: ['es-ES', 'es-MX', 'es-PE'], default_locale: 'es-ES', rules: 'full', features: {} };
const french: NormalizationLanguage = { language: 'fr', locales: ['fr-FR'], default_locale: 'fr-FR', rules: 'partial', features: {} };

beforeEach(() => {
  URL.createObjectURL = vi.fn(() => 'blob:test');
  URL.revokeObjectURL = vi.fn();
});
afterEach(() => { vi.useRealTimers(); });

it('accepts the documented normalize and capability examples', () => {
  expect(normalizePreviewSchema.parse(previewBody)).toMatchObject({ spoken: previewBody.spoken, locale: 'es-PE', changes: [{ kind: 'date' }], warnings: [{ code: 'ambiguous_date', before: '01/02/2026' }] });
  expect(normalizationInfoSchema.parse(infoBlock).languages[0].locales).toEqual(['es-ES', 'es-MX', 'es-PE']);
  expect(infoSchema.parse({ ...baseInfo, normalization: infoBlock }).normalization?.default_mode).toBe('auto');
});

it('rejects malformed normalize responses and capability blocks', () => {
  expect(normalizePreviewSchema.safeParse({ ...previewBody, spoken: undefined }).success).toBe(false);
  expect(normalizePreviewSchema.safeParse({ ...previewBody, mode: 'strict' }).success).toBe(false);
  expect(normalizePreviewSchema.safeParse({ ...previewBody, changes: [{ ...previewBody.changes[0], start: 5, end: 2 }] }).success).toBe(false);
  expect(normalizePreviewSchema.safeParse({ ...previewBody, changes: [{ ...previewBody.changes[0], start: -1 }] }).success).toBe(false);
  expect(normalizePreviewSchema.safeParse({ ...previewBody, warnings: [{ before: 'x', start: 0, end: 1 }] }).success).toBe(false);
  expect(normalizePreviewSchema.safeParse({ ...previewBody, coverage: { dates: 'sometimes' } }).success).toBe(false);
  expect(normalizationInfoSchema.safeParse({ ...infoBlock, default_mode: 'strict' }).success).toBe(false);
  expect(normalizationInfoSchema.safeParse({ ...infoBlock, languages: [{ language: 'es', locales: 'es-ES' }] }).success).toBe(false);
});

it('treats an absent or malformed normalization block as an older server', () => {
  expect(infoSchema.parse(baseInfo).normalization).toBeUndefined();
  expect(infoSchema.parse({ ...baseInfo, normalization: null }).normalization).toBeUndefined();
  expect(infoSchema.parse({ ...baseInfo, normalization: { languages: 'es' } }).normalization).toBeUndefined();
});

it('adds no normalization parameters unless fields are present', async () => {
  const paths: string[] = [];
  const api = createApi({ json: async (path: string) => { paths.push(path); return { id: 'job' }; } } as never);
  await api.createJob('hola', 'es', 'A', false);
  await api.createJob('hola', 'es', 'A', false, { seed: '7' }, undefined, undefined);
  await api.createJob('hola', 'es', 'A', false, undefined, undefined, {});
  await api.createJob('hola', 'es', 'A', false, undefined, undefined, { normalization: undefined, locale: '' });
  await api.createJob('hola', 'es', 'A', false, undefined, undefined, { locale: 'auto' });
  expect(paths[0]).toBe('/api/tts/jobs?lang=es&format=wav&voice=A');
  expect(paths[1]).toBe('/api/tts/jobs?lang=es&format=wav&voice=A&seed=7');
  expect(paths[2]).toBe(paths[0]);
  expect(paths[3]).toBe(paths[0]);
  expect(new URL(paths[4], 'http://localhost').searchParams.get('locale')).toBe('auto');
  expect(pronunciationQuery()).toEqual({});
});

it('adds normalization and a concrete locale when present', async () => {
  const paths: string[] = [];
  const api = createApi({ json: async (path: string) => { paths.push(path); return { id: 'job' }; } } as never);
  await api.createJob('hola', 'es', 'A', true, undefined, undefined, { normalization: 'auto', locale: 'es-PE' });
  await api.createJob('hola', 'es', 'A', true, undefined, undefined, { normalization: 'off' });
  const first = new URL(paths[0], 'http://localhost').searchParams;
  expect([first.get('normalization'), first.get('locale'), first.get('profile'), first.get('lang')]).toEqual(['auto', 'es-PE', 'A', 'es']);
  const second = new URL(paths[1], 'http://localhost').searchParams;
  expect([second.get('normalization'), second.has('locale')]).toEqual(['off', false]);
});

it('previewNormalization posts JSON, omits unset members, validates the response and honors the signal', async () => {
  const calls: { path: string; options: RequestInit }[] = [];
  const api = createApi({ json: async (path: string, options: RequestInit) => { calls.push({ path, options }); return previewBody; } } as never);
  const controller = new AbortController();
  const value = await api.previewNormalization({ text: '26/10/2026', language: 'es', locale: 'auto', mode: 'auto' }, controller.signal);
  expect(value.spoken).toBe(previewBody.spoken);
  expect(calls[0].path).toBe('/api/tts/normalize');
  expect(calls[0].options.method).toBe('POST');
  expect(calls[0].options.signal).toBe(controller.signal);
  expect(JSON.parse(String(calls[0].options.body))).toEqual({ text: '26/10/2026', language: 'es', locale: 'auto', mode: 'auto' });
  expect(previewRequestBody({ text: 'x', language: 'es', locale: 'es-PE', model: 'm' })).toEqual({ text: 'x', language: 'es', locale: 'es-PE', model: 'm' });
  const broken = createApi({ json: async () => ({ ...previewBody, spoken: 5 }) } as never);
  await expect(broken.previewNormalization({ text: 'x', language: 'es' })).rejects.toMatchObject({ kind: 'invalid_response' });
});

it('offers only server-listed locales and only where there is a choice', () => {
  expect(showLocaleChoice(spanish, 'auto')).toBe(true);
  expect(showLocaleChoice(spanish, 'off')).toBe(false);
  expect(showLocaleChoice(french, 'auto')).toBe(false);
  expect(showLocaleChoice(undefined, 'auto')).toBe(false);
  expect(effectiveLocale(spanish, 'es-PE', 'auto')).toBe('es-PE');
  expect(effectiveLocale(spanish, 'auto', 'auto')).toBe('auto');
  expect(effectiveLocale(spanish, 'es-AR', 'auto')).toBeUndefined();
  expect(effectiveLocale(spanish, '', 'auto')).toBeUndefined();
  expect(effectiveLocale(spanish, 'es-PE', 'off')).toBeUndefined();
  expect(effectiveLocale(french, 'fr-FR', 'auto')).toBeUndefined();
});

function synthesisFixture() {
  const calls: unknown[][] = [];
  let failAudio = true;
  const form = { text: ref('Pago 26/10/2026 '), language: ref('es'), voice: ref('A'), pronunciation: ref<{ normalization: NormalizationMode; locale?: string } | undefined>({ normalization: 'auto', locale: 'es-PE' }) };
  const studio = {
    scopeEpoch: ref(0), token: ref(''), info: ref(null), sampling: ref({ preset: 'balanced' }), samplingParams: ref({}),
    text: form.text, language: form.language, voice: form.voice, pronunciation: form.pronunciation,
    selectedModel: shallowRef({ id: 'm1', ready: true, cloned: false, provider: 'supertonic' }),
    api: {
      createJob: async (...args: unknown[]) => { calls.push(args); return { id: `job-${calls.length}` }; },
      job: async (id: string) => ({ id, stage: 'completed' }),
      jobAudio: async () => { if (failAudio) throw new Error('offline'); return new Blob(['x']); },
      cancelJob: async () => {},
    },
  } as unknown as Studio;
  return { form, calls, studio, synthesis: useSynthesis(studio), audio: (ok: boolean) => { failAudio = !ok; } };
}

it('freezes text, mode and locale at submission and reuses them for resume and regenerate', async () => {
  const f = synthesisFixture();
  try {
    await f.synthesis.generate();
    expect(f.calls[0].slice(0, 4)).toEqual(['Pago 26/10/2026', 'es', 'A', false]);
    expect(f.calls[0][6]).toEqual({ normalization: 'auto', locale: 'es-PE' });
    expect(Object.isFrozen(f.synthesis.active.value?.input)).toBe(true);
    f.form.text.value = 'Otro texto'; f.form.language.value = 'en'; f.form.pronunciation.value = { normalization: 'off' };
    expect(f.synthesis.active.value?.input).toMatchObject({ text: 'Pago 26/10/2026', language: 'es', normalization: 'auto', locale: 'es-PE' });
    f.audio(true);
    await f.synthesis.resume();
    expect(f.calls).toHaveLength(1);
    expect(f.synthesis.complete.value).toBe(true);
    await f.synthesis.regenerate();
    expect(f.calls).toHaveLength(2);
    expect(f.calls[1].slice(0, 4)).toEqual(['Pago 26/10/2026', 'es', 'A', false]);
    expect(f.calls[1][6]).toEqual({ normalization: 'auto', locale: 'es-PE' });
    await f.synthesis.generate();
    expect(f.calls[2].slice(0, 4)).toEqual(['Otro texto', 'en', 'A', false]);
    expect(f.calls[2][6]).toEqual({ normalization: 'off', locale: undefined });
  } finally { f.synthesis.dispose(); }
});

it('sends no pronunciation fields when the server has no normalization support', async () => {
  const f = synthesisFixture();
  try {
    f.form.pronunciation.value = undefined; f.audio(true);
    await f.synthesis.generate();
    expect(f.calls[0][6]).toEqual({ normalization: undefined, locale: undefined });
    expect(pronunciationQuery(f.calls[0][6] as never)).toEqual({});
  } finally { f.synthesis.dispose(); }
});

it('debounces runs, aborts a superseded request and invalidates its result', async () => {
  vi.useFakeTimers();
  const runner = createLatestRunner(500);
  const ran: string[] = [];
  runner.schedule(async () => { ran.push('a'); });
  await vi.advanceTimersByTimeAsync(300);
  runner.schedule(async () => { ran.push('b'); });
  await vi.advanceTimersByTimeAsync(499);
  expect(ran).toEqual([]);
  await vi.advanceTimersByTimeAsync(1);
  expect(ran).toEqual(['b']);
  let signal: AbortSignal | undefined; let current: (() => boolean) | undefined;
  runner.schedule(async (s, c) => { signal = s; current = c; await new Promise(() => {}); });
  await vi.advanceTimersByTimeAsync(500);
  expect(current?.()).toBe(true);
  runner.schedule(async () => { ran.push('c'); });
  expect(signal?.aborted).toBe(true);
  expect(current?.()).toBe(false);
  runner.cancel();
  await vi.advanceTimersByTimeAsync(1000);
  expect(ran).toEqual(['b']);
});

function previewFixture(deferred = false) {
  const resolvers: ((value: unknown) => void)[] = [];
  const signals: AbortSignal[] = [];
  const previewNormalization = vi.fn(async (_request: unknown, signal?: AbortSignal) => {
    if (signal) signals.push(signal);
    if (!deferred) return previewBody;
    return new Promise(resolve => { resolvers.push(resolve); });
  });
  const studio = {
    scopeEpoch: ref(0), token: ref('t'), info: ref({ auth: { required: true } }), normalizationAvailable: ref(true),
    text: ref('  26/10/2026 '), language: ref('es'), selectedModel: shallowRef({ id: 'm1' }),
    pronunciation: ref({ normalization: 'auto', locale: 'es-PE' }), api: { previewNormalization },
  } as unknown as Studio;
  return { studio, previewNormalization, resolvers, signals, preview: usePronunciationPreview(studio, 500) };
}

it('previews once per user action, then marks the result stale when the form changes', async () => {
  vi.useFakeTimers();
  const f = previewFixture();
  f.preview.request(); f.preview.request();
  expect(f.preview.status.value).toBe('loading');
  expect(f.previewNormalization).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(500);
  expect(f.previewNormalization).toHaveBeenCalledTimes(1);
  expect(f.previewNormalization.mock.calls[0][0]).toEqual({ text: '26/10/2026', language: 'es', mode: 'auto', locale: 'es-PE', model: 'm1' });
  expect(f.preview.status.value).toBe('ready');
  expect(f.preview.stale.value).toBe(false);
  f.studio.text.value = 'otro';
  expect(f.preview.stale.value).toBe(true);
  f.preview.dispose();
});

it('drops the response of a superseded preview', async () => {
  vi.useFakeTimers();
  const f = previewFixture(true);
  f.preview.request();
  await vi.advanceTimersByTimeAsync(500);
  f.studio.text.value = 'segundo';
  f.preview.request();
  expect(f.signals[0].aborted).toBe(true);
  await vi.advanceTimersByTimeAsync(500);
  f.resolvers[0]({ ...previewBody, spoken: 'viejo' });
  await vi.advanceTimersByTimeAsync(0);
  expect(f.preview.result.value).toBeNull();
  f.resolvers[1]({ ...previewBody, spoken: 'nuevo' });
  await vi.advanceTimersByTimeAsync(0);
  expect(f.preview.result.value?.spoken).toBe('nuevo');
  f.preview.dispose();
});

it('reports empty text without a request, and a 400 rejection with its status', async () => {
  vi.useFakeTimers();
  const f = previewFixture();
  f.studio.text.value = '   ';
  f.preview.request();
  await vi.advanceTimersByTimeAsync(1000);
  expect(f.preview.status.value).toBe('empty');
  expect(f.previewNormalization).not.toHaveBeenCalled();
  f.studio.text.value = 'x'.repeat(5);
  f.previewNormalization.mockRejectedValueOnce(new ApiError(400, 'too long'));
  f.preview.request();
  await vi.advanceTimersByTimeAsync(500);
  expect(f.preview.status.value).toBe('error');
  expect(f.preview.error.value).not.toBeNull();
  expect(f.preview.errorStatus.value).toBe(400);
  expect(isPreviewStale(null, previewKey({ text: 'a', language: 'es' }))).toBe(false);
  f.preview.dispose();
});
