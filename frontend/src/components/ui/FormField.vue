<script setup lang="ts">
defineProps<{ id: string; label: string; help?: string; error?: string; required?: boolean }>();
</script>
<template>
  <div class="form-group">
    <label :for="id">{{ label }}<span v-if="required" aria-hidden="true"> *</span></label>
    <slot :describedby="[help ? id + '-help' : '', error ? id + '-error' : ''].filter(Boolean).join(' ')" :invalid="!!error"/>
    <p v-if="help" :id="id + '-help'" class="help-note">{{ help }}</p>
    <p v-if="error" :id="id + '-error'" class="field-error" role="alert">{{ error }}</p>
  </div>
</template>
