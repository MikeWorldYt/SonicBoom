<script setup lang="ts">
import { computed } from 'vue';
import { t, renderMessage } from '../../i18n';
import { useStudio } from '../../composables/useStudio';
import { usePronunciationPreview } from '../../composables/usePronunciationPreview';
import { CHANGE_KIND_LABELS, WARNING_LABELS } from '../../features/speech/pronunciation';
const studio = useStudio();
const { normalizationAvailable } = studio;
const { status, result, error, errorStatus, stale, request } = usePronunciationPreview(studio);
const kindLabel = (kind: string) => CHANGE_KIND_LABELS[kind] ? t(CHANGE_KIND_LABELS[kind]) : kind;
const warningTitle = (code: string) => WARNING_LABELS[code] ? t(WARNING_LABELS[code].title) : code;
const warningText = (code: string) => t(WARNING_LABELS[code]?.explanation ?? 'This expression was left as written.');
const unchanged = computed(() => !!result.value && result.value.changes.length === 0);
// One concise sentence for the polite live region.
const announcement = computed(() => {
  if (status.value === 'loading') return t('Checking what will be spoken…');
  if (status.value === 'empty') return t('Enter some text to see how it will be spoken.');
  if (status.value === 'ready' && stale.value) return t('The text or settings changed since this preview.');
  if (status.value === 'ready' && result.value) return t('{changes} changes, {warnings} warnings.', { changes: result.value.changes.length, warnings: result.value.warnings.length });
  return '';
});
function toggled(event: Event) {
  if ((event.target as HTMLDetailsElement).open && status.value === 'idle') request();
}
</script>
<template>
  <details v-if="normalizationAvailable" id="spokenPreview" class="spoken-preview" @toggle="toggled">
    <summary>{{ t('What will be spoken?') }}</summary>
    <div class="toolbar">
      <button id="refreshSpokenPreview" type="button" :disabled="status === 'loading'" @click="request">{{ t(status === 'idle' ? 'Show preview' : 'Refresh preview') }}</button>
    </div>
    <p id="spokenPreviewStatus" class="help-note" role="status" aria-live="polite">{{ announcement }}</p>
    <p v-if="stale" class="feedback info spoken-stale">{{ t('This preview is out of date. Refresh it to match the current text and settings.') }}</p>
    <div v-if="status === 'error' && error" class="feedback error" role="alert">
      <p>{{ renderMessage(error) }}</p>
      <p v-if="errorStatus === 400">{{ t('The server could not prepare this text. It may be too long, or its spoken form may exceed the allowed length. Shorten the text and try again.') }}</p>
    </div>
    <template v-if="result">
      <p class="muted spoken-meta">{{ t('Language: {language} · Locale: {locale} · Pronunciation: {mode}', { language: t(`language.${result.language}`), locale: result.locale, mode: t(result.mode === 'auto' ? 'Automatic (recommended)' : 'Original text') }) }}</p>
      <div class="spoken-compare">
        <div class="field"><label for="spokenOriginal">{{ t('Original text') }}</label><textarea id="spokenOriginal" readonly :value="result.original" rows="4"/></div>
        <div class="field"><label for="spokenText">{{ t('Spoken text') }}</label><textarea id="spokenText" readonly :value="result.spoken" rows="4"/></div>
      </div>
      <p v-if="unchanged" class="muted">{{ t('Nothing needed to change; the text will be spoken as written.') }}</p>
      <section v-if="result.changes.length" class="spoken-section" aria-labelledby="spokenChangesTitle">
        <h4 id="spokenChangesTitle">{{ t('Changes ({count})', { count: result.changes.length }) }}</h4>
        <ul class="spoken-list" tabindex="0" :aria-label="t('Changes ({count})', { count: result.changes.length })">
          <li v-for="(change, index) in result.changes" :key="index"><span class="spoken-kind">{{ kindLabel(change.kind) }}</span> <bdi class="spoken-before">{{ change.before }}</bdi> <span aria-hidden="true">→</span><span class="sr-only">{{ t('becomes') }}</span> <bdi class="spoken-after">{{ change.after }}</bdi></li>
        </ul>
      </section>
      <section v-if="result.warnings.length" class="spoken-section" aria-labelledby="spokenWarningsTitle">
        <h4 id="spokenWarningsTitle">{{ t('Warnings ({count})', { count: result.warnings.length }) }}</h4>
        <ul class="spoken-list spoken-warnings" tabindex="0" :aria-label="t('Warnings ({count})', { count: result.warnings.length })">
          <li v-for="(warning, index) in result.warnings" :key="index"><strong>{{ warningTitle(warning.code) }}</strong> <bdi class="spoken-before">{{ warning.before }}</bdi><br><span class="muted">{{ warningText(warning.code) }}</span></li>
        </ul>
      </section>
    </template>
    <p v-else-if="status === 'loading'" class="muted"><progress class="generate-progress" :aria-label="t('Checking what will be spoken…')"/></p>
  </details>
</template>
