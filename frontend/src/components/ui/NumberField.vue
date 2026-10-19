<script setup lang="ts">
import { computed } from 'vue';
import BaseInput from './BaseInput.vue';
import { t } from '../../i18n';
import type { NumericBounds } from '../../services/contracts';
const props = defineProps<{ id: string; label: string; bounds?: NumericBounds; error?: string; disabled?: boolean; placeholder?: string }>();
const value = defineModel<string>({ default: '' });
const help = computed(() => props.bounds ? t(props.bounds.exclusive_min ? 'Greater than {min}, up to {max}' : 'Range {min} – {max}', { min: props.bounds.min, max: props.bounds.max }) : t('Sampling constraints are unavailable. Refresh the server status.'));
</script>
<template><BaseInput :id="id" v-model="value" :label="label" :help="help" :error="error" :disabled="disabled || !bounds" :inputmode="bounds?.integer ? 'numeric' : 'decimal'" :placeholder="placeholder" autocomplete="off"/></template>
