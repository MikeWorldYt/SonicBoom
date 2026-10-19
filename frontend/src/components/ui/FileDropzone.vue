<script setup lang="ts">
import { ref } from 'vue';
import FormField from './FormField.vue';
import { t } from '../../i18n';
defineProps<{ id: string; label: string; error?: string; disabled?: boolean; accept?: string }>();
const emit = defineEmits<{ select: [file: File | null] }>();
const dragging = ref(false);
function drop(event: DragEvent) { dragging.value = false; emit('select', event.dataTransfer?.files[0] || null); }
</script>
<template><FormField :id="id" :label="label" :error="error" :help="t('Drop a WAV here or choose a file.')" v-slot="{ describedby, invalid }"><div class="file-dropzone" :class="{ dragging }" @dragover.prevent="!disabled && (dragging = true)" @dragleave="dragging = false" @drop.prevent="!disabled && drop($event)"><input :id="id" type="file" :accept="accept || 'audio/wav,.wav'" :disabled="disabled" :aria-describedby="describedby" :aria-invalid="invalid || undefined" @change="emit('select', ($event.target as HTMLInputElement).files?.[0] || null)"></div></FormField></template>
