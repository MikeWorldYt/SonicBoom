/** Per-response Content-Security-Policy nonce.
 *
 * The audio player renders its waveform inside a shadow root that ships one
 * inline stylesheet. The server hands every page a nonce so that single
 * audited style loads without opening `style-src` to 'unsafe-inline'. The
 * value lives in a meta tag, so it survives code splitting. */
export function cspNonce(): string {
  if (typeof document === 'undefined') return '';
  return document.querySelector<HTMLMetaElement>('meta[name="csp-nonce"]')?.content ?? '';
}
