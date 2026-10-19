import { afterEach, it, expect, vi } from 'vitest';
import { createHttpClient, ApiError } from '../../src/services/http';
import { createApi } from '../../src/services/api';
afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers(); });
it('rejects wrong response shapes without echoing private values', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => Response.json({ data: [{ transcript: 'secret', owner_token_hash: 'private' }] })));
  await expect(createApi(createHttpClient()).voices()).rejects.toMatchObject({ kind: 'invalid_response' });
  try { await createApi(createHttpClient()).voices(); } catch (error) { expect(String(error)).not.toContain('secret'); }
});
it('distinguishes content type, malformed JSON and network failures', async () => {
  const client = createHttpClient();
  vi.stubGlobal('fetch', vi.fn(async () => new Response('<html>', { headers: { 'Content-Type': 'text/html' } })));
  await expect(client.json('/info')).rejects.toMatchObject({ kind: 'content_type' });
  vi.stubGlobal('fetch', vi.fn(async () => new Response('{', { headers: { 'Content-Type': 'application/json' } })));
  await expect(client.json('/info')).rejects.toMatchObject({ kind: 'invalid_json' });
  vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('fetch failed'); }));
  await expect(client.json('/info')).rejects.toBeInstanceOf(ApiError);
});
it('times out metadata, but does not set an inference deadline', async () => {
  vi.useFakeTimers();
  vi.stubGlobal('fetch', vi.fn((_path, options) => new Promise((_resolve, reject) => options.signal.addEventListener('abort', () => reject(options.signal.reason)))));
  const request = createHttpClient().json('/api/info');
  const assertion = expect(request).rejects.toMatchObject({ kind: 'timeout' });
  await vi.advanceTimersByTimeAsync(15001); await assertion;
  const controller = new AbortController();
  const preview = createHttpClient().response('/api/voices/id/preview', { signal: controller.signal });
  const cancelled = expect(preview).rejects.toMatchObject({ kind: 'cancelled' });
  await vi.advanceTimersByTimeAsync(120000);
  expect(controller.signal.aborted).toBe(false);
  controller.abort(); await cancelled;
});
it('respects Retry-After without network retry storms, isolated by token', async () => {
  vi.useFakeTimers();
  let token = 'first';
  const fetch = vi.fn(async () => Response.json({ error: 'too_many_requests', message: 'Wait', request_id: 'trace' }, { status: 429, headers: { 'Retry-After': '30' } }));
  vi.stubGlobal('fetch', fetch);
  const client = createHttpClient(() => token);
  await expect(client.json('/voices')).rejects.toMatchObject({ retryAfter: 30, requestId: 'trace' });
  await expect(client.json('/voices')).rejects.toMatchObject({ status: 429 });
  expect(fetch).toHaveBeenCalledTimes(1);
  token = 'second';
  await expect(client.json('/voices')).rejects.toMatchObject({ status: 429 });
  expect(fetch).toHaveBeenCalledTimes(2);
});
it('rejects late data from an old token even when a mock ignores abort', async () => {
  let token = 'first', resolve!: (value: Response) => void;
  vi.stubGlobal('fetch', vi.fn(() => new Promise<Response>(done => { resolve = done; })));
  const request = createHttpClient(() => token).json('/voices');
  const assertion = expect(request).rejects.toMatchObject({ kind: 'cancelled' });
  token = 'second'; resolve(Response.json({ data: ['foreign'] })); await assertion;
});

it('retains typed field issues and displays action, retry delay and correlation details', async () => {
  const { errorMessage, renderMessage } = await import('../../src/i18n');
  vi.stubGlobal('fetch', vi.fn(async () => Response.json({ code: 'bad_request', message: 'Invalid reference', request_id: 'trace-field', fields: [{ field: 'transcript', code: 'too_long', message: 'Too long', expected: 10000, received: 10001 }] }, { status: 400 })));
  try { await createHttpClient().json('/clone'); }
  catch (error) {
    expect(error).toMatchObject({ fields: [{ field: 'transcript', code: 'too_long', expected: 10000 }] });
    expect(renderMessage(errorMessage(error))).toContain('trace-field');
  }
  const failure = new ApiError(413, 'Exceeded cap', { requestId: 'trace-size' });
  expect(renderMessage(errorMessage(failure))).toContain('smaller recording');
  vi.stubGlobal('fetch', vi.fn(async () => new Response('<html>', { headers: { 'Content-Type': 'text/html' } })));
  await expect(createHttpClient().blob('/audio')).rejects.toMatchObject({ kind: 'content_type' });
});
