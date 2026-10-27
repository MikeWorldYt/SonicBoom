use anyhow::Result;
use unicode_normalization::UnicodeNormalization;

use crate::tts::normalization::segments;

/// Default hard maximum chunk size in Unicode characters.
pub const DEFAULT_MAX_CHUNK_CHARS: usize = 200;

pub struct TextProcessor {
    // Index = unicode codepoint, value = model internal ID (-1 means unsupported)
    indexer: Vec<i64>,
}

impl TextProcessor {
    pub fn load(unicode_indexer_path: &std::path::Path) -> Result<Self> {
        let data = std::fs::read_to_string(unicode_indexer_path)?;
        let indexer: Vec<i64> = serde_json::from_str(&data)?;
        Ok(Self { indexer })
    }

    /// Encode `text` into model token ids and an all-ones mask.
    ///
    /// Same values as [`TextProcessor::encode_with_report`] without the
    /// dropped-character count.
    pub fn encode(&self, text: &str) -> (Vec<i64>, Vec<i64>) {
        let (ids, mask, _dropped) = self.encode_with_report(text);
        (ids, mask)
    }

    /// Encode `text` and also return how many characters (after NFKD) the
    /// unicode indexer could not map and therefore dropped.
    ///
    /// Only the count is reported, never the characters, so the value is
    /// safe to log. The NFKD step is unchanged on purpose: whether the model
    /// expects decomposed accents still needs verification against the real
    /// indexer (open item in `docs/normalization.md`).
    pub fn encode_with_report(&self, text: &str) -> (Vec<i64>, Vec<i64>, usize) {
        let mut dropped = 0usize;
        let ids: Vec<i64> = text
            .nfkd()
            .filter_map(|c| {
                let id = self.indexer.get(c as usize).copied().filter(|&id| id >= 0);
                if id.is_none() {
                    dropped += 1;
                }
                id
            })
            .collect();

        let mask: Vec<i64> = vec![1i64; ids.len()];
        (ids, mask, dropped)
    }

    pub fn split_sentences(text: &str) -> Vec<String> {
        Self::split_sentences_with_limit(text, DEFAULT_MAX_CHUNK_CHARS)
    }

    /// Split `text` into chunks of at most `max_chars` Unicode characters.
    ///
    /// Strategy: prefer protected sentence boundaries, then whitespace, then
    /// a hard split between grapheme clusters (by character count only when
    /// a single cluster exceeds the limit). No returned chunk ever exceeds
    /// `max_chars` characters (measured with [`str::chars`], not UTF-8
    /// bytes).
    ///
    /// Sentence boundaries come from
    /// [`crate::tts::normalization::segments`]: a `.` only ends a sentence
    /// when followed by whitespace (or the end) and not after a known
    /// abbreviation or initial, so decimals (`3.14`), IP addresses, versions
    /// and URLs are never cut. `? ! ; …` follow the same rule; `。 ？ ！ ；`
    /// and newlines always end a sentence.
    pub fn split_sentences_with_limit(text: &str, max_chars: usize) -> Vec<String> {
        let max_chars = max_chars.max(1);
        let text = text.trim();
        if text.is_empty() {
            return Vec::new();
        }
        if text.chars().count() <= max_chars {
            return vec![text.to_string()];
        }

        // 1. Split at protected sentence boundaries.
        let sentences: Vec<&str> = segments::sentence_spans(text)
            .into_iter()
            .map(|span| text[span].trim())
            .filter(|sentence| !sentence.is_empty())
            .collect();

        // 2. Greedily pack sentences, splitting oversized ones.
        let mut chunks = Vec::new();
        let mut pending = String::new();
        let mut pending_len = 0usize;
        for sentence in sentences {
            for piece in split_oversized(sentence, max_chars) {
                let piece_len = piece.chars().count();
                if pending_len > 0 && pending_len + 1 + piece_len > max_chars {
                    chunks.push(std::mem::take(&mut pending));
                    pending_len = 0;
                }
                if pending_len > 0 {
                    pending.push(' ');
                    pending_len += 1;
                }
                pending.push_str(&piece);
                pending_len += piece_len;
            }
        }
        if !pending.trim().is_empty() {
            chunks.push(pending.trim().to_string());
        }
        if chunks.is_empty() {
            chunks.push(hard_truncate(text, max_chars));
        }
        chunks
    }
}

/// Split a single sentence that may exceed `max_chars`: first at whitespace,
/// then by hard character count. Every piece is <= `max_chars` chars.
fn split_oversized(sentence: &str, max_chars: usize) -> Vec<String> {
    if sentence.chars().count() <= max_chars {
        return vec![sentence.to_string()];
    }
    // Try whitespace splitting.
    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;
    for word in sentence.split_whitespace() {
        let word_len = word.chars().count();
        if word_len > max_chars {
            // Single word exceeds the limit: flush and hard-split the word.
            if !current.is_empty() {
                pieces.push(std::mem::take(&mut current));
                current_len = 0;
            }
            pieces.extend(hard_split(word, max_chars));
        } else {
            if current_len > 0 && current_len + 1 + word_len > max_chars {
                pieces.push(std::mem::take(&mut current));
                current_len = 0;
            }
            if current_len > 0 {
                current.push(' ');
                current_len += 1;
            }
            current.push_str(word);
            current_len += word_len;
        }
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    if pieces.is_empty() {
        // No whitespace at all: hard-split by character count.
        pieces.extend(hard_split(sentence, max_chars));
    }
    pieces
}

/// Hard split that never cuts inside a grapheme cluster unless the cluster
/// alone exceeds `max_chars`.
fn hard_split(text: &str, max_chars: usize) -> Vec<String> {
    segments::hard_split(text, max_chars)
}

fn hard_truncate(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_bounded(chunks: &[String], max_chars: usize) {
        assert!(!chunks.is_empty());
        for chunk in chunks {
            let len = chunk.chars().count();
            assert!(
                len <= max_chars,
                "chunk of {len} chars exceeds limit {max_chars}: {chunk:?}"
            );
            assert!(!chunk.is_empty());
        }
    }

    #[test]
    fn short_text_is_single_chunk() {
        assert_eq!(TextProcessor::split_sentences("Hello."), vec!["Hello."]);
    }

    #[test]
    fn normal_punctuation_splits() {
        let text = "Hello world. This is a test! How are you? Fine, thanks.";
        let chunks = TextProcessor::split_sentences_with_limit(text, 30);
        assert_bounded(&chunks, 30);
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn long_punctuation_free_ascii_is_bounded() {
        let text = "lorem ipsum dolor sit amet ".repeat(40);
        let chunks = TextProcessor::split_sentences_with_limit(&text, 200);
        assert_bounded(&chunks, 200);
        assert!(chunks.len() > 1);
    }

    #[test]
    fn long_single_word_without_whitespace_is_bounded() {
        let text = "a".repeat(1000);
        let chunks = TextProcessor::split_sentences_with_limit(&text, 200);
        assert_bounded(&chunks, 200);
        assert_eq!(chunks.len(), 5);
    }

    #[test]
    fn cjk_text_without_spaces_is_bounded() {
        let text = "日本語のテスト文章です。".repeat(60);
        let chunks = TextProcessor::split_sentences_with_limit(&text, 200);
        assert_bounded(&chunks, 200);
    }

    #[test]
    fn emoji_text_is_bounded_by_chars_not_bytes() {
        // Each emoji is 4 UTF-8 bytes but 1 char; 300 emojis = 1200 bytes.
        let text = "🎙️".repeat(150);
        let chunks = TextProcessor::split_sentences_with_limit(&text, 200);
        assert_bounded(&chunks, 200);
        // Must not split in the middle of nothing: rejoining covers input.
        let joined_len: usize = chunks.iter().map(|c| c.chars().count()).sum();
        assert_eq!(joined_len, text.chars().count());
    }

    #[test]
    fn mixed_unicode_long_sentence_is_bounded() {
        let text = "Héllo wörld, this is à ünïcodé sentence without any period ".repeat(20);
        let chunks = TextProcessor::split_sentences_with_limit(&text, 200);
        assert_bounded(&chunks, 200);
    }

    #[test]
    fn chunks_never_exceed_hard_limit_on_small_limit() {
        let text = "one two three four five six seven eight nine ten. ".repeat(10);
        for limit in [1, 7, 50] {
            let chunks = TextProcessor::split_sentences_with_limit(&text, limit);
            assert_bounded(&chunks, limit);
        }
    }

    /// Non-whitespace characters of `text`, in order.
    fn dense(text: &str) -> String {
        text.chars().filter(|c| !c.is_whitespace()).collect()
    }

    fn assert_covers(chunks: &[String], text: &str) {
        assert_eq!(
            dense(&chunks.concat()),
            dense(text),
            "chunks lost or reordered text"
        );
    }

    #[test]
    fn protected_tokens_are_never_split() {
        for token in [
            "3.14",
            "10.0.0.1:8080",
            "https://example.org/a?b=3.14",
            "v1.2.3",
            "v1.2.3-beta.2",
            "1.21.11",
            "1.234,56",
            "user@example.org",
        ] {
            let filler = "palabra ".repeat(6);
            let text = format!("{filler}valor {token} final. {filler}otra frase aquí.");
            let chunks = TextProcessor::split_sentences_with_limit(&text, 80);
            assert_bounded(&chunks, 80);
            assert_covers(&chunks, &text);
            assert!(
                chunks.iter().any(|c| c.contains(token)),
                "token {token} was cut: {chunks:?}"
            );
        }
    }

    #[test]
    fn decimal_is_not_a_sentence_boundary() {
        let text = "El valor es 3.14 y sigue la frase sin cortar nada más.";
        let chunks = TextProcessor::split_sentences_with_limit(text, 40);
        assert_bounded(&chunks, 40);
        assert_covers(&chunks, text);
        assert!(chunks.iter().any(|c| c.contains("3.14")));
    }

    #[test]
    fn abbreviation_is_not_a_sentence_boundary() {
        let text = "Ayer vino el Dr. García. Hoy no vino.";
        let chunks = TextProcessor::split_sentences_with_limit(text, 24);
        assert_bounded(&chunks, 24);
        assert_eq!(chunks, vec!["Ayer vino el Dr. García.", "Hoy no vino."]);
    }

    #[test]
    fn sentence_boundaries_still_split() {
        let text = "Primera frase. ¿Segunda? ¡Tercera! Cuarta; quinta\nsexta 。七";
        let chunks = TextProcessor::split_sentences_with_limit(text, 16);
        assert_bounded(&chunks, 16);
        assert_covers(&chunks, text);
        assert!(chunks.len() >= 5);
    }

    #[test]
    fn zwj_emoji_and_accents_survive_hard_split() {
        let family = "👨\u{200D}👩\u{200D}👧\u{200D}👦";
        let text = format!("{} Pingüino niño acción áéíóú", family.repeat(30));
        let chunks = TextProcessor::split_sentences_with_limit(&text, 20);
        assert_bounded(&chunks, 20);
        assert_covers(&chunks, &text);
        let families: usize = chunks.iter().map(|c| c.matches(family).count()).sum();
        assert_eq!(families, 30, "a ZWJ family was split: {chunks:?}");
        assert!(chunks.iter().any(|c| c.contains("Pingüino")));
    }

    #[test]
    fn encode_with_report_counts_dropped_chars() {
        // Indexer that knows only ASCII letters a..=z and the combining
        // acute accent (U+0301).
        let mut indexer = vec![-1i64; 0x302];
        for (id, cp) in (b'a'..=b'z').enumerate() {
            indexer[cp as usize] = id as i64;
        }
        indexer[0x301] = 99;
        let processor = TextProcessor { indexer };
        let (ids, mask, dropped) = processor.encode_with_report("ab é😊");
        // NFKD: a b ' ' e U+0301 😊 -> space and emoji are dropped.
        assert_eq!(ids, vec![0, 1, 4, 99]);
        assert_eq!(mask, vec![1; 4]);
        assert_eq!(dropped, 2);
        assert_eq!(processor.encode("ab é😊"), (ids, mask));
        assert_eq!(processor.encode_with_report(""), (vec![], vec![], 0));
    }
}
