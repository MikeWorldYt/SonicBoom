import { describe, it, expect } from 'vitest';
import { mount } from '@vue/test-utils';
import BaseInput from '../../src/components/ui/BaseInput.vue';
import BaseTextarea from '../../src/components/ui/BaseTextarea.vue';
import FileDropzone from '../../src/components/ui/FileDropzone.vue';
describe('accessible shared fields', () => {
  for (const component of [BaseInput, BaseTextarea]) it(component.__name || 'field', async () => {
    const wrapper = mount(component, { props: { id: 'reference', label: 'Reference', help: 'Exact words', error: 'Required', required: true } });
    const control = wrapper.find('input, textarea');
    expect(wrapper.get('label').attributes('for')).toBe('reference');
    expect(control.attributes('aria-describedby')).toBe('reference-help reference-error');
    expect(control.attributes('aria-invalid')).toBe('true');
    expect(wrapper.get('[role="alert"]').text()).toBe('Required');
    await control.setValue('words');
    expect(wrapper.emitted('update:modelValue')?.[0]).toEqual(['words']);
  });
  it('has a native keyboard-operable file picker', async () => {
    const wrapper = mount(FileDropzone, { props: { id: 'wav', label: 'WAV', error: 'Corrupt' } });
    expect(wrapper.get('input').attributes('type')).toBe('file');
    expect(wrapper.get('input').attributes('aria-describedby')).toContain('wav-error');
    const file = new File(['RIFF'], 'reference.wav');
    await wrapper.get('.file-dropzone').trigger('drop', { dataTransfer: { files: [file] } });
    expect(wrapper.emitted('select')?.[0]).toEqual([file]);
  });
});

it('select fields associate errors and loading buttons prevent duplicate actions', async () => {
  const { default: BaseSelect } = await import('../../src/components/ui/BaseSelect.vue');
  const { default: BaseButton } = await import('../../src/components/ui/BaseButton.vue');
  const select = mount(BaseSelect, { props: { id: 'engine', label: 'Engine', options: [{ value: 'pocket', label: 'Pocket' }], error: 'Unavailable' } });
  expect(select.get('select').attributes('aria-describedby')).toBe('engine-error');
  expect(select.get('select').attributes('aria-invalid')).toBe('true');
  const button = mount(BaseButton, { props: { loading: true, type: 'submit' }, slots: { default: 'Save' } });
  expect(button.get('button').attributes('disabled')).toBeDefined();
  expect(button.get('button').attributes('aria-busy')).toBe('true');
  expect(button.get('button').text()).toBe('Save');
  select.unmount(); button.unmount();
});
