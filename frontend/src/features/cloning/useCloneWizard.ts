import { computed, ref, watch, onBeforeUnmount, reactive, nextTick } from 'vue';
import { t, uiError, msg, renderMessage, errorMessage, type Message } from '../../i18n';
import { useStudio } from '../../composables/useStudio';
import { modelLanguages } from '../../services/providerLanguages';
import { ApiError } from '../../services/http';
import { useTask } from '../../composables/useTask';
import { validateReference, type ReferenceMetadata } from './validateReference';
import type { VoiceCreateResult, VoiceConstraints } from '../../services/contracts';

export function useCloneWizard(emit: (event: 'created' | 'engines') => void) {
const fieldIssues = reactive<Record<string, Message>>({});
const fieldErrors = computed(() => Object.fromEntries(Object.entries(fieldIssues).map(([field, issue]) => [field, renderMessage(issue)])));
const created = ref<VoiceCreateResult | null>(null);
let analysis: AbortController | null = null;
let serial = 0;
let revalidate = false;
async function invalid(field: string, message: Message): Promise<never> {
  fieldIssues[field] = message;
  step.value = ['cloneName', 'cloneConsent'].includes(field) ? 2 : field === 'cloneProvider' ? 0 : 1;
  await nextTick();
  document.getElementById(field)?.focus();
  throw Object.assign(new Error(renderMessage(message)), { localizedMessage: message });
}
function cancelAnalysis() { ++serial; analysis?.abort(); }
const limits = ref<VoiceConstraints | null>(null);
const metadata = ref<ReferenceMetadata | null>(null);
const previewUrl = ref('');
watch(previewUrl, (value, old) => { if (old) URL.revokeObjectURL(old); });
let disposed = false;
onBeforeUnmount(() => { disposed = true; cancelAnalysis(); if (previewUrl.value) URL.revokeObjectURL(previewUrl.value); });
const studio = useStudio();
const { capabilities, providers } = studio;
const identity = () => studio.scopeEpoch?.value ?? studio.token?.value;
const task = useTask(); const { pending, feedback } = task;
watch(pending, async value => {
  if (!value && revalidate && file.value && !created.value) { revalidate = false; void task.run(() => validateRecording(false)); return; }
  if (!value && Object.keys(fieldIssues).length) {
    await nextTick(); document.getElementById(Object.keys(fieldIssues)[0])?.focus();
  }
});
const step = ref(0);
const provider = ref(''); const model = ref(''); const language = ref('en'); const mode = ref('icl');
const referenceWarnings = ref<string[]>([]);
const referenceDefects = ref<string[]>([]);
const referenceGuidance = ref<string[]>([]);
function referenceMessage(code: string) {
  const messages: Record<string, string> = {
    reference_nearly_silent: 'The reference recording is nearly silent. Choose a clearer recording.',
    reference_heavily_clipped: 'The reference recording has heavy clipping. Try recording at a lower input level.',
    reference_large_dc_offset: 'The reference recording has a large DC offset. Check the recording source.',
    reference_non_finite_samples: 'The reference recording contains invalid (non-finite) samples and must be replaced.',
    reference_unusually_quiet: 'The reference recording is unusually quiet. Increase the recording level.',
    reference_ends_during_speech: 'The recording appears to end during speech. A little trailing silence may help cloning.',
    reference_possible_background_noise: 'The reference may contain background noise. A quieter recording is recommended.',
    reference_short_for_cloning: 'A 5-10 second recording usually clones better than this short clip.',
    reference_leading_silence: 'The recording starts with a long silence.',
    reference_trailing_silence: 'The recording ends with a long silence.',
  };
  return t(messages[code] || code);
}
const originalFile = ref<File | null>(null);
const name = ref(''); const consent = ref(''); const transcript = ref(''); const file = ref<File | null>(null);
const choices = computed(() => capabilities.value.filter(cap => cap.requires_reference_audio));
const cap = computed(() => choices.value.find(item => item.provider_id === provider.value));
const languages = computed(() => modelLanguages(cap.value, model.value));
watch(languages, value => { if (!value.includes(language.value)) language.value = value[0] || 'en'; });
const needsTranscript = computed(() => {
  const constraint = limits.value?.providers[provider.value];
  return constraint ? constraint.transcript_required_for.includes(mode.value) : mode.value === 'icl' || !!cap.value?.requires_reference_transcript;
});
const engineReady = computed(() => providers.value.some(item => item.provider_id === provider.value && item.state === 'ready'));
watch(choices, value => { if (!value.some(item => item.provider_id === provider.value)) provider.value = value[0]?.provider_id || ''; }, { immediate: true });
watch(cap, value => {
  if (!value?.models.includes(model.value)) model.value = value?.models[0] || '';
  if (!value?.languages.includes(language.value)) language.value = value?.languages[0] || 'en';
  if (!value?.clone_modes.includes(mode.value)) mode.value = value?.clone_modes[0] || 'icl';
}, { immediate: true });
watch(name, () => { delete fieldIssues.cloneName; });
watch(consent, () => { delete fieldIssues.cloneConsent; });
watch(transcript, () => { delete fieldIssues.cloneTranscript; });
watch(needsTranscript, value => { if (!value) transcript.value = ''; });
async function selectFile(selected: File | null, trimmed = false) {
  if (!trimmed) originalFile.value = selected;
  cancelAnalysis();
  metadata.value = null; previewUrl.value = '';
  referenceWarnings.value = []; referenceDefects.value = []; referenceGuidance.value = [];
  file.value = selected;
  delete fieldIssues.cloneFile;
  if (selected) await task.run(() => validateRecording(false));
}
async function validateRecording(checkTranscript = true) {
  for (const key of Object.keys(fieldIssues)) delete fieldIssues[key];
  const current = ++serial;
  analysis = new AbortController();
  if (!file.value) return invalid('cloneFile', msg('Choose a WAV reference recording.'));
  if (!file.value.size) throw uiError('The selected file is empty.');
  limits.value = await studio.api.voiceConstraints(analysis.signal);
  const selected = file.value;
  let result: ReferenceMetadata;
  try { result = await validateReference(selected, limits.value, provider.value, analysis.signal); }
  catch (error) {
    if (analysis.signal.aborted || current !== serial) throw uiError('Request cancelled.');
    return invalid('cloneFile', errorMessage(error));
  }
  if (disposed || current !== serial || file.value !== selected) throw uiError('Reference selection changed. Please try again.');
  metadata.value = result;
  if (!previewUrl.value) previewUrl.value = URL.createObjectURL(selected);
  const constraint = limits.value.providers[provider.value];
  if (!constraint?.models.includes(model.value) || !constraint.clone_modes.includes(mode.value) || !languages.value.includes(language.value)) return invalid('cloneProvider', msg('Engine selection changed. Choose a supported model, mode and language.'));
  if ([...transcript.value.trim()].length > limits.value.transcript_max_chars) return invalid('cloneTranscript', msg('Transcript exceeds {max} characters.', { max: limits.value.transcript_max_chars }));
  if (checkTranscript && needsTranscript.value && !transcript.value.trim()) return invalid('cloneTranscript', msg('ICL mode needs the exact reference transcript.'));
}
async function next() {
  await task.run(async () => {
    if (step.value === 0) { if (!cap.value) throw uiError('No cloning engine is available right now.'); if (!model.value) throw uiError('Pick a model.'); }
    else {
      await validateRecording();

    }
    step.value++;
  });
}
async function save() {
  const owner = identity();
  await task.run(async () => {
    await validateRecording();
    if (disposed || owner !== identity()) return;
    if (!name.value.trim() || [...name.value.trim()].length > limits.value!.name_max_chars) return invalid('cloneName', msg('Name must be 1–{max} characters.', { max: limits.value!.name_max_chars }));
    if (!consent.value.trim()) return invalid('cloneConsent', msg('Consent basis is required (e.g. owner).'));
    referenceWarnings.value = [];
    referenceDefects.value = [];
    referenceGuidance.value = [];
    let profile: VoiceCreateResult;
    try { profile = await studio.api.createVoice({ expected_revision: limits.value?.revision, name: name.value.trim(), provider: provider.value, model: model.value, language: language.value, mode: mode.value, consent_basis: consent.value.trim(), ...(needsTranscript.value ? { transcript: transcript.value.trim() } : {}) }, file.value!); }
    catch (error) {
      if (disposed || owner !== identity()) return;
      if (error instanceof ApiError && error.fields) {
        const ids: Record<string, string> = { name: 'cloneName', transcript: 'cloneTranscript', consent_basis: 'cloneConsent', provider: 'cloneProvider', model: 'cloneModel', language: 'cloneLang', mode: 'cloneMode', file: 'cloneFile' };
        for (const issue of error.fields) {
          const id = ids[issue.field];
          if (!id) continue;
          fieldIssues[id] = issue.field === 'transcript' && issue.code === 'required' ? msg('ICL mode needs the exact reference transcript.') :
            issue.field === 'transcript' && issue.code === 'too_long' && typeof issue.expected === 'number' ? msg('Transcript exceeds {max} characters.', { max: issue.expected }) :
            issue.field === 'name' ? msg('Name must be 1–{max} characters.', { max: limits.value!.name_max_chars }) :
            issue.field === 'consent_basis' ? msg('Consent basis is required (e.g. owner).') : msg(issue.message);
        }
        const id = Object.keys(fieldIssues)[0];
        if (id) step.value = ['cloneName', 'cloneConsent'].includes(id) ? 2 : ['cloneProvider', 'cloneModel', 'cloneLang'].includes(id) ? 0 : 1;
      }
      throw error;
    }
    if (disposed || owner !== identity()) return;
    referenceWarnings.value = profile.warnings || [];
    referenceDefects.value = profile.defects || [];
    referenceGuidance.value = profile.guidance || [];
    created.value = profile;
    await studio.refreshVoices();
    if (disposed || owner !== identity()) return;
    studio.selectProfile(profile);
    feedback.value = { kind: 'success', message: msg('Voice saved. Preview it before using it in a longer recording.') };
    await nextTick();
    document.getElementById('cloneResult')?.focus();
  });
  if (owner !== identity()) feedback.value = null;
}
watch(identity, () => {
  cancelAnalysis(); created.value = null; previewUrl.value = ''; metadata.value = null;
  referenceWarnings.value = []; referenceDefects.value = []; referenceGuidance.value = [];
}, { flush: 'sync' });
async function applyTrim(selected: File) { await selectFile(selected, true); }
async function restoreOriginal() { if (originalFile.value) await selectFile(originalFile.value, true); }
function createAnother() { created.value = null; originalFile.value = null; step.value = 0; name.value = ''; consent.value = ''; transcript.value = ''; file.value = null; metadata.value = null; previewUrl.value = ''; }
function tryVoice() { if (created.value) { studio.selectProfile(created.value, true); emit('created'); } }
watch([provider, model, mode, language], () => {
  cancelAnalysis(); metadata.value = null;
  if (!file.value || created.value) return;
  if (pending.value) revalidate = true;
  else { revalidate = false; void task.run(() => validateRecording(false)); }
});
return { originalFile, applyTrim, restoreOriginal, created, fieldErrors, cancelAnalysis, createAnother, tryVoice, limits, metadata, previewUrl, pending, feedback, step, provider, model, language, mode, referenceWarnings, referenceDefects, referenceGuidance, referenceMessage, name, consent, transcript, file, choices, cap, languages, needsTranscript, engineReady, selectFile, next, save };
}
