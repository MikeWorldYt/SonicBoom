<script setup lang="ts">
import { computed, nextTick, provide, ref } from 'vue';
import { t } from '../i18n';
import type { HomeData } from '../types';
import { createStudio, studioKey } from '../composables/useStudio';
import StudioLayout, { type StudioTab } from '../components/StudioLayout.vue';
import SpeechGenerator from '../components/studio/SpeechGenerator.vue';
import VoiceLibrary from '../components/studio/VoiceLibrary.vue';
import EnginesPanel from '../components/studio/EnginesPanel.vue';
import SettingsPanel from '../components/studio/SettingsPanel.vue';
import FeedbackNotice from '../components/FeedbackNotice.vue';
const props = defineProps<{ data: HomeData }>();
const studio = createStudio(props.data); provide(studioKey, studio);
const { token, info, loading, errors } = studio;
const active = ref<StudioTab>('create');
const modelStatus = computed(() => info.value?.model.status || (props.data.modelReady ? 'ready' : 'idle'));
async function compose() { active.value = 'create'; await nextTick(); document.getElementById('text')?.focus(); }
</script>
<template>
  <StudioLayout v-model="active">
    <template #actions><span v-if="modelStatus !== 'ready'" id="modelStatus" class="model-status">{{ t(modelStatus === 'failed' ? 'Model load failed. Check server logs.' : modelStatus === 'downloading' ? 'Downloading model…' : modelStatus === 'loading' ? 'Loading model...' : 'Preparing model...') }}</span><button :disabled="loading" @click="studio.refresh()">{{ t('Refresh') }}</button></template>
    <template #notice><div v-if="info?.auth.required" id="tokenGroup" class="access-notice"><label for="apiToken">{{ t('API token') }}</label><p class="muted"><a href="/admin">{{ t('create one in the admin panel') }}</a></p><input id="apiToken" v-model.lazy="token" type="password" :placeholder="t('Paste your API token here')" autocomplete="off" spellcheck="false"></div><FeedbackNotice :feedback="errors.info ? { kind: 'error', message: errors.info } : null"/></template>
    <template #create><SpeechGenerator/></template>
    <template #voices><VoiceLibrary @compose="compose" @engines="active = 'engines'"/></template>
    <template #engines><EnginesPanel/></template>
    <template #settings><SettingsPanel/></template>
  </StudioLayout>
</template>
