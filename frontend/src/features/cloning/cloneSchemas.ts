import { z } from '../../services/schema';
import type { VoiceCreateResult, Voice } from '../../services/contracts';
export const voiceSummarySchema: z.ZodType<Voice> = z.object({
  model_revision: z.string().optional(), id: z.string().min(1), name: z.string(), provider: z.string(), model: z.string(),
  language: z.string(), clone_mode: z.string(), created_at: z.string().optional(), updated_at: z.string().optional(),
  reference_duration_secs: z.number().finite().optional(), reference_sample_rate: z.number().int().positive().optional(),
});
export const voiceCreateResultSchema: z.ZodType<VoiceCreateResult> = z.object({
  revision: z.number().int().positive().optional(),
  model_revision: z.string().optional(), id: z.string().min(1), name: z.string(), provider: z.string(), model: z.string(), language: z.string(), clone_mode: z.string(),
  reference_duration_secs: z.number().finite().optional(), reference_sample_rate: z.number().int().positive().optional(),
  created_at: z.string().optional(), updated_at: z.string().optional(),
  reference_health: z.object({
    duration_secs: z.number().finite(), source_channels: z.number().int().positive(), source_sample_rate: z.number().int().positive(),
    peak: z.number().finite(), rms: z.number().finite(), clipping_fraction: z.number().finite(),
    low_amplitude_fraction: z.number().finite(), non_finite_samples: z.number().int().nonnegative(),
  }),
  warnings: z.array(z.string()), defects: z.array(z.string()), guidance: z.array(z.string()),
});
