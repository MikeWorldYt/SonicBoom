<script setup lang="ts">
defineProps<{ title: string; hint?: string }>();
</script>
<template>
  <div class="empty-state">
    <p class="empty-title">{{ title }}</p>
    <p v-if="hint" class="muted">{{ hint }}</p>
    <slot />
  </div>
</template>
