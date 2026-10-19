import type { VoiceConstraints } from '../../services/contracts';
import { uiError } from '../../i18n';
import { inspectWave, sampleAt } from '../audio/wav';
export interface ReferenceMetadata { duration: number; sampleRate: number; channels: number; bytes: number; encoding: string }
export async function validateReference(file: File, limits: VoiceConstraints, provider: string, signal?: AbortSignal): Promise<ReferenceMetadata> {
  if (!file.size || file.size > limits.max_upload_bytes) throw uiError('Reference must be 1–{max} bytes.', { max: limits.max_upload_bytes });
  const bytes = await file.arrayBuffer(); signal?.throwIfAborted();
  const wave = inspectWave(bytes);
  if (wave.channels > limits.max_channels) throw uiError('Use 1–{max} audio channels.', { max: limits.max_channels });
  if (wave.sampleRate < limits.min_sample_rate || wave.sampleRate > limits.max_sample_rate) throw uiError('Sample rate must be {min}–{max} Hz.', { min: limits.min_sample_rate, max: limits.max_sample_rate });
  const constraint = limits.providers[provider];
  if (!constraint) throw uiError('This cloning provider is unavailable.');
  const [min, max] = constraint.duration_seconds;
  if (wave.duration < min || wave.duration > max) throw uiError('Recording duration is {duration} s; expected {min}–{max} s.', { duration: wave.duration.toFixed(2), min, max });
  if (wave.format === 3) for (let index = 0; index < wave.dataSize / 4; index++) {
    if (!Number.isFinite(sampleAt(wave, index))) throw uiError('The recording contains invalid samples.');
    if (index % 65536 === 0) { signal?.throwIfAborted(); await new Promise<void>(resolve => setTimeout(resolve, 0)); }
  }
  signal?.throwIfAborted();
  return { duration: wave.duration, sampleRate: wave.sampleRate, channels: wave.channels, bytes: file.size, encoding: `${wave.format === 3 ? 'float' : 'PCM'} ${wave.validBits === wave.bits ? wave.bits : wave.validBits + '-in-' + wave.bits}-bit WAV` };
}
