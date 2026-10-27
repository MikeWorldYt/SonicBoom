//! Protected sentence boundaries and grapheme-safe hard splitting.
//!
//! A `.`, `?`, `!`, `;` or `…` ends a sentence only when the terminator run
//! (plus closing quotes/brackets) is followed by whitespace or the end of
//! the text, and a single `.` is not a known abbreviation or an initial.
//! Therefore decimals (`3.14`), IPs (`10.0.0.1:8080`), versions (`v1.2.3`)
//! and URLs (`https://example.org/a?b=3.14`) never split. `。 ？ ！ ；` and
//! newlines always end a sentence. Terminators inside backtick code or
//! `<tags>` are ignored.

use std::ops::Range;

use unicode_normalization::char::is_combining_mark;

use super::lexer::{in_regions, protected_regions};

/// Words that, followed by a single `.`, do not end a sentence.
const NON_TERMINAL_ABBREVIATIONS: &[&str] = &[
    "Dr", "Dra", "Sr", "Sra", "Srta", "Sres", "Av", "Avda", "Prof", "Ud", "Uds", "Mr", "Mrs", "Ms",
    "St", "vs", "e.g", "i.e", "approx", "aprox", "núm", "pág",
];

fn is_closer(c: char) -> bool {
    matches!(
        c,
        '"' | '\'' | '”' | '’' | '»' | '›' | ')' | ']' | '}' | '」' | '』'
    )
}

fn is_spaced_terminator(c: char) -> bool {
    matches!(c, '.' | '?' | '!' | ';' | '…')
}

fn is_hard_terminator(c: char) -> bool {
    matches!(c, '\n' | '。' | '？' | '！' | '；')
}

/// Word (letters and inner dots) ending right before byte `dot`.
fn word_before(text: &str, dot: usize) -> &str {
    let head = &text[..dot];
    let start = head
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphabetic() || *c == '.')
        .last()
        .map(|(i, _)| i)
        .unwrap_or(dot);
    &head[start..]
}

fn is_non_terminal(word: &str) -> bool {
    if NON_TERMINAL_ABBREVIATIONS.contains(&word) {
        return true;
    }
    let mut chars = word.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if c.is_uppercase())
}

/// Contiguous byte ranges covering `text`, one per sentence. Each range
/// ends after its terminator and closers; following whitespace starts the
/// next range.
pub(crate) fn sentence_spans(text: &str) -> Vec<Range<usize>> {
    let regions = protected_regions(text);
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let byte_after = |k: usize| chars.get(k + 1).map(|(i, _)| *i).unwrap_or(text.len());
    let mut spans = Vec::new();
    let mut start = 0usize;
    let mut k = 0usize;
    while k < chars.len() {
        let (i, c) = chars[k];
        if in_regions(&regions, i) {
            k += 1;
            continue;
        }
        if is_hard_terminator(c) {
            let mut e = k;
            while e + 1 < chars.len() && is_closer(chars[e + 1].1) {
                e += 1;
            }
            let end = byte_after(e);
            spans.push(start..end);
            start = end;
            k = e + 1;
            continue;
        }
        if is_spaced_terminator(c) {
            let mut e = k;
            while e + 1 < chars.len() && matches!(chars[e + 1].1, '.' | '?' | '!' | '…') {
                e += 1;
            }
            let run_end = e;
            while e + 1 < chars.len() && is_closer(chars[e + 1].1) {
                e += 1;
            }
            let followed_by_space = e + 1 >= chars.len() || chars[e + 1].1.is_whitespace();
            let abbreviation = c == '.' && run_end == k && is_non_terminal(word_before(text, i));
            if followed_by_space && !abbreviation {
                let end = byte_after(e);
                spans.push(start..end);
                start = end;
            }
            k = e + 1;
            continue;
        }
        k += 1;
    }
    if start < text.len() {
        spans.push(start..text.len());
    }
    spans
}

fn is_cluster_extender(c: char) -> bool {
    is_combining_mark(c)
        || matches!(
            c,
            '\u{200D}'
                | '\u{FE00}'..='\u{FE0F}'
                | '\u{1F3FB}'..='\u{1F3FF}'
                | '\u{E0020}'..='\u{E007F}'
                | '\u{E0100}'..='\u{E01EF}'
        )
}

fn is_regional_indicator(c: char) -> bool {
    matches!(c, '\u{1F1E6}'..='\u{1F1FF}')
}

/// Approximate extended grapheme clusters: combining marks, variation
/// selectors, skin-tone modifiers, tag characters and ZWJ sequences stay
/// attached to their base; regional indicators pair into flags.
pub(crate) fn clusters(text: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let mut start = 0usize;
    let mut prev: Option<char> = None;
    let mut ri_in_cluster = 0usize;
    for (i, c) in text.char_indices() {
        let joins = match prev {
            None => false,
            Some(p) => {
                is_cluster_extender(c)
                    || p == '\u{200D}'
                    || (is_regional_indicator(c) && ri_in_cluster == 1 && is_regional_indicator(p))
            }
        };
        if !joins && i > start {
            out.push(&text[start..i]);
            start = i;
            ri_in_cluster = 0;
        }
        if is_regional_indicator(c) {
            ri_in_cluster += 1;
        }
        prev = Some(c);
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// Split `text` into pieces of at most `max_chars` chars, cutting only
/// between clusters. A single cluster longer than `max_chars` is split by
/// chars as a last resort.
pub(crate) fn hard_split(text: &str, max_chars: usize) -> Vec<String> {
    let max_chars = max_chars.max(1);
    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;
    for cluster in clusters(text) {
        let len = cluster.chars().count();
        if len > max_chars {
            if !current.is_empty() {
                pieces.push(std::mem::take(&mut current));
                current_len = 0;
            }
            let chars: Vec<char> = cluster.chars().collect();
            pieces.extend(
                chars
                    .chunks(max_chars)
                    .map(|c| c.iter().collect::<String>()),
            );
            continue;
        }
        if current_len + len > max_chars {
            pieces.push(std::mem::take(&mut current));
            current_len = 0;
        }
        current.push_str(cluster);
        current_len += len;
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sentences(text: &str) -> Vec<&str> {
        sentence_spans(text)
            .into_iter()
            .map(|r| text[r].trim())
            .filter(|s| !s.is_empty())
            .collect()
    }

    #[test]
    fn spans_cover_text() {
        for text in ["", "a", "Hola. Adiós.", "x\n\ny", "Uno… dos? tres!"] {
            let spans = sentence_spans(text);
            let mut cursor = 0;
            for span in &spans {
                assert_eq!(span.start, cursor);
                cursor = span.end;
            }
            assert_eq!(cursor, text.len());
        }
    }

    #[test]
    fn protected_tokens_do_not_split() {
        for token in [
            "3.14",
            "10.0.0.1:8080",
            "https://example.org/a?b=3.14",
            "v1.2.3-beta.2",
            "Minecraft 1.21.11",
            "user@example.org",
            "1.234,56",
        ] {
            let text = format!("Valor {token} aquí. Fin.");
            let got = sentences(&text);
            assert_eq!(
                got,
                vec![format!("Valor {token} aquí."), "Fin.".to_string()]
            );
        }
    }

    #[test]
    fn abbreviations_and_initials() {
        assert_eq!(
            sentences("Vino el Dr. García. Luego J. K. Rowling habló."),
            vec!["Vino el Dr. García.", "Luego J. K. Rowling habló."]
        );
        assert_eq!(sentences("Fin etc. Otro."), vec!["Fin etc.", "Otro."]);
    }

    #[test]
    fn other_terminators() {
        assert_eq!(
            sentences("¿Qué? ¡Sí! Bien; ok"),
            vec!["¿Qué?", "¡Sí!", "Bien;", "ok"]
        );
        assert_eq!(sentences("日本語。テスト！"), vec!["日本語。", "テスト！"]);
        assert_eq!(
            sentences("Dijo \"hola.\" Luego"),
            vec!["Dijo \"hola.\"", "Luego"]
        );
        assert_eq!(sentences("Espera... ya"), vec!["Espera...", "ya"]);
        assert_eq!(sentences("a\nb"), vec!["a", "b"]);
        assert_eq!(
            sentences("run `a. b` now. Done"),
            vec!["run `a. b` now.", "Done"]
        );
    }

    #[test]
    fn clusters_keep_zwj_and_marks() {
        let family = "👨\u{200D}👩\u{200D}👧\u{200D}👦";
        assert_eq!(clusters(family), vec![family]);
        assert_eq!(clusters("e\u{301}a"), vec!["e\u{301}", "a"]);
        assert_eq!(clusters("🇵🇪🇪🇸"), vec!["🇵🇪", "🇪🇸"]);
        assert_eq!(clusters("🎙️x"), vec!["🎙️", "x"]);
        assert_eq!(clusters("👍🏽"), vec!["👍🏽"]);
    }

    #[test]
    fn hard_split_never_cuts_clusters() {
        let family = "👨\u{200D}👩\u{200D}👧\u{200D}👦"; // 7 chars
        let text = family.repeat(5);
        let pieces = hard_split(&text, 10);
        assert!(pieces.iter().all(|p| p == family));
        assert_eq!(pieces.concat(), text);
        // A cluster longer than the limit falls back to char splitting.
        let pieces = hard_split(family, 3);
        assert!(pieces.iter().all(|p| p.chars().count() <= 3));
        assert_eq!(pieces.concat(), family);
    }
}
