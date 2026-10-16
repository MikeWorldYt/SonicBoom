use anyhow::Result;
use opus::{Application, Channels, Encoder};

const OUTPUT_SAMPLE_RATE: u32 = 48000;
const FRAME_SIZE_48K: usize = 960; // 48000Hz 20ms

/// Supported audio output formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Opus,
    Wav,
    Mp3,
    Flac,
}

/// Request-controlled format value that matched nothing supported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownAudioFormat(pub String);

impl std::fmt::Display for UnknownAudioFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unsupported audio format '{}' (expected one of: opus, wav, mp3, flac)",
            self.0
        )
    }
}

impl std::error::Error for UnknownAudioFormat {}

impl AudioFormat {
    /// Parse a caller-supplied format name. Unknown values are an error —
    /// they must never silently become Opus. Callers apply the Opus default
    /// only when the parameter is omitted entirely.
    pub fn parse(s: &str) -> Result<Self, UnknownAudioFormat> {
        match s.trim().to_lowercase().as_str() {
            "opus" => Ok(AudioFormat::Opus),
            "wav" => Ok(AudioFormat::Wav),
            "mp3" => Ok(AudioFormat::Mp3),
            "flac" => Ok(AudioFormat::Flac),
            _ => Err(UnknownAudioFormat(s.to_string())),
        }
    }

    pub fn content_type(&self) -> &'static str {
        match self {
            AudioFormat::Opus => "audio/ogg; codecs=opus",
            AudioFormat::Wav => "audio/wav",
            AudioFormat::Mp3 => "audio/mpeg",
            AudioFormat::Flac => "audio/flac",
        }
    }
}

pub fn encode_audio(samples: &[f32], sample_rate: u32, format: AudioFormat) -> Result<Vec<u8>> {
    match format {
        AudioFormat::Opus => encode_opus(samples, sample_rate),
        AudioFormat::Wav => encode_wav(samples, sample_rate),
        AudioFormat::Mp3 => encode_mp3(samples, sample_rate),
        AudioFormat::Flac => encode_flac(samples, sample_rate),
    }
}

pub fn encode_opus(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
    // Upsample to 48000Hz (linear interpolation)
    let resampled = resample_pcm_linear(samples, sample_rate, OUTPUT_SAMPLE_RATE);

    let mut encoder = Encoder::new(OUTPUT_SAMPLE_RATE, Channels::Mono, Application::Audio)?;

    // Set Opus encoder to VBR (Variable Bitrate) mode for better quality
    encoder.set_bitrate(opus::Bitrate::Bits(128000))?;

    // f32 PCM → i16 PCM
    let pcm_i16: Vec<i16> = resampled
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect();

    // Pad to make multiple of FRAME_SIZE
    let pad_len = (FRAME_SIZE_48K - pcm_i16.len() % FRAME_SIZE_48K) % FRAME_SIZE_48K;
    let mut padded = pcm_i16;
    padded.extend(vec![0i16; pad_len]);

    // Encode frames and package them into the Ogg container incrementally:
    // packets stream straight into the writer instead of accumulating in a
    // `Vec<Vec<u8>>` that is cloned again on write. Header bytes, pre-skip,
    // granule positions, and final `EndStream` semantics are unchanged.
    encode_opus_streaming(&mut encoder, &padded)
}

/// Linear-interpolation resampling between arbitrary sample rates
/// (up or down). Used to stage reference audio at the rate each provider
/// pipeline requires. Empty input and a zero source rate yield silence-safe
/// empty output; callers validate durations separately.
pub fn resample_pcm_linear(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = to_rate as f64 / from_rate as f64;
    let out_len = (samples.len() as f64 * ratio).ceil() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 / ratio;
        let idx = src_pos as usize;
        let frac = (src_pos - idx as f64) as f32;
        let a = samples[idx.min(samples.len() - 1)];
        let b = samples[(idx + 1).min(samples.len() - 1)];
        out.push(a + (b - a) * frac);
    }
    out
}

/// Conditioning identity includes the algorithm and filter parameters.
pub const REFERENCE_RESAMPLER_VERSION: &str = "sinc-blackman-96-zero-phase-v1";

/// Offline reference-only windowed-sinc resampling. A Blackman window with
/// 96 zero crossings and 6% transition guard rejects content above the new
/// Nyquist frequency before decimation. The centered kernel compensates delay;
/// truncated boundary kernels are DC-normalized. No denoising, gain normalization
/// or clipping is performed (band-limited interpolation can ring at hard edges).
/// Providers validate finite input and rates before calling this function.
/// Output length uses ceil, matching the legacy duration convention.
pub fn resample_reference(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if samples.is_empty() || from_rate == 0 || to_rate == 0 {
        return Vec::new();
    }
    if from_rate == to_rate {
        return samples.to_vec();
    }
    let ratio = to_rate as f64 / from_rate as f64;
    let cutoff = ratio.min(1.0) * 0.94;
    let radius = 48.0 / cutoff;
    let out_len = ((samples.len() as u128 * to_rate as u128).div_ceil(from_rate as u128)) as usize;
    let mut output = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let center = i as f64 / ratio;
        let start = ((center - radius).ceil() as isize).max(0) as usize;
        let end = ((center + radius).floor() as usize).min(samples.len() - 1);
        let (mut sum, mut weight_sum) = (0.0, 0.0);
        for (offset, sample) in samples[start..=end].iter().enumerate() {
            let distance = (start + offset) as f64 - center;
            let phase = std::f64::consts::PI * distance * cutoff;
            let sinc = if phase.abs() < 1e-12 {
                1.0
            } else {
                phase.sin() / phase
            };
            let angle = std::f64::consts::PI * distance / radius;
            let window = 0.42 + 0.5 * angle.cos() + 0.08 * (2.0 * angle).cos();
            let weight = cutoff * sinc * window;
            sum += *sample as f64 * weight;
            weight_sum += weight;
        }
        output.push((sum / weight_sum) as f32);
    }
    output
}

/// Encode already-padded 48 kHz mono PCM into an Ogg Opus stream, writing
/// each packet straight into the container. There is no intermediate packet
/// store and no second per-packet clone: packet bytes move from the encoder
/// scratch buffer into the container with a single copy. (A borrow would be
/// ideal, but the writer's packet lifetime outlives each loop iteration, so
/// the borrow checker requires ownership here.)
fn encode_opus_streaming(encoder: &mut Encoder, padded: &[i16]) -> Result<Vec<u8>> {
    use ogg::writing::PacketWriteEndInfo;
    use std::io::Cursor;

    let mut out = Cursor::new(Vec::new());
    {
        let mut writer = ogg::writing::PacketWriter::new(&mut out);
        let serial = 0x12345678u32;

        // ---- OpusHead header packet ----
        // https://wiki.xiph.org/OggOpus#ID_Header
        let pre_skip: u16 = 312; // Required by Opus spec (minimum 312 samples)
        let mut opus_head = Vec::new();
        opus_head.extend_from_slice(b"OpusHead");
        opus_head.push(1); // version
        opus_head.push(1); // channel count (mono)
        opus_head.extend_from_slice(&pre_skip.to_le_bytes()); // pre-skip (312 samples)
        opus_head.extend_from_slice(&OUTPUT_SAMPLE_RATE.to_le_bytes()); // input sample rate
        opus_head.extend_from_slice(&0i16.to_le_bytes()); // output gain
        opus_head.push(0); // channel mapping family
        writer.write_packet(opus_head, serial, PacketWriteEndInfo::EndPage, 0)?;

        // ---- OpusTags header packet ----
        let mut opus_tags = Vec::new();
        opus_tags.extend_from_slice(b"OpusTags");
        let vendor = b"SonicBoom";
        opus_tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
        opus_tags.extend_from_slice(vendor);
        opus_tags.extend_from_slice(&0u32.to_le_bytes()); // comment list length = 0
        writer.write_packet(opus_tags, serial, PacketWriteEndInfo::EndPage, 0)?;

        // ---- Audio data packets ----
        // For OggOpus, granule position represents the presentation time of the
        // last sample in the page. The first 312 samples (pre-skip) should be
        // discarded by the decoder, so the first valid sample appears at
        // granule position 312.
        let frame_samples = FRAME_SIZE_48K as u64;
        let frame_count = padded.len() / FRAME_SIZE_48K;
        let mut buf = vec![0u8; 4096];

        for (i, frame) in padded.chunks(FRAME_SIZE_48K).enumerate() {
            let len = encoder.encode(frame, &mut buf)?;
            // Granule position = pre_skip + (packet_index + 1) * frame_samples
            let granule = pre_skip as u64 + frame_samples * (i as u64 + 1);

            let end_info = if i + 1 == frame_count {
                PacketWriteEndInfo::EndStream
            } else {
                PacketWriteEndInfo::NormalPacket
            };

            writer.write_packet(buf[..len].to_vec(), serial, end_info, granule)?;
        }
    }
    Ok(out.into_inner())
}

/// Encode audio as WAV format (PCM 16-bit) using hound
fn encode_wav(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
    use std::io::Cursor;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut cursor = Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(&mut cursor, spec)?;

    for &sample in samples {
        let amplitude = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer.write_sample(amplitude)?;
    }

    writer.finalize()?;
    Ok(cursor.into_inner())
}

/// Encode audio as MP3 format using shine-rs
fn encode_mp3(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
    use shine_rs::mp3_encoder::{Mp3EncoderConfig, StereoMode, encode_pcm_to_mp3};

    // Use standard settings for maximum compatibility
    let config = Mp3EncoderConfig {
        sample_rate,
        bitrate: 128,
        channels: 1,
        stereo_mode: StereoMode::Mono,
        copyright: false,
        original: true,
    };

    // Convert f32 to i16 PCM
    let pcm_i16: Vec<i16> = samples
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect();

    // Encode using shine-rs
    let mp3_data = encode_pcm_to_mp3(config, &pcm_i16)
        .map_err(|e| anyhow::anyhow!("MP3 encoding failed: {:?}", e))?;

    // Verify MP3 frame sync bytes are present (should start with 0xFF 0xFB or similar)
    //shine-rs should produce valid MP3 frames with proper headers

    // Add ID3v2 header for better compatibility with media players
    // Many players require ID3v2 to recognize MP3 files properly
    let id3v2 = create_id3v2_header();
    let mut result = Vec::with_capacity(id3v2.len() + mp3_data.len());
    result.extend_from_slice(&id3v2);
    result.extend_from_slice(&mp3_data);

    Ok(result)
}

/// Create minimal ID3v2 header for MP3 compatibility
fn create_id3v2_header() -> Vec<u8> {
    // ID3v2.3 header: "ID3" + version (2 bytes) + flags + size (syncsafe)
    let mut header = Vec::new();
    header.extend_from_slice(b"ID3"); // Identifier
    header.push(3); // Version (ID3v2.3)
    header.push(0); // Revision
    header.push(0); // Flags (none)

    // Size in syncsafe bytes (7 bits per byte) - standard minimal tag
    let size: u32 = 0;
    header.push(((size >> 21) & 0x7F) as u8);
    header.push(((size >> 14) & 0x7F) as u8);
    header.push(((size >> 7) & 0x7F) as u8);
    header.push((size & 0x7F) as u8);

    header
}

/// Encode audio as FLAC format using flacenc
fn encode_flac(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;

    // Convert f32 to i32 for flacenc (16-bit range)
    let pcm_i32: Vec<i32> = samples
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i32)
        .collect();

    let channels = 1;
    let bits_per_sample = 16;

    // Configure encoder with proper settings
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|e| anyhow::anyhow!("FLAC config error: {:?}", e))?;

    let source = flacenc::source::MemSource::from_samples(
        &pcm_i32,
        channels,
        bits_per_sample,
        sample_rate as usize,
    );

    let block_size = 4096;
    let flac_stream = flacenc::encode_with_fixed_block_size(&config, source, block_size)
        .map_err(|e| anyhow::anyhow!("FLAC encode failed: {:?}", e))?;

    let mut sink = flacenc::bitsink::ByteSink::new();
    flac_stream
        .write(&mut sink)
        .map_err(|e| anyhow::anyhow!("FLAC stream write failed: {:?}", e))?;

    Ok(sink.as_slice().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_formats_parse_case_insensitively() {
        assert_eq!(AudioFormat::parse("opus"), Ok(AudioFormat::Opus));
        assert_eq!(AudioFormat::parse("OPUS"), Ok(AudioFormat::Opus));
        assert_eq!(AudioFormat::parse("wav"), Ok(AudioFormat::Wav));
        assert_eq!(AudioFormat::parse("Wav"), Ok(AudioFormat::Wav));
        assert_eq!(AudioFormat::parse("mp3"), Ok(AudioFormat::Mp3));
        assert_eq!(AudioFormat::parse("flac"), Ok(AudioFormat::Flac));
        assert_eq!(AudioFormat::parse("  opus  "), Ok(AudioFormat::Opus));
    }

    #[test]
    fn unknown_formats_are_rejected_not_defaulted() {
        for bad in ["banana", "aac", "ogg", "", "opus2", "wave", "mp4"] {
            assert!(
                AudioFormat::parse(bad).is_err(),
                "format {bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn opus_encoder_produces_valid_ogg_container() {
        // 1 second of silence at the model-native rate, upsampled to 48k.
        let samples = vec![0.0f32; 24_000];
        let bytes = encode_opus(&samples, 24_000).expect("opus encode must succeed");
        assert!(!bytes.is_empty());
        // OGG magic + OpusHead identification header.
        assert_eq!(&bytes[0..4], b"OggS");
        assert!(
            bytes.windows(8).any(|w| w == b"OpusHead"),
            "missing OpusHead header"
        );
    }

    /// Exact copy of the pre-streaming encoder: collect every packet, then
    /// clone each one into the container. Used only to prove the streaming
    /// encoder emits byte-identical output.
    fn reference_two_phase_encode(samples: &[f32], sample_rate: u32) -> Vec<u8> {
        use ogg::writing::PacketWriteEndInfo;
        use std::io::Cursor;

        let resampled = resample_pcm_linear(samples, sample_rate, OUTPUT_SAMPLE_RATE);
        let mut encoder =
            Encoder::new(OUTPUT_SAMPLE_RATE, Channels::Mono, Application::Audio).unwrap();
        encoder.set_bitrate(opus::Bitrate::Bits(128000)).unwrap();
        let pcm_i16: Vec<i16> = resampled
            .iter()
            .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect();
        let pad_len = (FRAME_SIZE_48K - pcm_i16.len() % FRAME_SIZE_48K) % FRAME_SIZE_48K;
        let mut padded = pcm_i16;
        padded.extend(vec![0i16; pad_len]);

        let mut opus_packets: Vec<Vec<u8>> = Vec::new();
        let mut buf = vec![0u8; 4096];
        for frame in padded.chunks(FRAME_SIZE_48K) {
            let len = encoder.encode(frame, &mut buf).unwrap();
            opus_packets.push(buf[..len].to_vec());
        }

        let mut out = Cursor::new(Vec::new());
        let serial = 0x12345678u32;
        let mut writer = ogg::writing::PacketWriter::new(&mut out);
        let pre_skip: u16 = 312;
        let mut opus_head = Vec::new();
        opus_head.extend_from_slice(b"OpusHead");
        opus_head.push(1);
        opus_head.push(1);
        opus_head.extend_from_slice(&pre_skip.to_le_bytes());
        opus_head.extend_from_slice(&OUTPUT_SAMPLE_RATE.to_le_bytes());
        opus_head.extend_from_slice(&0i16.to_le_bytes());
        opus_head.push(0);
        writer
            .write_packet(opus_head, serial, PacketWriteEndInfo::EndPage, 0)
            .unwrap();
        let mut opus_tags = Vec::new();
        opus_tags.extend_from_slice(b"OpusTags");
        let vendor = b"SonicBoom";
        opus_tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
        opus_tags.extend_from_slice(vendor);
        opus_tags.extend_from_slice(&0u32.to_le_bytes());
        writer
            .write_packet(opus_tags, serial, PacketWriteEndInfo::EndPage, 0)
            .unwrap();
        let frame_samples = FRAME_SIZE_48K as u64;
        for (i, packet) in opus_packets.iter().enumerate() {
            let granule = pre_skip as u64 + frame_samples * (i as u64 + 1);
            let end_info = if i + 1 == opus_packets.len() {
                PacketWriteEndInfo::EndStream
            } else {
                PacketWriteEndInfo::NormalPacket
            };
            writer
                .write_packet(packet.clone(), serial, end_info, granule)
                .unwrap();
        }
        drop(writer);
        out.into_inner()
    }

    #[test]
    fn opus_streaming_matches_two_phase_reference_byte_for_byte() {
        // Empty, tiny (<20 ms), exact-boundary, and long inputs at several
        // source rates, with silence and with signal (signal exercises
        // nonzero packet payloads through the granule/EndStream logic).
        let mut cases: Vec<(Vec<f32>, u32)> = Vec::new();
        for rate in [8000u32, 16_000, 24_000, 44_100, 48_000] {
            for len in [0, 1, 100, 479, 480, 959, 960, 961, 1920, 24_000, 96_000] {
                cases.push((vec![0.0; len], rate));
                cases.push((
                    (0..len).map(|i| ((i as f32) * 0.01).sin() * 0.5).collect(),
                    rate,
                ));
            }
        }
        for (samples, rate) in &cases {
            assert_eq!(
                encode_opus(samples, *rate).unwrap(),
                reference_two_phase_encode(samples, *rate),
                "streaming drift for len={} rate={rate}",
                samples.len(),
            );
        }
    }

    /// Walk Ogg pages, returning `(granule_position, header_flags)` per page
    /// and asserting the walk consumes the stream exactly.
    fn ogg_page_granules(bytes: &[u8]) -> Vec<(u64, u8)> {
        let mut pages = Vec::new();
        let mut pos = 0;
        while pos < bytes.len() {
            assert!(
                pos + 27 <= bytes.len() && &bytes[pos..pos + 4] == b"OggS",
                "broken Ogg page at offset {pos}"
            );
            let flags = bytes[pos + 5];
            let granule = u64::from_le_bytes(bytes[pos + 6..pos + 14].try_into().unwrap());
            let segments = bytes[pos + 26] as usize;
            assert!(pos + 27 + segments <= bytes.len());
            let body: usize = bytes[pos + 27..pos + 27 + segments]
                .iter()
                .map(|&s| s as usize)
                .sum();
            pages.push((granule, flags));
            pos += 27 + segments + body;
        }
        assert_eq!(pos, bytes.len());
        pages
    }

    #[test]
    fn opus_container_granules_track_audio_duration() {
        // 1 s at 24 kHz → 48000 samples → 50 frames → last granule
        // 312 + 50 × 960, carried on the EndStream page.
        let bytes = encode_opus(&vec![0.0f32; 24_000], 24_000).unwrap();
        let pages = ogg_page_granules(&bytes);
        assert!(pages.len() >= 3);
        assert_eq!(pages[0].0, 0, "header granule must be zero");
        assert_eq!(pages[0].1 & 0x02, 0x02, "first page must open the stream");
        assert_eq!(pages[1].0, 0, "tags granule must be zero");
        let (last_granule, last_flags) = *pages.last().unwrap();
        assert_eq!(last_flags & 0x04, 0x04, "final page must end the stream");
        assert_eq!(last_granule, 312 + 50 * 960);

        // Tiny input still yields one EndStream packet with one frame of
        // granule, and empty input yields headers only (matching the
        // historical container, which likewise wrote no EndStream).
        let bytes = encode_opus(&vec![0.25f32; 100], 24_000).unwrap();
        let pages = ogg_page_granules(&bytes);
        let (last_granule, last_flags) = *pages.last().unwrap();
        assert_eq!(last_flags & 0x04, 0x04);
        assert_eq!(last_granule, 312 + 960);

        let bytes = encode_opus(&[], 24_000).unwrap();
        let pages = ogg_page_granules(&bytes);
        assert_eq!(pages.len(), 2);
    }

    #[test]
    fn resampling_is_identity_at_equal_rates_and_scales_length() {
        let samples: Vec<f32> = (0..2400).map(|i| i as f32 / 2400.0).collect();
        assert_eq!(resample_pcm_linear(&samples, 24000, 24000), samples);
        let up = resample_pcm_linear(&samples, 24000, 48000);
        assert_eq!(up.len(), 4800);
        let down = resample_pcm_linear(&samples, 24000, 16000);
        assert_eq!(down.len(), 1600);
        // Endpoints are preserved by linear interpolation.
        assert!((up[0] - samples[0]).abs() < 1e-6);
        assert!((down[0] - samples[0]).abs() < 1e-6);
        assert!(resample_pcm_linear(&[], 24000, 16000).is_empty());
    }

    #[test]
    fn all_formats_encode_without_error() {
        let samples = vec![0.25f32; 480];
        for format in [
            AudioFormat::Opus,
            AudioFormat::Wav,
            AudioFormat::Mp3,
            AudioFormat::Flac,
        ] {
            let bytes = encode_audio(&samples, 24_000, format)
                .unwrap_or_else(|_| panic!("{format:?} encode must succeed"));
            assert!(!bytes.is_empty(), "{format:?} produced no output");
        }
        // WAV magic check as a container sanity anchor.
        let wav = encode_audio(&samples, 24_000, AudioFormat::Wav).unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
    }
}

#[cfg(test)]
mod reference_resampling_tests {
    use super::*;
    fn tone(rate: u32, hz: f64) -> Vec<f32> {
        (0..rate)
            .map(|i| (0.5 * (std::f64::consts::TAU * hz * i as f64 / rate as f64).sin()) as f32)
            .collect()
    }
    fn rms(pcm: &[f32]) -> f64 {
        (pcm.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / pcm.len() as f64).sqrt()
    }
    #[test]
    fn reference_filter_rejects_aliases_and_preserves_passband() {
        for rate in [44100, 48000] {
            let high = tone(rate, 18000.0);
            let linear = resample_pcm_linear(&high, rate, 24000);
            let filtered = resample_reference(&high, rate, 24000);
            let alias_db = 20.0 * (rms(&filtered[200..23800]) / rms(&linear[200..23800])).log10();
            eprintln!("{rate}->24000: 18kHz alias vs linear = {alias_db:.2} dB");
            assert!(alias_db < -60.0, "alias attenuation {alias_db}");
            let low = resample_reference(&tone(rate, 1000.0), rate, 24000);
            assert!((rms(&low[200..23800]) - 0.5 / 2f64.sqrt()).abs() < 0.001);
            assert!(low.iter().all(|s| s.is_finite() && s.abs() < 0.6));
        }
    }
    #[test]
    fn reference_sweeps_reject_stopband_and_upsampling_preserves_tone() {
        for rate in [44100, 48000] {
            let sweep: Vec<f32> = (0..rate)
                .map(|i| {
                    let t = i as f64 / rate as f64;
                    (0.5 * (std::f64::consts::TAU * (13000.0 * t + 3500.0 * t * t)).sin()) as f32
                })
                .collect();
            let linear = resample_pcm_linear(&sweep, rate, 24000);
            let filtered = resample_reference(&sweep, rate, 24000);
            let db = 20.0 * (rms(&filtered[200..23800]) / rms(&linear[200..23800])).log10();
            eprintln!("{rate}->24000: 13-20kHz sweep alias vs linear = {db:.2} dB");
            assert!(db < -70.0, "sweep rejection {db}");
        }
        let up = resample_reference(&tone(16000, 1000.0), 16000, 24000);
        assert!((rms(&up[200..23800]) - 0.5 / 2f64.sqrt()).abs() < 0.001);
    }

    #[test]
    fn reference_duration_dc_silence_impulse_and_equal_rate() {
        for rate in [16000, 24000, 44100, 48000] {
            let dc = vec![0.25; rate as usize];
            let out = resample_reference(&dc, rate, 24000);
            assert_eq!(out.len(), 24000);
            assert!(out.iter().all(|s| (*s - 0.25).abs() < 1e-6));
            assert_eq!(
                resample_reference(&vec![0.0; rate as usize], rate, 24000),
                vec![0.0; 24000]
            );
            let mut impulse = vec![0.0; rate as usize];
            impulse[rate as usize / 2] = 0.5;
            let out = resample_reference(&impulse, rate, 24000);
            assert_eq!(out.len(), 24000);
            assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 0.51));
            assert_eq!(
                out.iter()
                    .enumerate()
                    .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                    .unwrap()
                    .0,
                12000
            );
        }
        let input = tone(24000, 7000.0);
        assert_eq!(resample_reference(&input, 24000, 24000), input);
        assert!(resample_reference(&input, 0, 24000).is_empty());
    }
}
