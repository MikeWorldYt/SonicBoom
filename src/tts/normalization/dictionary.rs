//! Pronunciation exception dictionary (grapheme rewrites, not phonemes).
//!
//! Status in `tn-v1`: data structure and matching only. It is **not wired**
//! into [`super::normalize`], there is no API to supply entries, and no
//! built-in entries ship, because the spec allows entries only with
//! language-specific tests or an authorized opt-in. Never inject IPA, SSML
//! or markup through this table.
//!
//! Guarantees: whole-word, case-sensitive matching; one left-to-right pass
//! over the *original* text (replacements are never re-scanned, so rules
//! cannot loop); bounded entry count and lengths.

/// Maximum number of entries.
pub(crate) const MAX_ENTRIES: usize = 512;
/// Maximum characters of a key or a replacement.
pub(crate) const MAX_ENTRY_CHARS: usize = 64;

/// Error building a dictionary. Carries no entry text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DictionaryError {
    TooManyEntries,
    EntryTooLong,
    EmptyKey,
    DuplicateKey,
}

/// Validated, sorted dictionary.
#[derive(Clone, Debug, Default)]
pub(crate) struct Dictionary {
    entries: Vec<(String, String)>,
}

impl Dictionary {
    /// Build from `(written, spoken)` pairs.
    pub(crate) fn new(pairs: &[(&str, &str)]) -> Result<Self, DictionaryError> {
        if pairs.len() > MAX_ENTRIES {
            return Err(DictionaryError::TooManyEntries);
        }
        let mut entries: Vec<(String, String)> = Vec::with_capacity(pairs.len());
        for (key, value) in pairs {
            if key.is_empty() {
                return Err(DictionaryError::EmptyKey);
            }
            if key.chars().count() > MAX_ENTRY_CHARS || value.chars().count() > MAX_ENTRY_CHARS {
                return Err(DictionaryError::EntryTooLong);
            }
            entries.push(((*key).to_string(), (*value).to_string()));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        if entries.windows(2).any(|w| w[0].0 == w[1].0) {
            return Err(DictionaryError::DuplicateKey);
        }
        Ok(Self { entries })
    }

    /// `true` when there are no entries.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Replacement for a whole word, if any.
    pub(crate) fn lookup(&self, word: &str) -> Option<&str> {
        self.entries
            .binary_search_by(|(key, _)| key.as_str().cmp(word))
            .ok()
            .map(|index| self.entries[index].1.as_str())
    }

    /// Byte spans `(start, end, replacement)` of whole-word matches in
    /// `text`, left to right, non-overlapping. Words are maximal runs of
    /// alphanumeric characters.
    pub(crate) fn matches<'a>(&'a self, text: &str) -> Vec<(usize, usize, &'a str)> {
        let mut out = Vec::new();
        if self.is_empty() {
            return out;
        }
        let mut start: Option<usize> = None;
        let push = |s: usize, e: usize, out: &mut Vec<(usize, usize, &'a str)>| {
            if let Some(replacement) = self.lookup(&text[s..e]) {
                out.push((s, e, replacement));
            }
        };
        for (i, c) in text.char_indices() {
            if c.is_alphanumeric() {
                if start.is_none() {
                    start = Some(i);
                }
            } else if let Some(s) = start.take() {
                push(s, i, &mut out);
            }
        }
        if let Some(s) = start {
            push(s, text.len(), &mut out);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_word_case_sensitive() {
        let dict = Dictionary::new(&[("Qwen", "Chuén")]).unwrap();
        let text = "Qwen, Qwen3 y qwen. (Qwen)";
        let spans = dict.matches(text);
        let words: Vec<&str> = spans.iter().map(|(s, e, _)| &text[*s..*e]).collect();
        assert_eq!(words, vec!["Qwen", "Qwen"]);
        assert!(spans.iter().all(|(_, _, r)| *r == "Chuén"));
    }

    #[test]
    fn no_recursion_and_limits() {
        // A replacement containing its own key is not re-scanned.
        let dict = Dictionary::new(&[("a", "a a")]).unwrap();
        assert_eq!(dict.matches("a b").len(), 1);
        assert_eq!(
            Dictionary::new(&[("", "x")]).unwrap_err(),
            DictionaryError::EmptyKey
        );
        assert_eq!(
            Dictionary::new(&[("k", "x"), ("k", "y")]).unwrap_err(),
            DictionaryError::DuplicateKey
        );
        let long = "x".repeat(MAX_ENTRY_CHARS + 1);
        assert_eq!(
            Dictionary::new(&[("k", long.as_str())]).unwrap_err(),
            DictionaryError::EntryTooLong
        );
        assert!(Dictionary::default().matches("anything").is_empty());
    }
}
