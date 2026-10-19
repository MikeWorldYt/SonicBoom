<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
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
const themeName = computed(() => theme.value === 'system' ? t('System') : theme.value === 'light' ? t('Light') : t('Dark'));
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
      <div class="app-actions"><button type="button" class="theme-toggle" :aria-label="t('Color theme: {theme}', { theme: themeName })" :title="t('Toggle color theme')" @click="cycle"><svg v-if="theme === 'light'" width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="4" fill="none" stroke="currentColor" stroke-width="2"/><path d="M12 2v2m0 16v2M4.9 4.9l1.4 1.4m11.4 11.4 1.4 1.4M2 12h2m16 0h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg><svg v-else-if="theme === 'dark'" width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8Z" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/></svg><svg v-else width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><rect x="2" y="3" width="20" height="14" rx="2" fill="none" stroke="currentColor" stroke-width="2"/><path d="M8 21h8m-4-4v4" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg></button><LanguageSwitcher/><a v-if="page !== 'setup'" class="admin-link" :href="page === 'home' ? '/admin' : '/'">{{ t(page === 'home' ? 'API access' : 'Back to studio') }}</a></div>
    </header>
    <slot/>
    <footer class="app-footer"><span>{{ t('Your words. Your voice.') }}</span></footer>
  </div>
</template>
