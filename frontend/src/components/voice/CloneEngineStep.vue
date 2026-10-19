<script setup lang="ts">
import BaseSelect from '../ui/BaseSelect.vue';
import type { Capabilities } from '../../services/contracts';
import { t } from '../../i18n';
defineProps<{ choices: Capabilities[]; cap?: Capabilities; languages: string[]; ready: boolean; disabled: boolean }>();
const provider = defineModel<string>('provider', { required: true }), model = defineModel<string>('model', { required: true }), language = defineModel<string>('language', { required: true });
</script>
<template><fieldset :disabled="disabled"><legend class="sr-only">{{ t('Choose an engine') }}</legend>
  <div class="row"><BaseSelect id="cloneProvider" v-model="provider" :label="t('Engine')" :options="choices.length ? choices.map(choice => ({ value: choice.provider_id, label: choice.provider_id })) : [{ value: '', label: t('No cloning engines available') }]"/><BaseSelect id="cloneModel" v-model="model" :label="t('Model')" :options="(cap?.models || []).map(value => ({ value, label: value }))"/></div>
  <BaseSelect id="cloneLang" v-model="language" :label="t('Language')" :options="languages.map(value => ({ value, label: t(`language.${value}`) }))"/>
  <p v-if="!ready" class="help-note">{{ t('You can save a reference now. Enable its engine before previewing or generating audio.') }}</p>
</fieldset></template>
