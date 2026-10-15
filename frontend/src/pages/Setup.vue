<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue';
import { t } from '../i18n';
import { createHttpClient } from '../services/http';
import { createApi } from '../services/api';
import type { AudioDevice, SetupInput } from '../services/contracts';
import { useTask } from '../composables/useTask';
import FeedbackNotice from '../components/FeedbackNotice.vue';
const api = createApi(createHttpClient());
const task = useTask(); const { pending, feedback } = task;
const form = reactive({ usage_mode: 'local', audio_device: 'default', inference_steps: 5, model_cache_dir: '', bind: '127.0.0.1', port: 17842, auth_mode: 'local', admin_username: 'admin', admin_password: '' });
const devices = ref<AudioDevice[]>([]);
const index = ref(0);
const formElement = ref<HTMLFormElement | null>(null);
const steps = computed(() => form.usage_mode === 'advanced' ? ['1', '1b', '2', '3', '4'] : ['1', '2', '3', '4']);
const current = computed(() => steps.value[index.value]);
const summary = computed(() => t('setup.summary', { url: `http://${form.usage_mode === 'lan' ? '0.0.0.0' : form.usage_mode === 'advanced' ? form.bind : '127.0.0.1'}:${form.port}`, auth: t(form.usage_mode === 'advanced' ? form.auth_mode : form.usage_mode === 'lan' ? 'token' : 'local'), audio: form.audio_device, steps: form.inference_steps }));
function next() { if (formElement.value?.reportValidity()) index.value++; }
async function complete() {
  await task.run(async () => {
    const input: SetupInput = { ...form, port: Number(form.port), inference_steps: Number(form.inference_steps), model_cache_dir: form.model_cache_dir || null };
    const result = await api.completeSetup(input);
    let text = `${t('SonicBoom is ready.')}\n\nAPI: ${result.api_url}\n${t('Audio:')} ${result.audio_device}\n${t('Authentication:')} ${t(result.auth)}`;
    if (result.api_token) text += `\n\n${t('API token (shown once — store it safely):')}\n${result.api_token}`;
    alert(text);
    const url = new URL(result.api_url);
    window.location.href = import.meta.env.DEV && url.port === __SONICBOOM_BACKEND_PORT__ ? '/' : `${window.location.protocol}//${window.location.hostname}:${url.port}/`;
  });
}
async function testAudio() {
  await task.run(async () => {
    await api.patchConfig({ audio: { output_device: form.audio_device } });
    for (let attempt = 0; attempt < 20; attempt++) {
      const active = await api.audioOutput();
      if (active.device === form.audio_device && active.available) { await api.testAudio(t('This is a test of the SonicBoom audio setup.')); return; }
      await new Promise(resolve => setTimeout(resolve, 150));
    }
    throw new Error('Selected output is unavailable. Check configuration status.');
  }, 'Test audio played (check your output device).');
}
onMounted(async () => {
  const results = await Promise.allSettled([api.setupStatus(), api.setupDevices()]);
  if (results[0].status === 'fulfilled') { form.model_cache_dir = results[0].value.model.cache_dir; form.port = results[0].value.server.port; }
  if (results[1].status === 'fulfilled') devices.value = results[1].value;
});
</script>
<template>

<main id="main-content" tabindex="-1" class="onboarding"><div class="section-intro"><span class="eyebrow">{{ t("LET’S GET STARTED") }}</span><h1>{{ t("Welcome to SonicBoom") }}</h1><p class="muted">{{ t("A few quick choices, then you’re ready to create.") }}</p></div>
<p id="progress"><span>{{ t('Step') }}</span> <span id="step-no">{{ index + 1 }}</span> / <span id="step-count">{{ steps.length }}</span></p>

<form id="wizard" ref="formElement" @submit.prevent="complete" :aria-busy="pending">
  <!-- Step 1: usage mode -->
  <fieldset class="step" :class="{ active: current === '1' }" :disabled="current !== '1' || pending" data-step="1">
    <legend>{{ t("How will SonicBoom be used?") }}</legend>
    <label><input type="radio" name="usage_mode" v-model="form.usage_mode" value="local" checked>
      <strong>{{ t("Local application") }}</strong><br>
      <span class="hint">{{ t("Only applications on this computer. No API token required. Recommended.") }}</span></label>
    <label><input type="radio" name="usage_mode" v-model="form.usage_mode" value="lan">
      <strong>{{ t("LAN / API server") }}</strong><br>
      <span class="hint">{{ t("Allow other devices or applications. Bearer token authentication required.") }}</span></label>
    <label><input type="radio" name="usage_mode" v-model="form.usage_mode" value="advanced">
      <strong>{{ t("Advanced") }}</strong><br>
      <span class="hint">{{ t("Configure networking and security manually.") }}</span></label>
  </fieldset>

  <!-- Step 1b: advanced network options -->
  <fieldset class="step" :class="{ active: current === '1b' }" :disabled="current !== '1b' || pending" data-step="1b">
    <legend>{{ t("Network & security") }}</legend>
    <label>{{ t("Bind address") }} <input name="bind" v-model="form.bind"></label>
    <label>{{ t("Port") }} <input name="port" v-model.number="form.port" type="number" min="1" max="65535"></label>
    <label>{{ t("Authentication mode") }}
      <select name="auth_mode" v-model="form.auth_mode">
        <option value="local">{{ t("local — no API token (loopback only)") }}</option>
        <option value="token">{{ t("token — bearer token required") }}</option>
        <option value="none">{{ t("none — development only") }}</option>
      </select></label>
    <p class="hint">{{ t("Local mode always requires loopback. Insecure remote mode\n    requires an explicit security.allow_insecure_remote option.") }}</p>
  </fieldset>

  <!-- Step 2: audio -->
  <fieldset class="step" :class="{ active: current === '2' }" :disabled="current !== '2' || pending" data-step="2">
    <legend>{{ t("Audio output") }}</legend>
    <label>{{ t("Output device") }} <select name="audio_device" v-model="form.audio_device" id="audio-device">
      <option value="" selected disabled>{{ t("Choose an output device") }}</option>
      <option value="default">{{ t("System Default") }}</option><option v-for="device in devices" :key="device.id" :value="device.id">{{ device.name }}</option>
    </select></label>
    <button type="button" id="test-audio" :disabled="!devices.length || pending" @click="testAudio">{{ t("Test Audio") }}</button>
    <p class="hint">{{ t("Devices are enumerated from this machine. Select\n    \"System Default\" to follow the OS default.") }}</p>
  </fieldset>

  <!-- Step 3: model -->
  <fieldset class="step" :class="{ active: current === '3' }" :disabled="current !== '3' || pending" data-step="3">
    <legend>{{ t("Model") }}</legend>
    <p>{{ t("Model: Supertonic 3") }}</p>
    <label>{{ t("Model directory") }} <input name="model_cache_dir" v-model="form.model_cache_dir" :placeholder="t(&quot;(default platform directory)&quot;)"></label>
    <label>{{ t("Inference steps") }} <input name="inference_steps" v-model.number="form.inference_steps" type="number" min="1" max="50"></label>
    <p class="hint">{{ t("Missing models are downloaded automatically on first use.") }}</p>
  </fieldset>

  <!-- Step 4: credentials + summary -->
  <fieldset class="step" :class="{ active: current === '4' }" :disabled="current !== '4' || pending" data-step="4">
    <legend>{{ t("Finish setup") }}</legend>
    <label>{{ t("Admin username") }} <input name="admin_username" v-model="form.admin_username"></label>
    <label>{{ t("Admin password") }} <input name="admin_password" v-model="form.admin_password" type="password"
      :placeholder="t(&quot;12+ characters, not a common password&quot;)" required></label>
    <p class="hint">{{ t("The password is stored in the OS-restricted secret store,\n    never in config.toml.") }}</p>
    <h2>{{ t("Summary") }}</h2>
    <div id="summary">{{ summary }}</div>
  </fieldset>

  <div class="nav">
    <button v-if="index > 0" type="button" id="back" :disabled="pending" @click="index--">{{ t("Back") }}</button>
    <button v-if="index < steps.length - 1" type="button" id="next" :disabled="pending" @click="next">{{ t("Continue") }}</button>
    <button v-else type="submit" id="finish" :disabled="pending">{{ t("Complete Setup") }}</button>
  </div>
  <FeedbackNotice :feedback="feedback"/>
</form>
</main>


</template>
