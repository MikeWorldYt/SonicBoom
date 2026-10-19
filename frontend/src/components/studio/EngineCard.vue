<script setup lang="ts">
import { computed } from 'vue';
import { t } from '../../i18n';
import type { ProviderStatus, Capabilities } from '../../services/contracts';
import { getPath, useStudio } from '../../composables/useStudio';
import { useTask } from '../../composables/useTask';
import FeedbackNotice from '../FeedbackNotice.vue';
const props = defineProps<{ provider: ProviderStatus; capabilities?: Capabilities }>();
const studio = useStudio();
const task = useTask();
const { pending, feedback } = task;
const togglePath = computed(() => `providers.${props.provider.provider_id.replaceAll('-', '_')}.enabled`);
const isPocket = computed(() => props.provider.provider_id === 'pocket-tts');
const selectedPocketModel = computed(() => getPath(studio.config.value?.config, 'providers.pocket_tts.model_id') || props.provider.active_model);
const switching = computed(() => isPocket.value && props.provider.enabled && props.provider.state !== 'error' && (
  props.provider.state === 'loading' || (selectedPocketModel.value && props.provider.active_model !== selectedPocketModel.value)
));
function active(model: string) {
  if (!isPocket.value) return props.provider.active_model === model;
  return props.provider.enabled && props.provider.state === 'ready' && props.provider.active_model === model && selectedPocketModel.value === model;
}
const vulkanError = computed(() => props.provider.backend_diagnostics?.vulkan_initialization_error || '');
const cpuFallback = computed(() => props.provider.backend_diagnostics?.cpu_fallback_reason || '');
const headline = computed(() => {
  if (props.provider.state === 'ready') return t('Ready to generate');
  if (props.provider.state === 'loading') return t('Loading…');
  if (props.provider.state === 'disabled' || !props.provider.enabled) return t('Disabled');
  return t('Not available');
});
</script>
<template>
  <article class="provider" :aria-busy="pending">
    <div class="provider-top"><h2 class="provider-name">{{ provider.provider_id }}</h2><span class="badge" :class="`badge-${provider.state === 'ready' ? 'ready' : provider.state === 'loading' ? 'loading' : provider.state === 'disabled' ? 'disabled' : 'not_installed'}`">{{ t(provider.state) }}</span><span v-if="provider.effective_device" class="device-chip">{{ provider.effective_device }}</span></div>
    <p class="engine-headline" role="status">{{ headline }}<span v-if="provider.active_model"> · {{ t('Active model:') }} {{ provider.active_model }}</span></p>
    <ul class="engine-readiness" aria-label="Engine status"><li :class="{ met: provider.enabled }">{{ t(provider.enabled ? 'Engine enabled' : 'Engine disabled') }}</li><li :class="{ met: !!provider.active_model }">{{ t(provider.active_model ? 'Model loaded' : 'No model loaded') }}</li><li :class="{ met: provider.state === 'ready' }">{{ t(provider.state === 'ready' ? 'Ready to generate' : 'Not ready') }}</li></ul>
    <p v-if="isPocket" class="help-note">{{ t('Activate a language model to download it if needed. Pocket uses one active model at a time; switching keeps your voices and downloaded models.') }}</p>
    <ul class="engine-models"><li v-for="model in capabilities?.models || []" :key="model">
      <code>{{ model }}</code>
      <span v-if="active(model)" class="badge badge-ready">{{ t('Active') }}</span>
      <span v-else-if="isPocket && provider.enabled && selectedPocketModel === model" class="badge" :class="switching ? 'badge-loading' : 'badge-not_installed'">{{ t(switching ? 'Loading…' : 'Selected') }}</span>
      <button v-else-if="isPocket" class="engine-activate" :disabled="pending || switching || !studio.schema.value['providers.pocket_tts.model_id']" :aria-label="t('Activate {model}', { model })" @click="task.run(() => studio.activatePocketModel(model))">{{ t('Activate') }}</button>
    </li></ul>
    <p v-if="switching" role="status" class="help-note">{{ t('Preparing the selected model. This page updates automatically; the first download may take a few minutes.') }}</p>
    <p v-if="provider.detail" class="engine-detail">{{ t(provider.detail) }}</p>
    <details v-if="provider.backend_diagnostics || vulkanError || cpuFallback" class="engine-solution"><summary>{{ t('View solution') }}</summary>
    <dl v-if="provider.backend_diagnostics" class="help-note">
      <dt>{{ provider.backend_diagnostics.status }}</dt>
      <dd>{{ provider.backend_diagnostics.vulkan_gpu_name }}</dd>
      <dt>{{ t('Compiled / available') }}</dt><dd>{{ provider.backend_diagnostics.compiled_backends.join(', ') }} / {{ provider.backend_diagnostics.available_runtime_backends.join(', ') }}</dd>
      <dt>{{ t('Requested / effective') }}</dt><dd>{{ provider.backend_diagnostics.requested_device }} / {{ provider.backend_diagnostics.effective_device || '—' }}</dd>
      <dt>{{ t('Model') }}</dt><dd>{{ provider.backend_diagnostics.model_loading_status }}</dd>
      <dd v-if="cpuFallback">{{ cpuFallback }}</dd>
      <dd v-else-if="vulkanError">{{ vulkanError }}</dd>
    </dl>
    <p v-if="vulkanError" class="help-note">{{ t('This build cannot start with Vulkan on this machine. Enable the engine only if CPU inference is supported for this model.') }}</p>
    </details>
    <p v-if="capabilities?.languages.length" class="muted">{{ t('{count} audio languages', { count: capabilities.languages.length }) }}</p>
    <div v-if="studio.schema.value[togglePath]" class="provider-actions"><button :disabled="pending" @click="task.run(() => studio.toggleProvider(provider))">{{ t(pending ? provider.enabled ? 'Stopping…' : 'Starting…' : provider.enabled ? 'Disable' : 'Enable') }}</button></div>
    <FeedbackNotice :feedback="feedback"/>
  </article>
</template>
