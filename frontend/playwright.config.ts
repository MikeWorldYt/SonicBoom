import { defineConfig, devices } from '@playwright/test';
export default defineConfig({
  testDir: './tests/e2e', use: { baseURL: 'http://127.0.0.1:5174', trace: 'retain-on-failure' },
  webServer: { command: 'npm run build && node tests/e2e/server.mjs', url: 'http://127.0.0.1:5174/tests/e2e/index.html', reuseExistingServer: false },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }, { name: 'firefox', use: { ...devices['Desktop Firefox'] } }, { name: 'mobile', use: { ...devices['Pixel 7'], defaultBrowserType: 'chromium' } }],
});
