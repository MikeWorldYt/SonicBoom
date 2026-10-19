<script setup lang="ts">
import FormField from './FormField.vue';
defineOptions({ inheritAttrs: false });
defineProps<{ id: string; label: string; help?: string; error?: string; required?: boolean; disabled?: boolean; loading?: boolean }>();
const model = defineModel<string>({ default: '' });
</script>
<template><FormField :id="id" :label="label" :help="help" :error="error" :required="required"><template #hint><slot name="hint"/></template><template #default="{ describedby, invalid }"><textarea v-bind="$attrs" :id="id" v-model="model" :required="required" :disabled="disabled || loading" :aria-busy="loading || undefined" :aria-invalid="invalid || undefined" :aria-describedby="describedby || undefined"/></template></FormField></template>
