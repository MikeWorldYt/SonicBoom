<script setup lang="ts">
import { locale, locales, setLocale, t } from '../i18n';
</script>
<template>
  <label class="language-switcher">
    <span aria-hidden="true">◎</span><span class="sr-only">{{ t('Interface language') }}</span>
    <select :value="locale" @change="setLocale(($event.target as HTMLSelectElement).value)" :aria-label="t('Interface language')">
      <option v-for="item in locales" :key="item.code" :value="item.code" :lang="item.code">{{ item.name }}</option>
    </select>
  </label>
</template>
