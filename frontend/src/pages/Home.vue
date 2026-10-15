<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue';
import { startHome } from '../controllers/home';
import type { HomeData } from '../types';
const { data } = defineProps<{ data: HomeData }>();
let stop: (() => void) | undefined;
onMounted(() => { stop = startHome(data.modelReady, data.providers); });
onUnmounted(() => stop?.());
</script>
<template>

<div class="page">
  <header class="page-header">
    <div>
      <h1>SonicBoom TTS</h1>
      <p class="subtitle">Synthesize speech, check voice engines, and tune the server.</p>
    </div>
    <div class="model-status" id="modelStatus" :hidden="!data.status">{{ data.status }}</div>
  </header>

  <div class="layout">
    <div class="col-main">
      <section class="card" aria-labelledby="tts-heading">
        <h2 id="tts-heading">Synthesize</h2>
        <div class="form-group" id="tokenGroup">
          <label for="apiToken">API token <span class="hint">(<a href="/admin">create one in the admin panel</a>)</span></label>
          <input type="password" id="apiToken" placeholder="Paste your API token here" autocomplete="off" spellcheck="false">
        </div>

        <div class="form-group">
          <div class="label-row">
            <label for="text">Text</label>
            <span class="char-count" id="charCount">0 characters</span>
          </div>
          <textarea id="text" placeholder="Enter text to convert to speech..."></textarea>
        </div>

        <div class="form-group">
          <div class="label-row">
            <label for="model">Model</label>
            <button type="button" class="btn-inline" id="modelRefresh" title="Reload engines, voices, and languages">Refresh</button>
          </div>
          <!-- Options are rendered live from /api/providers in the home controller. -->
          <select id="model" :disabled="!data.modelReady">
            <option value="supertonic-3">supertonic-3 (built-in voices)</option>
          </select>
          <p class="muted" id="modelHint"></p>
        </div>

        <div class="row">
          <div class="field">
            <label for="voice">Voice</label>
            <select id="voice" :disabled="!data.modelReady">
              <option v-for="voice in data.voices" :key="voice" :value="voice">{{ voice }}</option>
            </select>
          </div>
          <div class="field">
            <label for="lang">Language</label>
            <!-- Options are rendered from the LANGUAGES const in the home controller. -->
            <select id="lang" :disabled="!data.modelReady"></select>
          </div>
        </div>

        <button id="btn" class="primary" :disabled="!data.modelReady">Generate speech</button>
        <button type="button" id="cancelGeneration" class="btn-secondary" hidden>Cancel generation</button>
        <div id="status" role="status" aria-live="polite"></div>
        <a id="downloadAudio" download="sonicboom.wav" hidden>Download generated audio</a>
        <audio id="player" controls></audio>
      </section>

      <section class="card" aria-labelledby="providers-heading">
        <div class="section-head">
          <h2 id="providers-heading">Voice engines</h2>
          <span class="hint">Live status from the server</span>
        </div>
        <div class="provider-list"></div>
        <details><summary>Advanced inference diagnostics</summary><pre id="inferenceDiagnostics">Load provider status to view diagnostics.</pre></details>
        <p class="muted">Voice cloning (Qwen3-TTS, GPT-SoVITS) needs an API token and a reference recording uploaded via <code>POST /api/voices/clones</code>. See <code>docs/providers.md</code> for setup. Use Enable on a card to download and start an engine — no file edits or restarts needed.</p>
      </section>

      <section class="card" aria-labelledby="cloning-heading">
        <div class="section-head">
          <h2 id="cloning-heading">Voice cloning</h2>
          <span class="hint">Your uploaded reference voices</span>
        </div>
        <div class="voice-list" id="voicesList">
          <p class="muted">Loading voices…</p>
        </div>

        <h3 class="subhead">Clone a new voice</h3>
        <p class="muted">ICL uses the recording and its exact transcript. X-vector uses the voice embedding without a transcript. Use clear speech with little background noise; very short references may reduce quality. Saving a profile does not validate synthesis — preview it with short text.</p>
        <form id="cloneForm">
          <div class="row">
            <div class="field">
              <label for="cloneName">Name</label>
              <input type="text" id="cloneName" maxlength="64" placeholder="e.g. narrator" autocomplete="off">
            </div>
            <div class="field">
              <label for="cloneProvider">Engine</label>
              <select id="cloneProvider"></select>
            </div>
          </div>
          <div class="row">
            <div class="field">
              <label for="cloneModel">Model</label>
              <select id="cloneModel"></select>
            </div>
            <div class="field">
              <label for="cloneLang">Language</label>
              <select id="cloneLang"></select>
            </div>
          </div>
          <div class="row">
            <div class="field">
              <label for="cloneMode">Mode</label>
              <select id="cloneMode"></select>
            </div>
            <div class="field">
              <label for="cloneConsent">Consent basis</label>
              <input type="text" id="cloneConsent" placeholder="owner" autocomplete="off">
            </div>
          </div>
          <div class="form-group" id="cloneTranscriptGroup">
            <label for="cloneTranscript">Reference transcript (exact words in the recording)</label>
            <textarea id="cloneTranscript" class="compact-area" placeholder="Type exactly what the reference audio says"></textarea>
          </div>
          <div class="form-group">
            <label for="cloneFile">Reference recording (WAV, 3–10 s recommended)</label>
            <input type="file" id="cloneFile" accept="audio/wav,.wav">
          </div>
          <button type="submit" class="primary" id="cloneSubmit">Clone voice</button>
          <div id="cloneStatus" role="status"></div>
        </form>
      </section>
    </div>

    <div class="col-side">
      <section class="card" aria-labelledby="config-heading">
        <div class="section-head">
          <h2 id="config-heading">Configuration</h2>
          <span class="hint" id="configRevision"></span>
        </div>

        <dl class="stat-grid" id="configStats">
          <div><dt>Watcher</dt><dd id="statWatcher">—</dd></div>
          <div><dt>Last reload</dt><dd id="statReload">—</dd></div>
          <div><dt>Pending</dt><dd id="statPending">none</dd></div>
        </dl>
        <p class="muted mono-line">Config file: <code id="configPath"></code></p>

        <div class="toolbar">
          <button type="button" class="btn-secondary" id="configOpen" hidden>Open file</button>
          <button type="button" class="btn-secondary" id="configRefresh">Refresh</button>
          <button type="button" class="btn-secondary" id="configReload">Reload file</button>
          <button type="button" class="btn-secondary" id="configValidate">Validate</button>
        </div>

        <div class="config-filters">
          <input type="search" id="configSearch" placeholder="Search settings (e.g. port, volume, timeout)" aria-label="Search settings">
          <select id="configCategory" aria-label="Filter by category"></select>
        </div>

        <div id="configNotice" role="status"></div>
        <div id="configGroups" aria-live="polite">
          <p class="muted" id="configLoading">Loading configuration…</p>
        </div>
        <pre id="configError" role="alert" hidden></pre>
      </section>
    </div>
  </div>

  <p class="footer">
    Powered by <a href="https://huggingface.co/Supertone/supertonic-3" target="_blank" rel="noopener">Supertonic 3</a>
  </p>
</div>


</template>
