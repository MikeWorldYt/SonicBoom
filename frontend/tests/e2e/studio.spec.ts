import { test, expect, analyze } from './test';
import { studioFixtures, wav } from './studioFixtures';
test('speech outputs remain isolated and comparisons stay at four', async ({ page }) => {
  const fixture = await studioFixtures(page);
  await page.goto('/tests/e2e/index.html');
  for (let index = 0; index < 6; index++) {
    await page.getByLabel('What would you like to say?').fill('Output ' + index);
    await page.getByRole('button', { name: 'Generate speech', exact: true }).click();
    await expect(page.locator('#downloadAudio')).toHaveAttribute('download', 'sonicboom-job-' + (index + 1) + '.wav');
  }
  await page.getByText('Compare generated versions', { exact: true }).click();
  await expect(page.locator('.result-list li')).toHaveCount(4);
  expect(fixture.jobs()).toBe(6);
  // Every result carries its own waveform player.
  await expect(page.getByRole('slider', { name: 'Latest generated audio' })).toBeVisible();
  await expect(page.locator('.result-list .waveform-player')).toHaveCount(4);
  expect((await analyze(page)).violations).toEqual([]);
});
test('the waveform player draws the recording and the keyboard seeks', async ({ page }) => {
  await studioFixtures(page);
  await page.goto('/tests/e2e/index.html');
  await page.getByRole('tab', { name: 'Voice library', exact: true }).click();
  await page.getByRole('button', { name: 'New voice', exact: true }).first().click();
  await page.getByRole('button', { name: 'Continue', exact: true }).click();
  await page.getByLabel('Reference recording (WAV, 3–10 s recommended)', { exact: true }).setInputFiles({ name: 'reference.wav', mimeType: 'audio/wav', buffer: wav(5) });
  const scrubber = page.getByRole('slider', { name: 'Reference recording' });
  await expect(scrubber).toBeVisible();
  await expect(scrubber).toHaveAttribute('aria-valuemax', '5');
  // The waveform is drawn on a canvas inside the player.
  await expect(page.locator('.waveform-canvas canvas').first()).toBeVisible();
  await scrubber.focus();
  await page.keyboard.press('ArrowRight');
  await expect(scrubber).toHaveAttribute('aria-valuenow', '5');
  await page.keyboard.press('Home');
  await expect(scrubber).toHaveAttribute('aria-valuenow', '0');
  await expect(page.getByRole('button', { name: 'Play' })).toBeVisible();
});
test('cloning upload, retained report and keyboard delete dialog', async ({ page }) => {
  const fixture = await studioFixtures(page);
  await page.goto('/tests/e2e/index.html');
  await page.getByRole('tab', { name: 'Voice library', exact: true }).click();
  await page.getByRole('button', { name: 'New voice', exact: true }).first().click();
  await page.getByRole('button', { name: 'Continue', exact: true }).click();
  await page.getByLabel('Reference recording (WAV, 3–10 s recommended)', { exact: true }).setInputFiles({ name: 'test.wav', mimeType: 'audio/wav', buffer: wav() });
  await expect(page.getByText('1.00 s · 8000 Hz', { exact: false })).toBeVisible();
  await page.getByRole('button', { name: 'Continue', exact: true }).click();
  await page.getByRole('textbox', { name: 'Name', exact: true }).fill('Browser voice');
  await page.getByRole('textbox', { name: 'Consent basis', exact: true }).fill('private consent');
  await page.getByRole('button', { name: 'Clone voice', exact: true }).click();
  await expect(page.locator('#cloneResult')).toBeFocused();
  expect(fixture.requests[0].url).not.toContain('private');
  expect(fixture.requests[0].body).toContain('private consent');
  await page.keyboard.press('Escape');
  await expect(page.locator('.voice-row')).toHaveCount(1);
  await page.getByRole('button', { name: 'Delete', exact: true }).click();
  await expect(page.getByRole('dialog')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Delete', exact: true })).toBeFocused();
  await page.getByRole('button', { name: 'Delete', exact: true }).click();
  expect((await analyze(page)).violations).toEqual([]);
  await page.getByRole('dialog').getByRole('button', { name: 'Delete', exact: true }).click();
  await expect(page.locator('.voice-row')).toHaveCount(0);
});
test('settings preserve conflicting draft and engines expose readiness', async ({ page }) => {
  const fixture = await studioFixtures(page);
  await page.goto('/tests/e2e/index.html');
  await page.getByRole('tab', { name: 'Engines', exact: true }).click();
  await expect(page.locator('.provider')).toContainText('pocket-tts');
  await expect(page.locator('.provider')).toContainText('ready');
  await page.getByRole('tab', { name: 'Settings', exact: true }).click();
  const setting = page.locator('[data-path="model.inference_steps"]');
  await setting.locator('input').fill('6');
  await setting.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(setting).toContainText('Review the new value');
  await expect(setting.locator('input')).toHaveValue('6');
  await expect(setting.getByRole('button', { name: 'Save', exact: true })).toBeDisabled();
  expect(fixture.patches()).toBe(1);
  await setting.getByRole('button', { name: 'Use server value', exact: true }).click();
  await expect(setting.locator('input')).toHaveValue('7');
  expect((await analyze(page)).violations).toEqual([]);
});

test('large voice picker virtualizes and supports keyboard search and selection', async ({ page }) => {
  await studioFixtures(page);
  await page.route('**/v1/voices', route => route.fulfill({ json: { data: Array.from({ length: 500 }, (_, index) => ({ id: 'speaker-' + index, name: 'Speaker ' + index })) } }));
  await page.goto('/tests/e2e/index.html');
  const picker = page.getByRole('combobox', { name: 'Voice', exact: true });
  await expect(picker).toBeVisible();
  await page.getByRole('button', { name: 'Show voices' }).click();
  await expect(page.locator('.voice-combobox-content').getByRole('option').first()).toBeVisible();
  expect(await page.locator('.voice-combobox-content').getByRole('option').count()).toBeLessThan(100);
  await picker.fill('Speaker 449');
  await expect(page.locator('.voice-combobox-content').getByRole('option')).toHaveCount(1);
  await picker.press('ArrowDown');
  await picker.press('Enter');
  await expect(page.locator('#voice-selected')).toContainText('Speaker 449');
  expect((await analyze(page)).violations).toEqual([]);
});

test('waveform trim is opt-in and original recording can be restored', async ({ page }) => {
  await studioFixtures(page);
  await page.goto('/tests/e2e/index.html');
  await page.getByRole('tab', { name: 'Voice library', exact: true }).click();
  await page.getByRole('button', { name: 'New voice', exact: true }).first().click();
  await page.getByRole('button', { name: 'Continue', exact: true }).click();
  await page.getByLabel('Reference recording (WAV, 3–10 s recommended)', { exact: true }).setInputFiles({ name: 'original.wav', mimeType: 'audio/wav', buffer: wav(5) });
  await expect(page.getByText('5.00 s · 8000 Hz', { exact: false })).toBeVisible();
  await expect(page.locator('.audio-waveform')).toHaveCount(0);
  await page.getByRole('button', { name: 'Waveform and trim', exact: true }).click();
  await expect(page.getByRole('img', { name: 'Waveform of the original recording' })).toBeVisible();
  await page.getByLabel('Trim start (seconds)').fill('1');
  await page.getByLabel('Trim end (seconds)').fill('2.25');
  await page.getByRole('button', { name: 'Preview selection', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Use trimmed recording' })).toBeEnabled();
  expect((await analyze(page)).violations).toEqual([]);
  const downloading = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Export selected WAV', exact: true }).click();
  const download = await downloading;
  expect(download.suggestedFilename()).toBe('original-trim.wav');
  const stream = await download.createReadStream();
  const chunks: Buffer[] = [];
  for await (const chunk of stream!) chunks.push(Buffer.from(chunk));
  const exported = Buffer.concat(chunks);
  expect(exported.readUInt32LE(24)).toBe(8000);
  expect(exported.readUInt16LE(22)).toBe(1);
  expect(exported.readUInt32LE(40) / 2 / 8000).toBe(1.25);
  await page.getByRole('button', { name: 'Use trimmed recording', exact: true }).click();
  await expect(page.getByText('1.25 s · 8000 Hz', { exact: false })).toBeVisible();
  await expect(page.locator('.muted').filter({ hasText: 'original-trim.wav' })).toBeVisible();
  await page.getByRole('button', { name: 'Restore original recording', exact: true }).click();
  await expect(page.getByText('5.00 s · 8000 Hz', { exact: false })).toBeVisible();
});

test('English and Spanish layouts support dark/light and reduced motion at narrow and wide sizes', async ({ page }) => {
  await studioFixtures(page);
  test.setTimeout(60000);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/tests/e2e/index.html');
  for (const language of ['en', 'es']) {
    await page.locator('.language-switcher select').selectOption(language);
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const width of [390, 1440]) {
        await page.setViewportSize({ width, height: 900 });
        for (const tab of ['create', 'voices', 'engines', 'settings']) {
          await page.locator('#tab-' + tab).click();
          if (tab === 'voices') await page.locator('#voicesList button.primary').first().click();
          const panel = page.locator('#panel-' + tab);
          await expect(panel).toBeVisible();
          const subject = tab === 'voices'
            ? page.locator('.voice-creator-dialog .clone-wizard')
            : panel.locator(tab === 'create' ? '#text' : tab === 'engines' ? '.provider' : '.cfg-group').first();
          await expect(subject).toBeVisible();
          if (width === 390) expect((await analyze(page)).violations).toEqual([]);
          expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBe(true);
          if (tab === 'voices') await page.keyboard.press('Escape');
        }
      }
      expect((await analyze(page)).violations).toEqual([]);
    }
  }
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Ajustes');
});
