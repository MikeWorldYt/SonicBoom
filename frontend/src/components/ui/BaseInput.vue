<script setup lang="ts">
import FormField from './FormField.vue';
defineOptions({ inheritAttrs: false });
defineProps<{ id: string; label: string; help?: string; error?: string; required?: boolean; disabled?: boolean; loading?: boolean; type?: string }>();
const model = defineModel<string>({ default: '' });
</script>
<template><FormField :id="id" :label="label" :help="help" :error="error" :required="required" v-slot="{ describedby, invalid }"><input v-bind="$attrs" :id="id" v-model="model" :type="type || 'text'" :required="required" :disabled="disabled || loading" :aria-busy="loading || undefined" :aria-invalid="invalid || undefined" :aria-describedby="describedby || undefined"></FormField></template>
