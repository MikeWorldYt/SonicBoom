# Qwen3-TTS GGUF / ONNX native integration audit

Status: **Phase 1 complete; CPU backend validated and registered behind the
opt-in `qwen-gguf` feature.** Audit date: 2026-10-15; backend validation
2026-10-15/16. Native CPU generation produces EOS-terminated finite PCM
(41 frames / 3.28 s, seed-fixed determinism, conditioning reuse verified);
the `qwen3-gguf` provider owns `qwen3-tts-1.7b-base` in the registry with
background download/verify/load lifecycle. GPU inference, Vulkan packaging
and x-vector cloning acceptance are not claimed. Existing providers are
unchanged.

## Immutable sources and licensing

- SonicBoom `origin/main`: `5e4cddb` (fetched before changes).
- [Reference implementation](https://github.com/HaujetZhao/Qwen3-TTS-GGUF/tree/74feb581bc8cefb835fc608107e857036d7580a1):
  `74feb581bc8cefb835fc608107e857036d7580a1`. Root license is MIT,
  copyright HaujetZhao 2026. Preserve its notice when porting substantial code.
- [Converted model repository](https://huggingface.co/Emerald7664/Qwen3-TTS-ONNX/tree/0a36d692562f83a862466b0bf8c12154ddf1965d):
  `0a36d692562f83a862466b0bf8c12154ddf1965d`. Model card declares
  Apache-2.0 and Qwen3-TTS-12Hz-1.7B-Base provenance. This declaration is
  not a completed redistribution/notice audit. No standalone LICENSE or NOTICE
  is listed in the converted repository inventory.

The adjacent [inventory](models/qwen-gguf-onnx.inventory.json) records exact
sizes and registry LFS SHA-256 values for every large artifact. Entries identify whether hashes were checked against downloaded bytes or only
registry metadata. The selected Q5_K talker / Q8_0 predictor / INT8 ONNX set,
all embedding tables and tokenizer have now been fully downloaded and hashed.
Alternative precision artifacts remain audit-only.

## Artifact findings

The repository lists talker Q5_K (1,006,245,120 bytes) and F16; predictor Q8_0
(151,124,320 bytes) and F16; speaker encoder, codec encoder and streaming decoder
in FP16/FP32/INT8; tokenizer JSON; 16 codec embedding tables; text embedding
projection table; and predictor projection weight/bias. The six-file model-card
summary omits the embedding assets needed by the reference implementation.
The text table alone is 622,329,984 bytes; include all required assets in size
estimates. The card's download example points to a different repository, so it
must not be used as the source of pinned URLs.

Downloaded bytes of the three FP32 graphs and FP16 speaker graph matched the
inventory SHA-256 values. Structural checks using `onnx.checker` found:

| Artifact | Result |
| --- | --- |
| speaker encoder FP32 | Missing `qwen3_tts_speaker_encoder.fp32.onnx.data` |
| codec encoder FP32 | Missing `qwen3_tts_codec_encoder.fp32.onnx.data` |
| decoder FP32 | Missing `qwen3_tts_decoder.fp32.onnx.data` |
| speaker encoder FP16 | Invalid graph: `node_softmax` consumes undefined `masked_fill_cast_to_fp32` |

None of those external-data files appears in the pinned repository inventory.
FP32 graphs cannot constitute a complete offline bundle. FP16 speaker has no
external initializers, but fails topological validation. Do not silently patch
graphs or claim runtime compatibility from file extensions. The INT8 graphs all pass ONNX structural checking and execute successfully
using SonicBoom’s existing Rust `ort` rc.13 dependency (ORT build rel-1.28.0,
commit da9b5e3). The opt-in `qwen_gguf_artifacts` integration test runs speaker
encoding, codec encoding of one second of test signal (13 frames), and decoding
to 24,960 finite PCM samples. This is tensor-contract validation, not speech
quality or voice-cloning validation. Remaining FP16 graphs are unvalidated.

## Observed ONNX contracts

The downloaded FP32 graphs use default-domain opset 18. The FP32 observations are graph-level only; the selected INT8 graphs use the
same observed input/output types and passed the CPU execution test.

| Component | Inputs | Outputs |
| --- | --- | --- |
| Speaker | FLOAT `mels [B,T,128]` | FLOAT `spk_emb [B,2048]` |
| Codec | FLOAT `input_values [B,S]` | INT64 `audio_codes [B,((S-1)//1920)+1,16]` |
| Decoder | INT64 `audio_codes [B,F,16]`; FLOAT histories and KV tensors | FLOAT `final_wav`; INT64 `valid_samples`; updated histories and KV tensors |

Decoder histories are `pre_conv_history [B,512,P]`, `latent_buffer [B,1024,L]`,
`conv_history [B,1024,C]`; `is_last` is FLOAT `[1]`, not bool. Eight key and eight
value inputs each have shape `[B,16,past_seq,64]`. Output PCM capacity is symbolic
`[B,1920*latent_seq+1920*num_frames]`; trim using `valid_samples`, not capacity.
FP16 speaker exposes FLOAT16 `mels [B,T,128]`. Verify all input/output types
individually for the eventual selected precision.

## Reference dataflow and native ABI

Source review of `inference/{assets,encoder,prompt_builder,predictor,llama}.py`:

1. Resampled 24 kHz reference PCM feeds the codec encoder. Speaker conditioning
   uses mel features (1024 FFT, 256 hop, 128 bands, 0–12000 Hz), then speaker ONNX.
   Matching the reference's padding, normalization and mel math needs numerical tests.
2. `tokenizer.json` produces text IDs that index `text_embedding_projected.npy`.
   GGUF vocabularies are not a substitute for this text tokenizer.
3. ICL concatenates reference transcript and target text before tokenization;
   prompt vectors fuse text, speaker embedding, special tokens and sums over
   the 16 reference codebook embeddings. Excess text is injected during decoding.
4. Talker generates codebook zero plus hidden state. Predictor projects the
   2048-dimensional state to 1024 and generates codebooks 1–15 autoregressively.
   Its logits are partitioned into 15 ranges of 2048 entries, not one shared
   unrestricted vocabulary. Predictor KV state is cleared per frame.
5. Codebook embedding sums feed the next talker step. Completed frames feed the
   stateful ONNX decoder; histories/KV must remain request-local. Validate EOS,
   seed handling for both samplers, and final flush against actual reference audio.

Reference `llama.py` declares a ctypes ABI targeting llama.cpp **b10621** and
loads ggml/ggml-base/llama shared libraries in-process. The tag resolves to immutable commit
`c1d0e7a004015f23bc0233470b747b596f29b264`, now pinned as a Git submodule. Bind against headers from the exact
chosen revision; do not copy ctypes struct layouts into Rust against an arbitrary
system library. Downloaded GGUF metadata confirms talker architecture `qwen3vl`, embedding
dimension 2048, acoustic vocabulary 3072, and predictor architecture `qwen3`,
embedding dimension 1024, acoustic vocabulary 30720. Native loading, hidden
state extraction (talker/predictor init probes at load) and text tokenizer
parity (mel/scipy fixture tests) are validated by real-artifact tests; the
full EOS-terminated CPU synthesis run is recorded in
`benchmarks/results/2026-10-15-qwen-gguf/`.

ICL and zero-shot speaker-only paths exist in source; ICL has passed
SonicBoom real-model validation (end-to-end CPU synthesis with cache reuse
and cancellation recovery). X-vector remains rejected until validated.
CustomVoice, VoiceDesign and arbitrary GGUF exports are outside the
proposed Base-cloning backend scope.

## SonicBoom integration boundaries

Reviewed `qwen.rs`, `registry.rs`, `pool.rs`, `worker.rs`, `assets.rs`, `types.rs`
and `Cargo.toml` on main:

- The Candle backend was removed: `qwen3-gguf` is the only Qwen provider
  and owns the 1.7B model ID outright. Saved 0.6B voice profiles no longer
  resolve (explicit `unsupported_model`, never silent fallback); a leftover
  `[providers.qwen3]` config section is rejected as unknown.
- Registry model IDs overwrite earlier ownership on registration; with a
  single Qwen provider there is no collision to resolve.
- Reuse the pool's resident model lifecycle and serialized model-load guard,
  bounded worker pattern, request-carried reference PCM, inference controls,
  conditioning identity and verified atomic downloader.
- Extend conditioning identity with backend and complete artifact revision;
  maintain model ownership until cancelled native work actually stops.
- Reuse the workspace's `ort` rc.13 / ndarray 0.17 dependency graph. Actual ORT
  version/operator compatibility and shared-library packaging are not validated.
- Device enums currently support CPU/auto/CUDA; Vulkan must be added together
  with provider-specific validation and truthful effective-device reporting.
  No configuration or API defaults have been changed in this audit.

## Hardware observation and remaining acceptance gates

`vulkaninfo --summary` succeeded locally: AMD BC-250 (RADV GFX1013), vendor
`0x1002`, device `0x13fe`, Mesa `26.2.4-arch3.1`, reported device Vulkan
`1.4.354`. This proves enumeration only, not llama.cpp initialization, supported
shader operations, offload, speed, output correctness or ONNX acceleration.
No benchmark numbers are available for this backend.

Vulkan build notes (opt-in `qwen-gguf-vulkan`): besides the Vulkan driver,
headers (`vulkan-headers`), `glslc` and a `SPIRV-Headers` CMake package are
required. The pinned llama.cpp revision finds the SPIRV-Headers package but
does not add its include directory to the `ggml-vulkan` target, so the
headers must additionally be visible via `CXXFLAGS=-I<spirv-include>`
(or a system install); llama.cpp stays an untouched submodule.

Measured 2026-10-16 on this host (release, `qwen-gguf-vulkan`, seed 42,
256 tokens): Vulkan initializes on Vulkan0 with expected compute buffers,
reaches EOS with finite PCM (34 frames / 2.72 s), repeats bit-identically,
and generates ~28× faster than CPU. CPU/Vulkan content equivalence is not
established (near-zero waveform correlation, 41 vs 34 frames), so Vulkan
counts as functional, not as validated speech quality. Record:
`benchmarks/results/2026-10-15-qwen-gguf/bc250-vulkan-vs-cpu.json`.

The complete INT8 precision set, selected GGUF/embedding/tokenizer hashes and
GGUF metadata have now been inspected. Prototype native ABI validation is in
progress against the pinned headers. Next validate native CPU
speech before lifecycle, Vulkan and UI claims. Do not open the final acceptance
PR until the requested real-model, hardware and platform gates pass.

## Reproducing the graph audit

Python is development-only audit tooling, never a SonicBoom runtime dependency.
Install `onnx` into an isolated environment, download graph files using the
immutable revision above into a model directory with their original filenames,
then run `python scripts/audit_qwen_onnx.py MODEL_DIR`. It reports hashes, opsets,
tensor contracts, external-data locations and checker errors; a failed graph
returns a nonzero status. Only present graphs are inspected, so success does not
prove bundle completeness or inference compatibility.
