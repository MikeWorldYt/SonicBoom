<script setup lang="ts">
import { computed, ref } from 'vue';
import { ComboboxRoot, ComboboxAnchor, ComboboxInput, ComboboxTrigger, ComboboxPortal, ComboboxContent, ComboboxItem, ComboboxVirtualizer } from 'reka-ui';
import { t } from '../../i18n';
interface VoiceOption { id: string; name: string; detail: string; cloned: boolean }
const selected = defineModel<string>({ required: true });
const props = defineProps<{ voices: VoiceOption[]; disabled?: boolean }>();
const query = ref('');
const open = ref(false);
const combobox = ref<{ highlightedElement?: HTMLElement } | null>(null);
const activeId = computed(() => open.value ? combobox.value?.highlightedElement?.id : undefined);
const filtered = computed(() => props.voices.filter(voice => `${voice.name} ${voice.detail}`.toLocaleLowerCase().includes(query.value.toLocaleLowerCase())));
const current = computed({ get: () => props.voices.find(voice => voice.id === selected.value), set: value => { if (value) selected.value = value.id; } });
</script>
<template>
  <div class="voice-picker">
    <label for="voice-search">{{ t('Voice') }}</label>
    <ComboboxRoot ref="combobox" v-model="current" v-model:open="open" :disabled="disabled" ignore-filter by="id" @update:open="value => { if (value) query = ''; }">
      <ComboboxAnchor class="voice-combobox-anchor">
        <ComboboxInput id="voice-search" :aria-activedescendant="activeId" v-model="query" :display-value="(value: VoiceOption) => value?.name || ''" :placeholder="t('Search voices')" aria-describedby="voice-count voice-selected"/>
        <ComboboxTrigger type="button" :aria-label="t('Show voices')">⌄</ComboboxTrigger>
      </ComboboxAnchor>
      <ComboboxPortal><ComboboxContent class="voice-combobox-content" position="popper" :side-offset="4">
        <div class="voice-combobox-viewport" role="presentation">
          <p v-if="!filtered.length" class="muted">{{ t('No voices match this search.') }}</p>
          <ComboboxVirtualizer v-if="filtered.length > 100" v-slot="{ option }" :options="filtered" :text-content="(option: VoiceOption) => option.name" :estimate-size="64">
            <ComboboxItem :value="option" class="voice-combobox-item"><strong>{{ option.name }}</strong><small>{{ option.cloned ? option.detail : t(option.detail) }}</small></ComboboxItem>
          </ComboboxVirtualizer>
          <template v-else><ComboboxItem v-for="option in filtered" :key="option.id" :value="option" class="voice-combobox-item"><strong>{{ option.name }}</strong><small>{{ option.cloned ? option.detail : t(option.detail) }}</small></ComboboxItem></template>
        </div>
      </ComboboxContent></ComboboxPortal>
    </ComboboxRoot>
    <p id="voice-count" role="status" class="muted">{{ t('{count} voices', { count: filtered.length }) }}</p>
    <p id="voice-selected" class="muted">{{ current ? t('Selected voice: {name}', { name: current.name }) : t('No voices match this search.') }}</p>
  </div>
</template>
