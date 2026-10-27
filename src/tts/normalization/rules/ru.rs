//! Russian (`ru`; locale ru-RU) rules.
//!
//! Number format: decimal comma; thousands are grouped with a space or a
//! no-break space, which the lexer splits. A space-grouped integer
//! (`1 234`) is protected as a digit run. For a space-grouped decimal
//! (`1 234,5`) the leading group is preserved with `ambiguous_number` and a
//! decimal amount whose integer part has exactly three digits is
//! preserved. Dot grouping is not recognized and `1,234` (three decimals
//! or English grouping) is ambiguous.
//!
//! Russian numerals agree with the counted noun and inflect for case. The
//! rules only produce the nominative/accusative reading, and only where
//! it can be checked:
//! - a number before a word is converted only when the word is a form of a
//!   small verified noun set (рубль, доллар, евро, процент, год, километр,
//!   килограмм) in the form the number requires (`1 рубль`, `2 рубля`,
//!   `5 рублей`). Anything else is preserved with `ambiguous_number`.
//!   `год` is accepted only below 1000, because `2021 год` is an ordinal.
//! - percentages, money (euro, dollar, pound, yen) and kilometers and
//!   kilograms get the noun form the number requires.
//! - decimals use the formal reading with `целых` and `десятых`/`сотых`/
//!   `тысячных` (up to three fractional digits).
//!
//! Known limit: a genitive context (`до 5 рублей` = до пяти рублей) cannot
//! be seen by the rules, so it gets the nominative reading.
//!
//! Not converted: dates (the ordinal day changes with case:
//! `двадцать шестое` / `двадцать шестого`; dot dates have no classifier
//! hook), times, ordinals and ranges.

use super::{Amount, Conventions, Currency, DateOrder, Feature, LanguageRules, Meridiem, Unit};
use crate::tts::normalization::CoverageLevel;

/// Russian rule set.
pub(crate) struct Russian;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gender {
    Masculine,
    Feminine,
}

/// Noun form required after a number in the nominative/accusative.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Plural {
    /// 1, 21, 31, ... (not 11): nominative singular.
    One,
    /// 2-4, 22-24, ... (not 12-14): genitive singular.
    Few,
    /// Everything else: genitive plural.
    Many,
}

fn plural(n: u64) -> Plural {
    let last_two = n % 100;
    match n % 10 {
        1 if last_two != 11 => Plural::One,
        2..=4 if !(12..=14).contains(&last_two) => Plural::Few,
        _ => Plural::Many,
    }
}

/// Noun forms `[one, few, many]`.
type Forms = [&'static str; 3];

fn pick(forms: &Forms, n: u64) -> &'static str {
    match plural(n) {
        Plural::One => forms[0],
        Plural::Few => forms[1],
        Plural::Many => forms[2],
    }
}

const UNITS: [&str; 10] = [
    "ноль",
    "один",
    "два",
    "три",
    "четыре",
    "пять",
    "шесть",
    "семь",
    "восемь",
    "девять",
];

const TEENS: [&str; 10] = [
    "десять",
    "одиннадцать",
    "двенадцать",
    "тринадцать",
    "четырнадцать",
    "пятнадцать",
    "шестнадцать",
    "семнадцать",
    "восемнадцать",
    "девятнадцать",
];

const TENS: [&str; 10] = [
    "",
    "",
    "двадцать",
    "тридцать",
    "сорок",
    "пятьдесят",
    "шестьдесят",
    "семьдесят",
    "восемьдесят",
    "девяносто",
];

const HUNDREDS: [&str; 10] = [
    "",
    "сто",
    "двести",
    "триста",
    "четыреста",
    "пятьсот",
    "шестьсот",
    "семьсот",
    "восемьсот",
    "девятьсот",
];

const THOUSAND: Forms = ["тысяча", "тысячи", "тысяч"];
const MILLION: Forms = ["миллион", "миллиона", "миллионов"];
const BILLION: Forms = ["миллиард", "миллиарда", "миллиардов"];
const TRILLION: Forms = ["триллион", "триллиона", "триллионов"];

const RUBLE: Forms = ["рубль", "рубля", "рублей"];
const DOLLAR: Forms = ["доллар", "доллара", "долларов"];
const EURO: Forms = ["евро", "евро", "евро"];
const PERCENT: Forms = ["процент", "процента", "процентов"];
const YEAR: Forms = ["год", "года", "лет"];
const KILOMETER: Forms = ["километр", "километра", "километров"];
const KILOGRAM: Forms = ["килограмм", "килограмма", "килограммов"];
const CENT: Forms = ["цент", "цента", "центов"];
const POUND: Forms = ["фунт стерлингов", "фунта стерлингов", "фунтов стерлингов"];
const YEN: Forms = ["иена", "иены", "иен"];

/// `true` when `word` starts with exactly three digits followed by a
/// non-digit: the second half of a space-grouped number (`1 234,5`).
fn looks_like_group_tail(word: &str) -> bool {
    let bytes = word.as_bytes();
    bytes.len() > 3 && bytes[..3].iter().all(u8::is_ascii_digit) && !bytes[3].is_ascii_digit()
}

/// A decimal amount with a three-digit integer part may be the tail of a
/// space-grouped number (`1 234,5`).
fn possible_group_tail(int: u64, has_fraction: bool) -> bool {
    has_fraction && (100..=999).contains(&int)
}

fn below_1000(n: u64, gender: Gender, words: &mut Vec<&'static str>) {
    debug_assert!(n < 1000);
    let hundreds = (n / 100) as usize;
    let rest = n % 100;
    if hundreds > 0 {
        words.push(HUNDREDS[hundreds]);
    }
    match rest {
        0 => {}
        10..=19 => words.push(TEENS[(rest - 10) as usize]),
        _ => {
            if rest >= 20 {
                words.push(TENS[(rest / 10) as usize]);
            }
            let unit = rest % 10;
            if unit > 0 {
                words.push(match (unit, gender) {
                    (1, Gender::Feminine) => "одна",
                    (2, Gender::Feminine) => "две",
                    _ => UNITS[unit as usize],
                });
            }
        }
    }
}

/// Exact nominative cardinal for `n <= 999_999_999_999_999`.
fn cardinal(n: u64, gender: Gender) -> String {
    if n == 0 {
        return "ноль".to_string();
    }
    let mut words: Vec<&'static str> = Vec::with_capacity(12);
    let scales: [(u64, &Forms, Gender); 4] = [
        (1_000_000_000_000, &TRILLION, Gender::Masculine),
        (1_000_000_000, &BILLION, Gender::Masculine),
        (1_000_000, &MILLION, Gender::Masculine),
        (1000, &THOUSAND, Gender::Feminine),
    ];
    for (size, forms, scale_gender) in scales {
        let value = (n / size) % 1000;
        if value == 0 {
            continue;
        }
        // A leading "one thousand"/"one million" is read without "one".
        if !(value == 1 && words.is_empty()) {
            below_1000(value, scale_gender, &mut words);
        }
        words.push(pick(forms, value));
    }
    below_1000(n % 1000, gender, &mut words);
    words.join(" ")
}

/// Formal decimal reading: `три целых четырнадцать сотых`. `None` for more
/// than three fractional digits.
fn decimal_words(int: u64, frac: &str) -> Option<String> {
    let denominator = match frac.len() {
        1 => "десят",
        2 => "сот",
        3 => "тысячн",
        _ => return None,
    };
    let numerator = frac.parse::<u64>().ok()?;
    let whole = if plural(int) == Plural::One {
        "целая"
    } else {
        "целых"
    };
    let ending = if plural(numerator) == Plural::One {
        "ая"
    } else {
        "ых"
    };
    Some(format!(
        "{} {whole} {} {denominator}{ending}",
        cardinal(int, Gender::Feminine),
        cardinal(numerator, Gender::Feminine)
    ))
}

/// Number (with sign) before a noun with `forms`; decimals take the
/// genitive singular (`две целых пять десятых процента`).
fn counted(amount: &Amount, gender: Gender, forms: &Forms) -> Option<String> {
    let mut out = String::new();
    if amount.negative {
        out.push_str("минус ");
    }
    match &amount.frac {
        None => {
            out.push_str(&cardinal(amount.int, gender));
            out.push(' ');
            out.push_str(pick(forms, amount.int));
        }
        Some(frac) => {
            out.push_str(&decimal_words(amount.int, frac)?);
            out.push(' ');
            out.push_str(forms[1]);
        }
    }
    Some(out)
}

/// Forms of the verified (all masculine) noun `word` belongs to. `год` is
/// accepted only below 1000: `2021 год` is an ordinal year.
fn verified_noun(word: &str, int: u64) -> Option<Forms> {
    let nouns: [(Forms, bool); 7] = [
        (RUBLE, false),
        (DOLLAR, false),
        (EURO, false),
        (PERCENT, false),
        (YEAR, true),
        (KILOMETER, false),
        (KILOGRAM, false),
    ];
    nouns
        .into_iter()
        .find(|(forms, is_year)| (!is_year || int < 1000) && forms.contains(&word))
        .map(|(forms, _)| forms)
}

impl LanguageRules for Russian {
    fn coverage(&self, feature: Feature) -> CoverageLevel {
        match feature {
            Feature::CardinalNumbers
            | Feature::Decimals
            | Feature::Percentages
            | Feature::Currency
            | Feature::Units => CoverageLevel::Partial,
            Feature::Ordinals
            | Feature::Dates
            | Feature::Times
            | Feature::Abbreviations
            | Feature::Ranges
            | Feature::Fractions => CoverageLevel::None,
        }
    }

    fn conventions(&self, locale: &str) -> Option<Conventions> {
        (locale == "ru-RU").then_some(Conventions {
            decimal: ',',
            // Russian groups with spaces, which the lexer splits. No group
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
        let mut out = String::new();
        if amount.negative {
            out.push_str("минус ");
        }
        match &amount.frac {
            None => out.push_str(&cardinal(amount.int, Gender::Masculine)),
            Some(frac) => out.push_str(&decimal_words(amount.int, frac)?),
        }
        Some(out)
    }

    /// Before a word, the number is read only when the word is a verified
    /// noun in the form the number requires.
    fn number_gender_ambiguous(&self, int: u64, next_word: &str) -> bool {
        if looks_like_group_tail(next_word) {
            return true;
        }
        if !next_word.chars().next().is_some_and(char::is_alphabetic) {
            return false;
        }
        match verified_noun(next_word, int) {
            Some(forms) => pick(&forms, int) != next_word,
            None => true,
        }
    }

    fn percent(&self, amount: &Amount, _locale: &str) -> Option<String> {
        if possible_group_tail(amount.int, amount.frac.is_some()) {
            return None;
        }
        counted(amount, Gender::Masculine, &PERCENT)
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
        let (forms, gender, sub): (&Forms, Gender, Option<&Forms>) = match currency {
            Currency::Eur => (&EURO, Gender::Masculine, Some(&CENT)),
            Currency::Usd => (&DOLLAR, Gender::Masculine, Some(&CENT)),
            Currency::Gbp => (&POUND, Gender::Masculine, None),
            Currency::Jpy => (&YEN, Gender::Feminine, None),
            // Russian names of the real and the sol are not verified.
            Currency::Brl | Currency::Pen => return None,
        };
        let whole = |n: u64| Amount {
            negative: false,
            int: n,
            frac: None,
        };
        let main = counted(&whole(int), gender, forms)?;
        Some(match cents.filter(|c| *c > 0) {
            None => main,
            Some(c) => {
                let sub = counted(&whole(u64::from(c)), Gender::Masculine, sub?)?;
                if int == 0 {
                    sub
                } else {
                    format!("{main} {sub}")
                }
            }
        })
    }

    fn time(
        &self,
        _hour: u32,
        _minute: u32,
        _meridiem: Option<Meridiem>,
        _locale: &str,
    ) -> Option<String> {
        // The reading depends on case (`в 15:30` / `до 15:30`); preserved.
        None
    }

    fn unit(&self, amount: &Amount, unit: Unit, _locale: &str) -> Option<String> {
        if possible_group_tail(amount.int, amount.frac.is_some()) {
            return None;
        }
        let forms = match unit {
            Unit::Kilometer => &KILOMETER,
            Unit::Kilogram => &KILOGRAM,
            _ => return None,
        };
        counted(amount, Gender::Masculine, forms)
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
    fn plural_forms() {
        assert_eq!(plural(1), Plural::One);
        assert_eq!(plural(21), Plural::One);
        assert_eq!(plural(11), Plural::Many);
        assert_eq!(plural(2), Plural::Few);
        assert_eq!(plural(24), Plural::Few);
        assert_eq!(plural(12), Plural::Many);
        assert_eq!(plural(5), Plural::Many);
        assert_eq!(plural(0), Plural::Many);
        assert_eq!(plural(111), Plural::Many);
        assert_eq!(plural(101), Plural::One);
    }

    #[test]
    fn cardinals() {
        let cases: &[(u64, &str)] = &[
            (0, "ноль"),
            (1, "один"),
            (2, "два"),
            (11, "одиннадцать"),
            (21, "двадцать один"),
            (40, "сорок"),
            (100, "сто"),
            (101, "сто один"),
            (200, "двести"),
            (999, "девятьсот девяносто девять"),
            (1000, "тысяча"),
            (1001, "тысяча один"),
            (2000, "две тысячи"),
            (2026, "две тысячи двадцать шесть"),
            (5000, "пять тысяч"),
            (11_000, "одиннадцать тысяч"),
            (21_000, "двадцать одна тысяча"),
            (1_000_000, "миллион"),
            (2_001_000, "два миллиона одна тысяча"),
            (5_000_000, "пять миллионов"),
            (1_000_000_000, "миллиард"),
            (3_000_000_000_000, "три триллиона"),
        ];
        for (n, expected) in cases {
            assert_eq!(cardinal(*n, Gender::Masculine), *expected, "{n}");
        }
        assert_eq!(cardinal(2, Gender::Feminine), "две");
        assert_eq!(cardinal(21, Gender::Feminine), "двадцать одна");
    }

    #[test]
    fn decimals() {
        assert_eq!(
            decimal_words(3, "14").unwrap(),
            "три целых четырнадцать сотых"
        );
        assert_eq!(decimal_words(0, "5").unwrap(), "ноль целых пять десятых");
        assert_eq!(decimal_words(1, "1").unwrap(), "одна целая одна десятая");
        assert_eq!(decimal_words(2, "05").unwrap(), "две целых пять сотых");
        assert_eq!(
            decimal_words(12, "125").unwrap(),
            "двенадцать целых сто двадцать пять тысячных"
        );
        assert_eq!(decimal_words(1, "1234"), None);
        let rules = Russian;
        assert_eq!(rules.number(&amount(234, Some("5")), "ru-RU"), None);
    }

    #[test]
    fn agreement_with_verified_nouns() {
        let rules = Russian;
        assert!(!rules.number_gender_ambiguous(5, "рублей"));
        assert!(!rules.number_gender_ambiguous(1, "рубль"));
        assert!(!rules.number_gender_ambiguous(2, "рубля"));
        assert!(!rules.number_gender_ambiguous(21, "рубль"));
        assert!(!rules.number_gender_ambiguous(3, "евро"));
        assert!(!rules.number_gender_ambiguous(5, "лет"));
        // Wrong form for the number (another case): preserved.
        assert!(rules.number_gender_ambiguous(5, "рублями"));
        assert!(rules.number_gender_ambiguous(2, "рублей"));
        assert!(rules.number_gender_ambiguous(1, "рубля"));
        // Ordinal years and unknown nouns: preserved.
        assert!(rules.number_gender_ambiguous(2021, "год"));
        assert!(rules.number_gender_ambiguous(2, "книги"));
        assert!(rules.number_gender_ambiguous(5, "и"));
        assert!(!rules.number_gender_ambiguous(5, "—"));
        assert!(rules.number_gender_ambiguous(1, "234,5"));
    }

    #[test]
    fn percent_currency_units() {
        let rules = Russian;
        assert_eq!(
            rules.percent(&amount(5, None), "ru-RU").unwrap(),
            "пять процентов"
        );
        assert_eq!(
            rules.percent(&amount(21, None), "ru-RU").unwrap(),
            "двадцать один процент"
        );
        assert_eq!(
            rules.percent(&amount(12, Some("5")), "ru-RU").unwrap(),
            "двенадцать целых пять десятых процента"
        );
        let cur = |c, i, cents| rules.currency(c, i, cents, "ru-RU");
        assert_eq!(
            cur(Currency::Usd, 12, Some(50)).unwrap(),
            "двенадцать долларов пятьдесят центов"
        );
        assert_eq!(cur(Currency::Usd, 2, None).unwrap(), "два доллара");
        assert_eq!(
            cur(Currency::Eur, 1, Some(1)).unwrap(),
            "один евро один цент"
        );
        assert_eq!(cur(Currency::Jpy, 2, None).unwrap(), "две иены");
        assert_eq!(cur(Currency::Jpy, 5, Some(5)), None);
        assert_eq!(cur(Currency::Eur, 1000, None).unwrap(), "тысяча евро");
        assert_eq!(
            cur(Currency::Usd, 1_000_000, None).unwrap(),
            "миллион долларов"
        );
        assert_eq!(cur(Currency::Brl, 5, None), None);
        assert_eq!(
            rules
                .unit(&amount(2, Some("5")), Unit::Kilometer, "ru-RU")
                .unwrap(),
            "две целых пять десятых километра"
        );
        assert_eq!(
            rules
                .unit(&amount(3, None), Unit::Kilogram, "ru-RU")
                .unwrap(),
            "три килограмма"
        );
        assert_eq!(rules.unit(&amount(3, None), Unit::Meter, "ru-RU"), None);
        assert_eq!(rules.time(15, 30, None, "ru-RU"), None);
        assert_eq!(rules.date(2026, 10, 26, "ru-RU"), None);
    }
}
