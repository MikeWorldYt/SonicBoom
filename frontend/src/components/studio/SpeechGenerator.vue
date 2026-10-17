<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { t, renderMessage } from '../../i18n';
import { useStudio } from '../../composables/useStudio';
import { useSynthesis } from '../../composables/useSynthesis';
import VoiceSelector from './VoiceSelector.vue';
const studio = useStudio();
const { text, model, voice, language, models, selectedModel, availableVoices, preview, sampling, resetSampling } = studio;
const { pending, cancelling, jobId, active, snapshot, results, audioUrl, audioFilename, error, complete, canGenerate, generate, regenerate, removeResult, resume, cancel } = useSynthesis(studio);
const player = ref<HTMLAudioElement | null>(null);
const advancedOpen = ref(false);
const predictorOpen = ref(false);
const languages = computed(() => selectedModel.value.languages.length ? selectedModel.value.languages : ['en','ko','ja','zh','es','pt','fr','de','it','ru','ar','hi','nl','pl','tr','vi','id','th','sv','da','fi','el','cs','sk','sl','hr','ro','hu','bg','uk','lt','lv','et']);
// Advanced controls only exist for the Qwen3-TTS cloning engine.
const advancedAvailable = computed(() => selectedModel.value.cloned && selectedModel.value.provider === 'qwen3-gguf');
const quality = computed(() => snapshot.value?.quality);
const qualityIssues = computed(() => quality.value?.issues ?? []);
watch(preview, () => void generate(true));
function detachPlayback() { player.value?.pause(); player.value?.removeAttribute('src'); player.value?.load(); }
// Stop before v-if removes the old element and clears its template ref.
watch(audioUrl, (_value, previous) => { if (previous) detachPlayback(); }, { flush: 'sync' });
watch(audioUrl, value => { if (value) void player.value?.play().catch(() => {}); }, { flush: 'post' });
watch(advancedAvailable, value => { if (!value) advancedOpen.value = false; });
onBeforeUnmount(detachPlayback);
</script>
<template>
  <form @submit.prevent="generate()" :aria-busy="pending">
    <div class="form-group"><div class="label-row"><label for="text">{{ t('What would you like to say?') }}</label><span id="charCount" class="char-count">{{ t(text.length === 1 ? '{count} character' : '{count} characters', { count: text.length }) }}</span></div><textarea id="text" v-model="text" :placeholder="t('A story, an idea, a welcome message… Start writing here.')" :readonly="pending"/></div>
    <div class="row"><div class="field"><label for="model">{{ t('Model') }}</label><select id="model" v-model="model" :disabled="pending"><option v-for="item in models" :key="item.id" :value="item.id">{{ item.id }} · {{ t(item.ready ? 'ready' : 'Not ready') }}</option></select></div><div class="field"><label for="lang">{{ t('Language') }}</label><select id="lang" v-model="language" :disabled="pending"><option v-for="code in languages" :key="code" :value="code">{{ t(`language.${code}`) }}</option></select></div></div>
    <VoiceSelector v-model="voice" :voices="availableVoices" :disabled="pending"/>
    <details v-if="advancedAvailable" id="advancedGeneration" :open="advancedOpen" @toggle="advancedOpen = ($event.target as HTMLDetailsElement).open">
      <summary>{{ t('Advanced generation settings') }}</summary>
      <p class="help-note">{{ t('Sampling presets, a fixed seed and quality detection. Defaults reproduce the stock behavior; changes are experimental.') }}</p>
      <div class="row">
        <div class="field"><label for="samplingPreset">{{ t('Preset') }}</label><select id="samplingPreset" v-model="sampling.preset" :disabled="pending"><option value="balanced">{{ t('balanced') }}</option><option value="stable">{{ t('stable') }}</option><option value="expressive">{{ t('expressive') }}</option><option value="custom">{{ t('custom') }}</option></select></div>
        <div class="field"><label for="samplingSeed">{{ t('Seed (optional)') }}</label><input id="samplingSeed" v-model="sampling.seed" inputmode="numeric" autocomplete="off" :placeholder="t('random')" :disabled="pending"></div>
        <div class="field"><label for="qualityMode">{{ t('Quality detection') }}</label><select id="qualityMode" v-model="sampling.quality" :disabled="pending"><option value="">{{ t('Server default') }}</option><option value="off">{{ t('off') }}</option><option value="report">{{ t('report') }}</option><option value="retry">{{ t('retry') }}</option></select></div>
      </div>
      <fieldset :disabled="pending"><legend>{{ t('Talker sampling') }}</legend>
        <div class="row">
          <div class="field"><label for="talkerTemperature">{{ t('Temperature') }}</label><input id="talkerTemperature" v-model="sampling.talkerTemperature" inputmode="decimal" :placeholder="'0.9'" :disabled="pending"></div>
          <div class="field"><label for="talkerTopK">{{ t('Top K') }}</label><input id="talkerTopK" v-model="sampling.talkerTopK" inputmode="numeric" :placeholder="'50'" :disabled="pending"></div>
          <div class="field"><label for="talkerTopP">{{ t('Top P') }}</label><input id="talkerTopP" v-model="sampling.talkerTopP" inputmode="decimal" :placeholder="'1.0'" :disabled="pending"></div>
          <div class="field"><label for="talkerRepetitionPenalty">{{ t('Repetition penalty') }}</label><input id="talkerRepetitionPenalty" v-model="sampling.talkerRepetitionPenalty" inputmode="decimal" :placeholder="'1.05'" :disabled="pending"></div>
        </div>
      </fieldset>
      <details id="predictorSampling" :open="predictorOpen" @toggle="predictorOpen = ($event.target as HTMLDetailsElement).open">
        <summary>{{ t('Predictor sampling (advanced)') }}</summary>
        <div class="row">
          <div class="field"><label for="predictorTemperature">{{ t('Temperature') }}</label><input id="predictorTemperature" v-model="sampling.predictorTemperature" inputmode="decimal" :placeholder="'0.9'" :disabled="pending"></div>
          <div class="field"><label for="predictorTopK">{{ t('Top K') }}</label><input id="predictorTopK" v-model="sampling.predictorTopK" inputmode="numeric" :placeholder="'50'" :disabled="pending"></div>
          <div class="field"><label for="predictorTopP">{{ t('Top P') }}</label><input id="predictorTopP" v-model="sampling.predictorTopP" inputmode="decimal" :placeholder="'1.0'" :disabled="pending"></div>
          <div class="field"><label for="predictorRepetitionPenalty">{{ t('Repetition penalty') }}</label><input id="predictorRepetitionPenalty" v-model="sampling.predictorRepetitionPenalty" inputmode="decimal" :placeholder="'1.0'" :disabled="pending"></div>
        </div>
      </details>
      <div class="toolbar"><button id="resetSampling" type="button" :disabled="pending" @click="resetSampling">{{ t('Reset to defaults') }}</button></div>
    </details>
    <p v-if="!selectedModel.ready" class="help-note">{{ t('This model is not ready. Check its status in Engines.') }}</p>
    <p v-if="active" class="help-note">{{ t('Previous generation: {text}', { text: active.input.text }) }} · {{ active.input.model }} · {{ active.input.voice }} · {{ active.input.language }}</p>
    <div class="toolbar"><button id="btn" class="primary" :disabled="pending || !!jobId || !canGenerate">{{ t(pending ? 'Generating…' : 'Generate speech') }}</button><button v-if="results.length && !pending" id="regenerate" type="button" @click="regenerate">{{ t('Regenerate') }}</button><button v-if="jobId" type="button" :disabled="pending" @click="resume">{{ t('Resume previous generation') }}</button><button v-if="jobId" id="cancelGeneration" type="button" :disabled="cancelling" @click="cancel">{{ t(cancelling ? 'Cancellation requested. Waiting for native computation to stop…' : 'Cancel generation') }}</button></div>
  </form>
  <div id="status" role="status" aria-live="polite"><template v-if="pending && snapshot"><span v-if="snapshot.context">{{ snapshot.context.model }} · {{ snapshot.context.effective_device }} · </span>{{ t(snapshot.stage) }} · {{ t('{seconds}s elapsed', { seconds: Math.floor(snapshot.elapsed_secs || 0) }) }}<span v-if="snapshot.frames"> · {{ t('{count} generated frames', { count: snapshot.frames }) }}</span><p v-if="snapshot.context?.provider === 'qwen3-gguf' && snapshot.context.effective_device === 'cpu'" class="muted">{{ t('CPU inference can be slow.') }}</p></template><span v-if="complete">{{ t('Completed. Audio is ready to play or download.') }}</span></div>
  <details v-if="snapshot?.context" id="generationDiagnostics"><summary>{{ t('Generation diagnostics') }}</summary><p>{{ snapshot.context.provider }} · {{ snapshot.context.model }} · {{ snapshot.context.effective_device }} · {{ snapshot.context.voice }}</p><p v-if="snapshot.audio_secs">{{ t('Generated duration: {seconds}s', { seconds: snapshot.audio_secs.toFixed(2) }) }} · RTF {{ snapshot.rtf?.toFixed(2) ?? '—' }}</p><p v-if="snapshot.context.seed">{{ t('Seed: {seed}', { seed: snapshot.context.seed }) }}</p><p v-else>{{ t('Seed: not exposed by this API') }}</p><p v-if="quality">{{ t('Sampling preset: {preset}', { preset: quality.mode }) }} · {{ t('Quality: {status}', { status: quality.quality_status }) }} · {{ t('Warnings: {count}', { count: qualityIssues.length }) }} · {{ t('Retries: {count}', { count: quality.retries }) }}</p><ul v-if="qualityIssues.length" class="quality-issues"><li v-for="issue in qualityIssues" :key="issue.kind">{{ t('{kind}: {detail}', { kind: issue.kind, detail: issue.detail }) }}</li></ul><p v-if="quality?.echo?.asr_required" class="muted">{{ t('Reference-echo detection needs an offline ASR transcript; none is claimed here.') }}</p></details>
  <p v-if="error" role="alert" class="feedback error">{{ renderMessage(error) }} <span v-if="jobId">{{ t('Use Resume monitoring or Cancel generation.') }}</span></p>
  <template v-if="audioUrl"><audio id="player" ref="player" :src="audioUrl" controls/><a id="downloadAudio" :href="audioUrl" :download="audioFilename">{{ t('Download generated audio') }}</a></template>
  <p v-else class="output-hint">{{ t('Your audio will appear here, ready to listen or download.') }}</p>
  <details v-if="results.length > 1" id="resultComparison"><summary>{{ t('Compare generated versions') }}</summary>
    <ul class="result-list"><li v-for="(result, index) in results" :key="result.jobId">
      <audio :src="result.url" controls/>
      <span class="muted">{{ result.model }} · {{ t('{seconds}s', { seconds: result.durationSecs.toFixed(2) }) }}<template v-if="result.preset"> · {{ t('Preset: {preset}', { preset: result.preset }) }}</template><template v-if="result.seed"> · {{ t('Seed: {seed}', { seed: result.seed }) }}</template><template v-if="result.quality"> · {{ t('Quality: {status}', { status: result.quality.quality_status }) }}</template></span>
      <a :href="result.url" :download="result.filename">{{ t('Download') }}</a>
      <button type="button" class="btn-inline" @click="removeResult(index)">{{ t('Remove') }}</button>
    </li></ul>
  </details>
</template>
