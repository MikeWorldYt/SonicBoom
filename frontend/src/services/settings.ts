import { uiError } from '../i18n';
import type { Json, SettingSchema } from './contracts';
export function formatSetting(value: Json | undefined): string | boolean {
  if (typeof value === 'boolean') return value;
  if (value === null || value === undefined) return '';
  return typeof value === 'object' ? JSON.stringify(value) : String(value);
}
export function parseSetting(meta: SettingSchema, raw: string | number | boolean): Json {
  if (meta.nullable && (raw === '' || raw === 'null')) return null;
  if (meta.allowed_values?.length && !meta.allowed_values.includes(String(raw))) throw uiError('Choose an allowed value.');
  if (meta.type === 'boolean') {
    if (![true, false, 'true', 'false'].includes(raw as boolean | string)) throw uiError('Choose an allowed value.');
    return raw === true || raw === 'true';
  }
  if (meta.type === 'number' || meta.type === 'integer') {
    if (String(raw).trim() === '') throw uiError('Enter a number');
    const value = Number(raw);
    if (!Number.isFinite(value)) throw uiError('Enter a valid number');
    if (meta.type === 'integer' && !Number.isInteger(value)) throw uiError('Enter a whole number');
    if ((meta.min !== undefined && value < meta.min) || (meta.max !== undefined && value > meta.max)) throw uiError('Value is outside the allowed range.');
    return value;
  }
  if (meta.type === 'object' || meta.type === 'array') {
    let value: Json;
    try { value = JSON.parse(String(raw)) as Json; } catch { throw uiError('Enter valid JSON'); }
    if (meta.type === 'array' && !Array.isArray(value)) throw uiError('Enter a JSON array, e.g. ["a"]');
    if (meta.type === 'object' && (!value || typeof value !== 'object' || Array.isArray(value))) throw uiError('Enter a JSON object');
    return value;
  }
  return String(raw);
}
export const applyLabels = { hot: 'Live', restart_server: 'Restarts server', restart_subsystem: 'Restarts worker', reload_model: 'Reloads model', restart_process: 'Needs restart' };
export const categories: Record<string, string> = { server: 'Server', admin: 'Admin', audio: 'Audio playback', model: 'Model', tts: 'TTS limits', inference: 'Inference', rate_limit: 'Rate limiting', logging: 'Logging', paths: 'Paths', security: 'Security', providers: 'Voice providers', voices: 'Voices' };
export const frequentSettings = new Set(['server.port', 'audio.output_device', 'audio.volume', 'model.inference_steps', 'tts.max_text_length']);
