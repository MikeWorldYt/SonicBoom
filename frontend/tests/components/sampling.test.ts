import { it, expect } from 'vitest';
import { DEFAULT_SAMPLING } from '../../src/composables/useStudio';
import { samplingQuery, samplingErrors } from '../../src/features/speech/generationSchemas';
const bounds = { provider: 'qwen3-gguf', seed_max: '18446744073709551615', temperature: { min: 0, max: 5 }, top_k: { min: 0, max: 2048, integer: true }, top_p: { min: 0, max: 1, exclusive_min: true }, repetition_penalty: { min: 1, max: 4 } };
it('serializes only numeric sampling values within server bounds, accepting decimal comma', () => {
  expect(samplingQuery({ ...DEFAULT_SAMPLING, talkerTemperature: '0,9', talkerTopK: '2048', seed: bounds.seed_max }, bounds)).toMatchObject({ talker_temperature: .9, talker_top_k: 2048, seed: bounds.seed_max });
  for (const value of ['-1', '5.01', 'NaN', 'Infinity', '0x10', '1e3']) expect(samplingErrors({ ...DEFAULT_SAMPLING, talkerTemperature: value }, bounds)).toHaveProperty('talkerTemperature');
  expect(samplingErrors({ ...DEFAULT_SAMPLING, talkerTopP: '0', talkerTopK: '1.5', predictorRepetitionPenalty: '.9', seed: '18446744073709551616' }, bounds)).toMatchObject({ talkerTopP: expect.any(String), talkerTopK: expect.any(String), predictorRepetitionPenalty: expect.any(String), seed: expect.any(String) });
  expect(samplingErrors({ ...DEFAULT_SAMPLING, talkerTemperature: '.9' }, undefined)).toHaveProperty('talkerTemperature');
});
