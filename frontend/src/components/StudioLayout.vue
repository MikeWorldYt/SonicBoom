<script setup lang="ts">
import { computed, nextTick } from 'vue';
import { t } from '../i18n';
export type StudioTab = 'create' | 'voices' | 'engines' | 'settings';
const active = defineModel<StudioTab>({ required: true });
const sections: { id: StudioTab; label: string; icon: string; description: string }[] = [
  { id: 'create', label: 'Create speech', icon: '◉', description: 'Turn your ideas into natural speech.' },
  { id: 'voices', label: 'Voice library', icon: '≋', description: 'Create a voice that sounds like you.' },
  { id: 'engines', label: 'Engines', icon: '◈', description: 'Choose what powers your voice.' },
  { id: 'settings', label: 'Settings', icon: '⚙', description: 'Make SonicBoom work your way.' },
];
const current = computed(() => sections.find(section => section.id === active.value)!);
async function navigate(event: KeyboardEvent) {
  const index = sections.findIndex(section => section.id === active.value);
  let next: number;
  if (event.key === 'ArrowDown' || event.key === 'ArrowRight') next = (index + 1) % sections.length;
  else if (event.key === 'ArrowUp' || event.key === 'ArrowLeft') next = (index + sections.length - 1) % sections.length;
  else if (event.key === 'Home') next = 0;
  else if (event.key === 'End') next = sections.length - 1;
  else return;
  event.preventDefault(); active.value = sections[next].id;
  await nextTick();
  document.getElementById(`tab-${active.value}`)?.focus();
}
</script>
<template>
  <div class="studio-layout">
    <aside class="workspace-nav"><div role="tablist" :aria-label="t('Workspace')" @keydown="navigate">
      <button v-for="section in sections" :id="`tab-${section.id}`" :key="section.id" role="tab" :aria-selected="active === section.id" :aria-controls="`panel-${section.id}`" :tabindex="active === section.id ? 0 : -1" :class="{ selected: active === section.id }" @click="active = section.id"><span class="nav-icon" aria-hidden="true">{{ section.icon }}</span>{{ t(section.label) }}</button>
    </div></aside>
    <main id="main-content" class="workspace-content" tabindex="-1">
      <header class="workspace-heading"><div><h1>{{ t(current.label) }}</h1><p>{{ t(current.description) }}</p></div><slot name="actions"/></header>
      <slot name="notice"/>
      <section v-for="section in sections" :id="`panel-${section.id}`" v-show="active === section.id" :key="section.id" role="tabpanel" :aria-labelledby="`tab-${section.id}`" class="studio-panel"><slot :name="section.id"/></section>
    </main>
  </div>
</template>
