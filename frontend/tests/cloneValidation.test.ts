import { test } from 'node:test';
import assert from 'node:assert/strict';
import { validateReference } from '../src/features/cloning/validateReference.ts';
import { createApi } from '../src/services/api.ts';
import { createHttpClient, ApiError } from '../src/services/http.ts';
const limits = { version: 1, max_upload_bytes: 1000000, min_sample_rate: 8000, max_sample_rate: 192000, max_channels: 2, name_max_chars: 64, transcript_max_chars: 10000, providers: { pocket: { duration_seconds: [1, 30] as [number, number], clone_modes: ['audio_prompt'], models: ['pocket'], languages: ['en'], transcript_required_for: [] } } };
function wav(seconds: number, channels = 1, rate = 8000) {
  const bytes = new ArrayBuffer(44 + seconds * rate * channels * 2);
  const view = new DataView(bytes);
  const tag = (offset: number, value: string) => [...value].forEach((c, i) => view.setUint8(offset + i, c.charCodeAt(0)));
  tag(0, 'RIFF'); view.setUint32(4, bytes.byteLength - 8, true); tag(8, 'WAVE'); tag(12, 'fmt ');
  view.setUint32(16, 16, true); view.setUint16(20, 1, true); view.setUint16(22, channels, true);
  view.setUint32(24, rate, true); view.setUint32(28, rate * channels * 2, true);
  view.setUint16(32, channels * 2, true); view.setUint16(34, 16, true); tag(36, 'data'); view.setUint32(40, bytes.byteLength - 44, true);
  return new File([bytes], 'test.wav');
}
test('reference duration, rate, channel and byte boundaries', async () => {
  for (const duration of [1, 5, 30]) assert.equal((await validateReference(wav(duration), limits, 'pocket')).duration, duration);
  for (const file of [wav(0.5), wav(31), wav(1, 3), wav(1, 1, 4000), new File(['RIFF'], 'bad.wav')]) await assert.rejects(validateReference(file, limits, 'pocket'));
  const file = wav(1);
  await validateReference(file, { ...limits, max_upload_bytes: file.size }, 'pocket');
  await assert.rejects(validateReference(file, { ...limits, max_upload_bytes: file.size - 1 }, 'pocket'));
});
test('clone sends private metadata only in multipart body', async () => {
  const original = globalThis.fetch;
  globalThis.fetch = async (url, options) => {
    assert.equal(url, '/api/voices/clones');
    assert.ok(options?.body instanceof FormData);
    assert.equal(new Headers(options?.headers).has('Content-Type'), false);
    const part = options.body.get('metadata') as Blob;
    assert.equal(JSON.parse(await part.text()).transcript, 'private words');
    return Response.json({ id: 'voice', name: 'test', provider: 'pocket', model: 'pocket', language: 'en', clone_mode: 'audio_prompt', warnings: [], defects: [], guidance: [], reference_health: { duration_secs: 1, source_channels: 1, source_sample_rate: 8000, peak: 0, rms: 0, clipping_fraction: 0, low_amplitude_fraction: 1, non_finite_samples: 0 } });
  };
  try {
    await createApi(createHttpClient()).createVoice({ name: 'test', provider: 'pocket', model: 'pocket', language: 'en', mode: 'audio_prompt', consent_basis: 'owner', transcript: 'private words' }, wav(1));
  } finally { globalThis.fetch = original; }
});
test('structured errors retain correlation and retry advice', async () => {
  const original = globalThis.fetch;
  globalThis.fetch = async () => Response.json({ error: 'too_many_requests', message: 'Wait', request_id: 'trace' }, { status: 429, headers: { 'Retry-After': '3' } });
  try {
    await assert.rejects(createHttpClient().response('/test'), (error: unknown) => error instanceof ApiError && error.requestId === 'trace' && error.retryAfter === 3 && error.code === 'too_many_requests');
  } finally { globalThis.fetch = original; }
});

test('constraints boundary rejects unknown versions and invalid values', async () => {
  const { parseConstraints } = await import('../src/features/cloning/constraints.ts');
  assert.deepEqual(parseConstraints(limits), limits);
  for (const value of [null, {}, { ...limits, version: 2 }, { ...limits, max_upload_bytes: -1 }, { ...limits, providers: { pocket: {} } }]) assert.throws(() => parseConstraints(value));
});
