<script setup lang="ts">
import Home from './pages/Home.vue';
import Setup from './pages/Setup.vue';
import Admin from './pages/Admin.vue';
import LanguageSwitcher from './components/LanguageSwitcher.vue';
import { t } from './i18n';
import type { PageData } from './types';
defineProps<{ data: PageData }>();
</script>
<template>
  <div class="app-shell">
    <header class="app-bar">
      <a href="/" class="brand" :aria-label="t('SonicBoom home')"><span class="brand-mark" aria-hidden="true">ı▌ı▌ı</span><span>SonicBoom<span class="brand-caption">{{ t('VOICE STUDIO') }}</span></span></a>
      <div class="app-actions"><LanguageSwitcher/><a v-if="data.page !== 'setup'" class="admin-link" :href="data.page === 'home' ? '/admin' : '/'">{{ t(data.page === 'home' ? 'API access' : 'Back to studio') }} <span aria-hidden="true">↗</span></a></div>
    </header>
    <Home v-if="data.page === 'home'" :data="data"/>
    <Setup v-else-if="data.page === 'setup'"/>
    <Admin v-else :data="data"/>
    <footer class="app-footer"><span>SonicBoom</span><span>{{ t('Your words. Your voice.') }}</span></footer>
  </div>
</template>
