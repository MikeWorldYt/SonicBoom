<script setup lang="ts">
import { nextTick, onErrorCaptured, ref } from 'vue';
import { t } from '../../i18n';
const failed = ref(false), requestId = ref('');
onErrorCaptured(error => {
  failed.value = true;
  requestId.value = error && typeof error === 'object' && 'requestId' in error && typeof error.requestId === 'string' ? error.requestId : '';
  void nextTick(() => document.getElementById('interface-error')?.focus());
  return false;
});
</script>
<template><main v-if="failed" id="main-content"><section id="interface-error" tabindex="-1" class="card feedback error" role="alert" aria-labelledby="interface-error-title"><h1 id="interface-error-title">{{ t('The interface could not load.') }}</h1><p>{{ t('Try opening this view again. Existing server jobs are not automatically restarted.') }}</p><p v-if="requestId">{{ t('Request ID: {id}', { id: requestId }) }}</p><button type="button" @click="failed = false">{{ t('Try again') }}</button></section></main><slot v-else/></template>
