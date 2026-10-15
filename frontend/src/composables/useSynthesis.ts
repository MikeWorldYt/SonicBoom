import { computed, onUnmounted, ref, shallowRef } from 'vue';
import { ApiError } from '../services/http';
import { monitorJob } from '../services/jobs';
import { t, msg, uiError, errorMessage, type Message } from '../i18n';
import type { Job } from '../services/contracts';
import type { Studio } from './useStudio';
export function useSynthesis(studio: Studio) {
  const pending = ref(false);
  const cancelling = ref(false);
  const jobId = ref<string | null>(null);
  const snapshot = shallowRef<Job | null>(null);
  const audioUrl = ref('');
  const error = shallowRef<Message | null>(null);
  const complete = ref(false);
  let control: AbortController | null = null;
  let disposed = false;
  const canGenerate = computed(() => !!jobId.value || (studio.selectedModel.value.ready && !!studio.voice.value));
  async function generate(preview = false) {
    if (pending.value) return;
    error.value = null;
    const text = preview ? t('Hi there. This is a voice preview.') : studio.text.value.trim();
    if (!text && !jobId.value) { error.value = msg('Please enter some text.'); return; }
    if (studio.info.value?.auth.required && !studio.token.value.trim()) { error.value = msg('Please enter your API token.'); return; }
    if (!canGenerate.value) { error.value = msg('Create a voice profile for this model first.'); return; }
    pending.value = true;
    complete.value = false;
    control = new AbortController();
    try {
      if (!jobId.value) jobId.value = (await studio.api.createJob(text, studio.language.value, studio.voice.value, studio.selectedModel.value.cloned, control.signal)).id;
      const result = await monitorJob(studio.api, jobId.value, job => { if (!disposed) snapshot.value = job; }, control.signal);
      const id = jobId.value;
      if (result.stage !== 'completed') {
        jobId.value = null;
        throw uiError(result.error || (result.stage === 'failed' ? 'Inference failed.' : 'Generation cancelled or its deadline expired.'));
      }
      const audio = await studio.api.jobAudio(id, control.signal);
      if (disposed) return;
      if (audioUrl.value) URL.revokeObjectURL(audioUrl.value);
      audioUrl.value = URL.createObjectURL(audio);
      jobId.value = null;
      complete.value = true;
    } catch (failure) {
      if (failure instanceof ApiError && failure.status === 404) jobId.value = null;
      if (!disposed) error.value = errorMessage(failure);
    } finally { pending.value = false; cancelling.value = false; }
  }
  async function cancel() {
    if (!jobId.value || cancelling.value) return;
    cancelling.value = true;
    try { await studio.api.cancelJob(jobId.value); }
    catch (failure) { error.value = errorMessage(failure); cancelling.value = false; }
  }
  onUnmounted(() => { disposed = true; control?.abort(); if (audioUrl.value) URL.revokeObjectURL(audioUrl.value); });
  return { pending, cancelling, jobId, snapshot, audioUrl, error, complete, canGenerate, generate, cancel };
}
