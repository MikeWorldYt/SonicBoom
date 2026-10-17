//! Actual stage timings, without conflating requested and effective devices.
use serde::Serialize;
#[derive(Default, Clone, Debug, Serialize)]
pub struct Metrics {
    pub load_secs: f64,
    pub conditioning_secs: f64,
    pub prompt_secs: f64,
    pub generation_secs: f64,
    pub decode_secs: f64,
    pub frames: usize,
    pub reached_eos: bool,
    pub reference_cache_hit: bool,
    pub audio_secs: f64,
    /// Effective sampling preset name for diagnostics.
    pub sampling_preset: String,
    /// Effective seed used for the returned audio.
    pub seed: u64,
}
impl Metrics {
    pub fn rtf(&self) -> Option<f64> {
        (self.audio_secs > 0.0).then(|| {
            (self.conditioning_secs + self.prompt_secs + self.generation_secs + self.decode_secs)
                / self.audio_secs
        })
    }
}
