import { computed, getCurrentScope, onScopeDispose, ref, shallowRef, watch } from 'vue';
import { ApiError } from '../services/http';
import { errorMessage, msg, type Message } from '../i18n';
import type { NormalizeRequest, NormalizePreview } from '../services/contracts';
import { createLatestRunner, isPreviewStale, previewKey, PREVIEW_DEBOUNCE_MS } from '../features/speech/pronunciation';
import type { Studio } from './useStudio';

export type PreviewStatus = 'idle' | 'empty' | 'loading' | 'ready' | 'error';

/**
 * Drives the "What will be spoken?" panel. Requests happen only on explicit user
 * actions (never per keystroke), are debounced, and a superseded request is aborted.
 * The request is snapshotted when the user acts, so later edits mark the result
 * stale instead of silently changing it.
 */
export function usePronunciationPreview(studio: Studio, delayMs = PREVIEW_DEBOUNCE_MS) {
  const status = ref<PreviewStatus>('idle');
  const result = shallowRef<NormalizePreview | null>(null);
  const error = shallowRef<Message | null>(null);
  const errorStatus = ref(0);
  const previewedKey = ref<string | null>(null);
  const runner = createLatestRunner(delayMs);

  /** The request the form would send right now; matches what synthesis submits (trimmed text). */
  function currentRequest(): NormalizeRequest {
    const pronunciation = studio.pronunciation?.value;
    return {
      text: studio.text.value.trim(), language: studio.language.value,
      mode: pronunciation?.normalization, locale: pronunciation?.locale,
      model: studio.selectedModel.value.id,
    };
  }
  const currentKey = computed(() => previewKey(currentRequest()));
  const stale = computed(() => (status.value === 'ready' || status.value === 'error') && isPreviewStale(previewedKey.value, currentKey.value));

  function reset() {
    runner.cancel();
    status.value = 'idle'; result.value = null; error.value = null; errorStatus.value = 0; previewedKey.value = null;
  }

  /** Debounced: repeated calls inside the delay produce one request. */
  function request() {
    if (!studio.normalizationAvailable?.value) return;
    const body = currentRequest();
    error.value = null; errorStatus.value = 0;
    if (!body.text) { runner.cancel(); result.value = null; previewedKey.value = null; status.value = 'empty'; return; }
    if (studio.info.value?.auth.required && !studio.token.value.trim()) {
      runner.cancel(); result.value = null; previewedKey.value = null;
      error.value = msg('Please enter your API token.'); status.value = 'error'; return;
    }
    const key = previewKey(body);
    status.value = 'loading';
    runner.schedule(async (signal, isCurrent) => {
      try {
        const value = await studio.api.previewNormalization(body, signal);
        if (!isCurrent()) return;
        result.value = value; previewedKey.value = key; status.value = 'ready';
      } catch (failure) {
        if (!isCurrent()) return;
        result.value = null; previewedKey.value = key;
        error.value = errorMessage(failure);
        errorStatus.value = failure instanceof ApiError ? failure.status : 0;
        status.value = 'error';
      }
    });
  }

  // A different account or auth scope must never see the previous caller's text.
  const stopScopeWatch = watch([() => studio.scopeEpoch?.value, studio.token], reset, { flush: 'sync' });
  function dispose() { stopScopeWatch(); runner.cancel(); }
  if (getCurrentScope()) onScopeDispose(dispose);
  return { status, result, error, errorStatus, stale, request, reset, dispose };
}
