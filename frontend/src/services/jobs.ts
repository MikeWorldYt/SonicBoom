import type { Api } from './api';
import type { Job } from './contracts';
import { jobSchema, parseResponse } from './responseSchemas';
import { ApiError } from './http';
/** Always reconcile a snapshot after a dropped SSE connection. Never create a replacement job. */
export async function monitorJob(api: Api, id: string, onSnapshot: (job: Job) => void, signal: AbortSignal): Promise<Job> {
  let failures = 0;
  function sameJob(job: Job) { if (job.id && job.id !== id) throw new ApiError(0, 'The server response does not match the expected format.', { kind: 'invalid_response' }); return job; }
  const terminal = (job: Job) => ['completed', 'failed', 'cancelled'].includes(job.stage);
  while (!signal.aborted && failures < 5) {
    try {
      const timeout = AbortSignal.any([signal, AbortSignal.timeout(15000)]);
      const snapshot = sameJob(await api.job(id, timeout));
      onSnapshot(snapshot);
      if (terminal(snapshot)) return snapshot;
      const response = await api.jobEvents(id, AbortSignal.any([signal, AbortSignal.timeout(30000)]));
      if (!response.headers.get('Content-Type')?.includes('text/event-stream')) throw new ApiError(response.status, 'The server returned an unexpected content type.', { kind: 'content_type' });
      if (!response.body) throw new Error('Invalid progress response.');
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let pending = '';
      try {
        while (!signal.aborted) {
          const { value, done } = await reader.read();
          if (done) break;
          pending += decoder.decode(value, { stream: true }).replace(/\r/g, '');
          let boundary: number;
          while ((boundary = pending.indexOf('\n\n')) >= 0) {
            if (boundary > 65536) throw new ApiError(0, 'Invalid progress response.', { kind: 'invalid_response' });
            const event = pending.slice(0, boundary); pending = pending.slice(boundary + 2);
            const data = event.split('\n').filter(line => line.startsWith('data:')).map(line => line.slice(5).trim()).join('\n');
            if (data) {
              let decoded: unknown;
              try { decoded = JSON.parse(data); }
              catch { throw new ApiError(0, 'The server returned invalid JSON.', { kind: 'invalid_json' }); }
              const job = sameJob(parseResponse(jobSchema, decoded));
              failures = 0; onSnapshot(job); if (terminal(job)) return job;
            }
          }
          if (pending.length > 65536) throw new Error('Invalid progress response.');
        }
      } finally { await reader.cancel().catch(() => {}); reader.releaseLock(); }
    } catch (error) {
      if (signal.aborted) throw signal.reason;
      if (error instanceof ApiError && ([400, 401, 403, 404, 429].includes(error.status) || ['invalid_response', 'content_type', 'invalid_json'].includes(error.kind))) throw error;
      failures++;
    }
    await new Promise<void>(resolve => { const timer = setTimeout(done, 1000); function done() { clearTimeout(timer); signal.removeEventListener('abort', done); resolve(); } signal.addEventListener('abort', done, { once: true }); });
  }
  if (signal.aborted) throw signal.reason;
  throw new Error('Progress connection lost. The job may still be running; reconnect to check its status.');
}
