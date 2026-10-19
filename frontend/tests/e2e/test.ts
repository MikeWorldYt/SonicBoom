import { test as base, expect, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
export { expect };
/** Pages where an inline style is tolerated.
 *
 * axe-core copies the stylesheets of every shadow root it analyses into the
 * document head. The production CSP blocks inline styles, so that copy is
 * refused and reported: an artefact of the audit tool, not of the app. The
 * app's own shadow stylesheet carries the page nonce and always loads. */
const inlineStyles = new WeakSet<Page>();
export const test = base.extend({
  page: async ({ page }, use) => {
    const errors: string[] = [];
    let policy = '';
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => {
      if (message.type() !== 'error') return;
      const text = message.text();
      if (inlineStyles.has(page) && /Applying inline style/.test(text)) return;
      if (/Content.Security.Policy|Refused.*(?:style|script)|violat.*(?:script|style)-src/i.test(text)) errors.push(text);
    });
    page.on('response', response => { if (response.request().isNavigationRequest()) policy = response.headers()['content-security-policy'] || ''; });
    await use(page);
    expect(policy).toContain("style-src 'self'");
    expect(policy).not.toContain("'unsafe-inline'");
    expect(errors).toEqual([]);
  },
});
/** Run axe with the shadow stylesheet copy it makes tolerated. */
export async function analyze(page: Page) {
  inlineStyles.add(page);
  try { return await new AxeBuilder({ page }).analyze(); }
  finally { inlineStyles.delete(page); }
}
