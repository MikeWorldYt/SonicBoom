<script setup lang="ts">
import { watch } from 'vue';
import { DialogRoot, DialogPortal, DialogOverlay, DialogContent, DialogTitle, DialogDescription, DialogClose } from 'reka-ui';
import { t } from '../../i18n';
const open = defineModel<boolean>({ default: false });
defineProps<{ title: string; description: string; pending?: boolean; confirmLabel?: string }>();
defineEmits<{ confirm: [] }>();
let returnFocus: HTMLElement | null = null;
watch(open, value => { if (value) returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null; }, { flush: 'sync' });
function restoreFocus(event: Event) { event.preventDefault(); returnFocus?.focus(); }

</script>
<template><DialogRoot v-model:open="open"><DialogPortal><DialogOverlay class="dialog-overlay"/><DialogContent class="dialog-content" @close-auto-focus="restoreFocus" @escape-key-down="pending && $event.preventDefault()" @interact-outside="pending && $event.preventDefault()"><DialogTitle>{{ title }}</DialogTitle><DialogDescription>{{ description }}</DialogDescription><slot/><div class="toolbar"><DialogClose :disabled="pending">{{ t('Cancel') }}</DialogClose><button class="danger" :disabled="pending" @click="$emit('confirm')">{{ pending ? t('Working…') : confirmLabel || t('Delete') }}</button></div></DialogContent></DialogPortal></DialogRoot></template>
