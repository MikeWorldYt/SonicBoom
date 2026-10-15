<script setup lang="ts">
import { t } from '../../i18n';
import { useStudio } from '../../composables/useStudio';
import EngineCard from './EngineCard.vue';
import FeedbackNotice from '../FeedbackNotice.vue';
const { providers, capabilities, diagnostics, errors } = useStudio();
</script>
<template>
  <p class="help-note">{{ t('Enable an engine to download its models and make its voices available in the studio. The first download may take a few minutes.') }}</p>
  <FeedbackNotice :feedback="errors.providers ? { kind: 'error', message: errors.providers } : null"/>
  <div class="provider-list"><EngineCard v-for="provider in providers" :key="provider.provider_id" :provider="provider" :capabilities="capabilities.find(cap => cap.provider_id === provider.provider_id)"/></div>
  <details><summary>{{ t('Advanced inference diagnostics') }}</summary><pre id="inferenceDiagnostics">{{ JSON.stringify(diagnostics, null, 2) }}</pre></details>
</template>
