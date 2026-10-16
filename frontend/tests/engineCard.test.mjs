import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'vite';
import { createSSRApp, h, ref } from 'vue';
import { renderToString } from 'vue/server-renderer';

Object.defineProperty(globalThis, 'localStorage', { value: { getItem: () => null, setItem: () => {} }, configurable: true });
globalThis.document = { documentElement: { lang: '' }, title: '' };
const server = await createServer({ server: { middlewareMode: true }, appType: 'custom' });
after(() => server.close());
const { default: EngineCard } = await server.ssrLoadModule('/src/components/studio/EngineCard.vue');
const { studioKey } = await server.ssrLoadModule('/src/composables/useStudio.ts');
const { setLocale } = await server.ssrLoadModule('/src/i18n/index.ts');
const models = ['pocket-tts-en', 'pocket-tts-es', 'pocket-tts-pt', 'pocket-tts-de', 'pocket-tts-it', 'pocket-tts-fr-24l'];
const capabilities = { provider_id: 'pocket-tts', models, languages: ['en','es','pt','de','it','fr'] };
async function render(provider, selected = 'pocket-tts-en') {
  const app = createSSRApp({ render: () => h(EngineCard, { provider, capabilities }) });
  app.provide(studioKey, {
    schema: ref({ 'providers.pocket_tts.model_id': {}, 'providers.pocket_tts.enabled': {} }),
    config: ref({ config: { providers: { pocket_tts: { model_id: selected } } } }),
  });
  return renderToString(app);
}

test('ready English exposes activation buttons for all five other languages', async () => {
  const html = await render({ provider_id: 'pocket-tts', enabled: true, state: 'ready', active_model: models[0] });
  for (const model of models.slice(1)) assert.ok(html.includes(`aria-label="Activate ${model}"`));
  assert.ok(!html.includes('aria-label="Activate pocket-tts-en"'));
  assert.ok(html.includes('>Active</span>'));
});
test('disabled provider offers activation for every supported model', async () => {
  const html = await render({ provider_id: 'pocket-tts', enabled: false, state: 'disabled' });
  for (const model of models) assert.ok(html.includes(`aria-label="Activate ${model}"`));
  assert.ok(!html.includes('>Active</span>'));
});
test('loading selection is not labeled active and prevents overlapping switches', async () => {
  const html = await render({ provider_id: 'pocket-tts', enabled: true, state: 'loading', active_model: models[1] }, models[1]);
  assert.ok(html.includes('>Loading…</span>'));
  assert.ok(!html.includes('>Active</span>'));
  assert.equal((html.match(/disabled aria-label="Activate /g) || []).length, 5);
});
test('Spanish activation controls and loading help are localized', async () => {
  setLocale('es');
  try {
    const html = await render({ provider_id: 'pocket-tts', enabled: true, state: 'loading', active_model: models[1] }, models[1]);
    assert.ok(html.includes('aria-label="Activar pocket-tts-pt"'));
    assert.ok(html.includes('>Cargando…</span>'));
    assert.ok(html.includes('Preparando el modelo seleccionado.'));
  } finally { setLocale('en'); }
});
