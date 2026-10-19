<script setup lang="ts">
import { defineAsyncComponent, ref, watch } from 'vue';
import { t } from '../../i18n';
import BaseTextarea from '../ui/BaseTextarea.vue';
import FileDropzone from '../ui/FileDropzone.vue';
import type { VoiceConstraints } from '../../services/contracts';
import type { ReferenceMetadata } from '../../features/cloning/validateReference';
const ReferenceEditor = defineAsyncComponent(() => import('../audio/ReferenceEditor.vue'));
const props = defineProps<{ disabled: boolean; provider: string; modes: string[]; file: File | null; originalFile: File | null; metadata: ReferenceMetadata | null; previewUrl: string; limits: VoiceConstraints | null; errors: Record<string, string>; needsTranscript: boolean }>();
const emit = defineEmits<{ select: [file: File | null]; trim: [file: File]; restore: [] }>();
const transcript = defineModel<string>('transcript', { required: true }), mode = defineModel<string>('mode', { required: true });
const editing = ref(false);
watch(() => props.disabled, value => { if (value) editing.value = false; });
function select(file: File | null) { editing.value = false; emit('select', file); }
function trim(file: File) { editing.value = false; emit('trim', file); }
</script>
<template><fieldset :disabled="disabled"><legend>{{ t('Reference recording') }}</legend>
  <p class="help-note">{{ t('Upload 3–10 seconds of clear speech. Only use a voice you own or have permission to use. Add the exact transcript for ICL mode; X-vector and audio-prompt modes only need the recording.') }}</p>
  <FileDropzone id="cloneFile" :label="t('Reference recording (WAV, 3–10 s recommended)')" :error="errors.cloneFile" :disabled="disabled" @select="select"/>
  <p v-if="file" class="muted">{{ file.name }} · {{ file.size }} bytes</p>
  <p v-if="metadata" role="status">{{ metadata.duration.toFixed(2) }} s · {{ metadata.sampleRate }} Hz · {{ metadata.channels }} {{ t('channels') }} · {{ metadata.bytes }} bytes · {{ metadata.encoding }}</p>
  <audio v-if="previewUrl" :src="previewUrl" controls :aria-label="t('Reference recording')"/>
  <div v-if="file && limits" class="toolbar">
    <button v-if="!editing" type="button" :disabled="disabled || file.size > limits.max_upload_bytes" @click="editing = true">{{ t('Waveform and trim') }}</button>
    <button v-if="originalFile && file !== originalFile" type="button" @click="editing = false; emit('restore')">{{ t('Restore original recording') }}</button>
  </div>
  <ReferenceEditor v-if="editing && file && limits" :file="file" :max-bytes="limits.max_upload_bytes" @close="editing = false" @apply="trim"/>
  <div class="form-group"><label for="cloneMode">{{ t('Mode') }}</label><select id="cloneMode" v-model="mode"><option v-for="item in modes" :key="item">{{ item }}</option></select></div>
  <p v-if="provider === 'qwen3-gguf'" class="help-note">{{ t('Qwen ICL needs the exact words, punctuation and language of the recording. A mismatched transcript can cause reference words to be repeated. Speaker-only mode is not validated.') }}</p>
  <p v-if="provider === 'pocket-tts'" class="help-note">{{ t('Pocket uses the recording without a transcript. Each language needs its own model; select a matching model and language.') }}</p>
  <p v-if="needsTranscript && originalFile && file !== originalFile" class="help-note">{{ t('Review the transcript to match the trimmed recording. The transcript has not been rewritten.') }}</p>
  <BaseTextarea v-if="needsTranscript" id="cloneTranscript" v-model="transcript" :label="t('Reference transcript (exact words in the recording)')" :error="errors.cloneTranscript" :help="`${[...transcript].length} / ${limits?.transcript_max_chars ?? '—'}`" required/>
</fieldset></template>
