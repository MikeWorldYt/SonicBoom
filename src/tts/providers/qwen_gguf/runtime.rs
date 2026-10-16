//! Thread-confined native contexts. Construct and use inside one worker thread.
use anyhow::{Result, anyhow};
use sonicboom_qwen_llama::Model;
use std::path::Path;

pub struct Runtime {
    pub talker: Model,
    pub predictor: Model,
}
impl Runtime {
    pub fn load(dir: &Path, device: i32, gpu_layers: i32, threads: usize) -> Result<Self> {
        let open = |name: &str, context, talker| {
            let path = dir.join(name);
            let path = path
                .to_str()
                .ok_or_else(|| anyhow!("native model path must be UTF-8"))?;
            Model::load(path, device, gpu_layers, threads, context, talker)
                .map_err(|e| anyhow!("{name}: {e}"))
        };
        Ok(Self {
            talker: open("qwen3_tts_talker.q5_k.gguf", 4096, true)?,
            predictor: open("qwen3_tts_predictor.q8_0.gguf", 128, false)?,
        })
    }
    pub fn reset(&mut self) -> Result<()> {
        self.talker.clear().map_err(|e| anyhow!("{e}"))?;
        self.predictor.clear().map_err(|e| anyhow!("{e}"))?;
        Ok(())
    }
}
