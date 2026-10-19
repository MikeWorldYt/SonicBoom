<script setup lang="ts">
withDefaults(defineProps<{ type?: 'button' | 'submit' | 'reset'; variant?: 'primary' | 'secondary' | 'danger' | 'quiet'; disabled?: boolean; loading?: boolean }>(), { type: 'button', variant: 'secondary' });
</script>
<template><button :type="type" :class="{ primary: variant === 'primary', danger: variant === 'danger', 'btn-inline': variant === 'quiet' }" :disabled="disabled || loading" :aria-busy="loading || undefined"><span v-if="loading" class="button-spinner" aria-hidden="true"/><slot/></button></template>
