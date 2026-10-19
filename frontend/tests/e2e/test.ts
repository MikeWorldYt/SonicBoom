import { test as base, expect } from '@playwright/test';
export { expect };
export const test = base.extend({
  page: async ({ page }, use) => {
    const errors: string[] = [];
    let policy = '';
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => {
      if (message.type() === 'error' && /Content.Security.Policy|Refused.*(?:style|script)|violat.*(?:script|style)-src/i.test(message.text())) errors.push(message.text());
    });
    page.on('response', response => { if (response.request().isNavigationRequest()) policy = response.headers()['content-security-policy'] || ''; });
    await use(page);
    expect(policy).toContain("style-src 'self'");
    expect(policy).not.toContain("'unsafe-inline'");
    expect(errors).toEqual([]);
  },
});
