<script setup lang="ts">
defineProps<{ state: 'ok' | 'warn' | 'bad'; label: string }>();
</script>
<template><span class="status-dot" :class="`status-${state}`"><span class="dot" aria-hidden="true" />{{ label }}</span></template>
