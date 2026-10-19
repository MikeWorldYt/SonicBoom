import type { NumericBounds, SynthesisConstraints } from '../../services/contracts';
import { uiError } from '../../i18n';
export const samplingFields = [
  ['talkerTemperature', 'talker_temperature', 'temperature'],
  ['talkerTopK', 'talker_top_k', 'top_k'],
  ['talkerTopP', 'talker_top_p', 'top_p'],
  ['talkerRepetitionPenalty', 'talker_repetition_penalty', 'repetition_penalty'],
  ['predictorTemperature', 'predictor_temperature', 'temperature'],
  ['predictorTopK', 'predictor_top_k', 'top_k'],
  ['predictorTopP', 'predictor_top_p', 'top_p'],
  ['predictorRepetitionPenalty', 'predictor_repetition_penalty', 'repetition_penalty'],
] as const;
export interface SamplingDraft {
  preset: string; seed: string; quality: string;
  talkerTemperature: string; talkerTopK: string; talkerTopP: string; talkerRepetitionPenalty: string;
  predictorTemperature: string; predictorTopK: string; predictorTopP: string; predictorRepetitionPenalty: string;
}
export type SamplingParameters = Partial<Record<typeof samplingFields[number][1], number>> & { sampling_preset?: string; seed?: string; quality?: string };
export function parseNumeric(raw: string, bounds: NumericBounds): number {
  // Accept a single decimal comma, but never accept grouping or exponential/hex notation.
  const text = raw.trim().replace(',', '.');
  if (!/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)$/.test(text)) throw uiError('Enter a valid number');
  const value = Number(text);
  if (!Number.isFinite(value) || value < bounds.min || value > bounds.max || (bounds.exclusive_min && value === bounds.min)) throw uiError('Value is outside the allowed range.');
  if (bounds.integer && !Number.isInteger(value)) throw uiError('Enter a whole number');
  return value;
}
export function samplingErrors(draft: SamplingDraft, constraints: SynthesisConstraints['sampling'] | undefined): Record<string, string> {
  const errors: Record<string, string> = {};
  try { samplingQuery(draft, constraints); } catch { /* Collect every field below. */ }
  if (!['balanced', 'stable', 'expressive', 'custom'].includes(draft.preset)) errors.preset = 'Choose an allowed value.';
  if (!['', 'off', 'report', 'retry'].includes(draft.quality)) errors.quality = 'Choose an allowed value.';
  if (draft.seed.trim() && (!/^\d{1,20}$/.test(draft.seed.trim()) || BigInt(draft.seed.trim()) > BigInt(constraints?.seed_max ?? '18446744073709551615'))) errors.seed = 'Enter a valid unsigned 64-bit seed.';
  for (const [field, , bound] of samplingFields) {
    if (!draft[field].trim()) continue;
    if (!constraints) { errors[field] = 'Sampling constraints are unavailable. Refresh the server status.'; continue; }
    try { parseNumeric(draft[field], constraints[bound]); } catch (error) { errors[field] = error instanceof Error ? error.message : String(error); }
  }
  return errors;
}
export function samplingQuery(draft: SamplingDraft, constraints?: SynthesisConstraints['sampling']): SamplingParameters {
  const result: SamplingParameters = {};
  if (!['balanced', 'stable', 'expressive', 'custom'].includes(draft.preset) || !['', 'off', 'report', 'retry'].includes(draft.quality)) throw uiError('Choose an allowed value.');
  result.sampling_preset = draft.preset;
  if (draft.quality) result.quality = draft.quality;
  if (draft.seed.trim()) {
    if (!/^\d{1,20}$/.test(draft.seed.trim()) || BigInt(draft.seed.trim()) > BigInt(constraints?.seed_max ?? '18446744073709551615')) throw uiError('Enter a valid unsigned 64-bit seed.');
    result.seed = draft.seed.trim();
  }
  for (const [field, query, bound] of samplingFields) {
    if (!draft[field].trim()) continue;
    if (!constraints) throw uiError('Sampling constraints are unavailable. Refresh the server status.');
    result[query] = parseNumeric(draft[field], constraints[bound]);
  }
  return result;
}
