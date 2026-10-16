export class ApiError extends Error {
  readonly status: number;
  // Plain field assignment (no parameter property) so the class also loads
  // under node's strip-only TypeScript mode used by frontend tests.
  constructor(status: number, message: string) { super(message); this.name = 'ApiError'; this.status = status; }
}
export function createHttpClient(token: () => string = () => '', authRequired: () => boolean = () => true) {
  async function response(path: string, options: RequestInit = {}): Promise<Response> {
    const headers = new Headers(options.headers);
    if (authRequired() && token().trim()) headers.set('Authorization', `Bearer ${token().trim()}`);
    const result = await fetch(path, { credentials: 'same-origin', ...options, headers });
    if (!result.ok) {
      const text = await result.text();
      let message = text || `HTTP ${result.status}`;
      try { const body = JSON.parse(text) as { message?: string; error?: string }; message = body.message || body.error || message; } catch { /* Plain text errors are valid API responses. */ }
      throw new ApiError(result.status, message);
    }
    return result;
  }
  return {
    response,
    async json<T>(path: string, options?: RequestInit): Promise<T> { return (await response(path, options)).json() as Promise<T>; },
    async blob(path: string, options?: RequestInit): Promise<Blob> { return (await response(path, options)).blob(); },
  };
}
export type HttpClient = ReturnType<typeof createHttpClient>;
export const jsonBody = (body: unknown): RequestInit => ({ headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
