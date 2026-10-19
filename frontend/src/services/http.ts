export type ApiFailureKind = 'http' | 'network' | 'offline' | 'timeout' | 'cancelled' | 'invalid_json' | 'content_type' | 'invalid_response';
import type { Json } from './contracts';
export interface FieldIssue { field: string; code: string; message: string; expected?: Json; received?: Json }
export interface ErrorDetails { code?: string; requestId?: string; retryAfter?: number; fields?: FieldIssue[]; kind?: ApiFailureKind }
export class ApiError extends Error {
  readonly status: number;
  readonly code?: string;
  readonly requestId?: string;
  readonly retryAfter?: number;
  readonly fields?: FieldIssue[];
  readonly kind: ApiFailureKind;
  constructor(status: number, message: string, details: ErrorDetails = {}) {
    super(message); this.name = 'ApiError'; this.status = status;
    this.code = details.code; this.requestId = details.requestId; this.retryAfter = details.retryAfter; this.fields = details.fields;
    this.kind = details.kind ?? 'http';
  }
}
export interface HttpOptions extends RequestInit { timeoutMs?: number }
function defaultTimeout(path: string): number {
  if (path.endsWith('/events') || path.endsWith('/preview') || path === '/api/tts/play') return 0;
  if (path.includes('/clones') || path.endsWith('/audio')) return 120000;
  return 15000;
}
export function retryDelay(value: string | null, now = Date.now()): number | undefined {
  if (!value) return;
  const seconds = Number(value);
  if (Number.isFinite(seconds) && seconds >= 0) return seconds;
  const date = Date.parse(value);
  return Number.isFinite(date) ? Math.max(0, Math.ceil((date - now) / 1000)) : undefined;
}
export function createHttpClient(token: () => string = () => '', authRequired: () => boolean = () => true, sessionSignal?: () => AbortSignal) {
  const cooldowns = new Map<string, number>();
  let lastIdentity = token();
  async function perform<T>(path: string, options: HttpOptions, consume: (result: Response) => Promise<T>): Promise<T> {
    const identity = token();
    for (const [key, deadline] of cooldowns) if (deadline <= Date.now()) cooldowns.delete(key);
    if (identity !== lastIdentity) { cooldowns.clear(); lastIdentity = identity; }
    const remaining = Math.ceil(((cooldowns.get(path) ?? 0) - Date.now()) / 1000);
    if (remaining > 0) throw new ApiError(429, 'Wait before retrying this request.', { code: 'too_many_requests', retryAfter: remaining });
    const headers = new Headers(options.headers);
    if (authRequired() && identity.trim()) headers.set('Authorization', `Bearer ${identity.trim()}`);
    const { timeoutMs = defaultTimeout(path), ...init } = options;
    const timeout = new AbortController();
    const signals = [timeout.signal, options.signal, sessionSignal?.()].filter((value): value is AbortSignal => !!value);
    const signal = AbortSignal.any(signals);
    const timedOut = () => timeout.signal.aborted || (signal.reason instanceof DOMException && signal.reason.name === 'TimeoutError');
    const timer = timeoutMs > 0 ? setTimeout(() => timeout.abort(new DOMException('Request timed out', 'TimeoutError')), timeoutMs) : undefined;
    function checkSession() { if (signal.aborted || identity !== token()) throw new ApiError(0, 'Request cancelled.', { kind: timedOut() ? 'timeout' : 'cancelled' }); }
    try {
      checkSession();
      const result = await fetch(path, { credentials: 'same-origin', ...init, signal, headers });
      checkSession();
      if (!result.ok) {
        const text = await result.text();
        let message = text || `HTTP ${result.status}`;
        let details: ErrorDetails = {};
        try {
          const body: unknown = JSON.parse(text);
          if (body && typeof body === 'object') {
            const value = body as Record<string, unknown>;
            if (typeof value.message === 'string') message = value.message;
            else if (typeof value.error === 'string') message = value.error;
            details = { code: typeof value.code === 'string' ? value.code : typeof value.error === 'string' ? value.error : undefined,
              requestId: typeof value.request_id === 'string' ? value.request_id : undefined,
              fields: Array.isArray(value.fields) ? value.fields.filter((item): item is FieldIssue => !!item && typeof item === 'object' && typeof item.field === 'string' && typeof item.code === 'string' && typeof item.message === 'string') : undefined,
              retryAfter: typeof value.retry_after_secs === 'number' ? value.retry_after_secs : undefined };
          }
        } catch { /* The server may return a plain text rejection. */ }
        details.retryAfter = retryDelay(result.headers.get('Retry-After')) ?? details.retryAfter;
        if (result.status === 429 && details.retryAfter && Number.isFinite(details.retryAfter)) cooldowns.set(path, Date.now() + details.retryAfter * 1000);
        throw new ApiError(result.status, message, details);
      }
      const value = await consume(result);
      checkSession();
      return value;
    } catch (error) {
      if (error instanceof ApiError) throw error;
      if (signal.aborted || identity !== token()) throw new ApiError(0, timedOut() ? 'Request timed out. Try again.' : 'Request cancelled.', { kind: timedOut() ? 'timeout' : 'cancelled' });
      throw new ApiError(0, typeof navigator !== 'undefined' && navigator.onLine === false ? 'You are offline. Reconnect and try again.' : 'Network request failed. Try again.', { kind: typeof navigator !== 'undefined' && navigator.onLine === false ? 'offline' : 'network' });
    } finally { if (timer !== undefined) clearTimeout(timer); }
  }
  return {
    response: (path: string, options: HttpOptions = {}) => perform(path, options, async result => result),
    json: <T>(path: string, options: HttpOptions = {}): Promise<T> => perform(path, options, async result => {
      const type = result.headers.get('Content-Type')?.split(';')[0]?.trim().toLowerCase();
      if (type !== 'application/json' && !type?.endsWith('+json')) throw new ApiError(result.status, 'The server returned an unexpected content type.', { kind: 'content_type' });
      try { return await result.json() as T; }
      catch (error) {
        if (error instanceof SyntaxError) throw new ApiError(result.status, 'The server returned invalid JSON.', { kind: 'invalid_json' });
        throw error;
      }
    }),
    blob: (path: string, options: HttpOptions = {}): Promise<Blob> => perform(path, options, async result => {
      if (!result.headers.get('Content-Type')?.toLowerCase().startsWith('audio/')) throw new ApiError(result.status, 'The server returned an unexpected content type.', { kind: 'content_type' });
      return result.blob();
    }),
  };
}
export type HttpClient = ReturnType<typeof createHttpClient>;
export const jsonBody = (body: unknown): HttpOptions => ({ headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
