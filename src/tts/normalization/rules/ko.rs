//! Korean (`ko`; locale ko-KR) rules.
//!
//! Number format: decimal point, comma grouping (`1,234.5`), read strictly
//! by convention. Cardinals are Sino-Korean: `십` (not `일십`), `백`, `천`,
//! `만` (not `일만`), but `일억`, `일조`. Following the spacing rule for
//! numbers written in Hangul, a space separates the 만/억/조 sections
//! (`만 이천`, `십이억 삼천사백오십육만 칠천팔백구십`).
//!
//! Native vs Sino-Korean: counters such as 개, 명, 시, 살, 번 take native
//! numerals (`세 개`, `세 시`), while dates, money, minutes and measures
//! take Sino-Korean ones. The rules therefore:
//! - read a number before a separate word only when the word is a
//!   Sino-Korean noun from a small list (원, 년, 일, 초, 층, 퍼센트, and
//!   metric units); any other Hangul or Han word is preserved with
//!   `ambiguous_number`;
//! - read clock hours natively (`세 시 삼십 분`) only for 1..=12; 0 and
//!   13..=23 (Sino `십오 시` or native `열다섯 시`) are preserved with
//!   `ambiguous_time`;
//! - read dates as `이천이십육 년 시월 이십육 일`, with the irregular month
//!   names `유월` (June) and `시월` (October).
//!
//! Limits: numbers attached to Hangul (`2026년`, `3개`, `10000원`) are not
//! whitespace-delimited and are preserved unchanged. There is no won
//! currency code in the classifier; `10000 원` (with a space) is read as
//! `만 원`. Amounts with cents are preserved. Numeric slash dates are not a
//! Korean convention; when both day/month readings are valid under an
//! explicit locale, month/day is used.

use super::{Amount, Conventions, Currency, DateOrder, Feature, LanguageRules, Meridiem, Unit};
use crate::tts::normalization::CoverageLevel;

/// Korean rule set.
pub(crate) struct Korean;

const DIGITS: [&str; 10] = ["영", "일", "이", "삼", "사", "오", "육", "칠", "팔", "구"];
const SECTION_UNITS: [&str; 4] = ["", "만", "억", "조"];

/// Native Korean numerals for clock hours 1..=12 (prenominal forms).
const NATIVE_HOURS: [&str; 13] = [
    "", "한", "두", "세", "네", "다섯", "여섯", "일곱", "여덟", "아홉", "열", "열한", "열두",
];

/// Sino-Korean nouns after which a number is read with Sino-Korean
/// numerals.
const SINO_NOUNS: &[&str] = &[
    "원",
    "년",
    "일",
    "초",
    "층",
    "퍼센트",
    "미터",
    "킬로미터",
    "센티미터",
    "밀리미터",
    "그램",
    "킬로그램",
    "리터",
    "밀리리터",
    "달러",
    "유로",
    "엔",
];

/// `true` when `word` starts with exactly three digits followed by a
/// non-digit: the second half of a space-grouped number.
fn looks_like_group_tail(word: &str) -> bool {
    let bytes = word.as_bytes();
    bytes.len() > 3 && bytes[..3].iter().all(u8::is_ascii_digit) && !bytes[3].is_ascii_digit()
}

/// Han, kana or Hangul letter (syllables and jamo).
fn is_cjk_letter(c: char) -> bool {
    matches!(c,
        '\u{1100}'..='\u{11FF}'
        | '\u{3040}'..='\u{30FF}'
        | '\u{3130}'..='\u{318F}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}'
        | '\u{AC00}'..='\u{D7AF}'
        | '\u{F900}'..='\u{FAFF}'
    )
}

/// One section `1..=9999`; `천`, `백`, `십` drop a leading `일`.
fn section(n: u64, out: &mut String) {
    let places = [
        (n / 1000, "천"),
        ((n / 100) % 10, "백"),
        ((n / 10) % 10, "십"),
    ];
    for (d, place) in places {
        match d {
            0 => {}
            1 => out.push_str(place),
            _ => {
                out.push_str(DIGITS[d as usize]);
                out.push_str(place);
            }
        }
    }
    let units = n % 10;
    if units > 0 {
        out.push_str(DIGITS[units as usize]);
    }
}

/// Exact Sino-Korean cardinal for `n <= 999_999_999_999_999`.
fn cardinal(n: u64) -> String {
    if n == 0 {
        return "영".to_string();
    }
    let sections = [
        n % 10_000,
        (n / 10_000) % 10_000,
        (n / 100_000_000) % 10_000,
        (n / 1_000_000_000_000) % 10_000,
    ];
    let mut parts: Vec<String> = Vec::with_capacity(4);
    for idx in (0..4).rev() {
        let value = sections[idx];
        if value == 0 {
            continue;
        }
        let mut part = String::new();
        // 만 alone, but 일억 and 일조.
        if !(idx == 1 && value == 1) {
            section(value, &mut part);
        }
        part.push_str(SECTION_UNITS[idx]);
        parts.push(part);
    }
    parts.join(" ")
}

fn unsigned_words(amount: &Amount) -> String {
    let mut out = cardinal(amount.int);
    if let Some(frac) = &amount.frac {
        out.push_str(" 점 ");
        for d in frac.bytes() {
            out.push_str(DIGITS[usize::from(d - b'0')]);
        }
    }
    out
}

fn amount_words(amount: &Amount) -> String {
    let words = unsigned_words(amount);
    if amount.negative {
        format!("마이너스 {words}")
    } else {
        words
    }
}

fn month_name(month: u32) -> Option<String> {
    Some(match month {
        6 => "유월".to_string(),
        10 => "시월".to_string(),
        1..=12 => format!("{}월", cardinal(u64::from(month))),
        _ => return None,
    })
}

impl LanguageRules for Korean {
    fn coverage(&self, feature: Feature) -> CoverageLevel {
        match feature {
            Feature::CardinalNumbers
            | Feature::Decimals
            | Feature::Percentages
            | Feature::Currency
            | Feature::Dates
            | Feature::Times
            | Feature::Units => CoverageLevel::Partial,
            Feature::Ordinals | Feature::Abbreviations | Feature::Ranges | Feature::Fractions => {
                CoverageLevel::None
            }
        }
    }

    fn conventions(&self, locale: &str) -> Option<Conventions> {
        (locale == "ko-KR").then_some(Conventions {
            decimal: '.',
            group: ',',
            separators_unambiguous: true,
            date_order: DateOrder::MonthDay,
            dollar: None,
        })
    }

    fn number(&self, amount: &Amount, _locale: &str) -> Option<String> {
        Some(amount_words(amount))
    }

    /// Before a Hangul or Han word the numeral system depends on the word;
    /// only verified Sino-Korean nouns are read.
    fn number_gender_ambiguous(&self, _int: u64, next_word: &str) -> bool {
        if looks_like_group_tail(next_word) {
            return true;
        }
        next_word.chars().next().is_some_and(is_cjk_letter) && !SINO_NOUNS.contains(&next_word)
    }

    fn percent(&self, amount: &Amount, _locale: &str) -> Option<String> {
        Some(format!("{} 퍼센트", amount_words(amount)))
    }

    fn currency(
        &self,
        currency: Currency,
        int: u64,
        cents: Option<u32>,
        _locale: &str,
    ) -> Option<String> {
        if cents.is_some_and(|c| c > 0) {
            return None;
        }
        let name = match currency {
            Currency::Usd => "달러",
            Currency::Eur => "유로",
            Currency::Gbp => "파운드",
            Currency::Jpy => "엔",
            Currency::Brl | Currency::Pen => return None,
        };
        Some(format!("{} {name}", cardinal(int)))
    }

    fn date(&self, year: u32, month: u32, day: u32, _locale: &str) -> Option<String> {
        if !(1..=31).contains(&day) {
            return None;
        }
        Some(format!(
            "{} 년 {} {} 일",
            cardinal(u64::from(year)),
            month_name(month)?,
            cardinal(u64::from(day))
        ))
    }

    fn time(
        &self,
        hour: u32,
        minute: u32,
        meridiem: Option<Meridiem>,
        _locale: &str,
    ) -> Option<String> {
        if meridiem.is_some() || !(1..=12).contains(&hour) || minute > 59 {
            return None;
        }
        let mut out = format!("{} 시", NATIVE_HOURS[hour as usize]);
        if minute > 0 {
            out.push(' ');
            out.push_str(&cardinal(u64::from(minute)));
            out.push_str(" 분");
        }
        Some(out)
    }

    fn unit(&self, amount: &Amount, unit: Unit, _locale: &str) -> Option<String> {
        let prefixed = matches!(
            unit,
            Unit::KilometersPerHour | Unit::MetersPerSecond | Unit::Fahrenheit
        );
        if prefixed && amount.negative {
            return None;
        }
        let words = amount_words(amount);
        Some(match unit {
            Unit::Kilogram => format!("{words} 킬로그램"),
            Unit::Gram => format!("{words} 그램"),
            Unit::Kilometer => format!("{words} 킬로미터"),
            Unit::Meter => format!("{words} 미터"),
            Unit::Centimeter => format!("{words} 센티미터"),
            Unit::Millimeter => format!("{words} 밀리미터"),
            Unit::Liter => format!("{words} 리터"),
            Unit::Milliliter => format!("{words} 밀리리터"),
            Unit::Celsius => format!("{words} 도"),
            Unit::Fahrenheit => format!("화씨 {words} 도"),
            Unit::KilometersPerHour => format!("시속 {words} 킬로미터"),
            Unit::MetersPerSecond => format!("초속 {words} 미터"),
            Unit::Gigabyte => format!("{words} 기가바이트"),
            Unit::Megahertz => format!("{words} 메가헤르츠"),
            Unit::Millisecond => format!("{words} 밀리초"),
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
            (0, "영"),
            (1, "일"),
            (10, "십"),
            (11, "십일"),
            (100, "백"),
            (110, "백십"),
            (1000, "천"),
            (2026, "이천이십육"),
            (10_000, "만"),
            (12_000, "만 이천"),
            (20_000, "이만"),
            (100_000_000, "일억"),
            (1_234_567_890, "십이억 삼천사백오십육만 칠천팔백구십"),
            (1_000_000_000_000, "일조"),
        ];
        for (n, expected) in cases {
            assert_eq!(cardinal(*n), *expected, "{n}");
        }
    }

    #[test]
    fn decimals_percent_currency() {
        let rules = Korean;
        assert_eq!(
            rules.number(&amount(3, Some("14")), "ko-KR").unwrap(),
            "삼 점 일사"
        );
        assert_eq!(
            rules.number(&amount(3, Some("05")), "ko-KR").unwrap(),
            "삼 점 영오"
        );
        assert_eq!(
            rules.percent(&amount(12, Some("5")), "ko-KR").unwrap(),
            "십이 점 오 퍼센트"
        );
        assert_eq!(
            rules.currency(Currency::Usd, 100, None, "ko-KR").unwrap(),
            "백 달러"
        );
        assert_eq!(rules.currency(Currency::Usd, 12, Some(50), "ko-KR"), None);
        assert_eq!(rules.currency(Currency::Brl, 12, None, "ko-KR"), None);
    }

    #[test]
    fn dates_times() {
        let rules = Korean;
        assert_eq!(
            rules.date(2026, 10, 26, "ko-KR").unwrap(),
            "이천이십육 년 시월 이십육 일"
        );
        assert_eq!(
            rules.date(2026, 6, 1, "ko-KR").unwrap(),
            "이천이십육 년 유월 일 일"
        );
        assert_eq!(
            rules.date(2026, 12, 25, "ko-KR").unwrap(),
            "이천이십육 년 십이월 이십오 일"
        );
        assert_eq!(rules.time(3, 30, None, "ko-KR").unwrap(), "세 시 삼십 분");
        assert_eq!(rules.time(12, 0, None, "ko-KR").unwrap(), "열두 시");
        assert_eq!(rules.time(9, 5, None, "ko-KR").unwrap(), "아홉 시 오 분");
        assert_eq!(rules.time(15, 30, None, "ko-KR"), None);
        assert_eq!(rules.time(0, 30, None, "ko-KR"), None);
    }

    #[test]
    fn units_and_counters() {
        let rules = Korean;
        assert_eq!(
            rules
                .unit(&amount(5, None), Unit::Kilogram, "ko-KR")
                .unwrap(),
            "오 킬로그램"
        );
        assert_eq!(
            rules
                .unit(&amount(100, None), Unit::KilometersPerHour, "ko-KR")
                .unwrap(),
            "시속 백 킬로미터"
        );
        assert!(!rules.number_gender_ambiguous(10_000, "원"));
        assert!(!rules.number_gender_ambiguous(2026, "년"));
        assert!(rules.number_gender_ambiguous(3, "개"));
        assert!(rules.number_gender_ambiguous(3, "명"));
        assert!(rules.number_gender_ambiguous(3, "시"));
        assert!(rules.number_gender_ambiguous(6, "월"));
        assert!(rules.number_gender_ambiguous(3, "분"));
        assert!(!rules.number_gender_ambiguous(3, "apples"));
    }
}
