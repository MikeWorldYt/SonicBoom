<script setup lang="ts">
import { ref } from 'vue';
import FormField from './FormField.vue';
import { t } from '../../i18n';
defineProps<{ id: string; label: string; error?: string; disabled?: boolean; accept?: string; help?: string; filled?: boolean }>();
const emit = defineEmits<{ select: [file: File | null] }>();
const dragging = ref(false);
const input = ref<HTMLInputElement | null>(null);
function drop(event: DragEvent) { dragging.value = false; emit('select', event.dataTransfer?.files[0] || null); }
function onChange(event: Event) {
  const file = (event.target as HTMLInputElement).files?.[0] || null;
  // Clearing the native control keeps the same file selectable again later.
  if (input.value) input.value.value = '';
  emit('select', file);
}
function open() { input.value?.click(); }
defineExpose({ open });
</script>
<template><FormField :id="id" :label="label" :error="error" :help="help ?? t('Drop a WAV here or choose a file.')" v-slot="{ describedby, invalid }"><div class="file-dropzone" :class="{ dragging, filled }" @dragover.prevent="!disabled && (dragging = true)" @dragleave="dragging = false" @drop.prevent="!disabled && drop($event)"><input :id="id" ref="input" class="file-dropzone-input" type="file" :accept="accept || 'audio/wav,.wav'" :disabled="disabled" :aria-describedby="describedby || undefined" :aria-invalid="invalid || undefined" @change="onChange"><slot/></div></FormField></template>
