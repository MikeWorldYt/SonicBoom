//! English (`en`; locales en-US, en-GB) rules.
//!
//! Number format: decimal point, comma grouping (`1,234.56`), read strictly
//! by convention: `1,234` is an integer and `3.140` is "three point one four
//! zero". Other-convention shapes (`3,14`, `1.234,56`, `1.234.567`) are
//! preserved as ambiguous. In money a dot followed by three digits
//! (`$1.500`) is ambiguous. en-GB inserts "and" after hundreds
//! (`one hundred and five`) and uses `-re` unit spellings.
//!
//! Dates: en-US "December thirty-first, twenty twenty-six"; en-GB "the
//! thirty-first of December, twenty twenty-six". A bare `$` is US dollars
//! only under en-US.

use super::{
    AbbrevContext, Amount, Conventions, Currency, DateOrder, Feature, LanguageRules, Meridiem,
    OrdinalMarker, Unit,
};
use crate::tts::normalization::CoverageLevel;

/// English rule set.
pub(crate) struct English;

const ONES: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];

const TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

const SCALES: [&str; 5] = ["", "thousand", "million", "billion", "trillion"];

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn british(locale: &str) -> bool {
    locale == "en-GB"
}

fn below_100(n: u64) -> String {
    debug_assert!(n < 100);
    if n < 20 {
        return ONES[n as usize].to_string();
    }
    let tens = TENS[(n / 10) as usize];
    match n % 10 {
        0 => tens.to_string(),
        unit => format!("{tens}-{}", ONES[unit as usize]),
    }
}

fn below_1000(n: u64, uk: bool) -> String {
    debug_assert!(n < 1000);
    let hundreds = n / 100;
    let rest = n % 100;
    match (hundreds, rest) {
        (0, _) => below_100(rest),
        (h, 0) => format!("{} hundred", ONES[h as usize]),
        (h, r) if uk => format!("{} hundred and {}", ONES[h as usize], below_100(r)),
        (h, r) => format!("{} hundred {}", ONES[h as usize], below_100(r)),
    }
}

/// Exact English cardinal (short scale) for `n <= 999_999_999_999_999`.
pub(crate) fn cardinal(n: u64, uk: bool) -> String {
    if n == 0 {
        return "zero".to_string();
    }
    let mut groups = Vec::with_capacity(5);
    let mut rest = n;
    while rest > 0 {
        groups.push(rest % 1000);
        rest /= 1000;
    }
    let mut parts: Vec<String> = Vec::with_capacity(groups.len() * 2);
    for (scale, group) in groups.iter().enumerate().rev() {
        if *group == 0 {
            continue;
        }
        if scale == 0 && uk && n >= 1000 && *group < 100 {
            parts.push("and".to_string());
        }
        parts.push(below_1000(*group, uk));
        if scale > 0 {
            parts.push(SCALES.get(scale).copied().unwrap_or_default().to_string());
        }
    }
    parts.join(" ")
}

/// Ordinal form of a cardinal reading (`twenty-one` -> `twenty-first`).
pub(crate) fn ordinalize(cardinal: &str) -> String {
    let split = cardinal.rfind([' ', '-']).map(|i| i + 1).unwrap_or(0);
    let (head, last) = cardinal.split_at(split);
    let last = match last {
        "one" => "first".to_string(),
        "two" => "second".to_string(),
        "three" => "third".to_string(),
        "five" => "fifth".to_string(),
        "eight" => "eighth".to_string(),
        "nine" => "ninth".to_string(),
        "twelve" => "twelfth".to_string(),
        word => match word.strip_suffix('y') {
            Some(stem) => format!("{stem}ieth"),
            None => format!("{word}th"),
        },
    };
    format!("{head}{last}")
}

fn expected_suffix(n: u64) -> &'static str {
    if (11..=13).contains(&(n % 100)) {
        return "th";
    }
    match n % 10 {
        1 => "st",
        2 => "nd",
        3 => "rd",
        _ => "th",
    }
}

fn amount_words(amount: &Amount, uk: bool) -> String {
    let mut out = String::new();
    if amount.negative {
        out.push_str("minus ");
    }
    out.push_str(&cardinal(amount.int, uk));
    if let Some(frac) = &amount.frac {
        out.push_str(" point");
        for digit in frac.bytes() {
            out.push(' ');
            out.push_str(ONES[usize::from(digit - b'0')]);
        }
    }
    out
}

/// Year reading: `2026` -> "twenty twenty-six", `2005` -> "two thousand
/// five", `1900` -> "nineteen hundred", `1905` -> "nineteen oh five".
fn year_words(year: u32, uk: bool) -> String {
    let year = u64::from(year);
    let high = year / 100;
    let low = year % 100;
    if year % 1000 == 0 || (high % 10 == 0 && low < 10) {
        return cardinal(year, uk);
    }
    match low {
        0 => format!("{} hundred", below_100(high)),
        1..=9 => format!("{} oh {}", below_100(high), ONES[low as usize]),
        _ => format!("{} {}", below_100(high), below_100(low)),
    }
}

fn currency_names(currency: Currency) -> (&'static str, &'static str, &'static str, &'static str) {
    match currency {
        Currency::Usd => ("dollar", "dollars", "cent", "cents"),
        Currency::Eur => ("euro", "euros", "cent", "cents"),
        Currency::Gbp => ("pound", "pounds", "penny", "pence"),
        Currency::Pen => ("sol", "soles", "centimo", "centimos"),
        Currency::Brl => ("real", "reais", "centavo", "centavos"),
        Currency::Jpy => ("yen", "yen", "", ""),
    }
}

fn unit_names(unit: Unit, uk: bool) -> (&'static str, &'static str) {
    match (unit, uk) {
        (Unit::Kilogram, _) => ("kilogram", "kilograms"),
        (Unit::Gram, _) => ("gram", "grams"),
        (Unit::Kilometer, false) => ("kilometer", "kilometers"),
        (Unit::Kilometer, true) => ("kilometre", "kilometres"),
        (Unit::Meter, false) => ("meter", "meters"),
        (Unit::Meter, true) => ("metre", "metres"),
        (Unit::Centimeter, false) => ("centimeter", "centimeters"),
        (Unit::Centimeter, true) => ("centimetre", "centimetres"),
        (Unit::Millimeter, false) => ("millimeter", "millimeters"),
        (Unit::Millimeter, true) => ("millimetre", "millimetres"),
        (Unit::Liter, false) => ("liter", "liters"),
        (Unit::Liter, true) => ("litre", "litres"),
        (Unit::Milliliter, false) => ("milliliter", "milliliters"),
        (Unit::Milliliter, true) => ("millilitre", "millilitres"),
        (Unit::Celsius, _) => ("degree Celsius", "degrees Celsius"),
        (Unit::Fahrenheit, _) => ("degree Fahrenheit", "degrees Fahrenheit"),
        (Unit::KilometersPerHour, false) => ("kilometer per hour", "kilometers per hour"),
        (Unit::KilometersPerHour, true) => ("kilometre per hour", "kilometres per hour"),
        (Unit::MetersPerSecond, false) => ("meter per second", "meters per second"),
        (Unit::MetersPerSecond, true) => ("metre per second", "metres per second"),
        (Unit::Gigabyte, _) => ("gigabyte", "gigabytes"),
        (Unit::Megahertz, _) => ("megahertz", "megahertz"),
        (Unit::Millisecond, _) => ("millisecond", "milliseconds"),
    }
}

impl LanguageRules for English {
    fn coverage(&self, feature: Feature) -> CoverageLevel {
        match feature {
            Feature::CardinalNumbers | Feature::Percentages => CoverageLevel::Full,
            Feature::Decimals
            | Feature::Ordinals
            | Feature::Currency
            | Feature::Dates
            | Feature::Times
            | Feature::Units
            | Feature::Abbreviations
            | Feature::Ranges => CoverageLevel::Partial,
            Feature::Fractions => CoverageLevel::None,
        }
    }

    fn conventions(&self, locale: &str) -> Option<Conventions> {
        let (date_order, dollar) = match locale {
            "en-US" => (DateOrder::MonthDay, Some(Currency::Usd)),
            "en-GB" => (DateOrder::DayMonth, None),
            _ => return None,
        };
        Some(Conventions {
            decimal: '.',
            group: ',',
            separators_unambiguous: true,
            date_order,
            dollar,
        })
    }

    fn number(&self, amount: &Amount, locale: &str) -> Option<String> {
        Some(amount_words(amount, british(locale)))
    }

    fn ordinal(
        &self,
        n: u64,
        marker: OrdinalMarker<'_>,
        _next_word: Option<&str>,
        locale: &str,
    ) -> Option<String> {
        match marker {
            OrdinalMarker::Suffix(suffix) if suffix == expected_suffix(n) => {
                Some(ordinalize(&cardinal(n, british(locale))))
            }
            _ => None,
        }
    }

    fn percent(&self, amount: &Amount, locale: &str) -> Option<String> {
        Some(format!("{} percent", amount_words(amount, british(locale))))
    }

    fn currency(
        &self,
        currency: Currency,
        int: u64,
        cents: Option<u32>,
        locale: &str,
    ) -> Option<String> {
        let uk = british(locale);
        let (singular, plural, sub_singular, sub_plural) = currency_names(currency);
        let main = format!(
            "{} {}",
            cardinal(int, uk),
            if int == 1 { singular } else { plural }
        );
        Some(match cents.filter(|c| *c > 0) {
            None => main,
            Some(c) => {
                if !currency.has_subunit() {
                    return None;
                }
                let sub = format!(
                    "{} {}",
                    cardinal(u64::from(c), uk),
                    if c == 1 { sub_singular } else { sub_plural }
                );
                if int == 0 {
                    sub
                } else {
                    format!("{main} and {sub}")
                }
            }
        })
    }

    fn date(&self, year: u32, month: u32, day: u32, locale: &str) -> Option<String> {
        let uk = british(locale);
        let month_name = MONTHS.get(month.checked_sub(1)? as usize)?;
        let day_words = ordinalize(&cardinal(u64::from(day), uk));
        let year_words = year_words(year, uk);
        Some(if uk {
            format!("the {day_words} of {month_name}, {year_words}")
        } else {
            format!("{month_name} {day_words}, {year_words}")
        })
    }

    fn time(
        &self,
        hour: u32,
        minute: u32,
        meridiem: Option<Meridiem>,
        locale: &str,
    ) -> Option<String> {
        let uk = british(locale);
        if minute > 59 {
            return None;
        }
        match meridiem {
            Some(_) if !(1..=12).contains(&hour) => return None,
            None if hour > 23 => return None,
            _ => {}
        }
        let hour_words = cardinal(u64::from(hour), uk);
        let mut out = match (minute, meridiem) {
            (0, Some(_)) => hour_words,
            (0, None) if (1..=12).contains(&hour) => format!("{hour_words} o'clock"),
            (0, None) => format!("{hour_words} hundred"),
            (1..=9, _) => format!("{hour_words} oh {}", ONES[minute as usize]),
            _ => format!("{hour_words} {}", below_100(u64::from(minute))),
        };
        match meridiem {
            Some(Meridiem::Am) => out.push_str(" A M"),
            Some(Meridiem::Pm) => out.push_str(" P M"),
            None => {}
        }
        Some(out)
    }

    fn unit(&self, amount: &Amount, unit: Unit, locale: &str) -> Option<String> {
        let uk = british(locale);
        let (singular, plural) = unit_names(unit, uk);
        Some(format!(
            "{} {}",
            amount_words(amount, uk),
            if amount.is_one() { singular } else { plural }
        ))
    }

    fn range(&self, from: u64, to: u64, unit: Option<Unit>, locale: &str) -> Option<String> {
        let uk = british(locale);
        let mut out = format!("{} to {}", cardinal(from, uk), cardinal(to, uk));
        if let Some(unit) = unit {
            let (singular, plural) = unit_names(unit, uk);
            out.push(' ');
            out.push_str(if to == 1 { singular } else { plural });
        }
        Some(out)
    }

    fn abbreviation(&self, word: &str) -> Option<(&'static str, AbbrevContext)> {
        Some(match word {
            "Dr" => ("doctor", AbbrevContext::CapitalizedWord),
            "Mr" => ("mister", AbbrevContext::CapitalizedWord),
            "etc" => ("etcetera", AbbrevContext::NotSentenceEnd),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cardinals() {
        let cases: &[(u64, &str)] = &[
            (0, "zero"),
            (7, "seven"),
            (13, "thirteen"),
            (21, "twenty-one"),
            (100, "one hundred"),
            (105, "one hundred five"),
            (1000, "one thousand"),
            (1234, "one thousand two hundred thirty-four"),
            (1_000_000, "one million"),
            (
                999_999_999_999_999,
                "nine hundred ninety-nine trillion nine hundred ninety-nine billion \
                 nine hundred ninety-nine million nine hundred ninety-nine thousand \
                 nine hundred ninety-nine",
            ),
        ];
        for (n, expected) in cases {
            assert_eq!(cardinal(*n, false), *expected, "{n}");
        }
        assert_eq!(cardinal(105, true), "one hundred and five");
        assert_eq!(cardinal(1005, true), "one thousand and five");
        assert_eq!(cardinal(1200, true), "one thousand two hundred");
    }

    #[test]
    fn ordinals() {
        assert_eq!(ordinalize("one"), "first");
        assert_eq!(ordinalize("twenty-one"), "twenty-first");
        assert_eq!(ordinalize("thirty"), "thirtieth");
        assert_eq!(ordinalize("one hundred twelve"), "one hundred twelfth");
        assert_eq!(ordinalize("four"), "fourth");
        let rules = English;
        let ord = |n, s| rules.ordinal(n, OrdinalMarker::Suffix(s), None, "en-US");
        assert_eq!(ord(1, "st").as_deref(), Some("first"));
        assert_eq!(ord(2, "nd").as_deref(), Some("second"));
        assert_eq!(ord(3, "rd").as_deref(), Some("third"));
        assert_eq!(ord(11, "th").as_deref(), Some("eleventh"));
        assert_eq!(ord(22, "nd").as_deref(), Some("twenty-second"));
        assert_eq!(ord(1, "th"), None);
        assert_eq!(ord(11, "st"), None);
    }

    #[test]
    fn years() {
        assert_eq!(year_words(2026, false), "twenty twenty-six");
        assert_eq!(year_words(2000, false), "two thousand");
        assert_eq!(year_words(2005, false), "two thousand five");
        assert_eq!(year_words(2010, false), "twenty ten");
        assert_eq!(year_words(1900, false), "nineteen hundred");
        assert_eq!(year_words(1905, false), "nineteen oh five");
        assert_eq!(year_words(1984, false), "nineteen eighty-four");
    }

    #[test]
    fn dates_times_currency() {
        let rules = English;
        assert_eq!(
            rules.date(2026, 12, 31, "en-US").unwrap(),
            "December thirty-first, twenty twenty-six"
        );
        assert_eq!(
            rules.date(2026, 12, 31, "en-GB").unwrap(),
            "the thirty-first of December, twenty twenty-six"
        );
        assert_eq!(rules.time(15, 30, None, "en-US").unwrap(), "fifteen thirty");
        assert_eq!(
            rules.time(3, 30, Some(Meridiem::Pm), "en-US").unwrap(),
            "three thirty P M"
        );
        assert_eq!(rules.time(9, 5, None, "en-US").unwrap(), "nine oh five");
        assert_eq!(rules.time(3, 0, None, "en-US").unwrap(), "three o'clock");
        assert_eq!(rules.time(15, 30, Some(Meridiem::Pm), "en-US"), None);
        assert_eq!(
            rules
                .currency(Currency::Usd, 12, Some(50), "en-US")
                .unwrap(),
            "twelve dollars and fifty cents"
        );
        assert_eq!(
            rules.currency(Currency::Usd, 1, None, "en-US").unwrap(),
            "one dollar"
        );
        assert_eq!(
            rules.currency(Currency::Gbp, 0, Some(1), "en-GB").unwrap(),
            "one penny"
        );
        assert_eq!(
            rules.currency(Currency::Gbp, 5, Some(20), "en-GB").unwrap(),
            "five pounds and twenty pence"
        );
    }

    #[test]
    fn decimals() {
        let amount = Amount {
            negative: false,
            int: 3,
            frac: Some("140".to_string()),
        };
        assert_eq!(amount_words(&amount, false), "three point one four zero");
    }
}
