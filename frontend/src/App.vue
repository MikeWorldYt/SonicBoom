<script setup lang="ts">
import AppErrorBoundary from './components/ui/AppErrorBoundary.vue';
import AppShell from './components/AppShell.vue';
import { defineAsyncComponent } from 'vue';
const Home = defineAsyncComponent(() => import('./pages/Home.vue'));
const Setup = defineAsyncComponent(() => import('./pages/Setup.vue'));
const Admin = defineAsyncComponent(() => import('./pages/Admin.vue'));
import type { PageData } from './types';
defineProps<{ data: PageData }>();
</script>
<template><AppShell :page="data.page"><AppErrorBoundary><Home v-if="data.page === 'home'" :data="data"/><Setup v-else-if="data.page === 'setup'"/><Admin v-else :data="data"/></AppErrorBoundary></AppShell></template>
