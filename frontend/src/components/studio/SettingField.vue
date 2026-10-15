<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { t } from '../../i18n';
import type { SettingSchema, Json } from '../../services/contracts';
import { formatSetting, parseSetting, applyLabels } from '../../services/settings';
import { useStudio } from '../../composables/useStudio';
import { useTask } from '../../composables/useTask';
import FeedbackNotice from '../FeedbackNotice.vue';
const props = defineProps<{ meta: SettingSchema; value: Json | undefined }>();
const studio = useStudio(); const task = useTask(); const { pending, feedback } = task;
const baseline = ref(formatSetting(props.value));
const draft = ref<string | boolean | number>(baseline.value);
const dirty = computed(() => String(draft.value) !== String(baseline.value));
watch(() => props.value, value => { const edited = dirty.value; baseline.value = formatSetting(value); if (!edited) draft.value = baseline.value; });
const key = computed(() => `setting.${props.meta.path}.label`);
const label = computed(() => t(key.value) === key.value ? props.meta.path.split('.').at(-1)!.replaceAll('_', ' ') : t(key.value));
const controlId = computed(() => `setting-${props.meta.path}`);
const deviceOptions = computed(() => [{ id: 'default', name: t('System Default') }, ...studio.devices.value.filter(device => device.id !== 'default')]);
async function save() { await task.run(async () => { await studio.saveSetting(props.meta.path, parseSetting(props.meta, draft.value)); }, 'Saved.'); }
</script>
<template>
  <form class="setting" :class="{ dirty }" :data-path="meta.path" :aria-busy="pending" @submit.prevent="save">
    <div class="setting-head"><div><label :for="controlId" class="setting-title">{{ label }}</label><p class="setting-path">{{ meta.path }}</p></div><span class="apply" :class="`apply-${meta.apply}`">{{ t(applyLabels[meta.apply]) }}</span></div>
    <p :id="`${controlId}-description`" class="setting-desc">{{ t(meta.description) }}</p>
    <div class="setting-control">
      <label v-if="meta.type === 'boolean'" class="switch"><input :id="controlId" v-model="draft" type="checkbox" :aria-describedby="`${controlId}-description`" :disabled="pending"><span>{{ t(draft ? 'On' : 'Off') }}</span></label>
      <select v-else-if="meta.allowed_values?.length" :id="controlId" v-model="draft" :disabled="pending" :aria-describedby="`${controlId}-description`"><option v-for="option in meta.allowed_values" :key="option" :value="option">{{ option }}</option></select>
      <select v-else-if="meta.allowed_values_source === 'audio_devices'" :id="controlId" v-model="draft" :disabled="pending" :aria-describedby="`${controlId}-description`"><option v-if="!deviceOptions.some(option => option.id === draft)" :value="draft">{{ draft }}</option><option v-for="device in deviceOptions" :key="device.id" :value="device.id">{{ device.name }}</option></select>
      <div v-else-if="meta.path === 'audio.volume'" class="range-wrap"><input :id="controlId" v-model="draft" type="range" min="0" max="1" step="0.01" :disabled="pending" :aria-describedby="`${controlId}-description`"><output :for="controlId">{{ Number(draft).toFixed(2) }}</output></div>
      <input v-else :id="controlId" v-model="draft" :type="meta.type === 'integer' || meta.type === 'number' ? 'number' : 'text'" :min="meta.min" :max="meta.max" :step="meta.type === 'integer' ? 1 : 'any'" :disabled="pending" :aria-describedby="`${controlId}-description`">
      <button :disabled="pending || !dirty">{{ t(pending ? 'Saving…' : 'Save') }}</button>
    </div>
    <p v-if="meta.min !== undefined && meta.max !== undefined" class="field-hint">{{ t('Range {min} – {max}', { min: meta.min, max: meta.max }) }}</p><p v-if="meta.nullable" class="field-hint">{{ t('Empty clears the value') }}</p>
    <FeedbackNotice :feedback="feedback"/>
  </form>
</template>
