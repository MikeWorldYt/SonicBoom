//! Immutable model identity and verified asset manifest.
use super::super::assets::PinnedFile;

pub const REPOSITORY: &str = "Emerald7664/Qwen3-TTS-ONNX";
pub const REVISION: &str = "0a36d692562f83a862466b0bf8c12154ddf1965d";
pub const REFERENCE_REVISION: &str = "74feb581bc8cefb835fc608107e857036d7580a1";
pub const LLAMA_REVISION: &str = "c1d0e7a004015f23bc0233470b747b596f29b264";
pub const SAMPLE_RATE: u32 = 24_000;
pub const CODEBOOKS: usize = 16;
pub const CODEBOOK_SIZE: i64 = 2048;

/// Selected precision set only. FP32 exports have missing external data;
/// the FP16 speaker export is structurally invalid at this revision.
pub fn manifest() -> Vec<PinnedFile> {
    MANIFEST
        .iter()
        .map(|&(path, sha256, size)| PinnedFile {
            path,
            url: PinnedFile::huggingface(REPOSITORY, REVISION, path),
            sha256,
            size,
        })
        .collect()
}

const MANIFEST: &[(&str, &str, u64)] = &[
    (
        "embeddings/codec_embedding_0.npy",
        "c85fd9a94229fef07cd21830880a609f194efec85d0f3809822b2198275e4e40",
        25165952,
    ),
    (
        "embeddings/codec_embedding_1.npy",
        "6eeefb93b59d80ae3203c96357b5e22d0c9f6a99340623c504c9ef3845d165cc",
        16777344,
    ),
    (
        "embeddings/codec_embedding_10.npy",
        "c8d3ce993a0504b3d4a0d6b4f83d6465ce7f65ed724f7df4902c0c3a91ea47ae",
        16777344,
    ),
    (
        "embeddings/codec_embedding_11.npy",
        "fbcc5014e96f26af5025cfcbf899794b88298b57d7d4b9d54649cae75959fa64",
        16777344,
    ),
    (
        "embeddings/codec_embedding_12.npy",
        "e5c18d6ab0f21327765524735e79a62a70a80f18c10095a69e0b9baaef0ec5b9",
        16777344,
    ),
    (
        "embeddings/codec_embedding_13.npy",
        "72319019affd8ac13917389d6eb634585c10b1d74e9b4ef3c68d94616a780eca",
        16777344,
    ),
    (
        "embeddings/codec_embedding_14.npy",
        "c33a61e607f7b4a2dbfc729fc3302a1fb8fbd5a4c9128ea2e1a57538f93110e2",
        16777344,
    ),
    (
        "embeddings/codec_embedding_15.npy",
        "3fde9e115b374feb2142104b74260a04269994e45d2f3bf5077a83bf16a828d7",
        16777344,
    ),
    (
        "embeddings/codec_embedding_2.npy",
        "f59acdcf7459abef2b8b8817da6716eba70dd296bdedfe9e897aba8e332ae893",
        16777344,
    ),
    (
        "embeddings/codec_embedding_3.npy",
        "4fea12991c0386fe08d6ddeb8e674a68247cba0370c0b761e27b02478a3f8dd2",
        16777344,
    ),
    (
        "embeddings/codec_embedding_4.npy",
        "41627b3302b53258a25e55329eda4297a150eb96ecb21d933a2d0def50a4d87e",
        16777344,
    ),
    (
        "embeddings/codec_embedding_5.npy",
        "857cd908438894fb9130b3527941f3d1cb91055d3a99eaddde81c2c630921504",
        16777344,
    ),
    (
        "embeddings/codec_embedding_6.npy",
        "ab1e8aed58d3773113263cba5040f9bbc055f2e947995eda6dd24a8c50d15177",
        16777344,
    ),
    (
        "embeddings/codec_embedding_7.npy",
        "e4c576b938facec624a57f9bf2500f64805df56a4c06817b3e4bd90a3f0c34db",
        16777344,
    ),
    (
        "embeddings/codec_embedding_8.npy",
        "bb5e9150e032351ae7a9a7f42648846d86c9162c57bfc2bcc7cb4838fbae4bd5",
        16777344,
    ),
    (
        "embeddings/codec_embedding_9.npy",
        "ccdfbae0f5d706c539d17c28ccc1267f3c76cf001e4c593af38ec44d9ac6f464",
        16777344,
    ),
    (
        "embeddings/proj_bias.npy",
        "6c9534469070a22f1a46cedfbe13ded62f2f97a6d8715b7a505ddbd20023421a",
        4224,
    ),
    (
        "embeddings/proj_weight.npy",
        "7fa5850687d66da23e911f5cfd6eac96003c4b95cbbf7db7eb7a4d334bfdee81",
        8388736,
    ),
    (
        "embeddings/text_embedding_projected.npy",
        "3122f93be9a2a5aa65d2611f50c99626a0b94bf299c314b4575ba867cd376ad5",
        622329984,
    ),
    (
        "qwen3_tts_codec_encoder.int8.onnx",
        "b744d0570ad6420f1ddb6c65a15b180c345643f6e90aee6091bcaf3d008b8b9e",
        108829582,
    ),
    (
        "qwen3_tts_decoder.int8.onnx",
        "4e13954107651a7a7c0ecfa8b34cf82c686c386634b2043d1214f0357933be2f",
        221682275,
    ),
    (
        "qwen3_tts_predictor.q8_0.gguf",
        "945c8fc1565484fa1d1228cdadeeabee24899903ac917a971807b0e0fb51acb7",
        151124320,
    ),
    (
        "qwen3_tts_speaker_encoder.int8.onnx",
        "da1e0f5f5c6d4e04b376c1e862f6d4bd2905f94f622fade639654a2c507b2547",
        12411995,
    ),
    (
        "qwen3_tts_talker.q5_k.gguf",
        "569a72350d93e87b6b4d8dc968ccd9c4d54cda818e20878f4c7db1eefb6ecaf5",
        1006245120,
    ),
    (
        "tokenizer.json",
        "09267689b8362020b9763b65dd5be7e086b31e28d72e02837a9e781de9a91bc7",
        11423986,
    ),
];

use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

/// Restricted NPY reader for the pinned C-order little-endian F16/F32 tables.
/// Holds an open file; reads only needed rows instead of materializing the
/// 622 MB text table. Asset integrity must be verified before opening.
struct Table {
    file: File,
    offset: u64,
    rows: usize,
    columns: usize,
    width: usize,
}
impl Table {
    fn open(path: &Path, expected: &[usize]) -> Result<Self> {
        let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut magic = [0u8; 8];
        file.read_exact(&mut magic)?;
        ensure!(&magic[..6] == b"\x93NUMPY", "invalid NPY magic");
        let length = match (magic[6], magic[7]) {
            (1, 0) => {
                let mut bytes = [0; 2];
                file.read_exact(&mut bytes)?;
                u16::from_le_bytes(bytes) as usize
            }
            (2 | 3, 0) => {
                let mut bytes = [0; 4];
                file.read_exact(&mut bytes)?;
                u32::from_le_bytes(bytes) as usize
            }
            _ => anyhow::bail!("unsupported NPY version"),
        };
        ensure!(length <= 65536, "oversized NPY header");
        let mut header = vec![0; length];
        file.read_exact(&mut header)?;
        let header = std::str::from_utf8(&header)?;
        ensure!(
            header.contains("'fortran_order': False"),
            "NPY table must be C-order"
        );
        let width = if header.contains("'descr': '<f2'") {
            2
        } else if header.contains("'descr': '<f4'") {
            4
        } else {
            anyhow::bail!("unsupported NPY dtype")
        };
        let shape = header
            .split("'shape':")
            .nth(1)
            .context("NPY shape missing")?
            .split('(')
            .nth(1)
            .context("NPY shape missing")?
            .split(')')
            .next()
            .context("NPY shape missing")?;
        let dimensions: Vec<usize> = shape
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::parse)
            .collect::<std::result::Result<_, _>>()?;
        ensure!(
            dimensions == expected,
            "incompatible NPY shape: {dimensions:?}, expected {expected:?}"
        );
        let offset = file.stream_position()?;
        let rows = expected[0];
        let columns = *expected.get(1).unwrap_or(&1);
        ensure!(
            file.metadata()?.len() == offset + (rows * columns * width) as u64,
            "NPY file size mismatch"
        );
        Ok(Self {
            file,
            offset,
            rows,
            columns,
            width,
        })
    }
    fn row(&mut self, row: usize) -> Result<Vec<f32>> {
        ensure!(
            row < self.rows,
            "embedding index {row} outside {} rows",
            self.rows
        );
        self.file.seek(SeekFrom::Start(
            self.offset + (row * self.columns * self.width) as u64,
        ))?;
        let mut bytes = vec![0; self.columns * self.width];
        self.file.read_exact(&mut bytes)?;
        let values: Vec<f32> = if self.width == 2 {
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|v| half::f16::from_bits(u16::from_le_bytes([v[0], v[1]])).to_f32())
                .collect()
        } else {
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|v| f32::from_le_bytes([v[0], v[1], v[2], v[3]]))
                .collect()
        };
        ensure!(
            values.iter().all(|v| v.is_finite()),
            "non-finite embedding table"
        );
        Ok(values)
    }
}

pub struct Embeddings {
    text: Table,
    codec: Vec<Table>,
    projection: ndarray::Array2<f32>,
    bias: Vec<f32>,
    pub pad: Vec<f32>,
}
impl Embeddings {
    pub fn load(dir: &Path) -> Result<Self> {
        let dir = dir.join("embeddings");
        let mut text = Table::open(&dir.join("text_embedding_projected.npy"), &[151936, 2048])?;
        let pad = text.row(151671)?;
        let codec = (0..16)
            .map(|q| {
                Table::open(
                    &dir.join(format!("codec_embedding_{q}.npy")),
                    &[if q == 0 { 3072 } else { 2048 }, 2048],
                )
            })
            .collect::<Result<_>>()?;
        let mut projection = Table::open(&dir.join("proj_weight.npy"), &[1024, 2048])?;
        let mut weights = Vec::with_capacity(1024 * 2048);
        for row in 0..1024 {
            weights.extend(projection.row(row)?);
        }
        let mut bias_table = Table::open(&dir.join("proj_bias.npy"), &[1024])?;
        let mut bias = Vec::with_capacity(1024);
        for row in 0..1024 {
            bias.extend(bias_table.row(row)?);
        }
        Ok(Self {
            text,
            codec,
            projection: ndarray::Array2::from_shape_vec((1024, 2048), weights)?,
            bias,
            pad,
        })
    }
    pub fn text(&mut self, id: u32) -> Result<Vec<f32>> {
        self.text.row(id as usize)
    }
    pub fn codec(&mut self, quantizer: usize, code: usize) -> Result<Vec<f32>> {
        self.codec
            .get_mut(quantizer)
            .context("invalid codec table index")?
            .row(code)
    }
    pub fn project(&self, input: &[f32]) -> Result<Vec<f32>> {
        ensure!(
            input.len() == 2048,
            "projection input must be 2048-dimensional"
        );
        let mut projected = self
            .projection
            .dot(&ndarray::ArrayView1::from(input))
            .to_vec();
        for (v, b) in projected.iter_mut().zip(&self.bias) {
            *v += b;
        }
        ensure!(
            projected.iter().all(|v| v.is_finite()),
            "non-finite projected embedding"
        );
        Ok(projected)
    }
    pub fn frame(&mut self, codes: &[i64; CODEBOOKS]) -> Result<Vec<f32>> {
        let mut sum = vec![0.0; 2048];
        for (q, &code) in codes.iter().enumerate() {
            ensure!((0..2048).contains(&code), "invalid acoustic code");
            add(&mut sum, &self.codec(q, code as usize)?)?;
        }
        Ok(sum)
    }
}

pub fn add(left: &mut [f32], right: &[f32]) -> Result<()> {
    ensure!(left.len() == right.len(), "embedding dimension mismatch");
    for (a, b) in left.iter_mut().zip(right) {
        *a += b;
    }
    Ok(())
}
