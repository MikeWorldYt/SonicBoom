<script setup lang="ts">
import { computed, ref } from 'vue';
import { t } from '../../i18n';
import { useStudio, getPath } from '../../composables/useStudio';
import { categories, frequentSettings } from '../../services/settings';
import { useTask } from '../../composables/useTask';
import FeedbackNotice from '../FeedbackNotice.vue';
import SettingField from './SettingField.vue';
import type { SettingSchema } from '../../services/contracts';
const studio = useStudio(); const { config, schema, status, errors, info } = studio;
const query = ref(''); const category = ref('common'); const showKeys = ref(false); const task = useTask(); const { pending, feedback } = task;
const entries = computed(() => Object.values(schema.value).filter(meta => !meta.secret));
const groups = computed(() => [...new Set(entries.value.map(meta => meta.path.split('.')[0]))].sort());
function visible(meta: SettingSchema) {
  const q = query.value.trim().toLocaleLowerCase();
  if (q) return `${meta.path} ${t(`setting.${meta.path}.label`)} ${t(meta.description)}`.toLocaleLowerCase().includes(q);
  if (category.value === 'common') return frequentSettings.has(meta.path);
  return category.value === 'all' || meta.path.startsWith(category.value + '.');
}
const visibleCount = computed(() => entries.value.filter(visible).length);
async function action(value: 'reload' | 'validate' | 'open') { await task.run(async () => { await studio.api.configAction(value); if (value === 'reload') await studio.refreshConfig(true); }, value === 'validate' ? 'Configuration file is valid.' : value === 'reload' ? 'Reloaded from disk.' : undefined); }
</script>
<template>
  <p v-if="status" role="status">{{ t('Configured revision: {revision}', { revision: status.revision }) }} · {{ status.pending_restart ? t('Restart required') : status.pending_applies?.length ? t('Applying…') : t('No pending changes') }}</p>
  <p class="settings-guide">{{ t('Search or browse categories. Each change saves on its own.') }}</p>
  <div class="config-filters"><input id="configSearch" v-model="query" type="search" :placeholder="t('Search settings')" :aria-label="t('Search settings')"><select id="configCategory" v-model="category" :aria-label="t('Filter by category')"><option value="common">{{ t('Frequently used') }}</option><option value="all">{{ t('All settings') }}</option><option v-for="group in groups" :key="group" :value="group">{{ t(categories[group] || group) }}</option></select><label class="show-keys"><input v-model="showKeys" type="checkbox">{{ t('Show internal keys') }}</label></div>
  <FeedbackNotice :feedback="errors.settings ? { kind: 'error', message: errors.settings } : feedback"/>
  <p v-if="!config && !errors.settings" role="status">{{ t('Loading configuration…') }}</p>
  <p v-else-if="!visibleCount" class="empty-result">{{ t('No settings match this filter.') }}</p>
  <div id="configGroups" :class="{ 'hide-keys': !showKeys }"><section v-for="group in groups" v-show="entries.some(meta => meta.path.startsWith(group + '.') && visible(meta))" :key="group" class="cfg-group"><h2>{{ t(categories[group] || group) }}</h2><SettingField v-for="meta in entries.filter(meta => meta.path.startsWith(group + '.'))" v-show="visible(meta)" :key="meta.path" :meta="meta" :value="getPath(config?.config, meta.path)"/></section></div>
  <details><summary>{{ t('Advanced configuration tools') }}</summary><p class="mono-line">{{ t('Config file:') }} <code id="configPath">{{ status?.config_path }}</code></p><dl class="stat-grid"><div><dt>{{ t('Watcher') }}</dt><dd>{{ t(status?.watcher || '—') }}</dd></div><div><dt>{{ t('Last reload') }}</dt><dd>{{ status?.last_reload || '—' }}</dd></div><div><dt>{{ t('Pending') }}</dt><dd>{{ status?.pending_applies?.join(', ') || t('none') }}</dd></div><div><dt>{{ t('Revision') }}</dt><dd>{{ status?.revision || 0 }}</dd></div></dl>
    <div class="toolbar"><button id="configRefresh" :disabled="pending" @click="task.run(() => studio.refreshConfig(true))">{{ t('Refresh') }}</button><button id="configReload" :disabled="pending" @click="action('reload')">{{ t('Reload file') }}</button><button id="configValidate" :disabled="pending" @click="action('validate')">{{ t('Validate') }}</button><button v-if="info?.mode === 'desktop'" id="configOpen" :disabled="pending" @click="action('open')">{{ t('Open file') }}</button></div>
    <p v-if="status?.last_error" role="alert" class="error">{{ status.last_error }}</p><ul v-if="status?.apply_failures?.length" role="alert" class="error"><li v-for="failure in status.apply_failures" :key="failure.path">{{ failure.path }}: {{ failure.error }}</li></ul>
  </details>
</template>
