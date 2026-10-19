import { it, expect, vi } from 'vitest';
import { mount, flushPromises } from '@vue/test-utils';
import { defineComponent, h } from 'vue';
import AppErrorBoundary from '../../src/components/ui/AppErrorBoundary.vue';
import { parsePageData } from '../../src/app/pageData';
it('accepts Rust null optional page fields and rejects malformed pages', () => {
  expect(parsePageData('{"page":"login","error":null}')).toMatchObject({ page: 'login', error: undefined });
  expect(parsePageData(JSON.stringify({ page: 'home', modelReady: false, voices: [], status: 'idle', providers: [{ models: 'pocket-tts-en', status: { provider_id: 'pocket-tts', enabled: false, state: 'disabled', effective_device: null, active_model: null, detail: null } }] }))).toMatchObject({ page: 'home' });
  expect(() => parsePageData('{"page":"home","voices":"wrong"}')).toThrow();
});
it('contains an unexpected child failure without exposing private error contents', async () => {
  const Broken = defineComponent({ render() { throw new Error('private-transcript'); } });
  const wrapper = mount(AppErrorBoundary, { slots: { default: () => h(Broken) } });
  await flushPromises();
  expect(wrapper.get('[role="alert"]').text()).toContain('The interface could not load.');
  expect(wrapper.text()).not.toContain('private-transcript');
  wrapper.unmount();
});
