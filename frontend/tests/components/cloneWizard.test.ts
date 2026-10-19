import { beforeEach, afterEach, describe, it, expect, vi } from 'vitest';
import { mount, flushPromises } from '@vue/test-utils';
import { ref } from 'vue';
import { File as NodeFile } from 'node:buffer';
import CloneWizard from '../../src/components/studio/CloneWizard.vue';
import FileDropzone from '../../src/components/ui/FileDropzone.vue';
const mock = vi.hoisted(() => ({ studio: null as any }));
vi.mock('../../src/composables/useStudio', () => ({ useStudio: () => mock.studio }));
const constraints = { version: 1, max_upload_bytes: 1_000_000, min_sample_rate: 8000, max_sample_rate: 192000, max_channels: 2, name_max_chars: 64, transcript_max_chars: 10000, providers: { 'pocket-tts': { duration_seconds: [1, 30], models: ['pocket-tts-en'], languages: ['en'], clone_modes: ['audio_prompt'], transcript_required_for: [] } } };
function wav(duration = 1) {
  const bytes = new ArrayBuffer(44 + duration * 8000 * 2), view = new DataView(bytes);
  const tag = (i: number, value: string) => [...value].forEach((c, n) => view.setUint8(i + n, c.charCodeAt(0)));
  tag(0, 'RIFF'); view.setUint32(4, bytes.byteLength - 8, true); tag(8, 'WAVE'); tag(12, 'fmt '); view.setUint32(16, 16, true);
  view.setUint16(20, 1, true); view.setUint16(22, 1, true); view.setUint32(24, 8000, true); view.setUint16(32, 2, true); view.setUint16(34, 16, true);
  tag(36, 'data'); view.setUint32(40, bytes.byteLength - 44, true);
  return new NodeFile([bytes], 'ref.wav') as unknown as File;
}
let wrapper: ReturnType<typeof mount>;
beforeEach(() => {
  vi.stubGlobal('URL', Object.assign(URL, { createObjectURL: vi.fn(() => 'blob:reference'), revokeObjectURL: vi.fn() }));
  mock.studio = { capabilities: ref([{ provider_id: 'pocket-tts', requires_reference_audio: true, requires_reference_transcript: false, models: ['pocket-tts-en'], languages: ['en'], clone_modes: ['audio_prompt'] }]), providers: ref([]),
    api: { voiceConstraints: vi.fn(async () => constraints), createVoice: vi.fn(async () => ({ id: 'saved', name: 'My voice', model: 'pocket-tts-en', provider: 'pocket-tts', language: 'en', clone_mode: 'audio_prompt', warnings: ['reference_nearly_silent'], defects: [], guidance: [], reference_health: { peak: 0, rms: 0, clipping_fraction: 0, low_amplitude_fraction: 1 } })) },
    refreshVoices: vi.fn(async () => {}), selectProfile: vi.fn() };
  wrapper = mount(CloneWizard, { attachTo: document.body });
});
afterEach(() => { wrapper.unmount(); vi.unstubAllGlobals(); });
async function reference(file: File) {
  await wrapper.get('form').trigger('submit');
  wrapper.getComponent(FileDropzone).vm.$emit('select', file);
  await flushPromises();
}
describe('cloning wizard', () => {
  it('catches short WAV before upload, associates error and focuses field', async () => {
    await reference(wav(.5));
    expect(wrapper.get('#cloneFile').attributes('aria-invalid')).toBe('true');
    expect(wrapper.get('#cloneFile-error').text()).toContain('expected 1–30');
    expect(document.activeElement?.id).toBe('cloneFile');
    expect(mock.studio.api.createVoice).not.toHaveBeenCalled();
  });
  it('retains draft on failure and successful report until explicit next action', async () => {
    await reference(wav());
    expect(wrapper.text()).toContain('8000 Hz');
    await wrapper.get('form').trigger('submit'); await flushPromises();
    await wrapper.get('#cloneName').setValue('My voice'); await wrapper.get('#cloneConsent').setValue('owner');
    mock.studio.api.createVoice.mockRejectedValueOnce(new Error('Network failed'));
    await wrapper.get('form').trigger('submit'); await flushPromises();
    expect((wrapper.get('#cloneName').element as HTMLInputElement).value).toBe('My voice');
    expect(wrapper.text()).toContain('Network failed');
    await wrapper.get('form').trigger('submit'); await flushPromises();
    expect(wrapper.get('#cloneResult').text()).toContain('saved');
    expect(wrapper.get('#cloneResult').text()).toContain('RMS');
    expect(wrapper.text()).toContain('nearly silent');
    expect(wrapper.emitted('created')).toBeUndefined();
    const another = wrapper.findAll('button').find(button => button.text() === 'Create another')!;
    await another.trigger('click');
    expect(wrapper.find('#cloneResult').exists()).toBe(false);
  });
  it('rechecks a changed byte cap before save and preserves reference', async () => {
    await reference(wav());
    await wrapper.get('form').trigger('submit'); await flushPromises();
    await wrapper.get('#cloneName').setValue('My voice'); await wrapper.get('#cloneConsent').setValue('owner');
    mock.studio.api.voiceConstraints.mockResolvedValue({ ...constraints, max_upload_bytes: 1024 });
    await wrapper.get('form').trigger('submit'); await flushPromises();
    expect(wrapper.text()).toContain('1024 bytes');
    expect(mock.studio.api.createVoice).not.toHaveBeenCalled();
  });
});

it('checks Unicode names and consent without uploading invalid drafts', async () => {
  await reference(wav());
  await wrapper.get('form').trigger('submit'); await flushPromises();
  await wrapper.get('#cloneName').setValue('語'.repeat(65));
  await wrapper.get('#cloneConsent').setValue('owner');
  await wrapper.get('form').trigger('submit'); await flushPromises();
  expect(wrapper.get('#cloneName').attributes('aria-invalid')).toBe('true');
  expect(mock.studio.api.createVoice).not.toHaveBeenCalled();
  await wrapper.get('#cloneName').setValue('語'.repeat(64));
  await wrapper.get('#cloneConsent').setValue(' ');
  await wrapper.get('form').trigger('submit'); await flushPromises();
  expect(wrapper.get('#cloneConsent').attributes('aria-invalid')).toBe('true');
  expect(mock.studio.api.createVoice).not.toHaveBeenCalled();
});

it('revalidates provider switching and ICL Unicode transcript boundaries', async () => {
  mock.studio.capabilities.value.push({ provider_id: 'qwen3-gguf', requires_reference_audio: true, requires_reference_transcript: true, models: ['qwen-base'], languages: ['en'], clone_modes: ['icl'] });
  mock.studio.api.voiceConstraints.mockResolvedValue({ ...constraints, providers: { ...constraints.providers, 'qwen3-gguf': { duration_seconds: [1, 30], models: ['qwen-base'], languages: ['en'], clone_modes: ['icl'], transcript_required_for: ['icl'] } } });
  await reference(wav());
  // Return to engine selection after choosing a Pocket recording.
  await wrapper.findAll('button').find(button => button.text() === 'Back')!.trigger('click');
  await wrapper.get('#cloneProvider').setValue('qwen3-gguf'); await flushPromises();
  expect((wrapper.get('#cloneModel').element as HTMLSelectElement).value).toBe('qwen-base');
  await wrapper.get('form').trigger('submit'); await flushPromises();
  await wrapper.get('form').trigger('submit'); await flushPromises();
  expect(wrapper.get('#cloneTranscript').attributes('aria-invalid')).toBe('true');
  await wrapper.get('#cloneTranscript').setValue('語'.repeat(10001));
  await wrapper.get('form').trigger('submit'); await flushPromises();
  expect(wrapper.get('#cloneTranscript-error').text()).toContain('10000');
  await wrapper.get('#cloneTranscript').setValue('語'.repeat(10000));
  await wrapper.get('form').trigger('submit'); await flushPromises();
  expect(wrapper.get('#cloneName').isVisible()).toBe(true);
  expect(mock.studio.api.createVoice).not.toHaveBeenCalled();
});

it('maps server field issues and correlation IDs without clearing the draft', async () => {
  const { ApiError } = await import('../../src/services/http');
  await reference(wav());
  await wrapper.get('form').trigger('submit'); await flushPromises();
  await wrapper.get('#cloneName').setValue('Draft'); await wrapper.get('#cloneConsent').setValue('owner');
  mock.studio.api.createVoice.mockRejectedValueOnce(new ApiError(400, 'Invalid name', { requestId: 'field-trace', fields: [{ field: 'name', code: 'invalid_length', message: 'Invalid name', expected: { min: 1, max: 64 }, received: 65 }] }));
  await wrapper.get('form').trigger('submit'); await flushPromises();
  expect(wrapper.get('#cloneName').attributes('aria-invalid')).toBe('true');
  expect((wrapper.get('#cloneName').element as HTMLInputElement).value).toBe('Draft');
  expect(wrapper.text()).toContain('field-trace');
  expect(document.activeElement?.id).toBe('cloneName');
});

it('does not select or announce a saved profile after the token changes during refresh', async () => {
  mock.studio.token = ref('old');
  let release!: () => void;
  mock.studio.refreshVoices.mockImplementationOnce(() => new Promise<void>(resolve => { release = resolve; }));
  await reference(wav());
  await wrapper.get('form').trigger('submit'); await flushPromises();
  await wrapper.get('#cloneName').setValue('Draft'); await wrapper.get('#cloneConsent').setValue('owner');
  const saving = wrapper.get('form').trigger('submit'); await flushPromises();
  mock.studio.token.value = 'new';
  release(); await saving; await flushPromises();
  expect(mock.studio.selectProfile).not.toHaveBeenCalled();
  expect(wrapper.text()).not.toContain('Voice saved.');
});

it.each([401, 413, 415, 429, 503])('preserves a retryable draft and correlation details after HTTP %s', async status => {
  const { ApiError } = await import('../../src/services/http');
  await reference(wav());
  await wrapper.get('form').trigger('submit'); await flushPromises();
  await wrapper.get('#cloneName').setValue('Retained draft'); await wrapper.get('#cloneConsent').setValue('owner');
  mock.studio.api.createVoice.mockRejectedValueOnce(new ApiError(status, 'Upload rejected', { requestId: `trace-${status}`, ...(status === 429 ? { retryAfter: 3 } : {}) }));
  await wrapper.get('form').trigger('submit'); await flushPromises();
  expect((wrapper.get('#cloneName').element as HTMLInputElement).value).toBe('Retained draft');
  expect(wrapper.text()).toContain(`trace-${status}`);
  expect(wrapper.find('#cloneResult').exists()).toBe(false);
  await wrapper.get('form').trigger('submit'); await flushPromises();
  expect(wrapper.get('#cloneResult').text()).toContain('saved');
});

it('submits only one upload while repeated save events are pending', async () => {
  await reference(wav());
  await wrapper.get('form').trigger('submit'); await flushPromises();
  await wrapper.get('#cloneName').setValue('Draft'); await wrapper.get('#cloneConsent').setValue('owner');
  let release!: (value: unknown) => void;
  mock.studio.api.createVoice.mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
  await wrapper.get('form').trigger('submit'); await flushPromises();
  await wrapper.get('form').trigger('submit'); await flushPromises();
  expect(mock.studio.api.createVoice).toHaveBeenCalledTimes(1);
  release({ id: 'saved', name: 'Draft', model: 'pocket-tts-en', provider: 'pocket-tts', language: 'en', clone_mode: 'audio_prompt', reference_health: { peak: 0, rms: 0, clipping_fraction: 0, low_amplitude_fraction: 1 } });
  await flushPromises();
  expect(wrapper.get('#cloneResult').text()).toContain('saved');
});
