//! The text tokenizer is separate from the GGUF acoustic vocabulary.
use anyhow::{Result, anyhow, ensure};
use std::path::Path;

pub struct TextTokenizer(tokenizers::Tokenizer);

impl TextTokenizer {
    pub fn load(dir: &Path) -> Result<Self> {
        let tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| anyhow!("Qwen text tokenizer: {e}"))?;
        for (token, id) in [
            ("<|im_start|>", 151644),
            ("<|im_end|>", 151645),
            ("<tts_pad>", 151671),
            ("<tts_text_bos>", 151672),
            ("<tts_text_eod>", 151673),
        ] {
            ensure!(
                tokenizer.token_to_id(token) == Some(id),
                "incompatible tokenizer: {token} must be {id}"
            );
        }
        Ok(Self(tokenizer))
    }

    pub fn encode(&self, text: &str) -> Result<Vec<u32>> {
        Ok(self
            .0
            .encode(text, false)
            .map_err(|e| anyhow!("Qwen tokenization: {e}"))?
            .get_ids()
            .to_vec())
    }
}
