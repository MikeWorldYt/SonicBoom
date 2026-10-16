<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { t, renderMessage } from '../../i18n';
import { useStudio } from '../../composables/useStudio';
import { useSynthesis } from '../../composables/useSynthesis';
import VoiceSelector from './VoiceSelector.vue';
const studio = useStudio();
const { text, model, voice, language, models, selectedModel, availableVoices, preview } = studio;
const { pending, cancelling, jobId, active, snapshot, audioUrl, audioFilename, error, complete, canGenerate, generate, resume, cancel } = useSynthesis(studio);
const player = ref<HTMLAudioElement | null>(null);
const languages = computed(() => selectedModel.value.languages.length ? selectedModel.value.languages : ['en','ko','ja','zh','es','pt','fr','de','it','ru','ar','hi','nl','pl','tr','vi','id','th','sv','da','fi','el','cs','sk','sl','hr','ro','hu','bg','uk','lt','lv','et']);
watch(preview, () => void generate(true));
function detachPlayback() { player.value?.pause(); player.value?.removeAttribute('src'); player.value?.load(); }
// Stop before v-if removes the old element and clears its template ref.
watch(audioUrl, (_value, previous) => { if (previous) detachPlayback(); }, { flush: 'sync' });
watch(audioUrl, value => { if (value) void player.value?.play().catch(() => {}); }, { flush: 'post' });
onBeforeUnmount(detachPlayback);
</script>
<template>
  <form @submit.prevent="generate()" :aria-busy="pending">
    <div class="form-group"><div class="label-row"><label for="text">{{ t('What would you like to say?') }}</label><span id="charCount" class="char-count">{{ t(text.length === 1 ? '{count} character' : '{count} characters', { count: text.length }) }}</span></div><textarea id="text" v-model="text" :placeholder="t('A story, an idea, a welcome message… Start writing here.')" :readonly="pending"/></div>
    <div class="row"><div class="field"><label for="model">{{ t('Model') }}</label><select id="model" v-model="model" :disabled="pending"><option v-for="item in models" :key="item.id" :value="item.id">{{ item.id }} · {{ t(item.ready ? 'ready' : 'Not ready') }}</option></select></div><div class="field"><label for="lang">{{ t('Language') }}</label><select id="lang" v-model="language" :disabled="pending"><option v-for="code in languages" :key="code" :value="code">{{ t(`language.${code}`) }}</option></select></div></div>
    <VoiceSelector v-model="voice" :voices="availableVoices" :disabled="pending"/>
    <p v-if="!selectedModel.ready" class="help-note">{{ t('This model is not ready. Check its status in Engines.') }}</p>
    <p v-if="active" class="help-note">{{ t('Previous generation: {text}', { text: active.input.text }) }} · {{ active.input.model }} · {{ active.input.voice }} · {{ active.input.language }}</p>
    <div class="toolbar"><button id="btn" class="primary" :disabled="pending || !!jobId || !canGenerate">{{ t(pending ? 'Generating…' : 'Generate speech') }}</button><button v-if="jobId" type="button" :disabled="pending" @click="resume">{{ t('Resume previous generation') }}</button><button v-if="jobId" id="cancelGeneration" type="button" :disabled="cancelling" @click="cancel">{{ t(cancelling ? 'Cancellation requested. Waiting for native computation to stop…' : 'Cancel generation') }}</button></div>
  </form>
  <div id="status" role="status" aria-live="polite"><template v-if="pending && snapshot"><span v-if="snapshot.context">{{ snapshot.context.model }} · {{ snapshot.context.effective_device }} · </span>{{ t(snapshot.stage) }} · {{ t('{seconds}s elapsed', { seconds: Math.floor(snapshot.elapsed_secs || 0) }) }}<span v-if="snapshot.frames"> · {{ t('{count} generated frames', { count: snapshot.frames }) }}</span><p v-if="snapshot.context?.provider === 'qwen3-gguf' && snapshot.context.effective_device === 'cpu'" class="muted">{{ t('CPU inference can be slow.') }}</p></template><span v-if="complete">{{ t('Completed. Audio is ready to play or download.') }}</span></div>
  <details v-if="snapshot?.context"><summary>{{ t('Generation diagnostics') }}</summary><p>{{ snapshot.context.provider }} · {{ snapshot.context.model }} · {{ snapshot.context.effective_device }} · {{ snapshot.context.voice }}</p><p v-if="snapshot.audio_secs">{{ t('Generated duration: {seconds}s', { seconds: snapshot.audio_secs.toFixed(2) }) }} · RTF {{ snapshot.rtf?.toFixed(2) ?? '—' }}</p><p v-if="snapshot.context.seed">{{ t('Seed: {seed}', { seed: snapshot.context.seed }) }}</p><p v-else>{{ t('Seed: not exposed by this API') }}</p></details>
  <p v-if="error" role="alert" class="feedback error">{{ renderMessage(error) }} <span v-if="jobId">{{ t('Use Resume monitoring or Cancel generation.') }}</span></p>
  <template v-if="audioUrl"><audio id="player" ref="player" :src="audioUrl" controls/><a id="downloadAudio" :href="audioUrl" :download="audioFilename">{{ t('Download generated audio') }}</a></template>
  <p v-else class="output-hint">{{ t('Your audio will appear here, ready to listen or download.') }}</p>
</template>
