<script setup lang="ts">
import { DialogRoot, DialogPortal, DialogOverlay, DialogContent, DialogTitle, DialogDescription, DialogClose } from 'reka-ui';
import { t } from '../../i18n';
import CloneWizard from './CloneWizard.vue';
const open = defineModel<boolean>({ default: false });
defineEmits<{ created: []; engines: [] }>();
</script>
<template>
  <DialogRoot v-model:open="open">
    <DialogPortal>
      <DialogOverlay class="dialog-overlay" />
      <DialogContent class="dialog-content voice-creator-dialog" @interact-outside="$event.preventDefault()">
        <div class="dialog-head">
          <div>
            <DialogTitle>{{ t('New voice') }}</DialogTitle>
            <DialogDescription>{{ t('Clone a voice in three steps: engine, reference, review.') }}</DialogDescription>
          </div>
          <DialogClose class="dialog-x" :aria-label="t('Close')">×</DialogClose>
        </div>
        <CloneWizard @created="$emit('created')" @engines="$emit('engines')" />
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
