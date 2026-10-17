import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ref, shallowRef } from 'vue';
import { useSynthesis } from '../src/composables/useSynthesis.ts';
import { createApi } from '../src/services/api.ts';
import { ApiError } from '../src/services/http.ts';
import type { Studio } from '../src/composables/useStudio.ts';

function fixture() {
  const inputs: unknown[][] = [];
  const downloads: string[] = [];
  let failAudio = false;
  let stage = 'completed';
  let release: (() => void) | undefined;
  const studio = {
    text: ref('hola'), voice: ref('A'), language: ref('es'), token: ref(''),
    selectedModel: shallowRef({ id: 'pocket-es', ready: true, cloned: true }),
    info: ref(null),
    sampling: ref({ preset: 'balanced' }),
    samplingParams: ref({}),
    api: {
      createJob: async (...args: unknown[]) => { inputs.push(args.slice(0, 4)); return { id: `job-${inputs.length}` }; },
      job: async (id: string) => ({ id, stage }),
      jobAudio: async (id: string) => {
        if (failAudio) throw new ApiError(503, 'offline');
        downloads.push(id);
        if (release) await new Promise<void>(resolve => { release = resolve; });
        return new Blob([id]);
      },
      cancelJob: async () => {},
    },
  } as unknown as Studio;
  const synthesis = useSynthesis(studio);
  return { studio, synthesis, inputs, downloads, fail: (value: boolean) => { failAudio = value; },
    stage: (value: string) => { stage = value; }, delay: () => { release = () => {}; }, release: () => release?.() };
}

test('hola -> test -> mundo get independent jobs and old audio clears immediately', async () => {
  const f = fixture();
  try {
    for (const text of ['hola', 'test', 'mundo']) {
      f.studio.text.value = ` ${text} `;
      const pending = f.synthesis.generate();
      assert.equal(f.synthesis.audioUrl.value, '');
      await pending;
      assert.equal(await (await fetch(f.synthesis.audioUrl.value)).text(), `job-${f.inputs.length}`);
      assert.equal(f.synthesis.audioFilename.value, `sonicboom-job-${f.inputs.length}.wav`);
    }
    assert.deepEqual(f.inputs.map(input => input[0]), ['hola', 'test', 'mundo']);
    assert.deepEqual(f.downloads, ['job-1', 'job-2', 'job-3']);
  } finally { f.synthesis.dispose(); }
});

test('changed text, voice, model and preview cannot implicitly resume a failed download', async () => {
  const f = fixture();
  try {
    f.fail(true);
    await f.synthesis.generate();
    assert.equal(f.synthesis.jobId.value, 'job-1');
    assert.ok(Object.isFrozen(f.synthesis.active.value?.input));
    f.studio.text.value = 'test';
    f.studio.voice.value = 'B';
    f.studio.selectedModel.value = { ...f.studio.selectedModel.value, id: 'qwen' };
    await f.synthesis.generate();
    await f.synthesis.generate(true);
    assert.equal(f.inputs.length, 1);
    assert.equal(f.synthesis.active.value?.input.text, 'hola');
    assert.equal(f.synthesis.active.value?.input.voice, 'A');
    f.fail(false);
    await f.synthesis.resume();
    assert.deepEqual(f.downloads, ['job-1']);
    await f.synthesis.generate();
    assert.deepEqual(f.inputs[1], ['test', 'es', 'B', true]);
  } finally { f.synthesis.dispose(); }
});

test('repeated clicks submit once; teardown prevents late audio publication', async () => {
  const f = fixture();
  f.delay();
  const pending = f.synthesis.generate();
  await f.synthesis.generate();
  await new Promise(resolve => setImmediate(resolve));
  f.synthesis.dispose();
  f.release();
  await pending;
  assert.equal(f.inputs.length, 1);
  assert.equal(f.synthesis.audioUrl.value, '');
  assert.equal(f.synthesis.complete.value, false);
});

test('API maps exact body, language and profile versus built-in voice', async () => {
  const calls: { path: string; options: RequestInit }[] = [];
  const api = createApi({ json: async (path: string, options: RequestInit) => { calls.push({ path, options }); return { id: 'test' }; } } as never);
  await api.createJob('¿Hola?', 'es', 'voice A', true);
  await api.createJob('test', 'en', 'M1', false);
  assert.equal(calls[0].options.body, '¿Hola?');
  const profile = new URL(calls[0].path, 'http://localhost').searchParams;
  assert.equal(profile.get('profile'), 'voice A');
  assert.equal(profile.get('lang'), 'es');
  assert.equal(profile.has('voice'), false);
  const builtin = new URL(calls[1].path, 'http://localhost').searchParams;
  assert.equal(builtin.get('voice'), 'M1');
  assert.equal(builtin.has('profile'), false);
});


test('confirmed cancellation clears resumability and permits a distinct new job', async () => {
  const f = fixture();
  try {
    f.fail(true);
    await f.synthesis.generate();
    f.stage('cancelled');
    await f.synthesis.cancel();
    assert.equal(f.synthesis.jobId.value, null);
    assert.equal(f.synthesis.complete.value, false);
    assert.ok(f.synthesis.error.value);
    f.stage('completed');
    f.fail(false);
    f.studio.text.value = 'test';
    await f.synthesis.generate();
    assert.deepEqual(f.downloads, ['job-2']);
  } finally { f.synthesis.dispose(); }
});

test('server failure retains error and never fetches audio; next request is new', async () => {
  const f = fixture();
  try {
    f.stage('failed');
    await f.synthesis.generate();
    assert.equal(f.synthesis.jobId.value, null);
    assert.equal(f.downloads.length, 0);
    assert.ok(f.synthesis.error.value);
    f.stage('completed');
    await f.synthesis.generate(true);
    assert.equal(f.inputs.length, 2);
    assert.notEqual(f.inputs[0][0], f.inputs[1][0]);
    assert.equal(f.synthesis.error.value, null);
  } finally { f.synthesis.dispose(); }
});


test('late cancellation acknowledgement cannot resume a newer failed job', async () => {
  const f = fixture();
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  f.studio.api.cancelJob = async () => { await gate; return new Response(); };
  try {
    f.fail(true);
    await f.synthesis.generate();
    const oldCancellation = f.synthesis.cancel();
    f.fail(false);
    await f.synthesis.resume();
    f.studio.text.value = 'test';
    f.fail(true);
    await f.synthesis.generate();
    assert.equal(f.synthesis.jobId.value, 'job-2');
    f.fail(false);
    release();
    await oldCancellation;
    assert.equal(f.synthesis.jobId.value, 'job-2');
    assert.equal(f.synthesis.audioUrl.value, '');
    assert.equal(f.synthesis.complete.value, false);
  } finally { release(); f.synthesis.dispose(); }
});
