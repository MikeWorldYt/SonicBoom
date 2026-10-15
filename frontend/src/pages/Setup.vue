<script setup lang="ts">
import { t } from '../i18n';
import { onMounted } from 'vue';
import { startSetup } from '../controllers/setup';
onMounted(startSetup);
</script>
<template>

<main class="onboarding"><div class="section-intro"><span class="eyebrow">{{ t("LET’S GET STARTED") }}</span><h1>{{ t("Welcome to SonicBoom") }}</h1><p class="muted">{{ t("A few quick choices, then you’re ready to create.") }}</p></div>
<p id="progress"><span>{{ t('Step') }}</span> <span id="step-no"></span> / <span id="step-count"></span></p>

<form id="wizard">
  <!-- Step 1: usage mode -->
  <fieldset class="step active" data-step="1">
    <legend>{{ t("How will SonicBoom be used?") }}</legend>
    <label><input type="radio" name="usage_mode" value="local" checked>
      <strong>{{ t("Local application") }}</strong><br>
      <span class="hint">{{ t("Only applications on this computer. No API token required. Recommended.") }}</span></label>
    <label><input type="radio" name="usage_mode" value="lan">
      <strong>{{ t("LAN / API server") }}</strong><br>
      <span class="hint">{{ t("Allow other devices or applications. Bearer token authentication required.") }}</span></label>
    <label><input type="radio" name="usage_mode" value="advanced">
      <strong>{{ t("Advanced") }}</strong><br>
      <span class="hint">{{ t("Configure networking and security manually.") }}</span></label>
  </fieldset>

  <!-- Step 1b: advanced network options -->
  <fieldset class="step" data-step="1b">
    <legend>{{ t("Network & security") }}</legend>
    <label>{{ t("Bind address") }} <input name="bind" value="127.0.0.1"></label>
    <label>{{ t("Port") }} <input name="port" type="number" min="1" max="65535" value="17842"></label>
    <label>{{ t("Authentication mode") }}
      <select name="auth_mode">
        <option value="local">{{ t("local — no API token (loopback only)") }}</option>
        <option value="token">{{ t("token — bearer token required") }}</option>
        <option value="none">{{ t("none — development only") }}</option>
      </select></label>
    <p class="hint">{{ t("Local mode always requires loopback. Insecure remote mode\n    requires an explicit security.allow_insecure_remote option.") }}</p>
  </fieldset>

  <!-- Step 2: audio -->
  <fieldset class="step" data-step="2">
    <legend>{{ t("Audio output") }}</legend>
    <label>{{ t("Output device") }} <select name="audio_device" id="audio-device">
      <option value="" selected disabled>{{ t("Choose an output device") }}</option>
      <option value="default">{{ t("System Default") }}</option>
    </select></label>
    <button type="button" id="test-audio" disabled>{{ t("Test Audio") }}</button>
    <p class="hint">{{ t("Devices are enumerated from this machine. Select\n    \"System Default\" to follow the OS default.") }}</p>
  </fieldset>

  <!-- Step 3: model -->
  <fieldset class="step" data-step="3">
    <legend>{{ t("Model") }}</legend>
    <p>{{ t("Model: Supertonic 3") }}</p>
    <label>{{ t("Model directory") }} <input name="model_cache_dir" :placeholder="t(&quot;(default platform directory)&quot;)"></label>
    <label>{{ t("Inference steps") }} <input name="inference_steps" type="number" min="1" max="50" value="5"></label>
    <p class="hint">{{ t("Missing models are downloaded automatically on first use.") }}</p>
  </fieldset>

  <!-- Step 4: credentials + summary -->
  <fieldset class="step" data-step="4">
    <legend>{{ t("Finish setup") }}</legend>
    <label>{{ t("Admin username") }} <input name="admin_username" value="admin"></label>
    <label>{{ t("Admin password") }} <input name="admin_password" type="password"
      :placeholder="t(&quot;12+ characters, not a common password&quot;)" required></label>
    <p class="hint">{{ t("The password is stored in the OS-restricted secret store,\n    never in config.toml.") }}</p>
    <h2>{{ t("Summary") }}</h2>
    <div id="summary">…</div>
  </fieldset>

  <div class="nav">
    <button type="button" id="back">{{ t("Back") }}</button>
    <button type="button" id="next">{{ t("Continue") }}</button>
    <button type="submit" id="finish" hidden>{{ t("Complete Setup") }}</button>
  </div>
  <p id="error" role="alert"></p>
</form>
</main>


</template>
