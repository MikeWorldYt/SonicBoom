<script setup lang="ts">
import { t, uiError } from '../../i18n';
import type { Voice } from '../../services/contracts';
import { useStudio } from '../../composables/useStudio';
import { useTask } from '../../composables/useTask';
import CloneWizard from './CloneWizard.vue';
import FeedbackNotice from '../FeedbackNotice.vue';
const emit = defineEmits<{ compose: []; engines: [] }>();
const studio = useStudio(); const { voices, errors } = studio;
const task = useTask(); const { pending, feedback } = task;
async function select(voice: Voice, preview: boolean) {
  await task.run(async () => {
    if (!studio.models.value.some(model => model.id === voice.model && model.ready)) throw uiError('Model {model} is not ready — enable its engine first.', { model: voice.model });
    studio.selectProfile(voice, preview); emit('compose');
  });
}
async function remove(voice: Voice) {
  if (!confirm(t('Delete this cloned voice? Its reference audio is removed too.'))) return;
  await task.run(async () => { await studio.api.deleteVoice(voice.id); await studio.refreshVoices(); });
}
</script>
<template>
  <FeedbackNotice :feedback="errors.voices ? { kind: 'error', message: errors.voices } : null"/>
  <div id="voicesList" class="voice-list"><p v-if="!voices.length && !errors.voices" class="muted">{{ t('No cloned voices yet — upload a reference below.') }}</p><article v-for="voice in voices" :key="voice.id" class="voice-row"><div><h2 class="voice-name">{{ voice.name }}</h2><p class="voice-meta">{{ voice.provider }} · {{ voice.model }} · {{ t(`language.${voice.language}`) }} · {{ voice.clone_mode }}<span v-if="voice.reference_duration_secs"> · {{ voice.reference_duration_secs.toFixed(1) }}s {{ t('reference') }}</span></p></div><div class="voice-actions"><button :disabled="pending" @click="select(voice, false)">{{ t('Use') }}</button><button :disabled="pending" @click="select(voice, true)">{{ t('Preview') }}</button><button class="danger" :disabled="pending" @click="remove(voice)">{{ t('Delete') }}</button></div></article></div>
  <FeedbackNotice :feedback="feedback"/>
  <CloneWizard @engines="emit('engines')"/>
</template>
