import { it, expect } from 'vitest';
import { File as NodeFile } from 'node:buffer';
import { validateReference } from '../../src/features/cloning/validateReference';
import { inspectWave, cropWave } from '../../src/features/audio/wav';
const constraints = { version: 1, max_upload_bytes: 64 * 1024 * 1024, min_sample_rate: 8000, max_sample_rate: 192000, max_channels: 2, name_max_chars: 64, transcript_max_chars: 10000, providers: { pocket: { duration_seconds: [1, 30] as [number, number], clone_modes: ['audio_prompt'], models: ['pocket'], languages: ['en'], transcript_required_for: [] } } };
function buffer(seconds = 1, bits = 16, channels = 1, format = 1) {
  const size = seconds * 8000 * channels * bits / 8, bytes = new ArrayBuffer(44 + size + size % 2), view = new DataView(bytes);
  const tag = (offset: number, text: string) => [...text].forEach((char, index) => view.setUint8(offset + index, char.charCodeAt(0)));
  tag(0, 'RIFF'); view.setUint32(4, bytes.byteLength - 8, true); tag(8, 'WAVEfmt ');
  view.setUint32(16, 16, true); view.setUint16(20, format, true); view.setUint16(22, channels, true); view.setUint32(24, 8000, true);
  view.setUint32(28, 8000 * channels * bits / 8, true); view.setUint16(32, channels * bits / 8, true); view.setUint16(34, bits, true);
  tag(36, 'data'); view.setUint32(40, size, true); return bytes;
}
const file = (bytes: ArrayBuffer) => new NodeFile([bytes], 'renamed.wav') as unknown as File;
it('covers exact duration edges and supported PCM/float encodings without conversion', async () => {
  for (const seconds of [1, 5, 30]) expect((await validateReference(file(buffer(seconds)), constraints, 'pocket')).duration).toBe(seconds);
  for (const seconds of [.999, 30.001]) await expect(validateReference(file(buffer(seconds)), constraints, 'pocket')).rejects.toThrow('expected 1–30');
  for (const bits of [8, 16, 24, 32]) expect((await validateReference(file(buffer(1, bits)), constraints, 'pocket')).encoding).toContain(bits + '-bit');
  expect((await validateReference(file(buffer(1, 32, 2, 3)), constraints, 'pocket')).channels).toBe(2);
});
it('rejects empty, header-only, corrupt, renamed, multichannel, out-of-range and non-finite WAVs', async () => {
  const corrupt = buffer(); new DataView(corrupt).setUint32(16, 0xffffffff, true);
  const low = buffer(); new DataView(low).setUint32(24, 7999, true);
  const high = buffer(); new DataView(high).setUint32(24, 192001, true);
  const nonFinite = buffer(1, 32, 1, 3); new DataView(nonFinite).setFloat32(44, Infinity, true);
  const truncated = buffer().slice(0, 44);
  for (const bytes of [new ArrayBuffer(0), truncated, corrupt, new ArrayBuffer(44), buffer(1, 16, 3), low, high, nonFinite]) await expect(validateReference(file(bytes), constraints, 'pocket')).rejects.toBeDefined();
});
it('supports extensible WAV and preserves its exact format chunk during crop', async () => {
  const old = buffer(5, 16, 2), extended = new Uint8Array(old.byteLength + 24), view = new DataView(extended.buffer);
  extended.set(new Uint8Array(old, 0, 36)); extended.set(new Uint8Array(old, 36), 60);
  view.setUint32(4, extended.byteLength - 8, true); view.setUint32(16, 40, true); view.setUint16(20, 0xfffe, true);
  view.setUint16(36, 22, true); view.setUint16(38, 16, true); view.setUint32(40, 3, true);
  extended.set([1, 0, 0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113], 44);
  expect((await validateReference(file(extended.buffer), constraints, 'pocket')).duration).toBe(5);
  const cropped = inspectWave(cropWave(inspectWave(extended.buffer), 1, 3).buffer);
  expect(cropped.duration).toBe(2); expect(cropped.fmtSize).toBe(40);
  expect(new Uint8Array(cropped.buffer, cropped.fmtOffset, 40)).toEqual(new Uint8Array(extended.buffer, 20, 40));
});
it('uses the current configured byte cap below, equal and above the selected file size', async () => {
  const selected = file(buffer());
  for (const cap of [selected.size, selected.size + 1]) await expect(validateReference(selected, { ...constraints, max_upload_bytes: cap }, 'pocket')).resolves.toHaveProperty('duration', 1);
  await expect(validateReference(selected, { ...constraints, max_upload_bytes: selected.size - 1 }, 'pocket')).rejects.toThrow('bytes');
});
