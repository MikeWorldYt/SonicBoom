import type { VoiceConstraints } from '../../services/contracts';

function object(value: unknown): value is Record<string, unknown> { return !!value && typeof value === 'object' && !Array.isArray(value); }
function strings(value: unknown): value is string[] { return Array.isArray(value) && value.every(item => typeof item === 'string'); }
/** Fail closed: never enable cloning on an unknown or malformed constraints contract. */
export function parseConstraints(value: unknown): VoiceConstraints {
  if (!object(value) || value.version !== 1 || !object(value.providers)) throw new Error('Invalid voice constraints response.');
  if (value.revision !== undefined && (typeof value.revision !== 'number' || !Number.isSafeInteger(value.revision) || value.revision < 1)) throw new Error('Invalid voice constraints response.');
  for (const key of ['max_upload_bytes', 'min_sample_rate', 'max_sample_rate', 'max_channels', 'name_max_chars', 'transcript_max_chars']) {
    if (typeof value[key] !== 'number' || !Number.isSafeInteger(value[key]) || (value[key] as number) <= 0) throw new Error('Invalid voice constraints response.');
  }
  if ((value.min_sample_rate as number) > (value.max_sample_rate as number)) throw new Error('Invalid voice sample rate bounds.');
  for (const provider of Object.values(value.providers)) {
    if (!object(provider) || !Array.isArray(provider.duration_seconds) || provider.duration_seconds.length !== 2 ||
        !provider.duration_seconds.every(item => typeof item === 'number' && Number.isFinite(item) && item > 0) ||
        provider.duration_seconds[0] > provider.duration_seconds[1] ||
        !strings(provider.clone_modes) || !strings(provider.models) || !strings(provider.languages) || !strings(provider.transcript_required_for)) throw new Error('Invalid provider constraints response.');
  }
  return value as unknown as VoiceConstraints;
}
