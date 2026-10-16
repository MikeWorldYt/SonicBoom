//! Bounded, backend-specific ICL conditioning cache, owned by the native worker.
use super::{conditioning::Reference, models::REVISION};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

pub struct ReferenceCache {
    capacity: usize,
    entries: HashMap<String, Arc<Reference>>,
    order: VecDeque<String>,
}
impl ReferenceCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.clamp(1, 64),
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }
    pub fn key(voice: &str, pcm: &[f32], rate: u32, transcript: &str) -> String {
        let mut hash = Sha256::new();
        hash.update(rate.to_le_bytes());
        for sample in pcm {
            hash.update(sample.to_le_bytes());
        }
        let identity = serde_json::to_vec(&(
            REVISION,
            voice,
            hex::encode(hash.finalize()),
            transcript,
            "icl",
        ))
        .expect("string tuple serializes");
        format!("qwen3-gguf-onnx:{}", hex::encode(Sha256::digest(identity)))
    }
    pub fn get(&mut self, key: &str) -> Option<Arc<Reference>> {
        let value = self.entries.get(key)?.clone();
        self.order.retain(|k| k != key);
        self.order.push_back(key.to_owned());
        Some(value)
    }
    pub fn insert(&mut self, key: String, value: Reference) -> Arc<Reference> {
        let value = Arc::new(value);
        self.order.retain(|k| k != &key);
        self.entries.insert(key.clone(), value.clone());
        self.order.push_back(key);
        while self.entries.len() > self.capacity {
            if let Some(key) = self.order.pop_front() {
                self.entries.remove(&key);
            }
        }
        value
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conditioning_identity_includes_voice_audio_rate_and_transcript() {
        let key = ReferenceCache::key("voice", &[0.1], 24_000, "hello");
        assert_ne!(key, ReferenceCache::key("other", &[0.1], 24_000, "hello"));
        assert_ne!(key, ReferenceCache::key("voice", &[0.2], 24_000, "hello"));
        assert_ne!(key, ReferenceCache::key("voice", &[0.1], 44_100, "hello"));
        assert_ne!(key, ReferenceCache::key("voice", &[0.1], 24_000, "world"));
    }
    #[test]
    fn cache_evicts_least_recently_used_reference() {
        fn reference() -> Reference {
            Reference {
                speaker: vec![0.0; 2048],
                codes: vec![[0; 16]],
                transcript: "test".into(),
            }
        }
        let mut cache = ReferenceCache::new(2);
        cache.insert("a".into(), reference());
        cache.insert("b".into(), reference());
        assert!(cache.get("a").is_some());
        cache.insert("c".into(), reference());
        assert!(cache.get("b").is_none());
        assert!(cache.get("a").is_some());
        cache.clear();
        assert!(cache.get("a").is_none());
    }
}
