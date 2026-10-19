// 480px product tour: captures every studio tab at a 480x480 viewport
// (plus desktop references) into docs/ui/tour-480/. Run AFTER `npm run build`:
//   node --experimental-strip-types tour/capture.mts
// Spawns tests/e2e/server.mjs itself and reuses its API fixtures.
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdir } from 'node:fs/promises';
import { setTimeout as delay } from 'node:timers/promises';
import { chromium, type Page } from '@playwright/test';
import { studioFixtures, wav } from '../tests/e2e/studioFixtures.ts';

const OUT = new URL('../../docs/ui/tour-480/', import.meta.url);
const BASE = 'http://127.0.0.1:5174/tests/e2e/index.html';

async function waitForServer(): Promise<void> {
  for (let attempt = 0; attempt < 50; attempt++) {
    try {
      const response = await fetch(BASE, { method: 'HEAD' });
      if (response.ok) return;
    } catch { /* not up yet */ }
    await delay(200);
  }
  throw new Error('e2e server never came up');
}

async function shoot(page: Page, name: string): Promise<void> {
  await page.screenshot({ path: new URL(name, OUT).pathname, fullPage: true, animations: 'disabled' });
  console.log('captured', name);
}

async function captureTab(page: Page, tab: string, name: string): Promise<void> {
  await page.getByRole('tab', { name: tab, exact: true }).click();
  await page.waitForTimeout(300);
  await shoot(page, name);
}

async function run(): Promise<void> {
  await mkdir(OUT, { recursive: true });
  let server: ChildProcess | undefined;
  try {
    server = spawn('node', ['tests/e2e/server.mjs'], { stdio: 'ignore' });
    await waitForServer();
    for (const viewport of [{ width: 480, height: 480 }, { width: 1280, height: 800 }]) {
      const prefix = viewport.width === 480 ? 'm480' : 'd1280';
      const browser = await chromium.launch();
      try {
        // Full light tour.
        const context = await browser.newContext({
          viewport, locale: 'en-US', colorScheme: 'light', reducedMotion: 'reduce', deviceScaleFactor: 2,
        });
        const page = await context.newPage();
        await studioFixtures(page);
        await page.goto(BASE);
        await page.getByLabel('What would you like to say?').waitFor();
        await shoot(page, `${prefix}-create-light.png`);
        // Show a completed generation in the result panel.
        await page.getByLabel('What would you like to say?').fill('Night bus over the river');
        await page.getByRole('button', { name: 'Generate speech', exact: true }).click();
        await page.locator('#downloadAudio').waitFor({ timeout: 15000 });
        await shoot(page, `${prefix}-result-light.png`);
        await captureTab(page, 'Voice library', `${prefix}-voices-light.png`);
        await page.getByRole('button', { name: 'New voice', exact: true }).first().click();
        await page.waitForTimeout(300);
        await shoot(page, `${prefix}-creator-light.png`);
        await page.getByRole('button', { name: 'Continue', exact: true }).click();
        await page.getByLabel('Reference recording (WAV, 3–10 s recommended)', { exact: true })
          .setInputFiles({ name: 'tour-voice.wav', mimeType: 'audio/wav', buffer: wav(5) });
        await page.getByText('5.00 s · 8000 Hz', { exact: false }).waitFor({ timeout: 15000 });
        // The picker takes the focus, so the dialog scrolls down: show the file card.
        await page.evaluate(() => { const dialog = document.querySelector('.voice-creator-dialog'); if (dialog) dialog.scrollTop = 0; });
        await page.waitForTimeout(300);
        await shoot(page, `${prefix}-creator-file-light.png`);
        await page.keyboard.press('Escape');
        await captureTab(page, 'Engines', `${prefix}-engines-light.png`);
        await captureTab(page, 'Settings', `${prefix}-settings-light.png`);
        await context.close();
        // Dark theme: create + result only.
        const dark = await browser.newContext({
          viewport, locale: 'en-US', colorScheme: 'dark', reducedMotion: 'reduce', deviceScaleFactor: 2,
        });
        const night = await dark.newPage();
        await studioFixtures(night);
        await night.goto(BASE);
        await night.getByLabel('What would you like to say?').waitFor();
        await shoot(night, `${prefix}-create-dark.png`);
        await dark.close();
      } finally {
        await browser.close();
      }
    }
  } finally {
    server?.kill('SIGTERM');
  }
}

await run();
