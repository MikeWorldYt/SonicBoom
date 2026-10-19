import { uiError } from '../../i18n';
export interface WaveFile {
  buffer: ArrayBuffer; view: DataView<ArrayBuffer>; channels: number; sampleRate: number; bits: number; validBits: number; format: number;
  block: number; dataOffset: number; dataSize: number; fmtOffset: number; fmtSize: number; duration: number;
}
export function inspectWave(buffer: ArrayBuffer): WaveFile {
  const view = new DataView(buffer);
  const tag = (offset: number) => String.fromCharCode(...new Uint8Array(buffer, offset, 4));
  if (buffer.byteLength < 44 || tag(0) !== 'RIFF' || tag(8) !== 'WAVE') throw uiError('The selected file is not a valid WAV recording.');
  const end = view.getUint32(4, true) + 8;
  if (end > buffer.byteLength || end < 44) throw uiError('The WAV recording is truncated.');
  let channels = 0, sampleRate = 0, block = 0, bits = 0, format = 0, dataOffset = 0, dataSize = 0, fmtOffset = 0, fmtSize = 0, validBits = 0;
  for (let offset = 12; offset < end;) {
    if (offset + 8 > end) throw uiError('The WAV recording has corrupt chunks.');
    const size = view.getUint32(offset + 4, true), start = offset + 8;
    if (start + size + (size % 2) > end) throw uiError('The WAV recording has corrupt chunks.');
    if (tag(offset) === 'fmt ') {
      if (fmtOffset || size < 16) throw uiError('The WAV format header is incomplete.');
      fmtOffset = start; fmtSize = size;
      format = view.getUint16(start, true); channels = view.getUint16(start + 2, true);
      sampleRate = view.getUint32(start + 4, true); block = view.getUint16(start + 12, true); bits = view.getUint16(start + 14, true); validBits = bits;
      if (format === 0xfffe) {
        const tail = [0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113];
        if (size < 40 || view.getUint16(start + 16, true) < 22 || !tail.every((value, index) => view.getUint8(start + 28 + index) === value)) throw uiError('The WAV format header is incomplete.');
        format = view.getUint32(start + 24, true);
        validBits = view.getUint16(start + 18, true) || bits;
        if (validBits > bits) throw uiError('The WAV audio data is incomplete.');
      }
    } else if (tag(offset) === 'data') {
      if (dataOffset) throw uiError('The WAV recording has corrupt chunks.');
      dataOffset = start; dataSize = size;
    }
    offset = start + size + (size % 2);
  }
  if (![1, 3].includes(format) || ![8, 16, 24, 32].includes(bits) || (format === 3 && (bits !== 32 || validBits !== 32)) || (format === 1 && validBits !== bits && !(bits === 32 && validBits === 24))) throw uiError('Use an uncompressed PCM or float32 WAV recording.');
  if (!channels || !sampleRate || !dataSize || block !== channels * bits / 8 || dataSize % block) throw uiError('The WAV audio data is incomplete.');
  return { buffer, view, channels, sampleRate, block, bits, validBits, format, dataOffset, dataSize, fmtOffset, fmtSize, duration: dataSize / block / sampleRate };
}
export function sampleAt(wave: WaveFile, index: number): number {
  const view = wave.view, offset = wave.dataOffset + index * wave.bits / 8;
  if (wave.format === 3) return view.getFloat32(offset, true);
  if (wave.bits === 8) return (view.getUint8(offset) - 128) / 128;
  if (wave.bits === 16) return view.getInt16(offset, true) / 32768;
  if (wave.validBits === 24) {
    let sample = view.getUint8(offset) | view.getUint8(offset + 1) << 8 | view.getUint8(offset + 2) << 16;
    if (sample & 0x800000) sample |= ~0xffffff;
    return sample / 8388608;
  }
  return view.getInt32(offset, true) / 2147483648;
}
export interface WaveAnalysis { peaks: number[]; peak: number; rms: number; clipping: number; silence: number }
export async function analyzeWave(wave: WaveFile, signal?: AbortSignal): Promise<WaveAnalysis> {
  const peaks = new Array<number>(512).fill(0), count = wave.dataSize / (wave.bits / 8);
  let peak = 0, energy = 0, clipped = 0, quiet = 0;
  for (let index = 0; index < count; index++) {
    const sample = sampleAt(wave, index);
    if (!Number.isFinite(sample)) throw uiError('The recording contains invalid samples.');
    const amplitude = Math.abs(sample);
    peak = Math.max(peak, amplitude); energy += sample * sample;
    if (amplitude >= .999) clipped++;
    if (amplitude < .0001) quiet++;
    const bucket = Math.min(511, Math.floor(index * 512 / count));
    peaks[bucket] = Math.max(peaks[bucket], amplitude);
    if (index % 65536 === 0) { signal?.throwIfAborted(); await new Promise<void>(resolve => setTimeout(resolve, 0)); }
  }
  signal?.throwIfAborted();
  return { peaks, peak, rms: Math.sqrt(energy / count), clipping: clipped / count, silence: quiet / count };
}
/** Copy whole original sample frames. No decoding, resampling or quantization. */
export function cropWave(wave: WaveFile, start: number, end: number): Uint8Array<ArrayBuffer> {
  if (!Number.isFinite(start) || !Number.isFinite(end) || start < 0 || end > wave.duration || end <= start) throw uiError('Choose a valid start and end time.');
  const first = Math.floor(start * wave.sampleRate), last = Math.min(wave.dataSize / wave.block, Math.ceil(end * wave.sampleRate));
  if (last <= first) throw uiError('Choose a valid start and end time.');
  const size = (last - first) * wave.block, fmtPad = wave.fmtSize % 2;
  const result = new Uint8Array(12 + 8 + wave.fmtSize + fmtPad + 8 + size + size % 2);
  const view = new DataView(result.buffer), text = (offset: number, value: string) => [...value].forEach((c, index) => view.setUint8(offset + index, c.charCodeAt(0)));
  text(0, 'RIFF'); view.setUint32(4, result.length - 8, true); text(8, 'WAVE'); text(12, 'fmt ');
  view.setUint32(16, wave.fmtSize, true); result.set(new Uint8Array(wave.buffer, wave.fmtOffset, wave.fmtSize), 20);
  const dataHeader = 20 + wave.fmtSize + fmtPad; text(dataHeader, 'data'); view.setUint32(dataHeader + 4, size, true);
  result.set(new Uint8Array(wave.buffer, wave.dataOffset + first * wave.block, size), dataHeader + 8);
  return result;
}
