<script setup lang="ts">
import { t } from '../../i18n';
import BaseButton from '../ui/BaseButton.vue';
import CloneEngineStep from '../voice/CloneEngineStep.vue';
import CloneReferenceStep from '../voice/CloneReferenceStep.vue';
import CloneReviewStep from '../voice/CloneReviewStep.vue';
import FeedbackNotice from '../FeedbackNotice.vue';
import { useCloneWizard } from '../../features/cloning/useCloneWizard';
const emit = defineEmits<{ created: []; engines: [] }>();
const { originalFile, applyTrim, restoreOriginal, created, fieldErrors, cancelAnalysis, createAnother, tryVoice, limits, metadata, previewUrl, pending, feedback, step, provider, model, language, mode, referenceWarnings, referenceDefects, referenceGuidance, referenceMessage, name, consent, transcript, file, choices, cap, languages, needsTranscript, engineReady, selectFile, next, save } = useCloneWizard(event => event === 'created' ? emit('created') : emit('engines'));
</script>
<template>
  <section class="clone-wizard" aria-labelledby="clone-heading"><h2 id="clone-heading">{{ t('Add a voice') }}</h2>
    <ol class="stepper" :aria-label="t('Voice setup progress')"><li v-for="(label, index) in ['Choose an engine', 'Reference recording', 'Review and save']" :key="label" :aria-current="step === index ? 'step' : undefined" :class="{ current: step === index }">{{ index + 1 }}. {{ t(label) }}</li></ol>
    <form v-if="!created" id="cloneForm" :aria-busy="pending" @submit.prevent="step < 2 ? next() : save()">
      <CloneEngineStep v-show="step === 0" v-model:provider="provider" v-model:model="model" v-model:language="language" :choices="choices" :cap="cap" :languages="languages" :ready="engineReady" :disabled="step !== 0 || pending"/>
      <CloneReferenceStep v-show="step === 1" v-model:mode="mode" v-model:transcript="transcript" :disabled="step !== 1 || pending" :provider="provider" :modes="cap?.clone_modes || []" :file="file" :original-file="originalFile" :metadata="metadata" :preview-url="previewUrl" :limits="limits" :errors="fieldErrors" :needs-transcript="needsTranscript" @select="selectFile" @trim="applyTrim" @restore="restoreOriginal"/>
      <CloneReviewStep v-show="step === 2" v-model:name="name" v-model:consent="consent" :disabled="step !== 2 || pending" :model="model" :language="language" :file-name="file?.name" :errors="fieldErrors"/>
      <div class="toolbar"><button v-if="pending && step === 1" type="button" @click="cancelAnalysis">{{ t('Cancel') }}</button><button v-if="step > 0"  type="button" :disabled="pending" @click="step--; feedback = null">{{ t('Back') }}</button><BaseButton v-if="step < 2" type="submit" variant="primary" :disabled="pending || !cap">{{ t('Continue') }}</BaseButton><BaseButton v-else id="cloneSubmit" type="submit" variant="primary" :loading="pending">{{ t(pending ? 'Cloning…' : 'Clone voice') }}</BaseButton><button v-if="step === 0 && !engineReady" type="button" class="btn-inline" @click="emit('engines')">{{ t('Manage engines') }}</button></div>
    </form>
    <section v-if="created" id="cloneResult" tabindex="-1" class="clone-report" aria-labelledby="clone-result-heading">
      <h3 id="clone-result-heading">{{ t('Voice created') }}: {{ created.name }}</h3>
      <p>{{ t('Voice profile ID') }}: <code>{{ created.id }}</code></p>
      <h4>{{ t('Reference health') }}</h4>
      <dl><dt>{{ t('Peak') }}</dt><dd>{{ created.reference_health.peak.toFixed(4) }}</dd><dt>{{ t('RMS') }}</dt><dd>{{ created.reference_health.rms.toFixed(4) }}</dd><dt>{{ t('Clipping') }}</dt><dd>{{ (created.reference_health.clipping_fraction * 100).toFixed(2) }}%</dd><dt>{{ t('Near silence') }}</dt><dd>{{ (created.reference_health.low_amplitude_fraction * 100).toFixed(2) }}%</dd></dl>
      <div class="toolbar"><button type="button" class="primary" @click="tryVoice">{{ t('Try voice') }}</button><button type="button" @click="createAnother">{{ t('Create another') }}</button></div>
    </section><FeedbackNotice :feedback="feedback"/><p v-for="defect in referenceDefects" :key="'defect-' + defect" role="alert" class="feedback error">{{ referenceMessage(defect) }}</p><p v-for="warning in referenceWarnings" :key="'warn-' + warning" role="status" class="help-note">{{ referenceMessage(warning) }}</p><p v-for="note in referenceGuidance" :key="'note-' + note" role="status" class="muted">{{ referenceMessage(note) }}</p>
  </section>
</template>
