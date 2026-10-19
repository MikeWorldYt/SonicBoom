import { test, expect, analyze } from './test';
import { setupPolicy, studioFixtures } from './studioFixtures';
test('admin expiry explains UTC and keyboard validation blocks past dates', async ({ page }) => {
  await page.goto('/tests/e2e/index.html?view=admin');
  await page.getByLabel('Expires (UTC)').fill('2000-01-01T00:00');
  await page.getByRole('button', { name: 'Generate Token' }).click();
  await expect(page.getByRole('alert')).toContainText('future date in UTC');
  await expect(page.getByLabel('Expires (UTC)')).toBeFocused();
  const results = await analyze(page);
  expect(results.violations).toEqual([]);
});
test('setup navigates using keyboard and exposes accessible fields', async ({ page }) => {
  await page.route('**/setup/**', async route => {
    const path = new URL(route.request().url()).pathname;
    await route.fulfill({ json: path.endsWith('/complete') ? { api_url: 'http://127.0.0.1:17842', audio_device: 'default', auth: 'token', api_token: 'one-time-secret' } : path.includes('devices') ? { devices: [] } : { model: { cache_dir: '' }, server: { port: 17842 }, constraints: setupPolicy } });
  });
  await page.goto('/tests/e2e/index.html?view=setup');
  await page.getByRole('button', { name: 'Continue' }).focus();
  await page.keyboard.press('Enter');
  await expect(page.getByLabel('Output device', { exact: false })).toBeVisible();
  const results = await analyze(page);
  expect(results.violations).toEqual([]);
});

test('setup retains the one-time token until acknowledgement', async ({ page }) => {
  await page.route('**/setup/**', async route => {
    const path = new URL(route.request().url()).pathname;
    await route.fulfill({ json: path.endsWith('/complete') ? { api_url: 'http://127.0.0.1:17842', audio_device: 'default', auth: 'token', api_token: 'one-time-secret' } : path.includes('devices') ? { devices: [] } : { model: { cache_dir: '' }, server: { port: 17842 }, constraints: setupPolicy } });
  });
  await page.goto('/tests/e2e/index.html?view=setup');
  for (let i = 0; i < 3; i++) await page.getByRole('button', { name: 'Continue', exact: true }).click();
  await page.getByLabel('Admin password', { exact: true }).fill('a-long-valid-passphrase');
  await page.getByRole('button', { name: 'Complete Setup' }).click();
  await expect(page.locator('#setup-success')).toBeFocused();
  await expect(page.getByRole('button', { name: 'Open studio' })).toBeDisabled();
  await page.getByRole('button', { name: 'Reveal token' }).click();
  await expect(page.getByLabel('API token', { exact: true })).toHaveValue('one-time-secret');
  expect(await page.evaluate(() => Object.values(localStorage).join(' '))).not.toContain('one-time-secret');
  await page.getByRole('checkbox', { name: 'I have saved my token safely.' }).check();
  await expect(page.getByRole('button', { name: 'Open studio' })).toBeEnabled();
  expect((await analyze(page)).violations).toEqual([]);
});

test('setup without a one-time token redirects to the studio', async ({ page }) => {
  await studioFixtures(page);
  await page.route('**/setup/**', async route => {
    const path = new URL(route.request().url()).pathname;
    await route.fulfill({ json: path.endsWith('/complete') ? { api_url: 'http://127.0.0.1:5174', audio_device: 'default', auth: 'local' } : path.includes('devices') ? { devices: [] } : { model: { cache_dir: '' }, server: { port: 17842 }, constraints: setupPolicy } });
  });
  await page.goto('/tests/e2e/index.html?view=setup');
  for (let i = 0; i < 3; i++) await page.getByRole('button', { name: 'Continue', exact: true }).click();
  await page.getByLabel('Admin password', { exact: true }).fill('a-long-valid-passphrase');
  await page.getByRole('button', { name: 'Complete Setup' }).click();
  await expect(page).toHaveURL('http://127.0.0.1:5174/');
  await expect(page.getByLabel('What would you like to say?')).toBeVisible();
});

test('setup rejects remote local-auth binds and default passwords before submission', async ({ page }) => {
  let submitted = 0;
  await page.route('**/setup/**', async route => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith('/complete')) submitted++;
    await route.fulfill({ json: path.includes('devices') ? { devices: [] } : { model: { cache_dir: '' }, server: { port: 17842 }, constraints: setupPolicy } });
  });
  await page.goto('/tests/e2e/index.html?view=setup');
  await page.getByRole('radio', { name: 'Advanced', exact: false }).check();
  await page.getByRole('button', { name: 'Continue', exact: true }).click();
  await page.getByLabel('Bind address', { exact: true }).fill('0.0.0.0');
  await page.getByRole('button', { name: 'Continue', exact: true }).click();
  await expect(page.locator('#setup-auth_mode-error')).toContainText('token authentication');
  await page.getByRole('combobox', { name: /^Authentication mode/ }).selectOption('token');
  for (let i = 0; i < 3; i++) await page.getByRole('button', { name: 'Continue', exact: true }).click();
  await page.getByLabel('Admin password', { exact: true }).fill('example-password');
  await page.getByRole('button', { name: 'Complete Setup' }).click();
  await expect(page.locator('#setup-admin_password-error')).toContainText('well-known default');
  expect(submitted).toBe(0);
});
