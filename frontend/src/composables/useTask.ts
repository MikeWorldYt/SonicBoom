import { ref, shallowRef } from 'vue';
import { errorMessage, msg, type Message } from '../i18n';
export interface Feedback { kind: 'error' | 'success' | 'info'; message: Message }
export function useTask() {
  const pending = ref(false);
  const feedback = shallowRef<Feedback | null>(null);
  async function run<T>(action: () => Promise<T>, success?: string): Promise<T | undefined> {
    if (pending.value) return;
    pending.value = true;
    feedback.value = null;
    try { const result = await action(); if (success) feedback.value = { kind: 'success', message: msg(success) }; return result; }
    catch (error) { feedback.value = { kind: 'error', message: errorMessage(error) }; return undefined; }
    finally { pending.value = false; }
  }
  return { pending, feedback, run };
}
