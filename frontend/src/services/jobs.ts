import type { Api } from './api';
import type { Job } from './contracts';
import { ApiError } from './http';
/** Always reconcile a snapshot after a dropped SSE connection. Never create a replacement job. */
export async function monitorJob(api: Api, id: string, onSnapshot: (job: Job) => void, signal: AbortSignal): Promise<Job> {
  let failures = 0;
  const terminal = (job: Job) => ['completed', 'failed', 'cancelled'].includes(job.stage);
  while (!signal.aborted && failures < 5) {
    try {
      const timeout = AbortSignal.any([signal, AbortSignal.timeout(15000)]);
      const snapshot = await api.job(id, timeout);
      onSnapshot(snapshot);
      if (terminal(snapshot)) return snapshot;
      const response = await api.jobEvents(id, AbortSignal.any([signal, AbortSignal.timeout(30000)]));
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
            const event = pending.slice(0, boundary); pending = pending.slice(boundary + 2);
            const data = event.split('\n').filter(line => line.startsWith('data:')).map(line => line.slice(5).trim()).join('\n');
            if (data) { const job = JSON.parse(data) as Job; failures = 0; onSnapshot(job); if (terminal(job)) return job; }
          }
          if (pending.length > 65536) throw new Error('Invalid progress response.');
        }
      } finally { await reader.cancel().catch(() => {}); reader.releaseLock(); }
    } catch (error) {
      if (signal.aborted) throw signal.reason;
      if (error instanceof ApiError && error.status === 404) throw error;
      failures++;
    }
    await new Promise<void>(resolve => { const timer = setTimeout(done, 1000); function done() { clearTimeout(timer); signal.removeEventListener('abort', done); resolve(); } signal.addEventListener('abort', done, { once: true }); });
  }
  if (signal.aborted) throw signal.reason;
  throw new Error('Progress connection lost. The job may still be running; reconnect to check its status.');
}
