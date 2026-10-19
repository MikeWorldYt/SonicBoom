<script setup lang="ts">
import { computed, onMounted, reactive, ref, nextTick, onBeforeUnmount } from 'vue';
import { t, renderMessage, msg, type Message } from '../i18n';
import { createHttpClient } from '../services/http';
import { createApi } from '../services/api';
import type { AudioDevice, SetupInput, SetupResult, SetupConstraints } from '../services/contracts';
import { useTask } from '../composables/useTask';
import { setupIssues } from '../features/auth/setupValidation';
import SecretReveal from '../components/ui/SecretReveal.vue';
import FeedbackNotice from '../components/FeedbackNotice.vue';
const api = createApi(createHttpClient());
const task = useTask(); const { pending, feedback } = task;
const form = reactive({ usage_mode: 'local', audio_device: 'default', inference_steps: 5, model_cache_dir: '', bind: '127.0.0.1', port: 17842, auth_mode: 'local', admin_username: 'admin', admin_password: '' });
const policy = ref<SetupConstraints | null>(null);
const issues = ref<Record<string, Message>>({});
async function validate(section?: string[]) {
  const all = setupIssues(form, policy.value);
  issues.value = Object.fromEntries(Object.entries(all).filter(([key]) => !section || section.includes(key) || key === 'policy'));
  const first = Object.keys(issues.value)[0];
  if (!first) return true;
  index.value = ['bind', 'auth_mode', 'port'].includes(first) ? steps.value.indexOf('1b') : steps.value.indexOf('4');
  if (first === 'inference_steps') index.value = steps.value.indexOf('3');
  if (index.value < 0) index.value = 0;
  await nextTick(); document.getElementById('setup-' + first)?.focus();
  return false;
}
const completed = ref<SetupResult | null>(null);
const acknowledged = ref(false);
function leaveWarning(event: BeforeUnloadEvent) { if (completed.value?.api_token && !acknowledged.value) event.preventDefault(); }
window.addEventListener('beforeunload', leaveWarning);
onBeforeUnmount(() => window.removeEventListener('beforeunload', leaveWarning));
function continueToStudio() {
  if (!completed.value || (completed.value.api_token && !acknowledged.value)) return;
  const url = new URL(completed.value.api_url);
  completed.value = null;
  window.location.href = import.meta.env.DEV && url.port === __SONICBOOM_BACKEND_PORT__ ? '/' : `${window.location.protocol}//${window.location.hostname}:${url.port}/`;
}
const devices = ref<AudioDevice[]>([]);
const index = ref(0);
const formElement = ref<HTMLFormElement | null>(null);
const steps = computed(() => form.usage_mode === 'advanced' ? ['1', '1b', '2', '3', '4'] : ['1', '2', '3', '4']);
const current = computed(() => steps.value[index.value]);
const summary = computed(() => t('setup.summary', { url: `http://${form.usage_mode === 'lan' ? '0.0.0.0' : form.usage_mode === 'advanced' ? form.bind : '127.0.0.1'}:${form.port}`, auth: t(form.usage_mode === 'advanced' ? form.auth_mode : form.usage_mode === 'lan' ? 'token' : 'local'), audio: form.audio_device, steps: form.inference_steps }));
async function next() {
  if (current.value === '1b' && !await validate(['bind', 'auth_mode', 'port'])) return;
  if (current.value === '3' && !await validate(['inference_steps'])) return;
  if (formElement.value?.reportValidity()) index.value++;
}
async function complete() {
  await task.run(async () => {
    if (!await validate()) return;
    const input: SetupInput = { ...form, port: Number(form.port), inference_steps: Number(form.inference_steps), model_cache_dir: form.model_cache_dir || null };
    const result = await api.completeSetup(input);
    completed.value = result;
    form.admin_password = '';
    // No one-time secret to protect: go straight to the studio instead of
    // parking on the success screen. With a token the success screen stays,
    // gated on acknowledgement, so the secret is never skipped over.
    if (!result.api_token) { continueToStudio(); return; }
    await nextTick();
    document.getElementById('setup-success')?.focus();
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
  if (results[0].status === 'fulfilled') { if (!form.model_cache_dir) form.model_cache_dir = results[0].value.model.cache_dir; if (form.port === 17842) form.port = results[0].value.server.port; policy.value = results[0].value.constraints || null; }
  if (!policy.value) issues.value.policy = msg('Setup constraints are unavailable. Refresh the page before continuing.');
  if (results[1].status === 'fulfilled') devices.value = results[1].value;
});
</script>
<template>

<main id="main-content" tabindex="-1" class="onboarding"><div class="section-intro"><span class="eyebrow">{{ t("LET’S GET STARTED") }}</span><h1>{{ t("Welcome to SonicBoom") }}</h1><p class="muted">{{ t("A few quick choices, then you’re ready to create.") }}</p></div>
<p v-if="issues.policy" id="setup-policy" role="alert" class="field-error">{{ renderMessage(issues.policy) }}</p><p id="progress"><span>{{ t('Step') }}</span> <span id="step-no">{{ index + 1 }}</span> / <span id="step-count">{{ steps.length }}</span></p>

<section v-if="completed" id="setup-success" tabindex="-1" aria-labelledby="setup-ready">
  <h2 id="setup-ready">{{ t('SonicBoom is ready.') }}</h2>
  <p>API: {{ completed.api_url }}</p><p>{{ t('Authentication:') }} {{ t(completed.auth) }}</p>
  <SecretReveal v-if="completed.api_token" id="setup-token" :value="completed.api_token"/>
  <label v-if="completed.api_token"><input v-model="acknowledged" type="checkbox"> {{ t('I have saved my token safely.') }}</label>
  <button class="primary" :disabled="!!completed.api_token && !acknowledged" @click="continueToStudio">{{ t('Open studio') }}</button>
</section>
<form v-else id="wizard"  ref="formElement" @submit.prevent="complete" :aria-busy="pending">
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
    <label>{{ t("Bind address") }} <input id="setup-bind" name="bind" v-model="form.bind" :aria-invalid="!!issues.bind || undefined" aria-describedby="setup-bind-error"></label>
    <label>{{ t("Port") }} <input id="setup-port" name="port" v-model.number="form.port" :aria-invalid="!!issues.port || undefined" aria-describedby="setup-port-error" type="number" min="1" max="65535"></label>
    <label>{{ t("Authentication mode") }}
      <select id="setup-auth_mode" name="auth_mode" v-model="form.auth_mode" :aria-invalid="!!issues.auth_mode || undefined" aria-describedby="setup-auth_mode-error">
        <option value="local">{{ t("local — no API token (loopback only)") }}</option>
        <option value="token">{{ t("token — bearer token required") }}</option>
        <option value="none">{{ t("none — development only") }}</option>
      </select></label>
    <p v-for="key in ['bind', 'port', 'auth_mode']" :id="'setup-' + key + '-error'" :key="key" class="field-error" :role="issues[key] ? 'alert' : undefined">{{ issues[key] ? renderMessage(issues[key]) : '' }}</p>
    <p class="hint">{{ t("Local mode always requires loopback. Insecure remote mode\n    requires an explicit security.allow_insecure_remote option.") }}</p>
  </fieldset>

  <!-- Step 2: audio -->
  <fieldset class="step" :class="{ active: current === '2' }" :disabled="current !== '2' || pending" data-step="2">
    <legend>{{ t("Audio output") }}</legend>
    <label>{{ t("Output device") }} <select name="audio_device" v-model="form.audio_device" id="audio-device">
      <option value="" selected disabled>{{ t("Choose an output device") }}</option>
      <option value="default">{{ t("System Default") }}</option><option v-for="device in devices" :key="device.id" :value="device.id">{{ device.name }}</option>
    </select></label>
    <p class="help-note">{{ t('Testing audio saves the selected output device to server settings.') }}</p>
    <button type="button" id="test-audio"  :disabled="!devices.length || pending" @click="testAudio">{{ t("Test Audio") }}</button>
    <p class="hint">{{ t("Devices are enumerated from this machine. Select\n    \"System Default\" to follow the OS default.") }}</p>
  </fieldset>

  <!-- Step 3: model -->
  <fieldset class="step" :class="{ active: current === '3' }" :disabled="current !== '3' || pending" data-step="3">
    <legend>{{ t("Model") }}</legend>
    <p>{{ t("Model: Supertonic 3") }}</p>
    <label>{{ t("Model directory") }} <input name="model_cache_dir" v-model="form.model_cache_dir" :placeholder="t('(default platform directory)')"></label>
    <label>{{ t("Inference steps") }} <input id="setup-inference_steps" name="inference_steps" v-model.number="form.inference_steps" type="number" min="1" :max="policy?.max_inference_steps" :aria-invalid="!!issues.inference_steps || undefined" aria-describedby="setup-inference_steps-error"></label>
    <p id="setup-inference_steps-error" class="field-error" :role="issues.inference_steps ? 'alert' : undefined">{{ issues.inference_steps ? renderMessage(issues.inference_steps) : '' }}</p>
    <p class="hint">{{ t("Missing models are downloaded automatically on first use.") }}</p>
  </fieldset>

  <!-- Step 4: credentials + summary -->
  <fieldset class="step" :class="{ active: current === '4' }" :disabled="current !== '4' || pending" data-step="4">
    <legend>{{ t("Finish setup") }}</legend>
    <label>{{ t("Admin username") }} <input id="setup-admin_username" name="admin_username" v-model="form.admin_username" :aria-invalid="!!issues.admin_username || undefined" aria-describedby="setup-admin_username-error"></label>
    <label>{{ t("Admin password") }} <input id="setup-admin_password" name="admin_password" v-model="form.admin_password" :aria-invalid="!!issues.admin_password || undefined" aria-describedby="setup-admin_password-error" type="password"
      :placeholder="t('At least {min} characters.', { min: policy?.password_min_chars ?? '—' })" required></label>
    <p v-for="key in ['admin_username', 'admin_password']" :id="'setup-' + key + '-error'" :key="key" class="field-error" :role="issues[key] ? 'alert' : undefined">{{ issues[key] ? renderMessage(issues[key]) : '' }}</p>
    <p class="hint">{{ t("The password is stored in the OS-restricted secret store,\n    never in config.toml.") }}</p>
    <h2>{{ t("Summary") }}</h2>
    <div id="summary">{{ summary }}</div>
  </fieldset>

  <div class="nav">
    <button v-if="index > 0" type="button" id="back" :disabled="pending" @click="index--">{{ t("Back") }}</button>
    <button v-if="index < steps.length - 1" type="button" id="next" :disabled="pending" @click="next">{{ t("Continue") }}</button>
    <button v-else type="submit" id="finish" :disabled="pending || !policy">{{ t("Complete Setup") }}</button>
  </div>
  <FeedbackNotice :feedback="feedback"/>
</form>
</main>


</template>
