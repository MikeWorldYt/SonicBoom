import type { Capabilities } from './contracts';

/** Pocket exports are language-specific; other providers retain their capabilities. */
export function modelLanguages(cap: Capabilities | undefined, model: string): string[] {
  if (!cap) return [];
  if (cap.provider_id !== 'pocket-tts') return cap.languages;
  const language = /^pocket-tts-([a-z]{2})(?:-24l)?$/.exec(model)?.[1];
  return language && cap.languages.includes(language) ? [language] : [];
}
