//! Classification of fields into spoken-form changes and warnings.
//!
//! Precedence, per the spec's classification policy:
//! 1. protected regions and identifier-shaped cores (never touched);
//! 2. phone-like runs of space-separated digit groups (protected);
//! 3. contextual explicit types: currency, percentages, ordinals, dates,
//!    times, ranges, fractions/ratios (warned), units;
//! 4. plain cardinals/decimals, only when the locale parse is unambiguous.
//!
//! Every matcher requires the *whole* core to match. Anything else is left
//! byte-for-byte as written. Ambiguous shapes are preserved and reported.

use super::lexer::{Field, is_protected_identifier, scan_fields};
use super::rules::{
    AbbrevContext, Amount, Conventions, Currency, DateOrder, LanguageRules, Meridiem,
    OrdinalMarker, UNIT_FORMS, Unit, unit_from_str, valid_date,
};
use super::{
    ChangeKind, Confidence, NormalizationWarning, NormalizeOptions, TextChange, WarningCode,
};

/// Largest integer read as a number (15 digits).
pub(crate) const MAX_INTEGER: u64 = 999_999_999_999_999;
/// Maximum fractional digits read.
pub(crate) const MAX_FRACTION_DIGITS: usize = 12;

/// Result of parsing a numeric core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NumParse {
    Value(Amount),
    /// Number-shaped, but the locale reading is not provable.
    Ambiguous,
    /// Valid shape beyond [`MAX_INTEGER`] or [`MAX_FRACTION_DIGITS`].
    TooLarge,
    /// Not a plain number (identifier, version, IP, ...).
    NotNumber,
}

fn strip_sign(raw: &str) -> (bool, &str) {
    if let Some(rest) = raw.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = raw.strip_prefix('\u{2212}') {
        (true, rest)
    } else {
        (false, raw)
    }
}

fn valid_first_group(group: &str) -> bool {
    (1..=3).contains(&group.len()) && !group.starts_with('0')
}

fn grouped(groups: &[&str]) -> bool {
    groups.len() >= 2 && valid_first_group(groups[0]) && groups[1..].iter().all(|g| g.len() == 3)
}

fn integer(negative: bool, digits: &str) -> NumParse {
    if digits.len() > 15 {
        return NumParse::TooLarge;
    }
    match digits.parse::<u64>() {
        Ok(int) if int <= MAX_INTEGER => NumParse::Value(Amount {
            negative,
            int,
            frac: None,
        }),
        _ => NumParse::TooLarge,
    }
}

fn decimal(negative: bool, int_digits: &str, frac: &str) -> NumParse {
    if frac.len() > MAX_FRACTION_DIGITS {
        return NumParse::TooLarge;
    }
    match integer(negative, int_digits) {
        NumParse::Value(mut amount) => {
            amount.frac = Some(frac.to_string());
            NumParse::Value(amount)
        }
        other => other,
    }
}

/// Parse a plain number written with the locale's separators.
///
/// `currency_ctx`: a single separator followed by exactly three digits is a
/// thousands group when it is the locale's group separator (a currency
/// subunit has two digits).
pub(crate) fn parse_number(raw: &str, conv: &Conventions, currency_ctx: bool) -> NumParse {
    let (negative, body) = strip_sign(raw);
    let bytes = body.as_bytes();
    if bytes.is_empty()
        || !bytes
            .iter()
            .all(|b| b.is_ascii_digit() || *b == b'.' || *b == b',')
        || !bytes[0].is_ascii_digit()
        || !bytes[bytes.len() - 1].is_ascii_digit()
    {
        return NumParse::NotNumber;
    }
    let separators: Vec<char> = body.chars().filter(|c| *c == '.' || *c == ',').collect();
    if separators.len() > 8 {
        return NumParse::NotNumber;
    }
    let groups: Vec<&str> = body.split(['.', ',']).collect();
    if groups.iter().any(|g| g.is_empty()) {
        return NumParse::NotNumber;
    }
    match separators.len() {
        0 => {
            if body.len() > 1 && body.starts_with('0') {
                // Leading-zero sequences (007, postal codes) are IDs.
                return NumParse::Ambiguous;
            }
            integer(negative, body)
        }
        1 => {
            let (int_part, rest) = (groups[0], groups[1]);
            let sep = separators[0];
            if valid_first_group(int_part) && rest.len() == 3 {
                if sep == conv.group && (conv.separators_unambiguous || currency_ctx) {
                    return integer(negative, &format!("{int_part}{rest}"));
                }
                if sep == conv.decimal && conv.separators_unambiguous && !currency_ctx {
                    return decimal(negative, int_part, rest);
                }
                return NumParse::Ambiguous;
            }
            if sep == conv.decimal && (int_part == "0" || !int_part.starts_with('0')) {
                return decimal(negative, int_part, rest);
            }
            NumParse::Ambiguous
        }
        count => {
            let last = separators[count - 1];
            let head = &separators[..count - 1];
            if separators.iter().all(|c| *c == conv.group) {
                return if grouped(&groups) {
                    integer(negative, &groups.concat())
                } else {
                    NumParse::NotNumber
                };
            }
            let int_groups = &groups[..groups.len() - 1];
            let frac = groups[groups.len() - 1];
            if last == conv.decimal && head.iter().all(|c| *c == conv.group) {
                return if grouped(int_groups) {
                    decimal(negative, &int_groups.concat(), frac)
                } else {
                    NumParse::NotNumber
                };
            }
            // Other convention (`1,234.56` under es, `1.234.567` under en).
            if separators.iter().all(|c| *c == conv.decimal) {
                return if grouped(&groups) {
                    NumParse::Ambiguous
                } else {
                    NumParse::NotNumber
                };
            }
            if last == conv.group && head.iter().all(|c| *c == conv.decimal) && grouped(int_groups)
            {
                return NumParse::Ambiguous;
            }
            NumParse::NotNumber
        }
    }
}

fn all_digits(raw: &str) -> bool {
    !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit())
}

fn plain_integer(raw: &str) -> Option<u64> {
    if !all_digits(raw) || raw.len() > 15 || (raw.len() > 1 && raw.starts_with('0')) {
        return None;
    }
    raw.parse().ok().filter(|n| *n <= MAX_INTEGER)
}

fn is_vulgar_fraction(c: char) -> bool {
    matches!(c, '\u{00BC}'..='\u{00BE}' | '\u{2150}'..='\u{215F}' | '\u{2189}')
}

/// Written currency marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Symbol {
    Known(Currency),
    /// `$`: US dollars only where the locale says so.
    Dollar,
    /// Symbols shared by several currencies (`¥`).
    Ambiguous,
}

fn symbol(raw: &str) -> Option<Symbol> {
    Some(match raw {
        "S/" | "S/." | "PEN" => Symbol::Known(Currency::Pen),
        "€" | "EUR" => Symbol::Known(Currency::Eur),
        "£" | "GBP" => Symbol::Known(Currency::Gbp),
        "US$" | "USD" => Symbol::Known(Currency::Usd),
        "R$" | "BRL" => Symbol::Known(Currency::Brl),
        "JPY" => Symbol::Known(Currency::Jpy),
        "$" => Symbol::Dollar,
        "¥" => Symbol::Ambiguous,
        _ => return None,
    })
}

/// Prefix symbols attached to the amount, longest first.
const ATTACHED_PREFIXES: &[&str] = &["S/.", "S/", "US$", "R$", "$", "€", "£", "¥"];
/// Suffix symbols attached to the amount.
const ATTACHED_SUFFIXES: &[&str] = &["€", "£", "$", "¥"];

/// Words after an amount that make a currency reading unsafe
/// (`$1.5 million`).
const SCALE_WORDS: &[&str] = &[
    "thousand",
    "million",
    "millions",
    "billion",
    "billions",
    "trillion",
    "mil",
    "millón",
    "millones",
    "millardo",
    "millardos",
    "billón",
    "billones",
];

const MERIDIEMS: &[(&str, Meridiem)] = &[
    ("a.m", Meridiem::Am),
    ("p.m", Meridiem::Pm),
    ("A.M", Meridiem::Am),
    ("P.M", Meridiem::Pm),
    ("am", Meridiem::Am),
    ("pm", Meridiem::Pm),
    ("AM", Meridiem::Am),
    ("PM", Meridiem::Pm),
];

struct Classifier<'a> {
    text: &'a str,
    rules: &'a dyn LanguageRules,
    conv: Conventions,
    locale: &'a str,
    locale_explicit: bool,
    fields: Vec<Field>,
    changes: Vec<TextChange>,
    warnings: Vec<NormalizationWarning>,
}

/// Classify `text` for the options' language and locale.
pub(crate) fn classify(
    text: &str,
    rules: &dyn LanguageRules,
    opts: &NormalizeOptions,
) -> (Vec<TextChange>, Vec<NormalizationWarning>) {
    let Some(conv) = rules.conventions(&opts.locale) else {
        return (Vec::new(), Vec::new());
    };
    let mut classifier = Classifier {
        text,
        rules,
        conv,
        locale: &opts.locale,
        locale_explicit: opts.locale_explicit,
        fields: scan_fields(text),
        changes: Vec::new(),
        warnings: Vec::new(),
    };
    let mut index = 0;
    while index < classifier.fields.len() {
        let next = classifier.field(index);
        index = next.max(index + 1);
    }
    (classifier.changes, classifier.warnings)
}

impl<'a> Classifier<'a> {
    fn core(&self, index: usize) -> &'a str {
        let field = self.fields[index];
        &self.text[field.core_start..field.core_end]
    }

    fn trail(&self, index: usize) -> &'a str {
        let field = self.fields[index];
        &self.text[field.core_end..field.end]
    }

    /// Next field joined to `index` by a single (possibly non-breaking)
    /// space with no punctuation in between.
    fn joinable(&self, index: usize) -> Option<usize> {
        let current = self.fields[index];
        let next_index = index + 1;
        let next = self.fields.get(next_index)?;
        let gap = &self.text[current.end..next.start];
        (current.bare_end()
            && next.bare_start()
            && !next.protected
            && next.has_core()
            && matches!(gap, " " | "\u{a0}" | "\u{202f}"))
        .then_some(next_index)
    }

    /// Word following `index` on the same line with no punctuation between.
    fn next_word(&self, index: usize) -> Option<&'a str> {
        let current = self.fields[index];
        let next = self.fields.get(index + 1)?;
        let gap = &self.text[current.end..next.start];
        (current.bare_end() && next.bare_start() && next.has_core() && !gap.contains('\n'))
            .then(|| self.core(index + 1))
    }

    /// Whether a period at byte `dot` (inside field `index`) continues the
    /// sentence: followed by `, ; : )`, or by a lowercase word or digit on
    /// the same line.
    fn period_continues(&self, index: usize, dot: usize) -> bool {
        let field = self.fields[index];
        let after = &self.text[dot + 1..field.end];
        if let Some(c) = after.chars().next() {
            return matches!(c, ',' | ';' | ':' | ')');
        }
        let Some(next) = self.fields.get(index + 1) else {
            return false;
        };
        let gap = &self.text[field.end..next.start];
        !gap.contains('\n')
            && self.text[next.start..]
                .chars()
                .next()
                .is_some_and(|c| c.is_lowercase() || c.is_ascii_digit())
    }

    fn change(&mut self, kind: ChangeKind, start: usize, end: usize, replacement: String) {
        let confidence = match kind {
            ChangeKind::Time | ChangeKind::Abbreviation => Confidence::Medium,
            _ => Confidence::High,
        };
        self.changes.push(TextChange {
            kind,
            start,
            end,
            replacement,
            confidence,
        });
    }

    fn warn(&mut self, code: WarningCode, start: usize, end: usize) {
        self.warnings
            .push(NormalizationWarning { code, start, end });
    }

    /// Classify field `index`; returns the index of the next unconsumed
    /// field.
    fn field(&mut self, index: usize) -> usize {
        let field = self.fields[index];
        if field.protected || !field.has_core() {
            return index + 1;
        }
        let core = self.core(index);
        if let Some(next) = self.abbreviation(index) {
            return next;
        }
        if let Some(next) = self.currency_symbol_field(index) {
            return next;
        }
        if core.chars().any(is_vulgar_fraction) {
            let (_, body) = strip_sign(core);
            if body
                .chars()
                .all(|c| c.is_ascii_digit() || is_vulgar_fraction(c))
            {
                self.warn(
                    WarningCode::AmbiguousFraction,
                    field.core_start,
                    field.core_end,
                );
            }
            return index + 1;
        }
        if !core.bytes().any(|b| b.is_ascii_digit()) || is_protected_identifier(core) {
            return index + 1;
        }
        if let Some(next) = self.digit_run(index) {
            return next;
        }
        self.numeric(index)
    }

    fn abbreviation(&mut self, index: usize) -> Option<usize> {
        let field = self.fields[index];
        let core = self.core(index);
        if !self.trail(index).starts_with('.') {
            return None;
        }
        let (expansion, context) = self.rules.abbreviation(core)?;
        let dot = field.core_end;
        let after_dot_empty = dot + 1 == field.end;
        let next_first = self.fields.get(index + 1).and_then(|next| {
            let gap = &self.text[field.end..next.start];
            (!gap.contains('\n') && next.bare_start())
                .then(|| self.text[next.start..].chars().next())
                .flatten()
        });
        let ok = match context {
            AbbrevContext::CapitalizedWord => {
                after_dot_empty && next_first.is_some_and(char::is_uppercase)
            }
            AbbrevContext::CapitalizedWordOrNumber => {
                after_dot_empty
                    && next_first.is_some_and(|c| c.is_uppercase() || c.is_ascii_digit())
            }
            AbbrevContext::NotSentenceEnd => self.period_continues(index, dot),
        };
        if !ok {
            return None;
        }
        self.change(
            ChangeKind::Abbreviation,
            field.core_start,
            dot + 1,
            expansion.to_string(),
        );
        Some(index + 1)
    }

    /// Phone-like runs: two or more space-separated pure digit groups
    /// (`987 654 321`, `+51 987 654 321`) are protected.
    fn digit_run(&mut self, index: usize) -> Option<usize> {
        let core = self.core(index);
        let digits = core.strip_prefix('+').unwrap_or(core);
        if !all_digits(digits) {
            return None;
        }
        let mut last = index;
        while let Some(next) = self.joinable(last) {
            if !all_digits(self.core(next)) {
                break;
            }
            last = next;
        }
        (last > index).then_some(last + 1)
    }

    /// Standalone symbol field followed by an amount (`S/ 150,50`,
    /// `USD 12`).
    fn currency_symbol_field(&mut self, index: usize) -> Option<usize> {
        let field = self.fields[index];
        let core = self.core(index);
        let trail = self.trail(index);
        let (sym, joined_ok) = if core == "S/" && trail == "." {
            (symbol("S/."), true)
        } else {
            (symbol(core), field.bare_end())
        };
        let sym = sym?;
        if !joined_ok {
            return None;
        }
        let next_index = index + 1;
        let next = *self.fields.get(next_index)?;
        let gap = &self.text[field.end..next.start];
        if !matches!(gap, " " | "\u{a0}" | "\u{202f}") || !next.bare_start() || next.protected {
            return None;
        }
        let amount_raw = self.core(next_index);
        self.money(sym, amount_raw, field.core_start, next.core_end, next_index)
    }

    /// Shared currency handling. Returns `None` when `amount_raw` is not
    /// number-shaped (no match), otherwise consumes through `last_index`.
    fn money(
        &mut self,
        sym: Symbol,
        amount_raw: &str,
        start: usize,
        end: usize,
        last_index: usize,
    ) -> Option<usize> {
        let parsed = parse_number(amount_raw, &self.conv, true);
        if parsed == NumParse::NotNumber {
            return None;
        }
        let consumed = Some(last_index + 1);
        let currency = match sym {
            Symbol::Known(currency) => currency,
            Symbol::Dollar => match self.conv.dollar {
                Some(currency) => currency,
                None => {
                    self.warn(WarningCode::AmbiguousCurrency, start, end);
                    return consumed;
                }
            },
            Symbol::Ambiguous => {
                self.warn(WarningCode::AmbiguousCurrency, start, end);
                return consumed;
            }
        };
        let amount = match parsed {
            NumParse::Value(amount) => amount,
            NumParse::TooLarge => {
                self.warn(WarningCode::NumberTooLarge, start, end);
                return consumed;
            }
            _ => {
                self.warn(WarningCode::AmbiguousNumber, start, end);
                return consumed;
            }
        };
        let followed_by_scale = self
            .next_word(last_index)
            .is_some_and(|w| SCALE_WORDS.contains(&w.to_lowercase().as_str()));
        if followed_by_scale {
            self.warn(WarningCode::AmbiguousCurrency, start, end);
            return consumed;
        }
        let cents = match &amount.frac {
            None => None,
            Some(frac) if frac.len() == 2 && currency.has_subunit() => frac.parse::<u32>().ok(),
            Some(_) => {
                self.warn(WarningCode::AmbiguousNumber, start, end);
                return consumed;
            }
        };
        if amount.negative {
            return consumed;
        }
        if let Some(spoken) = self
            .rules
            .currency(currency, amount.int, cents, self.locale)
        {
            self.change(ChangeKind::Currency, start, end, spoken);
        }
        consumed
    }

    fn numeric(&mut self, index: usize) -> usize {
        let field = self.fields[index];
        let core = self.core(index);
        let (start, end) = (field.core_start, field.core_end);
        let done = index + 1;

        // Attached currency prefix / suffix.
        for prefix in ATTACHED_PREFIXES {
            if let Some(rest) = core.strip_prefix(prefix)
                && rest.starts_with(|c: char| c.is_ascii_digit())
                && let Some(sym) = symbol(prefix)
            {
                return self.money(sym, rest, start, end, index).unwrap_or(done);
            }
        }
        for suffix in ATTACHED_SUFFIXES {
            if let Some(rest) = core.strip_suffix(suffix)
                && rest.ends_with(|c: char| c.is_ascii_digit())
                && let Some(sym) = symbol(suffix)
            {
                return self.money(sym, rest, start, end, index).unwrap_or(done);
            }
        }
        if let Some(rest) = core.strip_suffix('%') {
            self.percent(rest, start, end);
            return done;
        }
        if let Some(next) = self.ordinal(index) {
            return next;
        }
        for (form, unit) in UNIT_FORMS {
            if let Some(rest) = core.strip_suffix(form)
                && rest.ends_with(|c: char| c.is_ascii_digit())
            {
                self.quantity(rest, *unit, start, end);
                return done;
            }
        }
        if core.contains('/') {
            self.slash(core, start, end);
            return done;
        }
        if core.contains(':') {
            return self.clock(index);
        }
        if core.contains('–') {
            return self.range(index);
        }
        if core.matches('-').count() == 2 && !core.starts_with('-') {
            self.iso_date(core, start, end);
            return done;
        }
        self.plain_number(index)
    }

    fn percent(&mut self, raw: &str, start: usize, end: usize) {
        match parse_number(raw, &self.conv, false) {
            NumParse::Value(amount) => {
                if let Some(spoken) = self.rules.percent(&amount, self.locale) {
                    self.change(ChangeKind::Percentage, start, end, spoken);
                }
            }
            NumParse::Ambiguous => self.warn(WarningCode::AmbiguousNumber, start, end),
            NumParse::TooLarge => self.warn(WarningCode::NumberTooLarge, start, end),
            NumParse::NotNumber => {}
        }
    }

    fn quantity(&mut self, raw: &str, unit: Unit, start: usize, end: usize) {
        match parse_number(raw, &self.conv, false) {
            NumParse::Value(amount) => {
                if let Some(spoken) = self.rules.unit(&amount, unit, self.locale) {
                    self.change(ChangeKind::Unit, start, end, spoken);
                }
            }
            NumParse::Ambiguous => self.warn(WarningCode::AmbiguousNumber, start, end),
            NumParse::TooLarge => self.warn(WarningCode::NumberTooLarge, start, end),
            NumParse::NotNumber => {}
        }
    }

    fn ordinal(&mut self, index: usize) -> Option<usize> {
        let field = self.fields[index];
        let core = self.core(index);
        let digit_end = core.find(|c: char| !c.is_ascii_digit())?;
        let (digits, suffix) = core.split_at(digit_end);
        let marker = match suffix {
            "st" | "nd" | "rd" | "th" => OrdinalMarker::Suffix(suffix),
            "º" | ".º" => OrdinalMarker::Masculine,
            "ª" | ".ª" => OrdinalMarker::Feminine,
            _ => return None,
        };
        let n = plain_integer(digits)?;
        let next_word = self.next_word(index);
        if let Some(spoken) = self.rules.ordinal(n, marker, next_word, self.locale) {
            self.change(
                ChangeKind::Ordinal,
                field.core_start,
                field.core_end,
                spoken,
            );
        }
        Some(index + 1)
    }

    /// Slash forms: dates (`26/10/2026`) and fractions (`3/4`).
    fn slash(&mut self, core: &str, start: usize, end: usize) {
        let parts: Vec<&str> = core.split('/').collect();
        if !parts.iter().all(|p| all_digits(p)) {
            return;
        }
        match parts.as_slice() {
            [_, _] => self.warn(WarningCode::AmbiguousFraction, start, end),
            [a, b, year] if a.len() <= 2 && b.len() <= 2 => match year.len() {
                4 => self.slash_date(a, b, year, start, end),
                2 => self.warn(WarningCode::AmbiguousDate, start, end),
                _ => {}
            },
            _ => {}
        }
    }

    fn slash_date(&mut self, a: &str, b: &str, year: &str, start: usize, end: usize) {
        let (Ok(a), Ok(b), Ok(year)) = (a.parse::<u32>(), b.parse::<u32>(), year.parse::<u32>())
        else {
            return;
        };
        let day_month = valid_date(year, b, a);
        let month_day = valid_date(year, a, b);
        let (month, day) = match (day_month, month_day) {
            (false, false) => {
                self.warn(WarningCode::InvalidDate, start, end);
                return;
            }
            (true, false) => (b, a),
            (false, true) => (a, b),
            (true, true) if a == b => (a, a),
            (true, true) if self.locale_explicit => match self.conv.date_order {
                DateOrder::DayMonth => (b, a),
                DateOrder::MonthDay => (a, b),
            },
            (true, true) => {
                self.warn(WarningCode::AmbiguousDate, start, end);
                return;
            }
        };
        if let Some(spoken) = self.rules.date(year, month, day, self.locale) {
            self.change(ChangeKind::Date, start, end, spoken);
        }
    }

    /// ISO `YYYY-MM-DD`; other hyphenated digit groups are protected.
    fn iso_date(&mut self, core: &str, start: usize, end: usize) {
        let parts: Vec<&str> = core.split('-').collect();
        let [year, month, day] = parts.as_slice() else {
            return;
        };
        if !(year.len() == 4 && month.len() == 2 && day.len() == 2)
            || !parts.iter().all(|p| all_digits(p))
        {
            return;
        }
        let (Ok(year), Ok(month), Ok(day)) = (
            year.parse::<u32>(),
            month.parse::<u32>(),
            day.parse::<u32>(),
        ) else {
            return;
        };
        if !valid_date(year, month, day) {
            self.warn(WarningCode::InvalidDate, start, end);
            return;
        }
        if let Some(spoken) = self.rules.date(year, month, day, self.locale) {
            self.change(ChangeKind::Date, start, end, spoken);
        }
    }

    /// Clock times (`15:30`, `3:30 PM`) and ratios (`1:2`, warned).
    fn clock(&mut self, index: usize) -> usize {
        let field = self.fields[index];
        let core = self.core(index);
        let start = field.core_start;
        let mut end = field.core_end;
        let mut consumed = index + 1;

        let mut clock = core;
        let mut meridiem = None;
        for (form, value) in MERIDIEMS {
            if let Some(rest) = core.strip_suffix(form)
                && rest.ends_with(|c: char| c.is_ascii_digit())
            {
                clock = rest;
                meridiem = Some(*value);
                if form.contains('.')
                    && self.trail(index).starts_with('.')
                    && self.period_continues(index, field.core_end)
                {
                    end = field.core_end + 1;
                }
                break;
            }
        }
        let parts: Vec<&str> = clock.split(':').collect();
        if !parts.iter().all(|p| all_digits(p)) {
            return consumed;
        }
        match parts.as_slice() {
            [hour, minute] if minute.len() == 2 && hour.len() <= 2 => {
                if meridiem.is_none()
                    && let Some(next) = self.joinable(index)
                {
                    let next_core = self.core(next);
                    if let Some((form, value)) = MERIDIEMS.iter().find(|(f, _)| *f == next_core) {
                        meridiem = Some(*value);
                        let next_field = self.fields[next];
                        end = next_field.core_end;
                        if form.contains('.')
                            && self.trail(next).starts_with('.')
                            && self.period_continues(next, next_field.core_end)
                        {
                            end += 1;
                        }
                        consumed = next + 1;
                    }
                }
                let (Ok(h), Ok(m)) = (hour.parse::<u32>(), minute.parse::<u32>()) else {
                    return consumed;
                };
                match self.rules.time(h, m, meridiem, self.locale) {
                    Some(spoken) => self.change(ChangeKind::Time, start, end, spoken),
                    None => self.warn(WarningCode::AmbiguousTime, start, end),
                }
            }
            [_, _] if meridiem.is_none() => {
                self.warn(WarningCode::AmbiguousFraction, start, end);
            }
            [hour, minute, second] if hour.len() <= 2 && minute.len() == 2 && second.len() == 2 => {
                self.warn(WarningCode::AmbiguousTime, start, end);
            }
            _ => {}
        }
        consumed
    }

    /// Integer ranges with an en dash (`10–15`, `10–15 kg`).
    fn range(&mut self, index: usize) -> usize {
        let field = self.fields[index];
        let core = self.core(index);
        let parts: Vec<&str> = core.split('–').collect();
        let [from, to] = parts.as_slice() else {
            return index + 1;
        };
        let (Some(from), Some(to)) = (plain_integer(from), plain_integer(to)) else {
            return index + 1;
        };
        if from >= to {
            return index + 1;
        }
        let mut end = field.core_end;
        let mut consumed = index + 1;
        let mut unit = None;
        if let Some(next) = self.joinable(index)
            && let Some(found) = unit_from_str(self.core(next))
        {
            unit = Some(found);
            end = self.fields[next].core_end;
            consumed = next + 1;
        }
        if let Some(spoken) = self.rules.range(from, to, unit, self.locale) {
            self.change(ChangeKind::Range, field.core_start, end, spoken);
        }
        consumed
    }

    fn plain_number(&mut self, index: usize) -> usize {
        let field = self.fields[index];
        let core = self.core(index);
        let (start, end) = (field.core_start, field.core_end);
        let amount = match parse_number(core, &self.conv, false) {
            NumParse::Value(amount) => amount,
            NumParse::Ambiguous => {
                self.warn(WarningCode::AmbiguousNumber, start, end);
                return index + 1;
            }
            NumParse::TooLarge => {
                self.warn(WarningCode::NumberTooLarge, start, end);
                return index + 1;
            }
            NumParse::NotNumber => return index + 1,
        };
        if let Some(next) = self.joinable(index) {
            let next_core = self.core(next);
            let next_end = self.fields[next].core_end;
            if next_core == "%" {
                self.percent(core, start, next_end);
                return next + 1;
            }
            if let Some(sym) = symbol(next_core)
                && next_core != "S/"
                && next_core != "S/."
            {
                return self
                    .money(sym, core, start, next_end, next)
                    .unwrap_or(next + 1);
            }
            if let Some(unit) = unit_from_str(next_core) {
                if let Some(spoken) = self.rules.unit(&amount, unit, self.locale) {
                    self.change(ChangeKind::Unit, start, next_end, spoken);
                }
                return next + 1;
            }
        }
        if amount.frac.is_none()
            && let Some(word) = self.next_word(index)
            && self.rules.number_gender_ambiguous(amount.int, word)
        {
            self.warn(WarningCode::AmbiguousNumber, start, end);
            return index + 1;
        }
        let kind = if amount.frac.is_some() {
            ChangeKind::Decimal
        } else {
            ChangeKind::Number
        };
        if let Some(spoken) = self.rules.number(&amount, self.locale) {
            self.change(kind, start, end, spoken);
        }
        index + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ES: Conventions = Conventions {
        decimal: ',',
        group: '.',
        separators_unambiguous: false,
        date_order: DateOrder::DayMonth,
        dollar: None,
    };
    const EN: Conventions = Conventions {
        decimal: '.',
        group: ',',
        separators_unambiguous: true,
        date_order: DateOrder::MonthDay,
        dollar: Some(Currency::Usd),
    };

    fn value(negative: bool, int: u64, frac: Option<&str>) -> NumParse {
        NumParse::Value(Amount {
            negative,
            int,
            frac: frac.map(str::to_string),
        })
    }

    #[test]
    fn spanish_number_parsing() {
        assert_eq!(parse_number("1234", &ES, false), value(false, 1234, None));
        assert_eq!(parse_number("-5", &ES, false), value(true, 5, None));
        assert_eq!(parse_number("\u{2212}5", &ES, false), value(true, 5, None));
        assert_eq!(
            parse_number("12,5", &ES, false),
            value(false, 12, Some("5"))
        );
        assert_eq!(
            parse_number("1.234,56", &ES, false),
            value(false, 1234, Some("56"))
        );
        assert_eq!(
            parse_number("1.234.567", &ES, false),
            value(false, 1_234_567, None)
        );
        assert_eq!(parse_number("1.234", &ES, false), NumParse::Ambiguous);
        assert_eq!(parse_number("1,234", &ES, false), NumParse::Ambiguous);
        assert_eq!(parse_number("3.14", &ES, false), NumParse::Ambiguous);
        assert_eq!(parse_number("1,234.56", &ES, false), NumParse::Ambiguous);
        assert_eq!(parse_number("007", &ES, false), NumParse::Ambiguous);
        assert_eq!(parse_number("1.21.11", &ES, false), NumParse::NotNumber);
        assert_eq!(parse_number("10.0.0.1", &ES, false), NumParse::NotNumber);
        assert_eq!(parse_number("1.500", &ES, true), value(false, 1500, None));
        assert_eq!(parse_number("1,500", &ES, true), NumParse::Ambiguous);
        assert_eq!(parse_number("0,5", &ES, false), value(false, 0, Some("5")));
        assert_eq!(parse_number("1,", &ES, false), NumParse::NotNumber);
        assert_eq!(parse_number(",5", &ES, false), NumParse::NotNumber);
        assert_eq!(parse_number("1..2", &ES, false), NumParse::NotNumber);
    }

    #[test]
    fn english_number_parsing() {
        assert_eq!(parse_number("1,234", &EN, false), value(false, 1234, None));
        assert_eq!(
            parse_number("3.140", &EN, false),
            value(false, 3, Some("140"))
        );
        assert_eq!(
            parse_number("0.123", &EN, false),
            value(false, 0, Some("123"))
        );
        assert_eq!(
            parse_number("1.234", &EN, false),
            value(false, 1, Some("234"))
        );
        assert_eq!(parse_number("1.500", &EN, true), NumParse::Ambiguous);
        assert_eq!(parse_number("3,14", &EN, false), NumParse::Ambiguous);
        assert_eq!(parse_number("1.234,56", &EN, false), NumParse::Ambiguous);
        assert_eq!(
            parse_number("1,234,567.89", &EN, false),
            value(false, 1_234_567, Some("89"))
        );
    }

    #[test]
    fn number_limits() {
        assert_eq!(
            parse_number("999999999999999", &EN, false),
            value(false, MAX_INTEGER, None)
        );
        assert_eq!(
            parse_number("1000000000000000", &EN, false),
            NumParse::TooLarge
        );
        assert_eq!(
            parse_number(&"9".repeat(10_000), &EN, false),
            NumParse::TooLarge
        );
        assert_eq!(
            parse_number("1.1234567890123", &EN, false),
            NumParse::TooLarge
        );
        assert_eq!(
            parse_number("1.123456789012", &EN, false),
            value(false, 1, Some("123456789012"))
        );
    }
}
