import { ref, watch } from 'vue';
import en from './en.json' with { type: 'json' };
import es from './es.json' with { type: 'json' };

export const locales = [{ code: 'en', name: 'English' }, { code: 'es', name: 'Español' }] as const;
export type Locale = typeof locales[number]['code'];
export type MessageKey = keyof typeof en;
const catalogs: Record<Locale, Partial<Record<MessageKey, string>>> = { en, es };
const storageKey = 'sonicboom.locale';
function initialLocale(): Locale {
  try {
    const saved = localStorage.getItem(storageKey);
    if (locales.some(item => item.code === saved)) return saved as Locale;
  } catch { /* Storage can be unavailable in private or restricted contexts. */ }
  return navigator.language.toLowerCase().startsWith('es') ? 'es' : 'en';
}
export const locale = ref<Locale>(initialLocale());
export function formatDate(value: string): string {
  const date = new Date(value.replace(' UTC', 'Z').replace(' ', 'T'));
  if (!Number.isFinite(date.getTime())) return value;
  return new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium', timeStyle: 'short', timeZone: 'UTC' }).format(date) + ' UTC';
}
export function setLocale(value: string) {
  if (locales.some(item => item.code === value)) locale.value = value as Locale;
}
export function t(key: string, params: Record<string, string | number> = {}): string {
  const download = key.match(/^Downloading model\.\.\. \((\d+)%\)$/);
  if (download) return t('Download model: {progress}%', { progress: download[1] });
  if (key.startsWith('language.')) return languageName(key.slice(9));
  const catalog = catalogs[locale.value];
  const translated = Object.hasOwn(catalog, key) ? catalog[key as MessageKey] : undefined;
  const english = Object.hasOwn(en, key) ? en[key as MessageKey] : undefined;
  const template = translated ?? english ?? key;
  return template.replace(/\{(\w+)\}/g, (match, name) => String(params[name] ?? match));
}
export interface Message { key: string; params: Record<string, string | number>; render?: () => string }
export const computedMessage = (render: () => string): Message => ({ key: "", params: {}, render });
export const renderMessage = (message: Message) => message.render ? message.render() : t(message.key, message.params);
export const msg = (key: string, params: Message['params'] = {}): Message => ({ key, params });
export function uiError(key: string, params: Message['params'] = {}) {
  return Object.assign(new Error(t(key, params)), { localizedMessage: msg(key, params) });
}
export function errorMessage(error: unknown): Message {
  return computedMessage(() => {
    const localized = error instanceof Error && 'localizedMessage' in error;
    let message = localized ? renderMessage(error.localizedMessage as Message) : t(error instanceof Error ? error.message : String(error));
    const actions: Record<number, string> = {
      401: 'Check your API token or sign in again.',
      403: 'Check the account permissions and server access policy.',
      409: 'Refresh the current selection and review your draft before retrying.',
      413: 'Choose a smaller recording or reduce the request size.',
      415: 'Choose a supported WAV recording.',
      429: 'Wait before retrying this request.',
      503: 'Check engine readiness and try again when it is available.',
    };
    if (!localized && error instanceof Error && 'status' in error && typeof error.status === 'number' && actions[error.status]) message += ' ' + t(actions[error.status]);
    if (error instanceof Error && 'retryAfter' in error && typeof error.retryAfter === 'number' && Number.isFinite(error.retryAfter) && error.retryAfter > 0) message += ' ' + t('Try again in {seconds} seconds.', { seconds: Math.ceil(error.retryAfter) });
    if (error instanceof Error && 'requestId' in error && typeof error.requestId === 'string') message += ' (' + t('Request ID: {id}', { id: error.requestId }) + ')';
    return message;
  });
}
export function languageName(code: string) {
  try { return new Intl.DisplayNames([locale.value], { type: 'language' }).of(code) || code; }
  catch { return code; }
}
watch(locale, value => {
  document.documentElement.lang = value;
  document.title = t('SonicBoom · Voice studio');
  try { localStorage.setItem(storageKey, value); } catch { /* Preference remains active for this session. */ }
}, { immediate: true, flush: 'sync' });
