<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { t, uiError } from '../../i18n';
import { useStudio } from '../../composables/useStudio';
import { modelLanguages } from '../../services/providerLanguages';
import { useTask } from '../../composables/useTask';
import FeedbackNotice from '../FeedbackNotice.vue';
const emit = defineEmits<{ created: []; engines: [] }>();
const studio = useStudio();
const { capabilities, providers } = studio;
const task = useTask(); const { pending, feedback } = task;
const step = ref(0);
const provider = ref(''); const model = ref(''); const language = ref('en'); const mode = ref('icl');
const fileInput = ref<HTMLInputElement | null>(null);
const name = ref(''); const consent = ref(''); const transcript = ref(''); const file = ref<File | null>(null);
const choices = computed(() => capabilities.value.filter(cap => cap.requires_reference_audio));
const cap = computed(() => choices.value.find(item => item.provider_id === provider.value));
const languages = computed(() => modelLanguages(cap.value, model.value));
watch(languages, value => { if (!value.includes(language.value)) language.value = value[0] || 'en'; });
const needsTranscript = computed(() => mode.value === 'icl' || !!cap.value?.requires_reference_transcript);
const engineReady = computed(() => providers.value.some(item => item.provider_id === provider.value && item.state === 'ready'));
watch(choices, value => { if (!value.some(item => item.provider_id === provider.value)) provider.value = value[0]?.provider_id || ''; }, { immediate: true });
watch(cap, value => {
  if (!value?.models.includes(model.value)) model.value = value?.models[0] || '';
  if (!value?.languages.includes(language.value)) language.value = value?.languages[0] || 'en';
  if (!value?.clone_modes.includes(mode.value)) mode.value = value?.clone_modes[0] || 'icl';
}, { immediate: true });
watch(needsTranscript, value => { if (!value) transcript.value = ''; });
function selectFile(event: Event) { file.value = (event.target as HTMLInputElement).files?.[0] || null; }
async function validateRecording() {
  if (!file.value) throw uiError('Choose a WAV reference recording.');
  if (!file.value.size) throw uiError('The selected file is empty.');
  const bytes = new Uint8Array(await file.value.slice(0, 12).arrayBuffer());
  const tag = (offset: number) => String.fromCharCode(...bytes.slice(offset, offset + 4));
  if (tag(0) !== 'RIFF' || tag(8) !== 'WAVE') throw uiError('The selected file is not a WAV recording.');
  if (needsTranscript.value && !transcript.value.trim()) throw uiError('ICL mode needs the exact reference transcript.');
}
async function next() {
  await task.run(async () => {
    if (step.value === 0) { if (!cap.value) throw uiError('No cloning engine is available right now.'); if (!model.value) throw uiError('Pick a model.'); }
    else await validateRecording();
    step.value++;
  });
}
async function save() {
  await task.run(async () => {
    if (!name.value.trim() || name.value.trim().length > 64) throw uiError('Name must be 1–64 characters.');
    if (!consent.value.trim()) throw uiError('Consent basis is required (e.g. owner).');
    await validateRecording();
    const profile = await studio.api.createVoice({ name: name.value.trim(), provider: provider.value, model: model.value, language: language.value, mode: mode.value, consent_basis: consent.value.trim(), ...(needsTranscript.value ? { transcript: transcript.value.trim() } : {}) }, file.value!);
    await studio.refreshVoices();
    studio.selectProfile(profile);
    step.value = 0; name.value = ''; consent.value = ''; transcript.value = ''; file.value = null; if (fileInput.value) fileInput.value.value = '';
    emit('created');
  }, 'Voice saved. Preview it before using it in a longer recording.');
}
</script>
<template>
  <section class="clone-wizard" aria-labelledby="clone-heading"><h2 id="clone-heading">{{ t('Add a voice') }}</h2>
    <ol class="stepper" :aria-label="t('Voice setup progress')"><li v-for="(label, index) in ['Choose an engine', 'Reference recording', 'Review and save']" :key="label" :aria-current="step === index ? 'step' : undefined" :class="{ current: step === index }">{{ index + 1 }}. {{ t(label) }}</li></ol>
    <form id="cloneForm" :aria-busy="pending" @submit.prevent="step < 2 ? next() : save()">
      <fieldset v-show="step === 0" :disabled="step !== 0 || pending"><legend>{{ t('Choose an engine') }}</legend>
        <div class="row"><div class="field"><label for="cloneProvider">{{ t('Engine') }}</label><select id="cloneProvider" v-model="provider"><option v-if="!choices.length" value="">{{ t('No cloning engines available') }}</option><option v-for="choice in choices" :key="choice.provider_id" :value="choice.provider_id">{{ choice.provider_id }}</option></select></div><div class="field"><label for="cloneModel">{{ t('Model') }}</label><select id="cloneModel" v-model="model"><option v-for="item in cap?.models || []" :key="item" :value="item">{{ item }}</option></select></div></div>
        <label for="cloneLang">{{ t('Language') }}</label><select id="cloneLang" v-model="language"><option v-for="code in languages" :key="code" :value="code">{{ t(`language.${code}`) }}</option></select>
        <p v-if="!engineReady" class="help-note">{{ t('You can save a reference now. Enable its engine before previewing or generating audio.') }}</p>
      </fieldset>
      <fieldset v-show="step === 1" :disabled="step !== 1 || pending"><legend>{{ t('Reference recording') }}</legend><p class="help-note">{{ t('Upload 3–10 seconds of clear speech. Only use a voice you own or have permission to use. Add the exact transcript for ICL mode; X-vector and audio-prompt modes only need the recording.') }}</p>
        <div class="form-group"><label for="cloneFile">{{ t('Reference recording (WAV, 3–10 s recommended)') }}</label><input id="cloneFile" ref="fileInput" type="file" accept="audio/wav,.wav" @change="selectFile"><p v-if="file" class="muted">{{ file.name }} · {{ Math.ceil(file.size / 1024) }} KB</p></div>
        <div class="form-group"><label for="cloneMode">{{ t('Mode') }}</label><select id="cloneMode" v-model="mode"><option v-for="item in cap?.clone_modes.length ? cap.clone_modes : ['icl']" :key="item" :value="item">{{ item }}</option></select></div>
        <div v-if="needsTranscript" class="form-group"><label for="cloneTranscript">{{ t('Reference transcript (exact words in the recording)') }}</label><textarea id="cloneTranscript" v-model="transcript"/></div>
      </fieldset>
      <fieldset v-show="step === 2" :disabled="step !== 2 || pending"><legend>{{ t('Review and save') }}</legend><p class="help-note">{{ model }} · {{ t(`language.${language}`) }} · {{ file?.name }}</p>
        <div class="form-group"><label for="cloneName">{{ t('Name') }}</label><input id="cloneName" v-model="name" maxlength="64" autocomplete="off"></div>
        <div class="form-group"><label for="cloneConsent">{{ t('Consent basis') }}</label><input id="cloneConsent" v-model="consent" :placeholder="t('I own this voice or have permission')" autocomplete="off"></div>
      </fieldset>
      <div class="toolbar"><button v-if="step > 0" type="button" :disabled="pending" @click="step--; feedback = null">{{ t('Back') }}</button><button v-if="step < 2" class="primary" :disabled="pending || !cap">{{ t('Continue') }}</button><button v-else id="cloneSubmit" class="primary" :disabled="pending">{{ t(pending ? 'Cloning…' : 'Clone voice') }}</button><button v-if="step === 0 && !engineReady" type="button" class="btn-inline" @click="emit('engines')">{{ t('Manage engines') }}</button></div>
    </form><FeedbackNotice :feedback="feedback"/>
  </section>
</template>
