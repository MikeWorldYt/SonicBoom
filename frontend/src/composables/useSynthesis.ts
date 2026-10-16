import { computed, getCurrentScope, onScopeDispose, ref, shallowRef } from 'vue';
import { ApiError } from '../services/http';
import { monitorJob } from '../services/jobs';
import { t, msg, uiError, errorMessage, type Message } from '../i18n';
import type { Job } from '../services/contracts';
import type { Studio } from './useStudio';
export type GenerationInput = Readonly<{ text: string; model: string; voice: string; language: string; cloned: boolean }>;
type ActiveGeneration = Readonly<{ jobId: string; input: GenerationInput; requestEpoch: number }>;
export function useSynthesis(studio: Studio) {
  const pending = ref(false);
  const cancelling = ref(false);
  const active = shallowRef<ActiveGeneration | null>(null);
  const jobId = computed(() => active.value?.jobId ?? null);
  const snapshot = shallowRef<Job | null>(null);
  const audioUrl = ref('');
  const error = shallowRef<Message | null>(null);
  const complete = ref(false);
  let control: AbortController | null = null;
  let disposed = false;
  let epoch = 0;
  const canGenerate = computed(() => studio.selectedModel.value.ready && !!studio.voice.value);
  function clearAudio() {
    if (audioUrl.value) URL.revokeObjectURL(audioUrl.value);
    audioUrl.value = '';
  }
  async function monitor(generation: ActiveGeneration, signal: AbortSignal) {
    const current = () => !disposed && generation.requestEpoch === epoch;
    try {
      const result = await monitorJob(studio.api, generation.jobId, job => {
        if (current()) snapshot.value = job;
      }, signal);
      if (!current()) return;
      if (result.stage !== 'completed') {
        active.value = null;
        throw uiError(result.error || (result.stage === 'failed' ? 'Inference failed.' : 'Generation cancelled or its deadline expired.'));
      }
      const audio = await studio.api.jobAudio(generation.jobId, signal);
      if (!current()) return;
      clearAudio();
      audioUrl.value = URL.createObjectURL(audio);
      active.value = null;
      complete.value = true;
    } catch (failure) {
      if (!current()) return;
      if (failure instanceof ApiError && failure.status === 404) active.value = null;
      error.value = errorMessage(failure);
    } finally {
      if (current()) { pending.value = false; cancelling.value = false; }
    }
  }
  async function generate(preview = false) {
    if (disposed || pending.value) return;
    // An unresolved job is resumed only by the explicit resume action.
    if (active.value) { error.value = msg('Use Resume monitoring or Cancel generation.'); return; }
    const input: GenerationInput = Object.freeze({
      text: preview ? t('Hi there. This is a voice preview.') : studio.text.value.trim(),
      model: studio.selectedModel.value.id, voice: studio.voice.value,
      language: studio.language.value, cloned: studio.selectedModel.value.cloned,
    });
    if (!input.text) { error.value = msg('Please enter some text.'); return; }
    if (studio.info.value?.auth.required && !studio.token.value.trim()) { error.value = msg('Please enter your API token.'); return; }
    if (!canGenerate.value) { error.value = msg('Create a voice profile for this model first.'); return; }
    const requestEpoch = ++epoch;
    error.value = null;
    snapshot.value = null;
    complete.value = false;
    clearAudio();
    pending.value = true;
    control = new AbortController();
    const signal = control.signal;
    try {
      const job = await studio.api.createJob(input.text, input.language, input.voice, input.cloned, signal);
      if (disposed || requestEpoch !== epoch) return;
      const generation = Object.freeze({ jobId: job.id, input, requestEpoch });
      active.value = generation;
      await monitor(generation, signal);
    } catch (failure) {
      if (!disposed && requestEpoch === epoch) { error.value = errorMessage(failure); pending.value = false; }
    }
  }
  async function resume() {
    if (disposed || pending.value || !active.value) return;
    error.value = null;
    pending.value = true;
    control = new AbortController();
    await monitor(active.value, control.signal);
  }
  async function cancel() {
    const generation = active.value;
    if (!generation || cancelling.value || disposed) return;
    cancelling.value = true;
    try {
      await studio.api.cancelJob(generation.jobId);
      // The acknowledgement is not terminal. Reconcile server state if disconnected.
      if (!pending.value) await resume();
    } catch (failure) {
      if (!disposed && generation.requestEpoch === epoch) { error.value = errorMessage(failure); cancelling.value = false; }
    }
  }
  function dispose() { disposed = true; ++epoch; control?.abort(); clearAudio(); }
  if (getCurrentScope()) onScopeDispose(dispose);
  return { pending, cancelling, jobId, active, snapshot, audioUrl, error, complete, canGenerate, generate, resume, cancel, dispose };
}
