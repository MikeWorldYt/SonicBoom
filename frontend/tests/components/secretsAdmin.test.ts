import { it, expect, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import SecretReveal from '../../src/components/ui/SecretReveal.vue';
import Admin from '../../src/pages/Admin.vue';
it('keeps the one-time token masked and supports explicit reveal/copy', async () => {
  const writeText = vi.fn(async () => {});
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
  const store = vi.spyOn(Storage.prototype, 'setItem');
  const wrapper = mount(SecretReveal, { props: { id: 'secret', value: 'private-token' } });
  expect(wrapper.get('input').attributes('type')).toBe('password');
  await wrapper.findAll('button')[0].trigger('click');
  expect(wrapper.get('input').attributes('type')).toBe('text');
  await wrapper.findAll('button')[1].trigger('click');
  expect(writeText).toHaveBeenCalledWith('private-token');
  expect(store).not.toHaveBeenCalled();
  wrapper.unmount();
});
it('blocks past UTC expiry and preserves native post/CSRF behavior', async () => {
  const wrapper = mount(Admin, { props: { data: { page: 'admin', csrf: 'csrf-value', tokens: [] } } });
  await wrapper.get('#token-expiry').setValue('2000-01-01T00:00');
  const event = new Event('submit', { cancelable: true });
  wrapper.get('form[action="/admin/tokens"]').element.dispatchEvent(event);
  await wrapper.vm.$nextTick();
  expect(event.defaultPrevented).toBe(true);
  expect(wrapper.get('#expiry-error').text()).toContain('UTC');
  expect(wrapper.get('form[action="/admin/tokens"] input[name="csrf_token"]').attributes('value')).toBe('csrf-value');
  expect(wrapper.get('form[action="/admin/tokens"]').attributes('method')).toBe('post');
  wrapper.unmount();
});
