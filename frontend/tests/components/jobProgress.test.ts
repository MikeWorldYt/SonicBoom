import { afterEach, it, expect, vi } from 'vitest';
import { monitorJob } from '../../src/services/jobs';
import { ApiError } from '../../src/services/http';
afterEach(() => vi.useRealTimers());
const stream = (data: string) => new Response('data: ' + data + '\n\n', { headers: { 'Content-Type': 'text/event-stream' } });
it('reconciles the same job after disconnect without creating another job', async () => {
  vi.useFakeTimers();
  const api = { job: vi.fn().mockResolvedValueOnce({ id: 'one', stage: 'generating' }).mockResolvedValueOnce({ id: 'one', stage: 'completed' }), jobEvents: vi.fn(async () => { throw new TypeError('Disconnected'); }), createJob: vi.fn() };
  const pending = monitorJob(api as any, 'one', () => {}, new AbortController().signal);
  await vi.advanceTimersByTimeAsync(1001);
  expect((await pending).stage).toBe('completed');
  expect(api.job).toHaveBeenNthCalledWith(2, 'one', expect.any(AbortSignal));
  expect(api.createJob).not.toHaveBeenCalled();
});
it('validates SSE JSON and rejects foreign job snapshots without retrying', async () => {
  for (const [payload, kind] of [['{', 'invalid_json'], [JSON.stringify({ id: 'foreign', stage: 'completed' }), 'invalid_response'], [JSON.stringify({ id: 'one', stage: [] }), 'invalid_response']]) {
    const api = { job: vi.fn(async () => ({ id: 'one', stage: 'generating' })), jobEvents: vi.fn(async () => stream(payload)) };
    await expect(monitorJob(api as any, 'one', () => {}, new AbortController().signal)).rejects.toMatchObject({ kind });
    expect(api.job).toHaveBeenCalledTimes(1);
  }
});
it('does not loop on expired authentication or throttling and preserves Retry-After', async () => {
  for (const status of [401, 429]) {
    const api = { job: vi.fn(async () => { throw new ApiError(status, 'Rejected', { retryAfter: 30 }); }) };
    await expect(monitorJob(api as any, 'one', () => {}, new AbortController().signal)).rejects.toMatchObject({ status, retryAfter: 30 });
    expect(api.job).toHaveBeenCalledTimes(1);
  }
});
