//! Italian (`it`; locale it-IT) rules.
//!
//! Number format: decimal comma, dot grouping (`1.234,56`). Bare `1.234`,
//! `1,234` and dot decimals such as `3.14` (and the dotted clock form
//! `15.30`) are ambiguous and preserved by the classifier.
//!
//! Spelling: numbers below one million are one word (`duemilaventisei`).
//! Tens drop their final vowel before `uno` and `otto` (`ventuno`,
//! `ventotto`); `cento` drops it before `ottanta` (`centottanta`); a final
//! `tre` in a compound takes an accent (`ventitré`, `centotré`).
//! `milione` and `miliardo` are separate nouns (`un milione`, `due
//! milioni`); 10^12 is read `mille miliardi`.
//!
//! A bare integer ending in `uno` before an unknown noun (`1 casa`,
//! `21 case`) is preserved as ambiguous (un/uno/una).

use super::{
    AbbrevContext, Amount, Conventions, Currency, DateOrder, Feature, LanguageRules, Meridiem,
    OrdinalMarker, Unit,
};
use crate::tts::normalization::CoverageLevel;

/// Italian rule set.
pub(crate) struct Italian;

const SMALL: [&str; 20] = [
    "zero",
    "uno",
    "due",
    "tre",
    "quattro",
    "cinque",
    "sei",
    "sette",
    "otto",
    "nove",
    "dieci",
    "undici",
    "dodici",
    "tredici",
    "quattordici",
    "quindici",
    "sedici",
    "diciassette",
    "diciotto",
    "diciannove",
];

const TENS: [&str; 10] = [
    "",
    "",
    "venti",
    "trenta",
    "quaranta",
    "cinquanta",
    "sessanta",
    "settanta",
    "ottanta",
    "novanta",
];

const MONTHS: [&str; 12] = [
    "gennaio",
    "febbraio",
    "marzo",
    "aprile",
    "maggio",
    "giugno",
    "luglio",
    "agosto",
    "settembre",
    "ottobre",
    "novembre",
    "dicembre",
];

/// Ordinals 1..=10.
const ORD_SMALL: [&str; 11] = [
    "", "primo", "secondo", "terzo", "quarto", "quinto", "sesto", "settimo", "ottavo", "nono",
    "decimo",
];

/// Function words after which a number keeps its standalone reading.
const STANDALONE_FOLLOWERS: &[&str] = &[
    "a", "agli", "ai", "al", "alla", "alle", "che", "come", "con", "da", "dal", "dalla", "dalle",
    "dei", "del", "della", "delle", "di", "e", "è", "ed", "fra", "il", "in", "la", "le", "lo",
    "ma", "meno", "nel", "nella", "nelle", "o", "per", "più", "sono", "su", "sul", "tra",
];

/// `true` when `word` starts with exactly three digits followed by a
/// non-digit: the second half of a space-grouped number (`1 234,50`).
fn looks_like_group_tail(word: &str) -> bool {
    let bytes = word.as_bytes();
    bytes.len() > 3 && bytes[..3].iter().all(u8::is_ascii_digit) && !bytes[3].is_ascii_digit()
}

fn below_100(n: u64) -> String {
    debug_assert!(n < 100);
    if n < 20 {
        return SMALL[n as usize].to_string();
    }
    let tens = TENS[(n / 10) as usize];
    match n % 10 {
        0 => tens.to_string(),
        unit @ (1 | 8) => format!("{}{}", &tens[..tens.len() - 1], SMALL[unit as usize]),
        unit => format!("{tens}{}", SMALL[unit as usize]),
    }
}

fn below_1000(n: u64) -> String {
    debug_assert!(n < 1000);
    let hundreds = n / 100;
    let rest = n % 100;
    let head = match hundreds {
        0 => return below_100(rest),
        1 => "cento".to_string(),
        h => format!("{}cento", SMALL[h as usize]),
    };
    match rest {
        0 => head,
        80..=89 => format!("{}{}", &head[..head.len() - 1], below_100(rest)),
        _ => format!("{head}{}", below_100(rest)),
    }
}

/// A final `tre` in a compound word takes an accent (`ventitré`).
fn accent_final_tre(word: String, value: u64) -> String {
    if value > 3
        && let Some(stem) = word.strip_suffix("tre")
    {
        return format!("{stem}tré");
    }
    word
}

/// One word for `0 < n < 1_000_000`.
fn below_million(n: u64) -> String {
    let thousands = n / 1000;
    let units = n % 1000;
    let mut out = match thousands {
        0 => String::new(),
        1 => "mille".to_string(),
        t => format!("{}mila", below_1000(t)),
    };
    if units > 0 {
        out.push_str(&below_1000(units));
    }
    accent_final_tre(out, n)
}

/// Exact Italian cardinal for `n <= 999_999_999_999_999`.
fn cardinal(n: u64) -> String {
    if n == 0 {
        return "zero".to_string();
    }
    let mut parts: Vec<String> = Vec::with_capacity(3);
    let billions = n / 1_000_000_000;
    match billions {
        0 => {}
        1 => parts.push("un miliardo".to_string()),
        b => parts.push(format!("{} miliardi", below_million(b))),
    }
    let millions = (n / 1_000_000) % 1000;
    match millions {
        0 => {}
        1 => parts.push("un milione".to_string()),
        m => parts.push(format!("{} milioni", below_million(m))),
    }
    let low = n % 1_000_000;
    if low > 0 {
        parts.push(below_million(low));
    }
    parts.join(" ")
}

/// Cardinal before a noun: `un` / `una` for exactly one.
fn cardinal_before_noun(n: u64, feminine: bool) -> String {
    match (n, feminine) {
        (1, false) => "un".to_string(),
        (1, true) => "una".to_string(),
        _ => cardinal(n),
    }
}

/// `true` when a noun after `n` is introduced by `di` (`un milione di euro`).
fn needs_di(n: u64) -> bool {
    n >= 1_000_000 && n.is_multiple_of(1_000_000)
}

/// Fractional part: one digit, or two digits without a leading zero, read
/// as a number (`virgola quattordici`); anything else digit by digit
/// (`virgola zero cinque`).
fn fraction_words(frac: &str) -> String {
    let bytes = frac.as_bytes();
    if bytes.len() == 1 || (bytes.len() == 2 && bytes[0] != b'0') {
        let value = frac.parse::<u64>().unwrap_or(0);
        return accent_final_tre(below_100(value), value);
    }
    bytes
        .iter()
        .map(|d| SMALL[usize::from(d - b'0')])
        .collect::<Vec<_>>()
        .join(" ")
}

fn amount_words(amount: &Amount) -> String {
    let mut out = String::new();
    if amount.negative {
        out.push_str("meno ");
    }
    out.push_str(&cardinal(amount.int));
    if let Some(frac) = &amount.frac {
        out.push_str(" virgola ");
        out.push_str(&fraction_words(frac));
    }
    out
}

/// Masculine ordinal for 1..=100 (`undicesimo`, `ventitreesimo`).
fn ordinal_masculine(n: u64) -> Option<String> {
    match n {
        1..=10 => Some(ORD_SMALL[n as usize].to_string()),
        11..=100 => {
            let word = cardinal(n);
            let stem = match word.strip_suffix("tré") {
                Some(stem) => format!("{stem}tre"),
                None if word.ends_with("sei") => word,
                None => word[..word.len() - 1].to_string(),
            };
            Some(format!("{stem}esimo"))
        }
        _ => None,
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
        Currency::Eur => ("euro", "euro", false, "centesimo", "centesimi"),
        Currency::Usd => ("dollaro", "dollari", false, "centesimo", "centesimi"),
        Currency::Gbp => ("sterlina", "sterline", true, "", ""),
        Currency::Jpy => ("yen", "yen", false, "", ""),
        // Italian names of the real and the sol are not verified.
        Currency::Brl | Currency::Pen => return None,
    })
}

fn unit_names(unit: Unit) -> (&'static str, &'static str) {
    match unit {
        Unit::Kilogram => ("chilogrammo", "chilogrammi"),
        Unit::Gram => ("grammo", "grammi"),
        Unit::Kilometer => ("chilometro", "chilometri"),
        Unit::Meter => ("metro", "metri"),
        Unit::Centimeter => ("centimetro", "centimetri"),
        Unit::Millimeter => ("millimetro", "millimetri"),
        Unit::Liter => ("litro", "litri"),
        Unit::Milliliter => ("millilitro", "millilitri"),
        Unit::Celsius => ("grado Celsius", "gradi Celsius"),
        Unit::Fahrenheit => ("grado Fahrenheit", "gradi Fahrenheit"),
        Unit::KilometersPerHour => ("chilometro orario", "chilometri orari"),
        Unit::MetersPerSecond => ("metro al secondo", "metri al secondo"),
        Unit::Gigabyte => ("gigabyte", "gigabyte"),
        Unit::Megahertz => ("megahertz", "megahertz"),
        Unit::Millisecond => ("millisecondo", "millisecondi"),
    }
}

impl LanguageRules for Italian {
    fn coverage(&self, feature: Feature) -> CoverageLevel {
        match feature {
            Feature::CardinalNumbers | Feature::Percentages => CoverageLevel::Full,
            Feature::Decimals
            | Feature::Ordinals
            | Feature::Currency
            | Feature::Dates
            | Feature::Times
            | Feature::Units
            | Feature::Abbreviations => CoverageLevel::Partial,
            Feature::Ranges | Feature::Fractions => CoverageLevel::None,
        }
    }

    fn conventions(&self, locale: &str) -> Option<Conventions> {
        (locale == "it-IT").then_some(Conventions {
            decimal: ',',
            group: '.',
            separators_unambiguous: false,
            date_order: DateOrder::DayMonth,
            dollar: None,
        })
    }

    fn number(&self, amount: &Amount, _locale: &str) -> Option<String> {
        Some(amount_words(amount))
    }

    fn number_gender_ambiguous(&self, int: u64, next_word: &str) -> bool {
        // `15 h` is a clock time the classifier does not recognize; the
        // hour is preserved.
        if looks_like_group_tail(next_word) || next_word == "h" {
            return true;
        }
        noun_like(next_word) && int % 10 == 1 && int % 100 != 11
    }

    fn ordinal(
        &self,
        n: u64,
        marker: OrdinalMarker<'_>,
        _next_word: Option<&str>,
        _locale: &str,
    ) -> Option<String> {
        let masculine = ordinal_masculine(n)?;
        match marker {
            OrdinalMarker::Masculine => Some(masculine),
            OrdinalMarker::Feminine => {
                let stem = masculine.strip_suffix('o')?;
                Some(format!("{stem}a"))
            }
            OrdinalMarker::Suffix(_) => None,
        }
    }

    fn percent(&self, amount: &Amount, _locale: &str) -> Option<String> {
        Some(format!("{} per cento", amount_words(amount)))
    }

    fn currency(
        &self,
        currency: Currency,
        int: u64,
        cents: Option<u32>,
        _locale: &str,
    ) -> Option<String> {
        let (singular, plural, feminine, sub_singular, sub_plural) = currency_names(currency)?;
        let main = || {
            let mut out = cardinal_before_noun(int, feminine);
            if needs_di(int) {
                out.push_str(" di");
            }
            out.push(' ');
            out.push_str(if int == 1 { singular } else { plural });
            out
        };
        Some(match cents.filter(|c| *c > 0) {
            None => main(),
            Some(c) => {
                if sub_singular.is_empty() {
                    return None;
                }
                let sub = format!(
                    "{} {}",
                    cardinal_before_noun(u64::from(c), false),
                    if c == 1 { sub_singular } else { sub_plural }
                );
                if int == 0 {
                    sub
                } else {
                    format!("{} e {sub}", main())
                }
            }
        })
    }

    fn date(&self, year: u32, month: u32, day: u32, _locale: &str) -> Option<String> {
        let month_name = MONTHS.get(month.checked_sub(1)? as usize)?;
        let day_words = if day == 1 {
            "primo".to_string()
        } else {
            cardinal(u64::from(day))
        };
        Some(format!(
            "{day_words} {month_name} {}",
            cardinal(u64::from(year))
        ))
    }

    fn time(
        &self,
        hour: u32,
        minute: u32,
        meridiem: Option<Meridiem>,
        _locale: &str,
    ) -> Option<String> {
        if meridiem.is_some() || !(1..=23).contains(&hour) || minute > 59 {
            return None;
        }
        // "l'una", "le quindici e trenta".
        let hour_words = if hour == 1 {
            "una".to_string()
        } else {
            cardinal(u64::from(hour))
        };
        Some(match minute {
            0 => hour_words,
            _ => format!("{hour_words} e {}", cardinal(u64::from(minute))),
        })
    }

    fn unit(&self, amount: &Amount, unit: Unit, _locale: &str) -> Option<String> {
        let (singular, plural) = unit_names(unit);
        let mut out = String::new();
        if amount.negative {
            out.push_str("meno ");
        }
        match &amount.frac {
            None => {
                out.push_str(&cardinal_before_noun(amount.int, false));
                if needs_di(amount.int) {
                    out.push_str(" di");
                }
            }
            Some(_) => {
                let unsigned = Amount {
                    negative: false,
                    ..amount.clone()
                };
                out.push_str(&amount_words(&unsigned));
            }
        }
        out.push(' ');
        out.push_str(if amount.is_one() { singular } else { plural });
        Some(out)
    }

    fn abbreviation(&self, word: &str) -> Option<(&'static str, AbbrevContext)> {
        Some(match word {
            "Dott" => ("dottor", AbbrevContext::CapitalizedWord),
            "Sig" => ("signor", AbbrevContext::CapitalizedWord),
            _ => return None,
        })
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
            (0, "zero"),
            (1, "uno"),
            (17, "diciassette"),
            (20, "venti"),
            (21, "ventuno"),
            (23, "ventitré"),
            (28, "ventotto"),
            (31, "trentuno"),
            (99, "novantanove"),
            (100, "cento"),
            (101, "centouno"),
            (103, "centotré"),
            (180, "centottanta"),
            (200, "duecento"),
            (1000, "mille"),
            (1001, "milleuno"),
            (2000, "duemila"),
            (2003, "duemilatré"),
            (2026, "duemilaventisei"),
            (21_000, "ventunomila"),
            (23_000, "ventitremila"),
            (1_000_000, "un milione"),
            (1_200_000, "un milione duecentomila"),
            (3_000_000, "tre milioni"),
            (23_000_000, "ventitré milioni"),
            (1_000_000_000, "un miliardo"),
            (1_000_000_000_000, "mille miliardi"),
        ];
        for (n, expected) in cases {
            assert_eq!(cardinal(*n), *expected, "{n}");
        }
    }

    #[test]
    fn ordinals() {
        let rules = Italian;
        let ord = |n, m| rules.ordinal(n, m, None, "it-IT");
        assert_eq!(ord(1, OrdinalMarker::Masculine).unwrap(), "primo");
        assert_eq!(ord(1, OrdinalMarker::Feminine).unwrap(), "prima");
        assert_eq!(ord(10, OrdinalMarker::Masculine).unwrap(), "decimo");
        assert_eq!(ord(11, OrdinalMarker::Masculine).unwrap(), "undicesimo");
        assert_eq!(ord(20, OrdinalMarker::Masculine).unwrap(), "ventesimo");
        assert_eq!(ord(21, OrdinalMarker::Masculine).unwrap(), "ventunesimo");
        assert_eq!(ord(23, OrdinalMarker::Masculine).unwrap(), "ventitreesimo");
        assert_eq!(ord(26, OrdinalMarker::Masculine).unwrap(), "ventiseiesimo");
        assert_eq!(ord(100, OrdinalMarker::Feminine).unwrap(), "centesima");
        assert_eq!(ord(101, OrdinalMarker::Masculine), None);
        assert_eq!(ord(1, OrdinalMarker::Suffix("st")), None);
    }

    #[test]
    fn decimals_percent_currency() {
        let rules = Italian;
        assert_eq!(
            rules.number(&amount(3, Some("14")), "it-IT").unwrap(),
            "tre virgola quattordici"
        );
        assert_eq!(
            rules.number(&amount(3, Some("05")), "it-IT").unwrap(),
            "tre virgola zero cinque"
        );
        assert_eq!(
            rules.percent(&amount(12, Some("5")), "it-IT").unwrap(),
            "dodici virgola cinque per cento"
        );
        let cur = |c, i, cents| rules.currency(c, i, cents, "it-IT");
        assert_eq!(
            cur(Currency::Eur, 12, Some(50)).unwrap(),
            "dodici euro e cinquanta centesimi"
        );
        assert_eq!(cur(Currency::Eur, 1, None).unwrap(), "un euro");
        assert_eq!(cur(Currency::Eur, 0, Some(1)).unwrap(), "un centesimo");
        assert_eq!(
            cur(Currency::Eur, 1_000_000, None).unwrap(),
            "un milione di euro"
        );
        assert_eq!(cur(Currency::Usd, 2, None).unwrap(), "due dollari");
        assert_eq!(cur(Currency::Gbp, 1, None).unwrap(), "una sterlina");
        assert_eq!(cur(Currency::Gbp, 1, Some(20)), None);
        assert_eq!(cur(Currency::Brl, 1, None), None);
    }

    #[test]
    fn dates_times_units() {
        let rules = Italian;
        assert_eq!(
            rules.date(2026, 10, 21, "it-IT").unwrap(),
            "ventuno ottobre duemilaventisei"
        );
        assert_eq!(
            rules.date(2026, 5, 1, "it-IT").unwrap(),
            "primo maggio duemilaventisei"
        );
        assert_eq!(
            rules.time(15, 30, None, "it-IT").unwrap(),
            "quindici e trenta"
        );
        assert_eq!(rules.time(1, 0, None, "it-IT").unwrap(), "una");
        assert_eq!(rules.time(9, 5, None, "it-IT").unwrap(), "nove e cinque");
        assert_eq!(rules.time(0, 5, None, "it-IT"), None);
        assert_eq!(
            rules
                .unit(&amount(1, None), Unit::Kilometer, "it-IT")
                .unwrap(),
            "un chilometro"
        );
        assert_eq!(
            rules
                .unit(&amount(1, Some("5")), Unit::Liter, "it-IT")
                .unwrap(),
            "uno virgola cinque litri"
        );
        assert_eq!(
            rules
                .unit(&amount(100, None), Unit::KilometersPerHour, "it-IT")
                .unwrap(),
            "cento chilometri orari"
        );
        let negative = Amount {
            negative: true,
            int: 3,
            frac: None,
        };
        assert_eq!(
            rules.unit(&negative, Unit::Celsius, "it-IT").unwrap(),
            "meno tre gradi Celsius"
        );
    }

    #[test]
    fn ambiguity() {
        let rules = Italian;
        assert!(rules.number_gender_ambiguous(1, "casa"));
        assert!(rules.number_gender_ambiguous(21, "case"));
        assert!(!rules.number_gender_ambiguous(11, "case"));
        assert!(!rules.number_gender_ambiguous(1, "di"));
        assert!(!rules.number_gender_ambiguous(5, "case"));
        assert!(rules.number_gender_ambiguous(1, "234,50"));
    }
}
