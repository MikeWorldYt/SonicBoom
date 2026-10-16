//! Bounded, backend-specific ICL conditioning cache, owned by the native worker.
use super::{conditioning::Reference, models::REVISION};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

/// Conservative hard cap on cache-owned reference bytes (256 MiB). Entry
/// count alone cannot bound memory: one reference holds seconds of speaker
/// vectors, codec frames, and transcript text.
pub const DEFAULT_MAX_BYTES: usize = 256 * 1024 * 1024;
/// Floor for an explicitly configured byte budget, so a tiny or zero value
/// cannot make the cache unusable (a single oversized entry is still
/// retained; see [`ReferenceCache::insert`]).
pub const MIN_MAX_BYTES: usize = 1024 * 1024;

impl Reference {
    /// Approximate owned heap bytes: speaker vectors, codec frames, and
    /// transcript text. Allocation headers and `Arc` bookkeeping are excluded;
    /// the figure is a lower bound used for eviction, not an allocator audit.
    /// Cloned `Arc` handles held by in-flight requests are *not* owned by the
    /// cache and are reported separately (see [`ReferenceCache::stats`]).
    pub fn heap_bytes(&self) -> usize {
        self.speaker
            .len()
            .saturating_mul(size_of::<f32>())
            .saturating_add(self.codes.len().saturating_mul(size_of::<[i64; 16]>()))
            .saturating_add(self.transcript.len())
    }
}

/// Cache counters for hits, misses, resident bytes, and evictions.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub entries: usize,
    /// Bytes owned by resident entries (excludes in-flight `Arc` clones).
    pub bytes: usize,
    pub evictions: u64,
}

pub struct ReferenceCache {
    capacity: usize,
    max_bytes: usize,
    entries: HashMap<String, Arc<Reference>>,
    bytes_by_key: HashMap<String, usize>,
    resident_bytes: usize,
    order: VecDeque<String>,
    hits: u64,
    misses: u64,
    evictions: u64,
}
impl ReferenceCache {
    pub fn new(capacity: usize) -> Self {
        Self::with_max_bytes(capacity, DEFAULT_MAX_BYTES)
    }

    pub fn with_max_bytes(capacity: usize, max_bytes: usize) -> Self {
        Self {
            capacity: capacity.clamp(1, 64),
            max_bytes: max_bytes.max(MIN_MAX_BYTES),
            entries: HashMap::new(),
            bytes_by_key: HashMap::new(),
            resident_bytes: 0,
            order: VecDeque::new(),
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits,
            misses: self.misses,
            entries: self.entries.len(),
            bytes: self.resident_bytes,
            evictions: self.evictions,
        }
    }

    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }
    pub fn key(voice: &str, pcm: &[f32], rate: u32, transcript: &str) -> String {
        let mut hash = Sha256::new();
        hash.update(rate.to_le_bytes());
        for sample in pcm {
            hash.update(sample.to_le_bytes());
        }
        let identity = serde_json::to_vec(&(
            REVISION,
            crate::tts::audio::REFERENCE_RESAMPLER_VERSION,
            voice,
            hex::encode(hash.finalize()),
            transcript,
            "icl",
        ))
        .expect("string tuple serializes");
        format!("qwen3-gguf-onnx:{}", hex::encode(Sha256::digest(identity)))
    }
    pub fn get(&mut self, key: &str) -> Option<Arc<Reference>> {
        let Some(value) = self.entries.get(key).cloned() else {
            self.misses += 1;
            return None;
        };
        self.hits += 1;
        self.order.retain(|k| k != key);
        self.order.push_back(key.to_owned());
        Some(value)
    }

    /// Insert a reference, evicting least-recently-used entries until **both**
    /// the entry-count and byte budgets hold. Evicted entries drop the
    /// cache's `Arc`; clones already handed to in-flight requests stay valid
    /// and are never mutated or freed by eviction. A single entry larger
    /// than the whole budget is still retained (evicting everything else) so
    /// synthesis can proceed; the overshoot stays visible in [`Self::stats`].
    pub fn insert(&mut self, key: String, value: Reference) -> Arc<Reference> {
        let bytes = value.heap_bytes();
        let value = Arc::new(value);
        if let Some(old_bytes) = self.bytes_by_key.insert(key.clone(), bytes) {
            self.resident_bytes = self.resident_bytes.saturating_sub(old_bytes);
        }
        self.resident_bytes = self.resident_bytes.saturating_add(bytes);
        self.order.retain(|k| k != &key);
        self.entries.insert(key.clone(), value.clone());
        self.order.push_back(key);
        // Always keep the newest entry, even when it alone exceeds the byte
        // budget; otherwise an oversized reference could never be served.
        while self.entries.len() > 1
            && (self.entries.len() > self.capacity || self.resident_bytes > self.max_bytes)
            && let Some(oldest) = self.order.pop_front()
        {
            if self.entries.remove(&oldest).is_some() {
                if let Some(old_bytes) = self.bytes_by_key.remove(&oldest) {
                    self.resident_bytes = self.resident_bytes.saturating_sub(old_bytes);
                }
                self.evictions += 1;
            }
        }
        value
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes_by_key.clear();
        self.resident_bytes = 0;
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
        let stats = cache.stats();
        assert_eq!(stats.entries, 2);
        assert!(stats.bytes > 0);
        assert_eq!(stats.evictions, 1);
        assert_eq!((stats.hits, stats.misses), (2, 1));
        cache.clear();
        assert!(cache.get("a").is_none());
        assert_eq!(cache.stats().bytes, 0);
    }

    fn sized_reference(speaker_len: usize, frames: usize, transcript_len: usize) -> Reference {
        Reference {
            speaker: vec![0.0; speaker_len],
            codes: vec![[0; 16]; frames],
            transcript: "x".repeat(transcript_len),
        }
    }

    #[test]
    fn heap_bytes_measures_speaker_codes_and_transcript() {
        let reference = sized_reference(2048, 10, 100);
        assert_eq!(
            reference.heap_bytes(),
            2048 * 4 + 10 * size_of::<[i64; 16]>() + 100
        );
        assert_eq!(
            sized_reference(0, 0, 0).heap_bytes(),
            0,
            "zero-sized references measure zero bytes"
        );
    }

    #[test]
    fn byte_budget_evicts_lru_before_count_budget_bites() {
        // Each entry ≈ 1 MiB speaker data; a 2.5 MiB budget holds two.
        let entry_bytes = 256 * 1024 * 4;
        let mut cache = ReferenceCache::with_max_bytes(64, entry_bytes * 2 + entry_bytes / 2);
        cache.insert("a".into(), sized_reference(256 * 1024, 0, 0));
        cache.insert("b".into(), sized_reference(256 * 1024, 0, 0));
        assert!(cache.get("a").is_some());
        assert!(cache.get("b").is_some());
        cache.insert("c".into(), sized_reference(256 * 1024, 0, 0));
        // Count budget (64) is nowhere near exhausted; the byte budget
        // evicted the least-recently-used entry instead.
        assert!(cache.get("a").is_none());
        assert!(cache.get("b").is_some());
        assert!(cache.get("c").is_some());
        assert!(cache.stats().bytes <= cache.max_bytes());
    }

    #[test]
    fn oversized_single_entry_is_retained_and_reported() {
        let mut cache = ReferenceCache::with_max_bytes(64, MIN_MAX_BYTES);
        let big = cache.insert("big".into(), sized_reference(1024 * 1024, 0, 0));
        // 4 MiB > 1 MiB floor: retained so synthesis can proceed, with the
        // overshoot visible in stats rather than silently dropped.
        assert!(cache.get("big").is_some());
        assert!(cache.stats().bytes > cache.max_bytes());
        assert_eq!(big.speaker.len(), 1024 * 1024);
    }

    #[test]
    fn replacing_a_key_updates_byte_accounting() {
        let mut cache = ReferenceCache::new(4);
        cache.insert("a".into(), sized_reference(100, 0, 0));
        let before = cache.stats().bytes;
        cache.insert("a".into(), sized_reference(10, 0, 0));
        assert!(cache.stats().bytes < before);
        assert_eq!(cache.stats().entries, 1);
        assert_eq!(cache.stats().evictions, 0);
    }

    #[test]
    fn evicted_entries_leave_in_flight_arcs_usable() {
        let mut cache = ReferenceCache::new(1);
        let live = cache.insert("a".into(), sized_reference(64, 2, 8));
        cache.insert("b".into(), sized_reference(64, 2, 8));
        // The cache dropped "a", but the in-flight clone stays valid.
        assert!(cache.get("a").is_none());
        assert_eq!(live.speaker.len(), 64);
        assert_eq!(live.codes.len(), 2);
        assert_eq!(live.transcript.len(), 8);
        assert_eq!(cache.stats().evictions, 1);
    }

    #[test]
    fn repeated_unique_references_stay_within_budget() {
        let entry_bytes = 64 * 1024 * 4;
        let mut cache = ReferenceCache::with_max_bytes(64, entry_bytes * 4);
        for i in 0..200 {
            cache.insert(format!("voice-{i}"), sized_reference(64 * 1024, 0, 0));
            assert!(cache.stats().bytes <= cache.max_bytes());
        }
        assert_eq!(cache.stats().entries, 4);
        assert_eq!(cache.stats().evictions, 200 - 4);
    }
}
