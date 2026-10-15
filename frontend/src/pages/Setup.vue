<script setup lang="ts">
import { onMounted } from 'vue';
import { startSetup } from '../controllers/setup';
onMounted(startSetup);
</script>
<template>

<h1>Welcome to SonicBoom</h1>
<p id="progress">Step <span id="step-no">1</span> of <span id="step-count">4</span></p>

<form id="wizard">
  <!-- Step 1: usage mode -->
  <fieldset class="step active" data-step="1">
    <legend>How will SonicBoom be used?</legend>
    <label><input type="radio" name="usage_mode" value="local" checked>
      <strong>Local application</strong><br>
      <span class="hint">Only applications on this computer. No API token required. Recommended.</span></label>
    <label><input type="radio" name="usage_mode" value="lan">
      <strong>LAN / API server</strong><br>
      <span class="hint">Allow other devices or applications. Bearer token authentication required.</span></label>
    <label><input type="radio" name="usage_mode" value="advanced">
      <strong>Advanced</strong><br>
      <span class="hint">Configure networking and security manually.</span></label>
  </fieldset>

  <!-- Step 1b: advanced network options -->
  <fieldset class="step" data-step="1b">
    <legend>Network &amp; security</legend>
    <label>Bind address <input name="bind" value="127.0.0.1"></label>
    <label>Port <input name="port" type="number" min="1" max="65535" value="17842"></label>
    <label>Authentication mode
      <select name="auth_mode">
        <option value="local">local — no API token (loopback only)</option>
        <option value="token">token — bearer token required</option>
        <option value="none">none — development only</option>
      </select></label>
    <p class="hint">Local mode always requires loopback. Insecure remote mode
    requires an explicit security.allow_insecure_remote option.</p>
  </fieldset>

  <!-- Step 2: audio -->
  <fieldset class="step" data-step="2">
    <legend>Audio output</legend>
    <label>Output device <select name="audio_device" id="audio-device">
      <option value="" selected disabled>Choose an output device</option>
      <option value="default">System Default</option>
    </select></label>
    <button type="button" id="test-audio" disabled>Test Audio</button>
    <p class="hint">Devices are enumerated from this machine. Select
    "System Default" to follow the OS default.</p>
  </fieldset>

  <!-- Step 3: model -->
  <fieldset class="step" data-step="3">
    <legend>Model</legend>
    <p>Model: Supertonic 3</p>
    <label>Model directory <input name="model_cache_dir" placeholder="(default platform directory)"></label>
    <label>Inference steps <input name="inference_steps" type="number" min="1" max="50" value="5"></label>
    <p class="hint">Missing models are downloaded automatically on first use.</p>
  </fieldset>

  <!-- Step 4: credentials + summary -->
  <fieldset class="step" data-step="4">
    <legend>Finish setup</legend>
    <label>Admin username <input name="admin_username" value="admin"></label>
    <label>Admin password <input name="admin_password" type="password"
      placeholder="12+ characters, not a common password" required></label>
    <p class="hint">The password is stored in the OS-restricted secret store,
    never in config.toml.</p>
    <h2>Summary</h2>
    <div id="summary">…</div>
  </fieldset>

  <div class="nav">
    <button type="button" id="back">Back</button>
    <button type="button" id="next">Continue</button>
    <button type="submit" id="finish" hidden>Complete Setup</button>
  </div>
  <p id="error" role="alert"></p>
</form>


</template>
