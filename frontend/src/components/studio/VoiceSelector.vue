<script setup lang="ts">
import { computed, ref } from 'vue';
import { t } from '../../i18n';
const selected = defineModel<string>({ required: true });
const props = defineProps<{ voices: { id: string; name: string; detail: string; cloned: boolean }[]; disabled?: boolean }>();
const query = ref('');
const filtered = computed(() => props.voices.filter(voice => `${voice.name} ${voice.detail}`.toLocaleLowerCase().includes(query.value.toLocaleLowerCase())));
const current = computed(() => props.voices.find(voice => voice.id === selected.value));
</script>
<template>
  <fieldset class="voice-picker" :disabled="disabled">
    <legend>{{ t('Voice') }}</legend>
    <input v-model="query" type="search" :placeholder="t('Search voices')" :aria-label="t('Search voices')">
    <div class="voice-options">
      <label v-for="voice in filtered.slice(0, 24)" :key="voice.id" class="voice-option" :class="{ selected: selected === voice.id }"><input v-model="selected" type="radio" name="studio-voice" :value="voice.id"><span><strong>{{ voice.name }}</strong><small>{{ voice.cloned ? voice.detail : t(voice.detail) }}</small></span></label>
    </div>
    <p v-if="!filtered.length" class="muted">{{ t('No voices match this search.') }}</p>
    <p v-if="filtered.length > 24" class="muted">{{ t('Showing the first {count} voices. Search to narrow the list.', { count: 24 }) }}</p>
    <p v-if="current" class="muted">{{ t('Selected voice: {name}', { name: current.name }) }}</p>
  </fieldset>
</template>
