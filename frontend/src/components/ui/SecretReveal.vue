<script setup lang="ts">
import { ref, onBeforeUnmount } from 'vue';
import { t } from '../../i18n';
const props = defineProps<{ value: string; id: string }>();
const reveal = ref(false), notice = ref('');
const urls = new Set<string>();
onBeforeUnmount(() => { for (const url of urls) URL.revokeObjectURL(url); });
async function copy() {
  try { await navigator.clipboard.writeText(props.value); notice.value = t('Token copied.'); }
  catch { reveal.value = true; notice.value = t('Copy failed. Select and copy the token manually.'); }
}
function save() {
  const url = URL.createObjectURL(new Blob([props.value + '\n'], { type: 'text/plain' }));
  urls.add(url);
  const link = document.createElement('a'); link.href = url; link.download = 'sonicboom-api-token.txt'; link.click();
  setTimeout(() => { URL.revokeObjectURL(url); urls.delete(url); }, 1000);
}
</script>
<template><div class="secret-reveal"><p>{{ t('Copy this token now. It cannot be displayed again.') }}</p><label :for="id">{{ t('API token') }}</label><input :id="id" :type="reveal ? 'text' : 'password'" :value="value" readonly autocomplete="off" spellcheck="false"><div class="toolbar"><button type="button" :aria-pressed="reveal" @click="reveal = !reveal">{{ t(reveal ? 'Hide token' : 'Reveal token') }}</button><button type="button" @click="copy">{{ t('Copy token') }}</button><button type="button" @click="save">{{ t('Save token to file') }}</button></div><p role="status">{{ notice }}</p></div></template>
