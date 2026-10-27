<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { t, renderMessage } from '../../i18n';
import { useStudio } from '../../composables/useStudio';
import { useSynthesis } from '../../composables/useSynthesis';
import NumberField from '../ui/NumberField.vue';
import WaveformPlayer from '../audio/WaveformPlayer.vue';
import VoiceSelector from './VoiceSelector.vue';
import PronunciationSettings from './PronunciationSettings.vue';
import SpokenPreview from './SpokenPreview.vue';
const studio = useStudio();
const { text, model, voice, language, models, selectedModel, availableVoices, preview, sampling, samplingIssues, info, resetSampling } = studio;
const { pending, cancelling, jobId, active, snapshot, results, audioUrl, audioFilename, audioBlob, error, complete, canGenerate, generate, regenerate, removeResult, resume, cancel } = useSynthesis(studio);
type PlayerHandle = { play: () => Promise<unknown> | undefined; pause: () => void };
const player = ref<PlayerHandle | null>(null);
const comparisons = new Map<string, PlayerHandle>();
function registerComparison(id: string, handle: PlayerHandle | null) { if (handle) comparisons.set(id, handle); else comparisons.delete(id); }
// One recording at a time: starting a preview silences the other versions.
function pauseComparisons(except?: string) { for (const [id, handle] of comparisons) if (id !== except) handle.pause(); }
function comparisonPlay(id: string) { player.value?.pause(); pauseComparisons(id); }
watch(pending, value => { if (value) pauseComparisons(); });
const advancedOpen = ref(false);
const predictorOpen = ref(false);
const languages = computed(() => selectedModel.value.languages);
// Advanced controls only exist for the Qwen3-TTS cloning engine.
const advancedAvailable = computed(() => selectedModel.value.cloned && selectedModel.value.provider === 'qwen3-gguf');
const constraints = computed(() => info.value?.synthesis);
const samplingBounds = computed(() => constraints.value?.sampling);
const codepoints = computed(() => [...text.value].length);
const textBytes = computed(() => new TextEncoder().encode(text.value).length);
// Show byte counts only when they matter: near the server limit.
const bytesRelevant = computed(() => {
  if (!constraints.value) return textBytes.value > 0;
  return textBytes.value / constraints.value.max_body_bytes > 0.5
    || codepoints.value / constraints.value.max_text_chars > 0.5;
});
// Remember the last voice/model/language across sessions.
const choiceKey = 'sonicboom.createChoices';
try {
  const saved = localStorage.getItem(choiceKey);
  if (saved) {
    const parsed = JSON.parse(saved) as { model?: string; voice?: string; language?: string };
    if (parsed.model && models.value.some(item => item.id === parsed.model)) model.value = parsed.model;
    if (parsed.voice && availableVoices.value.some(item => item.id === parsed.voice)) voice.value = parsed.voice;
    if (parsed.language && selectedModel.value.languages.includes(parsed.language)) language.value = parsed.language;
  }
} catch { /* session-only */ }
watch([model, voice, language], ([nextModel, nextVoice, nextLanguage]) => {
  try { localStorage.setItem(choiceKey, JSON.stringify({ model: nextModel, voice: nextVoice, language: nextLanguage })); } catch { /* session-only */ }
});
const quality = computed(() => snapshot.value?.quality);
const qualityIssues = computed(() => quality.value?.issues ?? []);
watch(preview, () => void generate(true));
function detachPlayback() { player.value?.pause(); }
watch(audioUrl, (_value, previous) => { if (previous) detachPlayback(); }, { flush: 'sync' });
watch(advancedAvailable, value => { if (!value) advancedOpen.value = false; });
onBeforeUnmount(() => { detachPlayback(); pauseComparisons(); });
</script>
<template>
  <div class="studio-create">
  <form class="create-editor" @submit.prevent="generate()" :aria-busy="pending">
    <div class="form-group"><div class="label-row"><label for="text">{{ t('What would you like to say?') }}</label><span id="charCount" class="char-count">{{ t(codepoints === 1 ? '{count} character' : '{count} characters', { count: codepoints }) }}<template v-if="constraints"> / {{ constraints.max_text_chars }}</template><template v-if="bytesRelevant && constraints"> · {{ textBytes }} / {{ constraints.max_body_bytes }} bytes</template></span></div><textarea id="text"  v-model="text" :placeholder="t('A story, an idea, a welcome message… Start writing here.')" :readonly="pending"/></div>
    <SpokenPreview/>
    <div class="row"><div class="field"><label for="model">{{ t('Model') }}</label><select id="model" v-model="model" :disabled="pending"><option v-for="item in models" :key="item.id" :value="item.id">{{ item.id }} · {{ t(item.ready ? 'ready' : 'Not ready') }}</option></select></div><div class="field"><label for="lang">{{ t('Language') }}</label><select id="lang" v-model="language" :disabled="pending"><option v-for="code in languages" :key="code" :value="code">{{ t(`language.${code}`) }}</option></select></div></div>
    <PronunciationSettings :disabled="pending"/>
    <VoiceSelector v-model="voice" :voices="availableVoices" :disabled="pending"/>
    <details v-if="advancedAvailable" id="advancedGeneration" :open="advancedOpen" @toggle="advancedOpen = ($event.target as HTMLDetailsElement).open">
      <summary>{{ t('Advanced generation settings') }}</summary>
      <p class="help-note">{{ t('Sampling presets, a fixed seed and quality detection. Defaults reproduce the stock behavior; changes are experimental.') }}</p>
      <div class="row">
        <div class="field"><label for="samplingPreset">{{ t('Preset') }}</label><select id="samplingPreset" v-model="sampling.preset" :disabled="pending"><option value="balanced">{{ t('balanced') }}</option><option value="stable">{{ t('stable') }}</option><option value="expressive">{{ t('expressive') }}</option><option value="custom">{{ t('custom') }}</option></select></div>
        <div class="field"><label for="samplingSeed">{{ t('Seed (optional)') }}</label><input id="samplingSeed" v-model="sampling.seed" maxlength="20" :aria-invalid="!!samplingIssues.seed || undefined" aria-describedby="seed-error" inputmode="numeric" autocomplete="off" :placeholder="t('random')" :disabled="pending"></div>
        <div class="field"><label for="qualityMode">{{ t('Quality detection') }}</label><select id="qualityMode" v-model="sampling.quality" :disabled="pending"><option value="">{{ t('Server default') }}</option><option value="off">{{ t('off') }}</option><option value="report">{{ t('report') }}</option><option value="retry">{{ t('retry') }}</option></select></div>
      </div>
      <fieldset :disabled="pending"><legend>{{ t('Talker sampling') }}</legend>
        <div class="row">
          <NumberField id="talkerTemperature" v-model="sampling.talkerTemperature" :label="t('Temperature')" :bounds="samplingBounds?.temperature" :error="samplingIssues.talkerTemperature ? t(samplingIssues.talkerTemperature) : undefined" :disabled="pending"/>
          <NumberField id="talkerTopK" v-model="sampling.talkerTopK" :label="t('Top K')" :bounds="samplingBounds?.top_k" :error="samplingIssues.talkerTopK ? t(samplingIssues.talkerTopK) : undefined" :disabled="pending"/>
          <NumberField id="talkerTopP" v-model="sampling.talkerTopP" :label="t('Top P')" :bounds="samplingBounds?.top_p" :error="samplingIssues.talkerTopP ? t(samplingIssues.talkerTopP) : undefined" :disabled="pending"/>
          <NumberField id="talkerRepetitionPenalty" v-model="sampling.talkerRepetitionPenalty" :label="t('Repetition penalty')" :bounds="samplingBounds?.repetition_penalty" :error="samplingIssues.talkerRepetitionPenalty ? t(samplingIssues.talkerRepetitionPenalty) : undefined" :disabled="pending"/>
        </div>
      </fieldset>
      <p v-if="samplingIssues.seed" id="seed-error" class="field-error" role="alert">{{ t(samplingIssues.seed) }}</p>
      <details id="predictorSampling" :open="predictorOpen" @toggle="predictorOpen = ($event.target as HTMLDetailsElement).open">
        <summary>{{ t('Predictor sampling (advanced)') }}</summary>
        <div class="row">
          <NumberField id="predictorTemperature" v-model="sampling.predictorTemperature" :label="t('Temperature')" :bounds="samplingBounds?.temperature" :error="samplingIssues.predictorTemperature ? t(samplingIssues.predictorTemperature) : undefined" :disabled="pending"/>
          <NumberField id="predictorTopK" v-model="sampling.predictorTopK" :label="t('Top K')" :bounds="samplingBounds?.top_k" :error="samplingIssues.predictorTopK ? t(samplingIssues.predictorTopK) : undefined" :disabled="pending"/>
          <NumberField id="predictorTopP" v-model="sampling.predictorTopP" :label="t('Top P')" :bounds="samplingBounds?.top_p" :error="samplingIssues.predictorTopP ? t(samplingIssues.predictorTopP) : undefined" :disabled="pending"/>
          <NumberField id="predictorRepetitionPenalty" v-model="sampling.predictorRepetitionPenalty" :label="t('Repetition penalty')" :bounds="samplingBounds?.repetition_penalty" :error="samplingIssues.predictorRepetitionPenalty ? t(samplingIssues.predictorRepetitionPenalty) : undefined" :disabled="pending"/>
        </div>
      </details>
      <div class="toolbar"><button id="resetSampling" type="button" :disabled="pending" @click="resetSampling">{{ t('Reset to defaults') }}</button></div>
    </details>
    <p v-if="!selectedModel.ready" class="help-note">{{ t('This model is not ready. Check its status in Engines.') }}</p>
    <p v-if="active" class="help-note">{{ t('Previous generation: {text}', { text: active.input.text }) }} · {{ active.input.model }} · {{ active.input.voice }} · {{ active.input.language }}</p>
    <div class="toolbar"><button id="btn" class="primary" :disabled="pending || !!jobId || !canGenerate || (advancedAvailable && Object.keys(samplingIssues).length > 0)">{{ t(pending ? 'Generating…' : 'Generate speech') }}</button><button v-if="results.length && !pending" id="regenerate" type="button" @click="regenerate">{{ t('Regenerate') }}</button><button v-if="jobId" type="button" :disabled="pending" @click="resume">{{ t('Resume previous generation') }}</button><button v-if="jobId" id="cancelGeneration" type="button" :disabled="cancelling" @click="cancel">{{ t(cancelling ? 'Cancellation requested. Waiting for native computation to stop…' : 'Cancel generation') }}</button></div>
  </form>
  <aside class="create-side" :aria-label="t('Result')">
  <div id="status" role="status" aria-live="polite"><progress v-if="pending" class="generate-progress" :aria-label="t('Generating…')"/><template v-if="pending && snapshot"><span v-if="snapshot.context">{{ snapshot.context.model }} · {{ snapshot.context.effective_device }} · </span>{{ t(snapshot.stage) }} · {{ t('{seconds}s elapsed', { seconds: Math.floor(snapshot.elapsed_secs || 0) }) }}<span v-if="snapshot.frames"> · {{ t('{count} generated frames', { count: snapshot.frames }) }}</span><p v-if="snapshot.context?.provider === 'qwen3-gguf' && snapshot.context.effective_device === 'cpu'" class="muted">{{ t('CPU inference can be slow.') }}</p></template><span v-if="complete">{{ t('Completed. Audio is ready to play or download.') }}</span></div>
  <details v-if="snapshot?.context" id="generationDiagnostics"><summary>{{ t('Generation diagnostics') }}</summary><p>{{ snapshot.context.provider }} · {{ snapshot.context.model }} · {{ snapshot.context.effective_device }} · {{ snapshot.context.voice }}</p><p v-if="snapshot.audio_secs">{{ t('Generated duration: {seconds}s', { seconds: snapshot.audio_secs.toFixed(2) }) }} · RTF {{ snapshot.rtf?.toFixed(2) ?? '—' }}</p><p v-if="snapshot.context.seed">{{ t('Seed: {seed}', { seed: snapshot.context.seed }) }}</p><p v-else>{{ t('Seed: not exposed by this API') }}</p><p v-if="quality">{{ t('Sampling preset: {preset}', { preset: quality.mode }) }} · {{ t('Quality: {status}', { status: quality.quality_status }) }} · {{ t('Warnings: {count}', { count: qualityIssues.length }) }} · {{ t('Retries: {count}', { count: quality.retries }) }}</p><ul v-if="qualityIssues.length" class="quality-issues"><li v-for="issue in qualityIssues" :key="issue.kind">{{ t('{kind}: {detail}', { kind: issue.kind, detail: issue.detail }) }}</li></ul><p v-if="quality?.echo?.asr_required" class="muted">{{ t('Reference-echo detection needs an offline ASR transcript; none is claimed here.') }}</p></details>
  <p v-if="error" role="alert" class="feedback error">{{ renderMessage(error) }} <span v-if="jobId">{{ t('Use Resume monitoring or Cancel generation.') }}</span></p>
  <template v-if="audioUrl"><WaveformPlayer id="player" ref="player" :blob="audioBlob" :label="t('Latest generated audio')" autoplay @play="pauseComparisons()"/><a id="downloadAudio" :href="audioUrl" :download="audioFilename">{{ t('Download generated audio') }}</a></template>
  <p v-else class="output-hint">{{ t('Your audio will appear here, ready to listen or download.') }}</p>
  <details v-if="results.length > 1" id="resultComparison"><summary>{{ t('Compare generated versions') }}</summary>
    <ul class="result-list"><li v-for="(result, index) in results" :key="result.jobId">
      <WaveformPlayer :ref="handle => registerComparison(result.jobId, handle as PlayerHandle | null)" :blob="result.blob" :label="t('Generated version {version}', { version: index + 1 })" compact @play="comparisonPlay(result.jobId)"/>
      <span class="muted">{{ result.text.slice(0, 80) }} · {{ result.voice }} · {{ t(`language.${result.language}`) }} · {{ result.model }} · {{ t('{seconds}s', { seconds: result.durationSecs.toFixed(2) }) }}<template v-if="result.preset"> · {{ t('Preset: {preset}', { preset: result.preset }) }}</template><template v-if="result.seed"> · {{ t('Seed: {seed}', { seed: result.seed }) }}</template><template v-if="result.quality"> · {{ t('Quality: {status}', { status: result.quality.quality_status }) }}</template></span>
      <a :href="result.url" :download="result.filename">{{ t('Download') }}</a>
      <button type="button" class="btn-inline" @click="removeResult(index)">{{ t('Remove') }}</button>
    </li></ul>
  </details>
  </aside>
  </div>
</template>
