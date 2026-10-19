<script setup lang="ts">
import { onMounted, ref } from 'vue';
import LanguageSwitcher from './LanguageSwitcher.vue';
import { t } from '../i18n';
defineProps<{ page: string }>();
type Theme = 'system' | 'light' | 'dark';
const theme = ref<Theme>('system');
function apply(value: Theme) {
  theme.value = value;
  const root = document.documentElement;
  if (value === 'system') root.removeAttribute('data-theme');
  else root.setAttribute('data-theme', value);
  try { localStorage.setItem('sonicboom.theme', value); } catch { /* session-only */ }
}
function cycle() { apply(theme.value === 'system' ? 'light' : theme.value === 'light' ? 'dark' : 'system'); }
onMounted(() => {
  try {
    const saved = localStorage.getItem('sonicboom.theme');
    if (saved === 'light' || saved === 'dark' || saved === 'system') apply(saved);
  } catch { /* session-only */ }
});
</script>
<template>
  <div class="app-shell">
    <a class="skip-link" href="#main-content">{{ t('Skip to content') }}</a>
    <header class="app-bar">
      <a href="/" class="brand" :aria-label="t('SonicBoom home')"><span class="brand-mark" aria-hidden="true"><svg width="24" height="24" viewBox="0 0 24 24" aria-hidden="true"><path d="M3 9v6m5-10v14m4-11v8m4-13v18m5-12v6" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg></span><span>SonicBoom</span></a>
      <div class="app-actions"><button type="button" class="theme-toggle" :aria-label="t('Color theme: {theme}', { theme: theme })" :title="t('Toggle color theme')" @click="cycle">{{ theme === 'dark' ? '◐' : theme === 'light' ? '○' : '◑' }}</button><LanguageSwitcher/><a v-if="page !== 'setup'" class="admin-link" :href="page === 'home' ? '/admin' : '/'">{{ t(page === 'home' ? 'API access' : 'Back to studio') }}</a></div>
    </header>
    <slot/>
    <footer class="app-footer"><span>{{ t('Your words. Your voice.') }}</span></footer>
  </div>
</template>
