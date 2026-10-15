<script setup lang="ts">
import AppShell from './components/AppShell.vue';
import Home from './pages/Home.vue';
import Setup from './pages/Setup.vue';
import Admin from './pages/Admin.vue';
import type { PageData } from './types';
defineProps<{ data: PageData }>();
</script>
<template><AppShell :page="data.page"><Home v-if="data.page === 'home'" :data="data"/><Setup v-else-if="data.page === 'setup'"/><Admin v-else :data="data"/></AppShell></template>
