<script setup lang="ts">
import { computed } from 'vue';
import { t } from '../../i18n';
import type { ProviderStatus, Capabilities } from '../../services/contracts';
import { useStudio } from '../../composables/useStudio';
import { useTask } from '../../composables/useTask';
import FeedbackNotice from '../FeedbackNotice.vue';
const props = defineProps<{ provider: ProviderStatus; capabilities?: Capabilities }>();
const studio = useStudio();
const task = useTask();
const { pending, feedback } = task;
const togglePath = computed(() => `providers.${props.provider.provider_id.replaceAll('-', '_')}.enabled`);
</script>
<template>
  <article class="provider" :aria-busy="pending">
    <div class="provider-top"><h2 class="provider-name">{{ provider.provider_id }}</h2><span class="badge" :class="`badge-${provider.state === 'ready' ? 'ready' : provider.state === 'loading' ? 'loading' : provider.state === 'disabled' ? 'disabled' : 'not_installed'}`">{{ t(provider.state) }}</span><span v-if="provider.effective_device" class="device-chip">{{ provider.effective_device }}</span></div>
    <ul class="engine-models"><li v-for="model in capabilities?.models || []" :key="model"><code>{{ model }}</code><span v-if="provider.active_model === model" class="badge badge-ready">{{ t('Active') }}</span></li></ul>
    <p v-if="provider.detail">{{ t(provider.detail) }}</p>
    <p v-if="capabilities?.languages.length" class="muted">{{ t('{count} audio languages', { count: capabilities.languages.length }) }}</p>
    <div v-if="studio.schema.value[togglePath]" class="provider-actions"><button :disabled="pending" @click="task.run(() => studio.toggleProvider(provider))">{{ t(pending ? provider.enabled ? 'Stopping…' : 'Starting…' : provider.enabled ? 'Disable' : 'Enable') }}</button></div>
    <FeedbackNotice :feedback="feedback"/>
  </article>
</template>
