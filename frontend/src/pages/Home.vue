<script setup lang="ts">
import { onMounted, onUnmounted, ref, computed, nextTick } from 'vue';
import { startHome } from '../controllers/home';
import { t } from '../i18n';
import type { HomeData } from '../types';
const { data } = defineProps<{ data: HomeData }>();
const active = ref('create');
const currentSection = computed(() => sections.find(s => s.id === active.value)!);
const sections = [{ id: 'create', label: 'Create speech', icon: '◉', description: 'Turn your ideas into natural speech.' }, { id: 'voices', label: 'Voice library', icon: '≋', description: 'Create a voice that sounds like you.' }, { id: 'engines', label: 'Engines', icon: '◈', description: 'Choose what powers your voice.' }, { id: 'settings', label: 'Settings', icon: '⚙', description: 'Make SonicBoom work your way.' }];
function navigateTabs(event: KeyboardEvent) {
  const index = sections.findIndex(section => section.id === active.value);
  let next = index;
  if (event.key === 'ArrowDown' || event.key === 'ArrowRight') next = (index + 1) % sections.length;
  else if (event.key === 'ArrowUp' || event.key === 'ArrowLeft') next = (index + sections.length - 1) % sections.length;
  else if (event.key === 'Home') next = 0;
  else if (event.key === 'End') next = sections.length - 1;
  else return;
  event.preventDefault();
  active.value = sections[next].id;
  document.getElementById(`tab-${active.value}`)?.focus();
}
async function openComposer() {
  active.value = 'create';
  await nextTick();
  document.getElementById('text')?.focus();
}
let stop: (() => void) | undefined;
onMounted(() => { window.addEventListener('sonicboom:compose', openComposer); stop = startHome(data.modelReady, data.providers, data.status); });
onUnmounted(() => { window.removeEventListener('sonicboom:compose', openComposer); stop?.(); });
</script>
<template>
<div class="studio-layout">
  <aside class="workspace-nav">
    <p class="eyebrow">{{ t("WORKSPACE") }}</p>
    <div role="tablist" aria-orientation="vertical" :aria-label="t('Workspace')" @keydown="navigateTabs">
      <button v-for="section in sections" :key="section.id" :id="`tab-${section.id}`" role="tab" :aria-selected="active === section.id" :aria-controls="`panel-${section.id}`" :tabindex="active === section.id ? 0 : -1" :class="{ selected: active === section.id }" @click="active = section.id">
        <span class="nav-icon" aria-hidden="true">{{ section.icon }}</span><span>{{ t(section.label) }}</span><span class="nav-arrow" aria-hidden="true">↗</span>
      </button>
    </div>
    <div class="workspace-tip"><span class="tip-dot"></span><strong>{{ t("Made for your voice") }}</strong><p>{{ t("Start with built-in voices. Explore cloning when you’re ready.") }}</p></div>
  </aside>
  <main class="workspace-content">
    <header class="workspace-heading"><div><p class="eyebrow">{{ t("YOUR SOUND, YOUR WAY") }}</p><h1>{{ t(currentSection.label) }}</h1><p>{{ t(currentSection.description) }}</p></div><div class="model-status" id="modelStatus" :hidden="!data.status"></div></header>
        <div class="access-notice" id="tokenGroup">
          <label for="apiToken">{{ t("API token") }} <span class="hint">{{ t("(") }}<a href="/admin">{{ t("create one in the admin panel") }}</a>{{ t(")") }}</span></label>
          <input type="password" id="apiToken" :placeholder="t(&quot;Paste your API token here&quot;)" autocomplete="off" spellcheck="false">
        </div>
      <section v-show="active === 'create'" id="panel-create" role="tabpanel" :aria-labelledby="'tab-create'" class="card studio-panel">
        <div class="section-intro"><span class="eyebrow">{{ t("THE STUDIO") }}</span><h2 id="tts-heading">{{ t("Create speech") }}</h2><p class="muted">{{ t("Write something, choose a voice, and bring it to life.") }}</p></div>


        <div class="form-group">
          <div class="label-row">
            <label for="text">{{ t("What would you like to say?") }}</label>
            <span class="char-count" id="charCount"></span>
          </div>
          <textarea id="text" :placeholder="t(&quot;A story, an idea, a welcome message… Start writing here.&quot;)"></textarea>
        </div>

        <div class="form-group">
          <div class="label-row">
            <label for="model">{{ t("Model") }}</label>
            <button type="button" class="btn-inline" id="modelRefresh" :title="t(&quot;Reload engines, voices, and languages&quot;)">{{ t("Refresh") }}</button>
          </div>
          <!-- Options are rendered live from /api/providers in the home controller. -->
          <select id="model" :disabled="!data.modelReady">
            <option value="supertonic-3">{{ t("supertonic-3 (built-in voices)") }}</option>
          </select>
          <p class="muted" id="modelHint"></p>
        </div>

        <div class="row">
          <div class="field">
            <label for="voice">{{ t("Voice") }}</label>
            <select id="voice" :disabled="!data.modelReady">
              <option v-for="voice in data.voices" :key="voice" :value="voice">{{ voice }}</option>
            </select>
          </div>
          <div class="field">
            <label for="lang">{{ t("Language") }}</label>
            <!-- Options are rendered from the LANGUAGES const in the home controller. -->
            <select id="lang" :disabled="!data.modelReady"></select>
          </div>
        </div>

        <button id="btn" class="primary" :disabled="!data.modelReady"></button>
        <button type="button" id="cancelGeneration" class="btn-secondary" hidden>{{ t("Cancel generation") }}</button>
        <div id="status" role="status" aria-live="polite"></div>
        <a id="downloadAudio" download="sonicboom.wav" hidden>{{ t("Download generated audio") }}</a>
        <audio id="player" controls></audio><p class="output-hint">{{ t("Your audio will appear here, ready to listen or download.") }}</p>
      </section>
      <section v-show="active === 'engines'" id="panel-engines" role="tabpanel" :aria-labelledby="'tab-engines'" class="card studio-panel">
        <div class="section-head">
          <h2 id="providers-heading">{{ t("Voice engines") }}</h2>
          <span class="hint">{{ t("Live status from the server") }}</span>
        </div>
        <div class="provider-list"></div>
        <details><summary>{{ t("Advanced inference diagnostics") }}</summary><pre id="inferenceDiagnostics">{{ t("Load provider status to view diagnostics.") }}</pre></details>
        <p class="help-note">{{ t("Enable an engine to download its models and make its voices available in the studio. The first download may take a few minutes.") }}</p>
      </section>
      <section v-show="active === 'voices'" id="panel-voices" role="tabpanel" :aria-labelledby="'tab-voices'" class="card studio-panel">
        <div class="section-head">
          <h2 id="cloning-heading">{{ t("Voice cloning") }}</h2>
          <button type="button" class="btn-inline" @click="active = 'engines'">{{ t("Manage engines") }} ↗</button>
        </div>
        <div class="voice-list" id="voicesList">
          <p class="muted">{{ t("Loading voices…") }}</p>
        </div>

        <h3 class="subhead">{{ t("Add a voice") }}</h3>
        <p class="muted">{{ t("Upload 3–10 seconds of clear speech. Only use a voice you own or have permission to use. Add the exact transcript for ICL mode; X-vector mode only needs the recording.") }}</p>
        <form id="cloneForm">
          <div class="row">
            <div class="field">
              <label for="cloneName">{{ t("Name") }}</label>
              <input type="text" id="cloneName" maxlength="64" :placeholder="t(&quot;e.g. narrator&quot;)" autocomplete="off">
            </div>
            <div class="field">
              <label for="cloneProvider">{{ t("Engine") }}</label>
              <select id="cloneProvider"></select>
            </div>
          </div>
          <div class="row">
            <div class="field">
              <label for="cloneModel">{{ t("Model") }}</label>
              <select id="cloneModel"></select>
            </div>
            <div class="field">
              <label for="cloneLang">{{ t("Language") }}</label>
              <select id="cloneLang"></select>
            </div>
          </div>
          <div class="row">
            <div class="field">
              <label for="cloneMode">{{ t("Mode") }}</label>
              <select id="cloneMode"></select>
            </div>
            <div class="field">
              <label for="cloneConsent">{{ t("Consent basis") }}</label>
              <input type="text" id="cloneConsent" :placeholder="t(&quot;I own this voice or have permission&quot;)" autocomplete="off">
            </div>
          </div>
          <div class="form-group" id="cloneTranscriptGroup">
            <label for="cloneTranscript">{{ t("Reference transcript (exact words in the recording)") }}</label>
            <textarea id="cloneTranscript" class="compact-area" :placeholder="t(&quot;Type exactly what the reference audio says&quot;)"></textarea>
          </div>
          <div class="form-group">
            <label for="cloneFile">{{ t("Reference recording (WAV, 3–10 s recommended)") }}</label>
            <input type="file" id="cloneFile" accept="audio/wav,.wav">
          </div>
          <button type="submit" class="primary" id="cloneSubmit"></button>
          <div id="cloneStatus" role="status"></div>
        </form>
      </section>
      <section v-show="active === 'settings'" id="panel-settings" role="tabpanel" :aria-labelledby="'tab-settings'" class="card studio-panel">
        <div class="section-head">
          <h2 id="config-heading">{{ t("Configuration") }}</h2>
          <span class="hint" id="configRevision"></span>
        </div>

        <p class="muted settings-guide">{{ t("Choose a category or search for a setting. Each change is saved individually; its badge explains when it takes effect.") }}</p><dl class="stat-grid" id="configStats">
          <div><dt>{{ t("Watcher") }}</dt><dd id="statWatcher">{{ t("—") }}</dd></div>
          <div><dt>{{ t("Last reload") }}</dt><dd id="statReload">{{ t("—") }}</dd></div>
          <div><dt>{{ t("Pending") }}</dt><dd id="statPending"></dd></div>
        </dl>
        <p class="muted mono-line">{{ t("Config file:") }} <code id="configPath"></code></p>

        <div class="toolbar">
          <button type="button" class="btn-secondary" id="configOpen" hidden>{{ t("Open file") }}</button>
          <button type="button" class="btn-secondary" id="configRefresh">{{ t("Refresh") }}</button>
          <button type="button" class="btn-secondary" id="configReload">{{ t("Reload file") }}</button>
          <button type="button" class="btn-secondary" id="configValidate">{{ t("Validate") }}</button>
        </div>

        <div class="config-filters">
          <input type="search" id="configSearch" :placeholder="t(&quot;Search settings (e.g. port, volume, timeout)&quot;)" :aria-label="t(&quot;Search settings&quot;)">
          <select id="configCategory" :aria-label="t(&quot;Filter by category&quot;)"></select>
        </div>

        <div id="configNotice" role="status"></div>
        <div id="configGroups" aria-live="polite">
          <p class="muted" id="configLoading">{{ t("Loading configuration…") }}</p>
        </div>
        <pre id="configError" role="alert" hidden></pre>
      </section>
  </main>
</div>
</template>
