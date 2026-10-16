import { test } from 'node:test';
import assert from 'node:assert/strict';
import { activatePocketModel } from '../src/services/pocketActivation.ts';

const models = ['pocket-tts-en', 'pocket-tts-es', 'pocket-tts-pt', 'pocket-tts-de', 'pocket-tts-it', 'pocket-tts-fr-24l'];

test('every supported Pocket model is enabled and selected atomically at the latest revision', async () => {
  for (const model of models) {
    const calls: unknown[] = [];
    await activatePocketModel({ config: async () => ({ revision: 17 }), patchConfig: async (...args) => { calls.push(args); } }, model, models);
    assert.deepEqual(calls, [[{ providers: { pocket_tts: { model_id: model, enabled: true } } }, 17]]);
  }
});

test('conflicting settings revisions retry the explicit selection once', async () => {
  let revision = 1;
  const calls: unknown[] = [];
  await activatePocketModel({
    config: async () => ({ revision: revision++ }),
    patchConfig: async (...args) => { calls.push(args); if (calls.length === 1) throw Object.assign(new Error('conflict'), { status: 409 }); },
  }, models[1]!, models);
  assert.deepEqual(calls, [1, 2].map(rev => [{ providers: { pocket_tts: { model_id: 'pocket-tts-es', enabled: true } } }, rev]));
});

test('unsupported bundles never read or write configuration', async () => {
  let calls = 0;
  await assert.rejects(activatePocketModel({ config: async () => { calls++; return { revision: 1 }; }, patchConfig: async () => { calls++; } }, 'pocket-tts-ja', models));
  assert.equal(calls, 0);
});

test('authentication failures propagate without retry or unrelated configuration writes', async () => {
  let calls = 0;
  const failure = Object.assign(new Error('Unauthorized'), { status: 401 });
  await assert.rejects(activatePocketModel({ config: async () => ({ revision: 1 }), patchConfig: async () => { calls++; throw failure; } }, models[1]!, models), error => error === failure);
  assert.equal(calls, 1);
});

test('a second conflict is reported instead of retrying indefinitely', async () => {
  let calls = 0;
  await assert.rejects(activatePocketModel({ config: async () => ({ revision: 1 }), patchConfig: async () => { calls++; throw Object.assign(new Error('conflict'), { status: 409 }); } }, models[1]!, models));
  assert.equal(calls, 2);
});
