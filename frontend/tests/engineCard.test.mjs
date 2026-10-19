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
// Locale is auto-detected from `navigator.language`; force English so these
// assertions are deterministic on non-English hosts (the Spanish case below
// switches explicitly and restores English).
setLocale('en');
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

function selectedOption(html) {
  const matches = html.match(/<option[^>]*selected[^>]*>/g) || [];
  assert.equal(matches.length, 1, `exactly one model must be selected: ${matches.join(' ')}`);
  return matches[0];
}
function activateButton(html) {
  const match = html.match(/<button[^>]*class="engine-activate"[^>]*>[\s\S]*?<\/button>/);
  assert.ok(match, 'a single Activate control must exist');
  return match[0];
}
test('ready English lists every model once with the active one selected', async () => {
  const html = await render({ provider_id: 'pocket-tts', enabled: true, state: 'ready', active_model: models[0] });
  for (const model of models) assert.ok(html.includes(`value="${model}"`), `missing option ${model}`);
  assert.ok(selectedOption(html).includes(`value="${models[0]}"`));
  assert.ok(/disabled/.test(activateButton(html)), 'activating the already-active model must be disabled');
});
test('disabled provider offers every model with an enabled Activate control', async () => {
  const html = await render({ provider_id: 'pocket-tts', enabled: false, state: 'disabled' });
  for (const model of models) assert.ok(html.includes(`value="${model}"`), `missing option ${model}`);
  assert.ok(!/disabled/.test(activateButton(html)));
  assert.ok(!html.includes('>Active</span>'));
});
test('loading selection is marked selected and blocks overlapping switches', async () => {
  const html = await render({ provider_id: 'pocket-tts', enabled: true, state: 'loading', active_model: models[1] }, models[1]);
  assert.ok(selectedOption(html).includes(`value="${models[1]}"`));
  assert.ok(/disabled/.test(activateButton(html)));
  assert.ok(html.includes('Preparing the selected model.'));
});
test('Spanish activation controls and loading help are localized', async () => {
  setLocale('es');
  try {
    const html = await render({ provider_id: 'pocket-tts', enabled: true, state: 'loading', active_model: models[1] }, models[1]);
    assert.ok(html.includes('>Activar</button>') || html.includes('>Cargando…</button>'));
    assert.ok(html.includes('Preparando el modelo seleccionado.'));
  } finally { setLocale('en'); }
});
