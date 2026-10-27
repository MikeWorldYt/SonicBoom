//! French (`fr`; locale fr-FR) rules.
//!
//! Number format: decimal comma; thousands are grouped with a space or a
//! narrow no-break space (`1 234,50`). The lexer splits fields at every
//! space, so a space-grouped integer (`1 234`) is protected as a digit run
//! and preserved. A space-grouped decimal (`1 234,50 €`) cannot be joined:
//! the leading group is preserved with `ambiguous_number` and a decimal
//! amount whose integer part has exactly three digits (`234,50`) is
//! preserved, because it may be the tail of such a number. Dot grouping is
//! not French and is not recognized (`1.234` is ambiguous).
//!
//! Spelling: traditional hyphenation (`vingt et un`, `soixante-dix`,
//! `quatre-vingts`, `deux cents`, `deux cent mille`). `cent` and
//! `quatre-vingt` take a plural `s` only at the end of a number or before
//! `millions`/`milliards`/`billions`, never before `mille`. `et` only in
//! 21, 31, 41, 51, 61 and 71.
//!
//! Plural nouns from 2 upward (`1,5 kilomètre`, `zéro euro`, `2 euros`), as
//! French grammar requires. A bare integer ending in "un" before an unknown
//! noun (`1 maison`, `21 voitures`) is preserved as ambiguous (un/une).

use super::{
    AbbrevContext, Amount, Conventions, Currency, DateOrder, Feature, LanguageRules, Meridiem,
    OrdinalMarker, Unit,
};
use crate::tts::normalization::CoverageLevel;

/// French rule set.
pub(crate) struct French;

const SMALL: [&str; 17] = [
    "zéro", "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf", "dix", "onze",
    "douze", "treize", "quatorze", "quinze", "seize",
];

const TENS: [&str; 7] = [
    "",
    "",
    "vingt",
    "trente",
    "quarante",
    "cinquante",
    "soixante",
];

const MONTHS: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];

/// Function words after which a number keeps its standalone reading.
const STANDALONE_FOLLOWERS: &[&str] = &[
    "à", "au", "aux", "avec", "ce", "comme", "contre", "dans", "de", "des", "du", "en", "entre",
    "est", "et", "était", "la", "le", "les", "mais", "ni", "ou", "par", "pour", "que", "qui",
    "sans", "selon", "sont", "sur", "vers",
];

/// `true` when `word` starts with exactly three digits followed by a
/// non-digit: the second half of a space-grouped number (`1 234,50`).
fn looks_like_group_tail(word: &str) -> bool {
    let bytes = word.as_bytes();
    bytes.len() > 3 && bytes[..3].iter().all(u8::is_ascii_digit) && !bytes[3].is_ascii_digit()
}

/// A decimal amount with a three-digit integer part may be the tail of a
/// space-grouped number (`1 234,50`).
fn possible_group_tail(int: u64, has_fraction: bool) -> bool {
    has_fraction && (100..=999).contains(&int)
}

fn small(n: u64, feminine: bool) -> &'static str {
    if n == 1 && feminine {
        "une"
    } else {
        SMALL[n as usize]
    }
}

/// `plural_ok`: `quatre-vingts` keeps its `s` (false before `mille`).
fn below_100(n: u64, feminine: bool, plural_ok: bool) -> String {
    debug_assert!(n < 100);
    match n {
        0..=16 => small(n, feminine).to_string(),
        17..=19 => format!("dix-{}", SMALL[(n - 10) as usize]),
        71 => "soixante et onze".to_string(),
        70..=79 => format!("soixante-{}", below_100(n - 60, feminine, true)),
        80 if plural_ok => "quatre-vingts".to_string(),
        80 => "quatre-vingt".to_string(),
        81..=99 => format!("quatre-vingt-{}", below_100(n - 80, feminine, true)),
        _ => {
            let tens = TENS[(n / 10) as usize];
            match n % 10 {
                0 => tens.to_string(),
                1 => format!("{tens} et {}", small(1, feminine)),
                unit => format!("{tens}-{}", SMALL[unit as usize]),
            }
        }
    }
}

/// `plural_ok`: `cents` and `quatre-vingts` keep their `s`.
fn below_1000(n: u64, feminine: bool, plural_ok: bool) -> String {
    debug_assert!(n < 1000);
    let hundreds = n / 100;
    let rest = n % 100;
    if hundreds == 0 {
        return below_100(rest, feminine, plural_ok);
    }
    let mut out = if hundreds == 1 {
        "cent".to_string()
    } else {
        format!("{} cent", SMALL[hundreds as usize])
    };
    if rest == 0 {
        if hundreds > 1 && plural_ok {
            out.push('s');
        }
    } else {
        out.push(' ');
        out.push_str(&below_100(rest, feminine, plural_ok));
    }
    out
}

/// Exact French cardinal (long scale) for `n <= 999_999_999_999_999`.
fn cardinal(n: u64, feminine: bool) -> String {
    if n == 0 {
        return "zéro".to_string();
    }
    let mut parts: Vec<String> = Vec::with_capacity(5);
    let scales = [
        (1_000_000_000_000, "billion", "billions"),
        (1_000_000_000, "milliard", "milliards"),
        (1_000_000, "million", "millions"),
    ];
    for (size, singular, plural) in scales {
        let value = (n / size) % 1000;
        match value {
            0 => {}
            1 => parts.push(format!("un {singular}")),
            _ => parts.push(format!("{} {plural}", below_1000(value, false, true))),
        }
    }
    let thousands = (n / 1000) % 1000;
    match thousands {
        0 => {}
        1 => parts.push("mille".to_string()),
        _ => parts.push(format!("{} mille", below_1000(thousands, false, false))),
    }
    let units = n % 1000;
    if units > 0 {
        parts.push(below_1000(units, feminine, true));
    }
    parts.join(" ")
}

/// `true` when a noun after `n` is introduced by `de` (`un million d'euros`).
fn needs_de(n: u64) -> bool {
    n >= 1_000_000 && n.is_multiple_of(1_000_000)
}

/// `de` or elided `d'` before `noun`.
fn de(noun: &str) -> String {
    if noun.starts_with(['a', 'e', 'i', 'o', 'u', 'é', 'h']) {
        format!("d'{noun}")
    } else {
        format!("de {noun}")
    }
}

/// Fractional part: one digit, or two digits without a leading zero, read
/// as a number (`virgule quatorze`); anything else digit by digit
/// (`virgule zéro cinq`).
fn fraction_words(frac: &str) -> String {
    let bytes = frac.as_bytes();
    if bytes.len() == 1 || (bytes.len() == 2 && bytes[0] != b'0') {
        let value = frac.parse::<u64>().unwrap_or(0);
        return below_100(value, false, true);
    }
    bytes
        .iter()
        .map(|d| SMALL[usize::from(d - b'0')])
        .collect::<Vec<_>>()
        .join(" ")
}

fn amount_words(amount: &Amount, feminine: bool) -> String {
    let mut out = String::new();
    if amount.negative {
        out.push_str("moins ");
    }
    match &amount.frac {
        None => out.push_str(&cardinal(amount.int, feminine)),
        Some(frac) => {
            out.push_str(&cardinal(amount.int, false));
            out.push_str(" virgule ");
            out.push_str(&fraction_words(frac));
        }
    }
    out
}

/// Noun phrase after an amount: `de`/`d'` after exact millions, plural
/// from 2 upward.
fn counted(amount_text: String, int: u64, exact: bool, singular: &str, plural: &str) -> String {
    let noun = if int >= 2 { plural } else { singular };
    if exact && needs_de(int) {
        format!("{amount_text} {}", de(noun))
    } else {
        format!("{amount_text} {noun}")
    }
}

fn is_standalone_follower(word: &str) -> bool {
    let lower = word.to_lowercase();
    STANDALONE_FOLLOWERS.contains(&lower.as_str())
}

fn noun_like(next_word: &str) -> bool {
    next_word.chars().next().is_some_and(char::is_alphabetic) && !is_standalone_follower(next_word)
}

/// (singular, plural, feminine, subunit singular, subunit plural). An empty
/// subunit means amounts with cents are preserved.
type CurrencyNames = (&'static str, &'static str, bool, &'static str, &'static str);

fn currency_names(currency: Currency) -> Option<CurrencyNames> {
    Some(match currency {
        Currency::Eur => ("euro", "euros", false, "centime", "centimes"),
        Currency::Usd => ("dollar", "dollars", false, "cent", "cents"),
        Currency::Gbp => ("livre sterling", "livres sterling", true, "", ""),
        Currency::Jpy => ("yen", "yens", false, "", ""),
        // French names of the real and the sol are not verified.
        Currency::Brl | Currency::Pen => return None,
    })
}

fn unit_names(unit: Unit) -> (&'static str, &'static str) {
    match unit {
        Unit::Kilogram => ("kilogramme", "kilogrammes"),
        Unit::Gram => ("gramme", "grammes"),
        Unit::Kilometer => ("kilomètre", "kilomètres"),
        Unit::Meter => ("mètre", "mètres"),
        Unit::Centimeter => ("centimètre", "centimètres"),
        Unit::Millimeter => ("millimètre", "millimètres"),
        Unit::Liter => ("litre", "litres"),
        Unit::Milliliter => ("millilitre", "millilitres"),
        Unit::Celsius => ("degré Celsius", "degrés Celsius"),
        Unit::Fahrenheit => ("degré Fahrenheit", "degrés Fahrenheit"),
        Unit::KilometersPerHour => ("kilomètre par heure", "kilomètres par heure"),
        Unit::MetersPerSecond => ("mètre par seconde", "mètres par seconde"),
        Unit::Gigabyte => ("gigaoctet", "gigaoctets"),
        Unit::Megahertz => ("mégahertz", "mégahertz"),
        Unit::Millisecond => ("milliseconde", "millisecondes"),
    }
}

impl LanguageRules for French {
    fn coverage(&self, feature: Feature) -> CoverageLevel {
        match feature {
            Feature::CardinalNumbers
            | Feature::Decimals
            | Feature::Percentages
            | Feature::Currency
            | Feature::Dates
            | Feature::Times
            | Feature::Units
            | Feature::Abbreviations
            | Feature::Ranges => CoverageLevel::Partial,
            Feature::Ordinals | Feature::Fractions => CoverageLevel::None,
        }
    }

    fn conventions(&self, locale: &str) -> Option<Conventions> {
        (locale == "fr-FR").then_some(Conventions {
            decimal: ',',
            // French groups with spaces, which the lexer splits. No group
            // character is recognized inside a digit field.
            group: '\u{202f}',
            separators_unambiguous: false,
            date_order: DateOrder::DayMonth,
            dollar: None,
        })
    }

    fn number(&self, amount: &Amount, _locale: &str) -> Option<String> {
        if possible_group_tail(amount.int, amount.frac.is_some()) {
            return None;
        }
        Some(amount_words(amount, false))
    }

    fn number_gender_ambiguous(&self, int: u64, next_word: &str) -> bool {
        // `15 h 30` / `15 h` is a clock time the classifier does not
        // recognize; the hour is preserved. The minutes field cannot see
        // the preceding `h` and is still read as a number.
        if looks_like_group_tail(next_word) || next_word == "h" {
            return true;
        }
        if !noun_like(next_word) {
            return false;
        }
        let units = int % 1000;
        units % 10 == 1 && !matches!(units % 100, 11 | 71 | 91)
    }

    fn percent(&self, amount: &Amount, _locale: &str) -> Option<String> {
        if possible_group_tail(amount.int, amount.frac.is_some()) {
            return None;
        }
        Some(format!("{} pour cent", amount_words(amount, false)))
    }

    fn currency(
        &self,
        currency: Currency,
        int: u64,
        cents: Option<u32>,
        _locale: &str,
    ) -> Option<String> {
        if possible_group_tail(int, cents.is_some()) {
            return None;
        }
        let (singular, plural, feminine, sub_singular, sub_plural) = currency_names(currency)?;
        let main = || counted(cardinal(int, feminine), int, true, singular, plural);
        Some(match cents.filter(|c| *c > 0) {
            None => main(),
            Some(c) => {
                if sub_singular.is_empty() {
                    return None;
                }
                let c = u64::from(c);
                let sub = counted(cardinal(c, false), c, false, sub_singular, sub_plural);
                if int == 0 {
                    sub
                } else {
                    format!("{} et {sub}", main())
                }
            }
        })
    }

    fn date(&self, year: u32, month: u32, day: u32, _locale: &str) -> Option<String> {
        let month_name = MONTHS.get(month.checked_sub(1)? as usize)?;
        let day_words = if day == 1 {
            "premier".to_string()
        } else {
            cardinal(u64::from(day), false)
        };
        Some(format!(
            "{day_words} {month_name} {}",
            cardinal(u64::from(year), false)
        ))
    }

    fn time(
        &self,
        hour: u32,
        minute: u32,
        meridiem: Option<Meridiem>,
        _locale: &str,
    ) -> Option<String> {
        if meridiem.is_some() || hour > 23 || minute > 59 {
            return None;
        }
        // `heure` and `minute` are feminine: "une heure", "vingt et une".
        let hour = u64::from(hour);
        let mut out = format!(
            "{} {}",
            cardinal(hour, true),
            if hour >= 2 { "heures" } else { "heure" }
        );
        if minute > 0 {
            out.push(' ');
            out.push_str(&cardinal(u64::from(minute), true));
        }
        Some(out)
    }

    fn unit(&self, amount: &Amount, unit: Unit, _locale: &str) -> Option<String> {
        if possible_group_tail(amount.int, amount.frac.is_some()) {
            return None;
        }
        let (singular, plural) = unit_names(unit);
        Some(counted(
            amount_words(amount, false),
            amount.int,
            amount.frac.is_none(),
            singular,
            plural,
        ))
    }

    fn range(&self, from: u64, to: u64, unit: Option<Unit>, _locale: &str) -> Option<String> {
        let mut out = format!("{} à {}", cardinal(from, false), cardinal(to, false));
        if let Some(unit) = unit {
            let (singular, plural) = unit_names(unit);
            out.push(' ');
            out.push_str(if to >= 2 { plural } else { singular });
        }
        Some(out)
    }

    fn ordinal(
        &self,
        _n: u64,
        _marker: OrdinalMarker<'_>,
        _next_word: Option<&str>,
        _locale: &str,
    ) -> Option<String> {
        // French markers (`1er`, `2e`) are not recognized by the classifier,
        // and `2nd` may be English. Preserved.
        None
    }

    fn abbreviation(&self, word: &str) -> Option<(&'static str, AbbrevContext)> {
        match word {
            "Dr" => Some(("docteur", AbbrevContext::CapitalizedWord)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn amount(int: u64, frac: Option<&str>) -> Amount {
        Amount {
            negative: false,
            int,
            frac: frac.map(str::to_string),
        }
    }

    #[test]
    fn cardinals() {
        let cases: &[(u64, &str)] = &[
            (0, "zéro"),
            (1, "un"),
            (16, "seize"),
            (17, "dix-sept"),
            (21, "vingt et un"),
            (22, "vingt-deux"),
            (31, "trente et un"),
            (61, "soixante et un"),
            (70, "soixante-dix"),
            (71, "soixante et onze"),
            (72, "soixante-douze"),
            (77, "soixante-dix-sept"),
            (80, "quatre-vingts"),
            (81, "quatre-vingt-un"),
            (90, "quatre-vingt-dix"),
            (91, "quatre-vingt-onze"),
            (99, "quatre-vingt-dix-neuf"),
            (100, "cent"),
            (101, "cent un"),
            (200, "deux cents"),
            (201, "deux cent un"),
            (280, "deux cent quatre-vingts"),
            (1000, "mille"),
            (1001, "mille un"),
            (1234, "mille deux cent trente-quatre"),
            (2026, "deux mille vingt-six"),
            (80_000, "quatre-vingt mille"),
            (200_000, "deux cent mille"),
            (1_000_000, "un million"),
            (200_000_000, "deux cents millions"),
            (80_000_000, "quatre-vingts millions"),
            (1_000_000_000, "un milliard"),
            (1_000_000_000_000, "un billion"),
        ];
        for (n, expected) in cases {
            assert_eq!(cardinal(*n, false), *expected, "{n}");
        }
        assert_eq!(cardinal(21, true), "vingt et une");
        assert_eq!(cardinal(81, true), "quatre-vingt-une");
    }

    #[test]
    fn decimals_percent() {
        let rules = French;
        assert_eq!(
            rules.number(&amount(3, Some("14")), "fr-FR").unwrap(),
            "trois virgule quatorze"
        );
        assert_eq!(
            rules.number(&amount(3, Some("05")), "fr-FR").unwrap(),
            "trois virgule zéro cinq"
        );
        assert_eq!(
            rules.percent(&amount(12, Some("5")), "fr-FR").unwrap(),
            "douze virgule cinq pour cent"
        );
        // Possible tail of a space-grouped number.
        assert_eq!(rules.number(&amount(234, Some("5")), "fr-FR"), None);
        assert_eq!(rules.percent(&amount(234, Some("5")), "fr-FR"), None);
    }

    #[test]
    fn currency() {
        let rules = French;
        let cur = |c, i, cents| rules.currency(c, i, cents, "fr-FR");
        assert_eq!(
            cur(Currency::Eur, 12, Some(50)).unwrap(),
            "douze euros et cinquante centimes"
        );
        assert_eq!(cur(Currency::Eur, 1, None).unwrap(), "un euro");
        assert_eq!(cur(Currency::Eur, 21, None).unwrap(), "vingt et un euros");
        assert_eq!(
            cur(Currency::Eur, 1, Some(50)).unwrap(),
            "un euro et cinquante centimes"
        );
        assert_eq!(cur(Currency::Eur, 0, Some(1)).unwrap(), "un centime");
        assert_eq!(
            cur(Currency::Eur, 1_000_000, None).unwrap(),
            "un million d'euros"
        );
        assert_eq!(
            cur(Currency::Usd, 2_000_000, None).unwrap(),
            "deux millions de dollars"
        );
        assert_eq!(
            cur(Currency::Gbp, 21, None).unwrap(),
            "vingt et une livres sterling"
        );
        assert_eq!(cur(Currency::Gbp, 1, Some(5)), None);
        assert_eq!(cur(Currency::Brl, 1, None), None);
        assert_eq!(cur(Currency::Eur, 234, Some(50)), None);
    }

    #[test]
    fn dates_times() {
        let rules = French;
        assert_eq!(
            rules.date(2026, 10, 26, "fr-FR").unwrap(),
            "vingt-six octobre deux mille vingt-six"
        );
        assert_eq!(
            rules.date(2026, 5, 1, "fr-FR").unwrap(),
            "premier mai deux mille vingt-six"
        );
        assert_eq!(
            rules.time(15, 30, None, "fr-FR").unwrap(),
            "quinze heures trente"
        );
        assert_eq!(rules.time(1, 0, None, "fr-FR").unwrap(), "une heure");
        assert_eq!(
            rules.time(21, 1, None, "fr-FR").unwrap(),
            "vingt et une heures une"
        );
        assert_eq!(
            rules.time(0, 15, None, "fr-FR").unwrap(),
            "zéro heure quinze"
        );
        assert_eq!(rules.time(3, 30, Some(Meridiem::Pm), "fr-FR"), None);
        assert_eq!(rules.time(24, 0, None, "fr-FR"), None);
    }

    #[test]
    fn units_ranges() {
        let rules = French;
        assert_eq!(
            rules
                .unit(&amount(1, Some("5")), Unit::Kilometer, "fr-FR")
                .unwrap(),
            "un virgule cinq kilomètre"
        );
        assert_eq!(
            rules
                .unit(&amount(2, None), Unit::Kilometer, "fr-FR")
                .unwrap(),
            "deux kilomètres"
        );
        assert_eq!(
            rules
                .unit(&amount(25, None), Unit::Celsius, "fr-FR")
                .unwrap(),
            "vingt-cinq degrés Celsius"
        );
        assert_eq!(
            rules
                .unit(&amount(16, None), Unit::Gigabyte, "fr-FR")
                .unwrap(),
            "seize gigaoctets"
        );
        assert_eq!(
            rules.range(10, 15, Some(Unit::Kilogram), "fr-FR").unwrap(),
            "dix à quinze kilogrammes"
        );
    }

    #[test]
    fn ambiguity() {
        let rules = French;
        assert!(rules.number_gender_ambiguous(1, "maison"));
        assert!(rules.number_gender_ambiguous(21, "voitures"));
        assert!(rules.number_gender_ambiguous(81, "pages"));
        assert!(!rules.number_gender_ambiguous(71, "pages"));
        assert!(!rules.number_gender_ambiguous(11, "pages"));
        assert!(!rules.number_gender_ambiguous(1, "et"));
        assert!(!rules.number_gender_ambiguous(5, "pages"));
        assert!(rules.number_gender_ambiguous(1, "234,50"));
        assert!(rules.number_gender_ambiguous(15, "h"));
        assert!(!rules.number_gender_ambiguous(15, "heures"));
        assert_eq!(
            rules.ordinal(1, OrdinalMarker::Masculine, None, "fr-FR"),
            None
        );
    }
}
