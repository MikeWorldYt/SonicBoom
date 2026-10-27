<script setup lang="ts">
import { computed } from 'vue';
import { t } from '../../i18n';
import { useStudio } from '../../composables/useStudio';
defineProps<{ disabled?: boolean }>();
const { normalizationAvailable, normalizationMode, pronunciationLocale, pronunciationLocales, showPronunciationLocale, normalizationLanguage } = useStudio();
// Locales and the default come from /api/info; nothing is hard-coded here.
const defaultLocale = computed(() => normalizationLanguage.value?.default_locale ?? '');
</script>
<template>
  <div v-if="normalizationAvailable" class="row pronunciation-settings">
    <div class="field">
      <label for="pronunciation">{{ t('Pronunciation') }}</label>
      <select id="pronunciation" v-model="normalizationMode" aria-describedby="pronunciationHint" :disabled="disabled">
        <option value="auto">{{ t('Automatic (recommended)') }}</option>
        <option value="off">{{ t('Original text') }}</option>
      </select>
      <p id="pronunciationHint" class="field-hint">{{ t(normalizationMode === 'auto' ? 'Numbers, dates, currency and abbreviations are read out as words where the rules are confident.' : 'The text is spoken exactly as typed.') }}</p>
    </div>
    <div v-if="showPronunciationLocale" class="field">
      <label for="pronunciationLocale">{{ t('Regional pronunciation') }}</label>
      <select id="pronunciationLocale" v-model="pronunciationLocale" :disabled="disabled">
        <option value="">{{ defaultLocale ? t('Language default ({locale})', { locale: defaultLocale }) : t('Language default') }}</option>
        <option v-for="tag in pronunciationLocales" :key="tag" :value="tag">{{ tag }}</option>
      </select>
    </div>
  </div>
</template>
