<script setup lang="ts">
import { renderMessage } from '../i18n';
import type { Feedback } from '../composables/useTask';
defineProps<{ feedback: Feedback | null }>();
</script>
<template><div v-if="feedback" class="feedback" :class="feedback.kind" :role="feedback.kind === 'error' ? 'alert' : 'status'">{{ renderMessage(feedback.message) }}</div></template>
