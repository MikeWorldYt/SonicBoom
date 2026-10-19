import { it, expect, vi, afterEach } from 'vitest';
import { mount, flushPromises } from '@vue/test-utils';
import { File as NodeFile } from 'node:buffer';
import { inspectWave, cropWave, analyzeWave, sampleAt } from '../../src/features/audio/wav';
import ReferenceEditor from '../../src/components/audio/ReferenceEditor.vue';
function source(format = 1, bits = 16, channels = 2) {
  const frames = 8000, block = bits / 8 * channels, buffer = new ArrayBuffer(44 + frames * block), view = new DataView(buffer);
  const text = (offset: number, word: string) => [...word].forEach((char, index) => view.setUint8(offset + index, char.charCodeAt(0)));
  text(0, 'RIFF'); view.setUint32(4, buffer.byteLength - 8, true); text(8, 'WAVEfmt '); view.setUint32(16, 16, true);
  view.setUint16(20, format, true); view.setUint16(22, channels, true); view.setUint32(24, 8000, true); view.setUint32(28, 8000 * block, true);
  view.setUint16(32, block, true); view.setUint16(34, bits, true); text(36, 'data'); view.setUint32(40, frames * block, true);
  for (let index = 0; index < frames * channels; index++) {
    if (format === 3) view.setFloat32(44 + index * 4, index % 2 ? -.5 : .5, true);
    else if (bits === 16) view.setInt16(44 + index * 2, index % 2 ? -16384 : 16384, true);
  }
  return buffer;
}
afterEach(() => vi.unstubAllGlobals());
it('crops exact original sample frames and retains rate, channels and encoding', () => {
  for (const [format, bits] of [[1, 16], [3, 32]]) {
    const original = source(format, bits), wave = inspectWave(original);
    const derived = cropWave(wave, .25, .75), cropped = inspectWave(derived.buffer);
    expect(cropped.duration).toBe(.5); expect(cropped.channels).toBe(2); expect(cropped.sampleRate).toBe(8000); expect(cropped.format).toBe(format);
    expect(new Uint8Array(cropped.buffer, cropped.dataOffset, cropped.dataSize)).toEqual(new Uint8Array(original, wave.dataOffset + 2000 * wave.block, 4000 * wave.block));
    expect(inspectWave(original).duration).toBe(1);
  }
});
it('measures source channels and makes no sample modifications', async () => {
  const wave = inspectWave(source());
  const before = wave.buffer.slice(0);
  const health = await analyzeWave(wave);
  expect(health.peak).toBe(.5); expect(health.rms).toBe(.5); expect(health.clipping).toBe(0); expect(health.peaks).toHaveLength(512);
  expect(wave.buffer).toEqual(before);
  expect(sampleAt(wave, 0)).toBe(.5); expect(sampleAt(wave, 1)).toBe(-.5);
});
it('rejects non-finite samples and supports analysis cancellation', async () => {
  const wave = inspectWave(source(3, 32));
  wave.view.setFloat32(wave.dataOffset, NaN, true);
  await expect(analyzeWave(wave)).rejects.toThrow('invalid samples');
  const control = new AbortController(); control.abort();
  await expect(analyzeWave(inspectWave(source()), control.signal)).rejects.toBeDefined();
  expect(() => cropWave(wave, .8, .2)).toThrow();
});
it('editor keeps original untouched, emits only after explicit apply and releases preview URLs', async () => {
  vi.stubGlobal('File', NodeFile);
  const create = vi.fn(() => 'blob:trim'), revoke = vi.fn();
  vi.stubGlobal('URL', Object.assign(URL, { createObjectURL: create, revokeObjectURL: revoke }));
  const file = new NodeFile([source()], 'original.wav', { type: 'audio/wav' });
  const wrapper = mount(ReferenceEditor, { props: { file: file as unknown as File, maxBytes: 1000000 } });
  await flushPromises();
  // Analysis yields to permit cancellation; wait for its two sample batches.
  await vi.waitFor(() => expect(wrapper.find('.audio-waveform').exists()).toBe(true));
  expect(wrapper.emitted('apply')).toBeUndefined();
  const inputs = wrapper.findAll('input[type="number"]');
  await inputs[0].setValue(.25); await inputs[1].setValue(.75);
  await wrapper.findAll('button').find(button => button.text() === 'Preview selection')!.trigger('click');
  // The preview keeps the cropped audio in memory: no object URL is created.
  expect(create).not.toHaveBeenCalled();
  expect(wrapper.find('.waveform-player').exists()).toBe(true);
  await wrapper.findAll('button').find(button => button.text() === 'Use trimmed recording')!.trigger('click');
  const result = wrapper.emitted('apply')![0][0] as File;
  expect(inspectWave(await result.arrayBuffer()).duration).toBe(.5);
  expect(inspectWave(await file.arrayBuffer()).duration).toBe(1);
});

it('the exported selection releases its temporary URL', async () => {
  vi.stubGlobal('File', NodeFile);
  const create = vi.fn(() => 'blob:trim'), revoke = vi.fn();
  vi.stubGlobal('URL', Object.assign(URL, { createObjectURL: create, revokeObjectURL: revoke }));
  // The download click would navigate the test document.
  const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {});
  const file = new NodeFile([source()], 'original.wav', { type: 'audio/wav' });
  const wrapper = mount(ReferenceEditor, { props: { file: file as unknown as File, maxBytes: 1000000 } });
  await flushPromises();
  await vi.waitFor(() => expect(wrapper.find('.audio-waveform').exists()).toBe(true));
  const inputs = wrapper.findAll('input[type="number"]');
  await inputs[0].setValue(.25); await inputs[1].setValue(.75);
  await wrapper.findAll('button').find(button => button.text() === 'Export selected WAV')!.trigger('click');
  expect(create).toHaveBeenCalledTimes(1);
  await vi.waitFor(() => expect(revoke).toHaveBeenCalledWith('blob:trim'));
  click.mockRestore();
});
