export const setupPolicy = { version: 1, password_min_chars: 12, rejected_passwords: ['example-password'], max_inference_steps: 50, auth_modes: ['local', 'token', 'none'], allow_insecure_remote: false };
import type { Page } from '@playwright/test';
export function wav(seconds = 1) {
  const buffer = Buffer.alloc(44 + seconds * 8000 * 2);
  buffer.write('RIFF', 0); buffer.writeUInt32LE(buffer.length - 8, 4); buffer.write('WAVEfmt ', 8); buffer.writeUInt32LE(16, 16);
  buffer.writeUInt16LE(1, 20); buffer.writeUInt16LE(1, 22); buffer.writeUInt32LE(8000, 24); buffer.writeUInt32LE(16000, 28);
  buffer.writeUInt16LE(2, 32); buffer.writeUInt16LE(16, 34); buffer.write('data', 36); buffer.writeUInt32LE(buffer.length - 44, 40); return buffer;
}
export async function studioFixtures(page: Page) {
  const voices: any[] = [];
  let revision = 1, jobs = 0, patches = 0;
  const config = { model: { inference_steps: 5 }, voices: { max_upload_bytes: 8388608 }, providers: { pocket_tts: { enabled: true, model_id: 'pocket-tts-en' } } };
  const sampling = { provider: 'qwen3-gguf', seed_max: '18446744073709551615', temperature: { min: 0, max: 5 }, top_k: { min: 0, max: 2048, integer: true }, top_p: { min: 0, max: 1, exclusive_min: true }, repetition_penalty: { min: 1, max: 4 } };
  const schema = [
    { path: 'model.inference_steps', description: 'Inference steps', type: 'integer', apply: 'hot', min: 1, max: 50, nullable: false, secret: false },
    { path: 'voices.max_upload_bytes', description: 'Maximum voice reference upload size in bytes', type: 'integer', apply: 'hot', min: 1024, max: 67108864, nullable: false, secret: false },
    { path: 'providers.pocket_tts.enabled', description: 'Enabled', type: 'boolean', apply: 'restart_subsystem', nullable: false, secret: false },
    { path: 'providers.pocket_tts.model_id', description: 'Model', type: 'string', apply: 'reload_model', allowed_values: ['pocket-tts-en'], nullable: false, secret: false },
  ];
  const requests: { url: string; body?: string }[] = [];
  await page.route(/\/(api|v1)\//, async route => {
    const request = route.request(), path = new URL(request.url()).pathname;
    const json = (body: unknown, status = 200) => route.fulfill({ status, json: body });
    if (path === '/api/info') return json({ version: 'test', mode: 'server', setup_complete: true, auth: { mode: 'local', required: false }, features: { playback: false, audio_devices: false, hot_reload: true }, config_revision: revision, model: { status: 'ready' }, synthesis: { max_text_chars: 1000, max_body_bytes: 10000, sampling } });
    if (path === '/api/providers') return json({ providers: [{ provider_id: 'pocket-tts', enabled: true, state: 'ready', active_model: 'pocket-tts-en', effective_device: 'cpu', detail: null }], capabilities: [{ provider_id: 'pocket-tts', models: ['pocket-tts-en'], languages: ['en'], devices: ['cpu'], clone_modes: ['audio_prompt'], requires_reference_audio: true, requires_reference_transcript: false, sample_rate: 24000 }] });
    if (path === '/v1/voices') return json({ data: [{ id: 'builtin', name: 'Built-in speaker' }] });
    if (path === '/api/audio/devices') return json({ devices: [] });
    if (path === '/api/config/status') return json({ revision, config_path: '/test/config.toml', watcher: 'active', last_reload: null, last_error: null, pending_applies: [], apply_failures: [] });
    if (path === '/api/config/schema') return json({ settings: schema });
    if (path === '/api/config' && request.method() === 'GET') return json({ revision, config });
    if (path === '/api/config' && request.method() === 'PATCH') {
      patches++;
      config.model.inference_steps = 7; revision++;
      return json({ error: 'conflict', message: 'Revision changed', status: 409, request_id: 'test-config-conflict' }, 409);
    }
    if (path === '/api/voices/constraints') return json({ version: 1, max_upload_bytes: config.voices.max_upload_bytes, min_sample_rate: 8000, max_sample_rate: 192000, max_channels: 2, name_max_chars: 64, transcript_max_chars: 10000, providers: { 'pocket-tts': { duration_seconds: [1, 30], clone_modes: ['audio_prompt'], models: ['pocket-tts-en'], languages: ['en'], transcript_required_for: [] } } });
    if (path === '/api/voices') return json({ data: voices });
    if (path === '/api/voices/clones') {
      const body = request.postDataBuffer()?.toString('utf8') || '';
      requests.push({ url: request.url(), body });
      const metadata = JSON.parse(body.match(/\r\n\r\n(\{[^\r\n]+\})\r\n/)?.[1] || '{}');
      const voice = { id: 'voice-1', name: metadata.name, provider: metadata.provider, model: metadata.model, language: metadata.language, clone_mode: metadata.mode, reference_duration_secs: 1, reference_sample_rate: 8000, created_at: '2026-10-19T00:00:00Z' };
      voices.push(voice);
      return json({ ...voice, warnings: ['reference_nearly_silent'], defects: [], guidance: [], reference_health: { duration_secs: 1, source_channels: 1, source_sample_rate: 8000, peak: 0, rms: 0, clipping_fraction: 0, low_amplitude_fraction: 1, non_finite_samples: 0 } }, 201);
    }
    if (path === '/api/voices/voice-1' && request.method() === 'DELETE') { voices.splice(0); return json({ deleted: 'voice-1' }); }
    if (path === '/api/tts/jobs') { jobs++; return json({ id: 'job-' + jobs }, 202); }
    if (/\/api\/tts\/jobs\/[^/]+\/audio$/.test(path)) return route.fulfill({ body: wav(), contentType: 'audio/wav' });
    if (/\/api\/tts\/jobs\/[^/]+$/.test(path)) return json({ id: path.split('/').pop(), stage: 'completed', audio_secs: 1, elapsed_secs: 1, frames: 1, context: { provider: 'supertonic', model: 'supertonic-3', voice: 'builtin', effective_device: 'cpu' } });
    return json({ error: 'not_found', message: path }, 404);
  });
  return { requests, jobs: () => jobs, patches: () => patches };
}
