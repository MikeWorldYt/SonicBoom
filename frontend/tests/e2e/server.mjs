// Test-only static server: actual production assets and the unmodified Rust CSP.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { randomBytes } from 'node:crypto';
import { resolve, extname } from 'node:path';
const root = resolve('dist'), template = await readFile(resolve(root, 'index.html'), 'utf8');
const security = await readFile('../src/security.rs', 'utf8');
const policyTemplate = [...security.matchAll(/"(default-src 'self';[^"\n]+)"/g)].map(match => match[1]).find(value => !value.includes("'unsafe-inline'"));
if (!policyTemplate) throw new Error('Production CSP could not be found in src/security.rs');
const escape = value => value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;').replaceAll("'", '&#39;');
const server = createServer(async (request, response) => {
  const url = new URL(request.url || '/', 'http://127.0.0.1:5174');
  if (url.pathname === '/' || url.pathname === '/tests/e2e/index.html') {
    const view = url.searchParams.get('view');
    // The server hands every page a fresh CSP nonce; the page carries it.
    const nonce = randomBytes(16).toString('hex');
    const data = view === 'setup' ? { page: 'setup' } : view === 'admin' ? { page: 'admin', csrf: 'test-csrf', tokens: [] } : {
      page: 'home', modelReady: true, voices: ['builtin'], status: 'ready', providers: [],
    };
    const policy = policyTemplate.replace('{nonce}', nonce);
    const body = template
      .replace('SONICBOOM_PAGE_DATA', escape(JSON.stringify(data)))
      .replace('SONICBOOM_CSP_NONCE', nonce);
    response.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Content-Security-Policy': policy, 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff', 'X-Frame-Options': 'DENY' });
    response.end(body);
    return;
  }
  const path = resolve(root, '.' + url.pathname);
  if (url.pathname.startsWith('/assets/') && path.startsWith(root + '/assets/')) {
    try {
      const bytes = await readFile(path);
      const type = { '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.woff2': 'font/woff2' }[extname(path)] || 'application/octet-stream';
      response.writeHead(200, { 'Content-Type': type, 'Cache-Control': 'public, max-age=31536000, immutable', 'X-Content-Type-Options': 'nosniff' });
      response.end(bytes); return;
    } catch { /* Unknown asset is an ordinary 404. */ }
  }
  response.writeHead(404); response.end();
});
server.listen(5174, '127.0.0.1');
function close() { server.closeAllConnections(); server.close(() => process.exit(0)); }
process.on('SIGTERM', close); process.on('SIGINT', close);
