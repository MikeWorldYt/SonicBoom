//! Unicode-aware lexical scanning with byte offsets.
//!
//! The text is split into whitespace-delimited *fields*. Each field has a
//! *core* with leading opening punctuation (`(`, `¿`, `"`, ...) and trailing
//! closing/sentence punctuation (`.`, `,`, `)`, ...) removed. Byte offsets
//! always fall on char boundaries because they come from `char_indices`.
//!
//! *Protected regions* (inline/fenced code in backticks and `<tags>`) may
//! span whitespace; every field that overlaps one is marked protected and is
//! never classified. Field-level identifier protection (URLs, emails,
//! handles, paths, versions, IPs, ...) is applied by
//! [`is_protected_identifier`] and by the strictness of the numeric
//! matchers: a core is only rewritten when it matches a pattern in full.

use std::ops::Range;

/// One whitespace-delimited field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Field {
    pub start: usize,
    pub end: usize,
    pub core_start: usize,
    pub core_end: usize,
    pub protected: bool,
}

impl Field {
    /// `true` when the field has no leading punctuation.
    pub(crate) fn bare_start(&self) -> bool {
        self.core_start == self.start
    }

    /// `true` when the field has no trailing punctuation.
    pub(crate) fn bare_end(&self) -> bool {
        self.core_end == self.end
    }

    /// `true` when the core is non-empty.
    pub(crate) fn has_core(&self) -> bool {
        self.core_end > self.core_start
    }
}

/// Maximum bytes scanned for a closing backtick run.
const MAX_CODE_SPAN: usize = 4096;
/// Maximum bytes of a `<tag>`.
const MAX_TAG_SPAN: usize = 256;

fn is_leading_punct(c: char) -> bool {
    matches!(
        c,
        '(' | '[' | '{' | '"' | '\'' | '“' | '‘' | '«' | '¿' | '¡' | '„' | '‹'
    )
}

fn is_trailing_punct(c: char) -> bool {
    matches!(
        c,
        '.' | ','
            | ';'
            | ':'
            | '!'
            | '?'
            | ')'
            | ']'
            | '}'
            | '"'
            | '\''
            | '”'
            | '’'
            | '»'
            | '›'
            | '…'
    )
}

/// Byte ranges of backtick code spans and `<tag>`s.
pub(crate) fn protected_regions(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut regions = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'`' => {
                let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
                let fence = &text[i..i + run];
                let search_from = i + run;
                let window_end =
                    floor_char_boundary(text, (search_from + MAX_CODE_SPAN).min(text.len()));
                match text[search_from..window_end].find(fence) {
                    Some(offset) => {
                        let end = search_from + offset + run;
                        regions.push(i..end);
                        i = end;
                    }
                    None => i = search_from,
                }
            }
            b'<' => {
                let next = bytes.get(i + 1).copied();
                let tag_like =
                    next.is_some_and(|b| b.is_ascii_alphabetic() || b == b'/' || b == b'!');
                let mut end = None;
                if tag_like {
                    for (offset, b) in bytes[i + 1..].iter().enumerate().take(MAX_TAG_SPAN) {
                        match b {
                            b'>' => {
                                end = Some(i + 1 + offset + 1);
                                break;
                            }
                            b'\n' | b'<' => break,
                            _ => {}
                        }
                    }
                }
                match end {
                    Some(end) => {
                        regions.push(i..end);
                        i = end;
                    }
                    None => i += 1,
                }
            }
            _ => i += 1,
        }
    }
    regions
}

/// Largest char boundary `<= index`.
pub(crate) fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// `true` when `pos` lies inside one of `regions` (sorted, disjoint).
pub(crate) fn in_regions(regions: &[Range<usize>], pos: usize) -> bool {
    let idx = regions.partition_point(|r| r.end <= pos);
    regions
        .get(idx)
        .is_some_and(|r| r.start <= pos && pos < r.end)
}

fn overlaps(regions: &[Range<usize>], start: usize, end: usize) -> bool {
    let idx = regions.partition_point(|r| r.end <= start);
    regions.get(idx).is_some_and(|r| r.start < end)
}

/// Split `text` into whitespace-delimited fields.
pub(crate) fn scan_fields(text: &str) -> Vec<Field> {
    let regions = protected_regions(text);
    let mut fields = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                fields.push(make_field(text, s, i, &regions));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        fields.push(make_field(text, s, text.len(), &regions));
    }
    fields
}

fn make_field(text: &str, start: usize, end: usize, regions: &[Range<usize>]) -> Field {
    let raw = &text[start..end];
    let mut core_start = start;
    for (i, c) in raw.char_indices() {
        if is_leading_punct(c) {
            core_start = start + i + c.len_utf8();
        } else {
            break;
        }
    }
    let mut core_end = end;
    for (i, c) in text[core_start..end].char_indices().rev() {
        if is_trailing_punct(c) {
            core_end = core_start + i;
        } else {
            break;
        }
    }
    Field {
        start,
        end,
        core_start,
        core_end: core_end.max(core_start),
        protected: overlaps(regions, start, end),
    }
}

/// `true` for identifier-shaped cores that must never be reinterpreted:
/// URLs, emails, @handles, #tags, paths, `key=value` pairs, `::` (IPv6 or
/// code paths), versions such as `v1.2.3`, and underscores.
pub(crate) fn is_protected_identifier(core: &str) -> bool {
    if core.contains("://")
        || core.starts_with("www.")
        || core.contains('@')
        || core.starts_with('#')
        || core.contains('\\')
        || core.contains('_')
        || core.contains('=')
        || core.contains('?')
        || core.contains('&')
        || core.contains("::")
        || core.starts_with('/')
        || core.starts_with("./")
        || core.starts_with("~/")
    {
        return true;
    }
    let mut chars = core.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some('v' | 'V'), Some(d)) if d.is_ascii_digit()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_strip_punctuation() {
        let text = "¿Cuesta (S/ 150,50)? Sí.";
        let fields = scan_fields(text);
        let cores: Vec<&str> = fields
            .iter()
            .map(|f| &text[f.core_start..f.core_end])
            .collect();
        assert_eq!(cores, vec!["Cuesta", "S/", "150,50", "Sí"]);
        assert!(!fields[1].bare_start());
        assert!(!fields[2].bare_end());
    }

    #[test]
    fn protected_code_and_tags() {
        let text = "run `pip 1.2 3` then <b class=\"x 4\">5</b> and ``a`b``";
        let regions = protected_regions(text);
        let spans: Vec<&str> = regions.iter().map(|r| &text[r.clone()]).collect();
        assert_eq!(
            spans,
            vec!["`pip 1.2 3`", "<b class=\"x 4\">", "</b>", "``a`b``"]
        );
        let fields = scan_fields(text);
        assert!(fields.iter().any(|f| f.protected));
        // Unclosed backtick and lone '<' are not regions.
        assert!(protected_regions("a ` b < 3").is_empty());
    }

    #[test]
    fn identifiers() {
        for core in [
            "https://example.org/a?b=3.14",
            "user@example.org",
            "@handle",
            "#tag",
            "C:\\x\\y",
            "/usr/bin",
            "v1.2.3-beta.2",
            "2001:db8::1",
            "snake_case_1",
        ] {
            assert!(is_protected_identifier(core), "{core}");
        }
        for core in ["3.14", "12,5%", "S/", "1st", "vino"] {
            assert!(!is_protected_identifier(core), "{core}");
        }
    }

    #[test]
    fn all_punctuation_field_has_empty_core() {
        let fields = scan_fields("... ¿?");
        assert!(fields.iter().all(|f| !f.has_core()));
    }
}
