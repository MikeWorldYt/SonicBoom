<script setup lang="ts">
import { computed, ref } from 'vue';
import { t } from '../../i18n';
import type { ProviderStatus, Capabilities } from '../../services/contracts';
import { getPath, useStudio } from '../../composables/useStudio';
import { useTask } from '../../composables/useTask';
import FeedbackNotice from '../FeedbackNotice.vue';
import HelpTip from '../ui/HelpTip.vue';
import StatusDot from '../ui/StatusDot.vue';
const props = defineProps<{ provider: ProviderStatus; capabilities?: Capabilities }>();
const studio = useStudio();
const task = useTask();
const { pending, feedback } = task;
const togglePath = computed(() => `providers.${props.provider.provider_id.replaceAll('-', '_')}.enabled`);
const isPocket = computed(() => props.provider.provider_id === 'pocket-tts');
const selectedPocketModel = computed(() => {
  const value = getPath(studio.config.value?.config, 'providers.pocket_tts.model_id') || props.provider.active_model;
  return typeof value === 'string' ? value : '';
});
const switching = computed(() => isPocket.value && props.provider.enabled && props.provider.state !== 'error' && (
  props.provider.state === 'loading' || (selectedPocketModel.value && props.provider.active_model !== selectedPocketModel.value)
));
function active(model: string) {
  if (!isPocket.value) return props.provider.active_model === model;
  return props.provider.enabled && props.provider.state === 'ready' && props.provider.active_model === model && selectedPocketModel.value === model;
}
// Draft selection for the pocket model picker; applying is always explicit
// through the Activate button, and the draft resets once config catches up.
const pocketDraft = ref<string | null>(null);
const pocketChoice = computed({
  get: () => pocketDraft.value ?? selectedPocketModel.value ?? props.provider.active_model ?? '',
  set: value => { pocketDraft.value = value; },
});
async function activateChoice() {
  const model = pocketChoice.value;
  if (!model || active(model)) return;
  await task.run(() => studio.activatePocketModel(model));
  pocketDraft.value = null;
}
const vulkanError = computed(() => props.provider.backend_diagnostics?.vulkan_initialization_error || '');
const cpuFallback = computed(() => props.provider.backend_diagnostics?.cpu_fallback_reason || '');
</script>
<template>
  <article class="provider" :aria-busy="pending">
    <div class="provider-top"><h2 class="provider-name">{{ provider.provider_id }}</h2><span class="badge" :class="`badge-${provider.state === 'ready' ? 'ready' : provider.state === 'loading' ? 'loading' : provider.state === 'disabled' ? 'disabled' : 'not_installed'}`">{{ t(provider.state) }}</span><span v-if="provider.effective_device" class="device-chip">{{ provider.effective_device }}</span></div>
    <div class="engine-readiness-row"><ul class="engine-readiness" aria-label="Engine status"><li><StatusDot :state="provider.enabled ? 'ok' : 'bad'" :label="t(provider.enabled ? 'Engine enabled' : 'Engine disabled')" /></li><li><StatusDot :state="provider.active_model ? 'ok' : 'bad'" :label="t(provider.active_model ? 'Model loaded' : 'No model loaded')" /></li><li><StatusDot :state="provider.state === 'ready' ? 'ok' : 'warn'" :label="t(provider.state === 'ready' ? 'Ready to generate' : 'Not ready')" /></li></ul><HelpTip :label="t('About engine status')" :text="t('An engine must be enabled, have a model loaded, and report ready before it can generate.')" /></div>
    <p v-if="provider.active_model && !isPocket" class="engine-active-model">{{ t('Active model:') }} <code>{{ provider.active_model }}</code></p>
    <details v-if="isPocket" class="engine-about"><summary>{{ t('About this engine') }}</summary><p class="help-note">{{ t('Activate a language model to download it if needed. Pocket uses one active model at a time; switching keeps your voices and downloaded models.') }}</p></details>
    <div v-if="isPocket" class="engine-model-picker"><label :for="`engine-model-${provider.provider_id}`">{{ t('Model') }}<select :id="`engine-model-${provider.provider_id}`" v-model="pocketChoice" :disabled="pending || switching || !studio.schema.value['providers.pocket_tts.model_id']"><option v-for="model in capabilities?.models || []" :key="model" :value="model">{{ model }}</option></select></label><button type="button" class="engine-activate" :disabled="pending || switching || !pocketChoice || active(pocketChoice) || !studio.schema.value['providers.pocket_tts.model_id']" @click="activateChoice">{{ t(switching ? 'Loading…' : 'Activate') }}</button></div>
    <ul v-else class="engine-models"><li v-for="model in capabilities?.models || []" :key="model">
      <code>{{ model }}</code>
      <span v-if="active(model)" class="badge badge-ready">{{ t('Active') }}</span>
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
