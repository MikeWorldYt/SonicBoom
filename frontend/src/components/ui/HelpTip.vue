<script setup lang="ts">
import { TooltipProvider, TooltipRoot, TooltipTrigger, TooltipPortal, TooltipContent } from 'reka-ui';
defineProps<{ label: string; text: string }>();
</script>
<template>
  <TooltipProvider>
    <TooltipRoot :delay-duration="200">
      <TooltipTrigger type="button" class="help-tip" :aria-label="label">?</TooltipTrigger>
      <TooltipPortal><TooltipContent class="help-tip-content" :side-offset="6">{{ text }}</TooltipContent></TooltipPortal>
    </TooltipRoot>
  </TooltipProvider>
</template>
