<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, shallowRef, useId, watch } from 'vue';
import { t, renderMessage, errorMessage, type Message } from '../../i18n';
import { inspectWave, analyzeWave, cropWave, type WaveFile, type WaveAnalysis } from '../../features/audio/wav';
import WaveformPlayer from './WaveformPlayer.vue';
const props = defineProps<{ file: File; maxBytes: number }>();
const emit = defineEmits<{ apply: [file: File]; close: [] }>();
const wave = shallowRef<WaveFile | null>(null), health = shallowRef<WaveAnalysis | null>(null);
const pending = ref(true), error = shallowRef<Message | null>(null);
const start = ref<string | number>(0), end = ref<string | number>(0), preview = shallowRef<Blob | null>(null);
const controller = new AbortController(), title = useId();
const selection = computed(() => wave.value ? { x: Number(start.value) / wave.value.duration * 512, width: (Number(end.value) - Number(start.value)) / wave.value.duration * 512 } : { x: 0, width: 0 });
const rangeError = computed(() => !wave.value || start.value === '' || end.value === '' || !Number.isFinite(Number(start.value)) || !Number.isFinite(Number(end.value)) || Number(start.value) < 0 || Number(end.value) > wave.value.duration || Number(end.value) <= Number(start.value) ? t('Choose a valid start and end time.') : '');
const path = computed(() => health.value?.peaks.map((value, index) => `M${index},${50 - Math.min(1, value) * 48}v${Math.min(1, value) * 96}`).join(' ') || '');
onBeforeUnmount(() => { controller.abort(); });
onMounted(async () => {
  try {
    if (props.file.size > props.maxBytes || !props.file.size) throw new Error(t('Reference must be 1–{max} bytes.', { max: props.maxBytes }));
    const buffer = await props.file.arrayBuffer(); controller.signal.throwIfAborted();
    wave.value = inspectWave(buffer); end.value = wave.value.duration;
    health.value = await analyzeWave(wave.value, controller.signal);
  } catch (failure) { if (!controller.signal.aborted) error.value = errorMessage(failure); }
  finally { pending.value = false; }
});
function croppedFile() {
  if (!wave.value || rangeError.value) return;
  return new File([cropWave(wave.value, Number(start.value), Number(end.value))], props.file.name.replace(/\.wav$/i, '') + '-trim.wav', { type: 'audio/wav' });
}
function previewSelection() { const file = croppedFile(); if (file) preview.value = file; }
function apply() { const file = croppedFile(); if (file) emit('apply', file); }
function exportSelection() {
  const file = croppedFile();
  if (!file) return;
  const url = URL.createObjectURL(file), link = document.createElement('a');
  link.href = url; link.download = file.name; link.click();
  // The download owns this URL until the browser has taken the file.
  setTimeout(() => URL.revokeObjectURL(url), 0);
  preview.value = file;
}
</script>
<template>
  <section class="reference-editor" :aria-labelledby="title" :aria-busy="pending">
    <h3 :id="title">{{ t('Waveform and trim') }}</h3>
    <p v-if="pending" role="status">{{ t('Analyzing reference…') }}</p>
    <p v-if="error" role="alert" class="field-error">{{ renderMessage(error) }}</p>
    <template v-if="wave && health">
      <svg viewBox="0 0 512 100" role="img" :aria-label="t('Waveform of the original recording')" class="audio-waveform"><path :d="path" fill="none" stroke="currentColor" stroke-width="1"/><rect :x="selection.x" y="0" :width="Math.max(0, selection.width)" height="100" fill="currentColor" opacity=".12"/></svg>
      <dl class="stat-grid"><div><dt>{{ t('Peak') }}</dt><dd>{{ health.peak.toFixed(4) }}</dd></div><div><dt>{{ t('RMS') }}</dt><dd>{{ health.rms.toFixed(4) }}</dd></div><div><dt>{{ t('Clipping') }}</dt><dd>{{ (health.clipping * 100).toFixed(2) }}%</dd></div><div><dt>{{ t('Near silence') }}</dt><dd>{{ (health.silence * 100).toFixed(2) }}%</dd></div></dl>
      <details class="health-notes"><summary>{{ t('Recording health notes') }}</summary>
      <p class="help-note">{{ t('Select original sample frames without resampling or changing encoding. Your original file is kept until you apply a trim. Export omits ancillary WAV metadata.') }}</p>
      <p class="help-note">{{ t('Local measurements cover original channels. Server measurements use the mono mix.') }}</p>
      <p v-if="health.silence > .95" role="status" class="help-note">{{ t('The reference recording is nearly silent. Choose a clearer recording.') }}</p>
      <p v-if="health.clipping > .1" role="status" class="help-note">{{ t('The reference recording has heavy clipping. Try recording at a lower input level.') }}</p>
      <p v-if="health.rms < .01 && health.silence <= .95" class="help-note">{{ t('The reference recording is unusually quiet. Increase the recording level.') }}</p>
      <p class="help-note">{{ t('Quality advice does not impose an additional upload restriction.') }}</p>
      </details>
      <div class="row"><label>{{ t('Trim start (seconds)') }}<input :form="title + '-tools'" v-model.number="start" type="number" min="0" :max="wave.duration" step="any" :aria-invalid="!!rangeError || undefined" :aria-describedby="rangeError ? title + '-range-error' : undefined"></label><label>{{ t('Trim end (seconds)') }}<input :form="title + '-tools'" v-model.number="end" type="number" min="0" :max="wave.duration" step="any" :aria-invalid="!!rangeError || undefined" :aria-describedby="rangeError ? title + '-range-error' : undefined"></label></div>
      <p v-if="rangeError" :id="title + '-range-error'" role="alert" class="field-error">{{ rangeError }}</p>
      <p>{{ t('Selected duration: {seconds} s', { seconds: rangeError ? '—' : (Number(end) - Number(start)).toFixed(3) }) }}</p>
      <WaveformPlayer v-if="preview" :blob="preview" :label="t('Trim preview')" compact/>
      <div class="toolbar"><button type="button" :disabled="!!rangeError" @click="previewSelection">{{ t('Preview selection') }}</button><button type="button" :disabled="!!rangeError" @click="exportSelection">{{ t('Export selected WAV') }}</button><button type="button" :disabled="!!rangeError" @click="apply">{{ t('Use trimmed recording') }}</button></div>
    </template>
    <button type="button" @click="emit('close')">{{ t(pending ? 'Cancel analysis' : 'Close editor') }}</button>
  </section>
</template>
