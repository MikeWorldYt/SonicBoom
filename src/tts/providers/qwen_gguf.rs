//! Experimental Qwen3-TTS 1.7B Base GGUF / ONNX backend.
//!
//! Native CPU generation passed real-model acceptance (finite PCM,
//! EOS-terminated, conditioning reuse verified); the `qwen3-gguf`
//! provider below exposes it through the shared registry. The existing
//! Candle provider remains independently available for 0.6B.
pub mod cache;
pub mod codec;
pub mod conditioning;
pub mod generation;
pub mod metrics;
pub mod models;
pub mod provider;
pub mod runtime;
pub mod tokenizer;
pub mod worker;
