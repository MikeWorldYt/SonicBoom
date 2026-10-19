<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { t, errorMessage, renderMessage } from '../../i18n';
import type { SettingSchema, Json } from '../../services/contracts';
import { formatSetting, parseSetting, applyLabels } from '../../services/settings';
import { useStudio } from '../../composables/useStudio';
import { useTask } from '../../composables/useTask';
import FeedbackNotice from '../FeedbackNotice.vue';
const props = defineProps<{ meta: SettingSchema; value: Json | undefined }>();
const studio = useStudio(); const task = useTask(); const { pending, feedback } = task;
const baseline = ref(formatSetting(props.value));
const draft = ref<string | boolean | number>(baseline.value);
const conflict = ref(false);
const dirty = computed(() => String(draft.value) !== String(baseline.value));
watch(() => props.value, value => {
  const edited = dirty.value, incoming = formatSetting(value);
  if (edited && String(incoming) !== String(baseline.value)) conflict.value = true;
  baseline.value = incoming;
  if (!edited) draft.value = incoming;
});
function useServer() { draft.value = baseline.value; conflict.value = false; feedback.value = null; }
const key = computed(() => `setting.${props.meta.path}.label`);
const label = computed(() => t(key.value) === key.value ? props.meta.path.split('.').at(-1)!.replaceAll('_', ' ') : t(key.value));
const localError = computed(() => {
  if (!dirty.value) return null;
  try { parseSetting(props.meta, draft.value); return null; } catch (error) { return errorMessage(error); }
});
const applying = computed(() => studio.status.value?.pending_applies?.includes(props.meta.path));
const applyFailure = computed(() => studio.status.value?.apply_failures?.find(failure => failure.path === props.meta.path));
const needsRestart = computed(() => props.meta.apply === 'restart_process' && studio.status.value?.pending_restart);
const describedBy = computed(() => `setting-${props.meta.path}-description${localError.value ? ' setting-' + props.meta.path + '-error' : ''}`);
const controlId = computed(() => `setting-${props.meta.path}`);
const deviceOptions = computed(() => [{ id: 'default', name: t('System Default') }, ...studio.devices.value.filter(device => device.id !== 'default')]);
async function save() { if (conflict.value || localError.value) return; await task.run(async () => { await studio.saveSetting(props.meta.path, parseSetting(props.meta, draft.value)); }, 'Saved.'); }
</script>
<template>
  <form class="setting" :class="{ dirty }" :data-path="meta.path" :aria-busy="pending" @submit.prevent="save">
    <div class="setting-head"><div><label :for="controlId" class="setting-title">{{ label }}</label><p class="setting-path">{{ meta.path }}</p></div><span class="apply" :class="`apply-${meta.apply}`">{{ t(applyLabels[meta.apply]) }}</span></div>
    <p :id="`${controlId}-description`" class="setting-desc">{{ t(meta.description) }}</p>
    <div class="setting-control">
      <label v-if="meta.type === 'boolean' && !meta.nullable" class="switch"><input :id="controlId" v-model="draft" type="checkbox" :aria-describedby="describedBy" :aria-invalid="!!localError || undefined" :disabled="pending"><span>{{ t(draft ? 'On' : 'Off') }}</span></label>
      <select v-else-if="meta.type === 'boolean' && meta.nullable" :id="controlId" v-model="draft" :disabled="pending" :aria-describedby="describedBy" :aria-invalid="!!localError || undefined"><option value="">{{ t('Empty clears the value') }}</option><option :value="true">{{ t('On') }}</option><option :value="false">{{ t('Off') }}</option></select>
      <select v-else-if="meta.allowed_values?.length"  :id="controlId" v-model="draft" :disabled="pending" :aria-describedby="describedBy" :aria-invalid="!!localError || undefined"><option v-if="meta.nullable" value="">{{ t('Empty clears the value') }}</option><option v-for="option in meta.allowed_values" :key="option" :value="option">{{ option }}</option></select>
      <select v-else-if="meta.allowed_values_source === 'audio_devices'" :id="controlId" v-model="draft" :disabled="pending" :aria-describedby="describedBy" :aria-invalid="!!localError || undefined"><option v-if="!deviceOptions.some(option => option.id === draft)" :value="draft">{{ draft }}</option><option v-for="device in deviceOptions" :key="device.id" :value="device.id">{{ device.name }}</option></select>
      <div v-else-if="meta.path === 'audio.volume'" class="range-wrap"><input :id="controlId" v-model="draft" type="range" min="0" max="1" step="0.01" :disabled="pending" :aria-describedby="describedBy" :aria-invalid="!!localError || undefined"><output :for="controlId">{{ Number(draft).toFixed(2) }}</output></div>
      <input v-else :id="controlId" v-model="draft" :type="meta.type === 'integer' || meta.type === 'number' ? 'number' : 'text'" :min="meta.min" :max="meta.max" :step="meta.type === 'integer' ? 1 : 'any'" :disabled="pending" :aria-describedby="describedBy" :aria-invalid="!!localError || undefined">
      <button :disabled="pending || !dirty || conflict || !!localError">{{ t(pending ? 'Saving…' : 'Save') }}</button>
    </div>
    <p v-if="localError" :id="`${controlId}-error`" role="alert" class="field-error">{{ renderMessage(localError) }}</p>
    <p v-if="applyFailure" role="alert" class="field-error">{{ t('Apply failed: {error}', { error: applyFailure.error }) }}</p><p v-else-if="applying" role="status">{{ t('Applying…') }}</p><p v-else-if="needsRestart" role="status">{{ t('Restart required') }}</p>
    <p v-if="meta.min !== undefined && meta.max !== undefined" class="field-hint">{{ t('Range {min} – {max}', { min: meta.min, max: meta.max }) }}</p><p v-if="meta.nullable" class="field-hint">{{ t('Empty clears the value') }}</p>
    <div v-if="conflict" role="alert" class="feedback error"><p>{{ t('This setting changed on the server. Review the new value before saving your draft.') }}</p><p>{{ t('Server value: {value}', { value: String(baseline) }) }}</p><div class="toolbar"><button type="button" @click="conflict = false">{{ t('Keep my draft') }}</button><button type="button" @click="useServer">{{ t('Use server value') }}</button></div></div>
    <FeedbackNotice :feedback="feedback"/>
  </form>
</template>
