//! Chinese (`zh`; locales zh-CN, zh-TW) rules.
//!
//! Number format: decimal point, comma grouping (`1,234.5`), read strictly
//! by convention as in English. Readings use Hanzi numerals: `十` for 10
//! (never `一十` at the start), `一百一十` for 110, a single `零` for each
//! run of skipped places (`一千零一`, `一万零一`, `一亿零一十万`), and the
//! sections `万`, `亿`, `万亿`.
//!
//! zh-TW uses the same readings written in Traditional characters (`萬`,
//! `億`, `點`, `負`, `兩`). Currency and units are converted for zh-CN only:
//! Taiwan usage differs (`日圓`, `公尺`, `公分`) and is not verified, so
//! zh-TW preserves them.
//!
//! Chosen readings (documented, not reviewed by a native speaker):
//! - `二` everywhere in cardinals (`二千零二十六`, `二百`), the formal
//!   reading. `两` is used only where `二` would be wrong: two o'clock
//!   (`两点`) and an amount of exactly two before a currency or a
//!   measure (`两美元`, `两公里`).
//! - Years in dates are read digit by digit (`二零二六年`), the
//!   conventional reading; month and day are cardinals (`十月二十六日`).
//! - Decimals are read digit by digit after `点` (`三点一四`).
//!
//! Limits: Chinese text usually has no spaces, and the classifier only
//! looks at whitespace-delimited fields, so numbers attached to Hanzi
//! (`2026年10月26日`, `3个`) are preserved unchanged. A number followed by
//! a separate Hanzi word (a measure word: `2 个` reads `两个`) is preserved
//! with `ambiguous_number`. Numeric slash dates are not a Chinese
//! convention; when both day/month readings are valid under an explicit
//! locale, month/day is used.

use super::{Amount, Conventions, Currency, DateOrder, Feature, LanguageRules, Meridiem, Unit};
use crate::tts::normalization::CoverageLevel;

/// Chinese rule set.
pub(crate) struct Chinese;

const DIGITS: [char; 10] = ['零', '一', '二', '三', '四', '五', '六', '七', '八', '九'];
const PLACES: [char; 4] = ['千', '百', '十', '\0'];

/// Script-dependent characters.
struct Script {
    wan: &'static str,
    yi: &'static str,
    point: &'static str,
    negative: &'static str,
    two: &'static str,
}

const SIMPLIFIED: Script = Script {
    wan: "万",
    yi: "亿",
    point: "点",
    negative: "负",
    two: "两",
};

const TRADITIONAL: Script = Script {
    wan: "萬",
    yi: "億",
    point: "點",
    negative: "負",
    two: "兩",
};

fn script(locale: &str) -> &'static Script {
    if locale == "zh-TW" {
        &TRADITIONAL
    } else {
        &SIMPLIFIED
    }
}

/// `true` when `word` starts with exactly three digits followed by a
/// non-digit: the second half of a space-grouped number.
fn looks_like_group_tail(word: &str) -> bool {
    let bytes = word.as_bytes();
    bytes.len() > 3 && bytes[..3].iter().all(u8::is_ascii_digit) && !bytes[3].is_ascii_digit()
}

/// Han, kana or Hangul letter.
fn is_cjk_letter(c: char) -> bool {
    matches!(c,
        '\u{3040}'..='\u{30FF}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}'
        | '\u{AC00}'..='\u{D7AF}'
        | '\u{F900}'..='\u{FAFF}'
    )
}

/// One section `1..=9999`. `leading`: the section starts the number, so
/// 10..=19 drop the leading `一` (`十五`).
fn section(n: u64, leading: bool, out: &mut String) {
    let digits = [n / 1000, (n / 100) % 10, (n / 10) % 10, n % 10];
    let mut started = false;
    let mut zero_pending = false;
    for (i, d) in digits.into_iter().enumerate() {
        if d == 0 {
            if started {
                zero_pending = true;
            }
            continue;
        }
        if zero_pending {
            out.push('零');
            zero_pending = false;
        }
        if !(leading && !started && i == 2 && d == 1) {
            out.push(DIGITS[d as usize]);
        }
        if PLACES[i] != '\0' {
            out.push(PLACES[i]);
        }
        started = true;
    }
}

/// Exact Hanzi cardinal for `n <= 999_999_999_999_999`.
fn cardinal(n: u64, script: &Script) -> String {
    if n == 0 {
        return "零".to_string();
    }
    let sections = [
        n % 10_000,
        (n / 10_000) % 10_000,
        (n / 100_000_000) % 10_000,
        (n / 1_000_000_000_000) % 10_000,
    ];
    let units = ["", script.wan, script.yi, ""];
    let mut out = String::new();
    let mut first = true;
    let mut gap = false;
    for idx in (0..4).rev() {
        let value = sections[idx];
        if value == 0 {
            if !first {
                gap = true;
            }
            continue;
        }
        if !first && (gap || value < 1000) {
            out.push('零');
        }
        section(value, first, &mut out);
        if idx == 3 {
            // 10^12: 万亿 / 萬億.
            out.push_str(script.wan);
            out.push_str(script.yi);
        } else {
            out.push_str(units[idx]);
        }
        first = false;
        gap = false;
    }
    out
}

/// Cardinal before a currency or a measure: exactly two is `两`.
fn counted(n: u64, script: &Script) -> String {
    if n == 2 {
        script.two.to_string()
    } else {
        cardinal(n, script)
    }
}

fn digits_words(digits: &str) -> String {
    digits
        .bytes()
        .map(|d| DIGITS[usize::from(d - b'0')])
        .collect()
}

/// Unsigned reading (`三点一四`).
fn unsigned_words(amount: &Amount, script: &Script, before_measure: bool) -> String {
    let mut out = if before_measure && amount.frac.is_none() {
        counted(amount.int, script)
    } else {
        cardinal(amount.int, script)
    };
    if let Some(frac) = &amount.frac {
        out.push_str(script.point);
        out.push_str(&digits_words(frac));
    }
    out
}

fn amount_words(amount: &Amount, script: &Script, before_measure: bool) -> String {
    let words = unsigned_words(amount, script, before_measure);
    if amount.negative {
        format!("{}{words}", script.negative)
    } else {
        words
    }
}

impl LanguageRules for Chinese {
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
        matches!(locale, "zh-CN" | "zh-TW").then_some(Conventions {
            decimal: '.',
            group: ',',
            separators_unambiguous: true,
            date_order: DateOrder::MonthDay,
            dollar: None,
        })
    }

    fn number(&self, amount: &Amount, locale: &str) -> Option<String> {
        Some(amount_words(amount, script(locale), false))
    }

    /// A following Hanzi word is usually a measure word, which changes the
    /// reading of two (`两个`); preserved.
    fn number_gender_ambiguous(&self, _int: u64, next_word: &str) -> bool {
        looks_like_group_tail(next_word) || next_word.chars().next().is_some_and(is_cjk_letter)
    }

    fn percent(&self, amount: &Amount, locale: &str) -> Option<String> {
        let script = script(locale);
        let words = unsigned_words(amount, script, false);
        Some(if amount.negative {
            format!("{}百分之{words}", script.negative)
        } else {
            format!("百分之{words}")
        })
    }

    fn currency(
        &self,
        currency: Currency,
        int: u64,
        cents: Option<u32>,
        locale: &str,
    ) -> Option<String> {
        if locale != "zh-CN" || cents.is_some_and(|c| c > 0) {
            return None;
        }
        let name = match currency {
            Currency::Usd => "美元",
            Currency::Eur => "欧元",
            Currency::Gbp => "英镑",
            Currency::Jpy => "日元",
            Currency::Brl | Currency::Pen => return None,
        };
        Some(format!("{}{name}", counted(int, &SIMPLIFIED)))
    }

    fn date(&self, year: u32, month: u32, day: u32, locale: &str) -> Option<String> {
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        let script = script(locale);
        Some(format!(
            "{}年{}月{}日",
            digits_words(&year.to_string()),
            cardinal(u64::from(month), script),
            cardinal(u64::from(day), script)
        ))
    }

    fn time(
        &self,
        hour: u32,
        minute: u32,
        meridiem: Option<Meridiem>,
        locale: &str,
    ) -> Option<String> {
        if meridiem.is_some() || hour > 23 || minute > 59 {
            return None;
        }
        let script = script(locale);
        let mut out = if hour == 2 {
            script.two.to_string()
        } else {
            cardinal(u64::from(hour), script)
        };
        out.push_str(script.point);
        match minute {
            0 => {}
            1..=9 => {
                out.push('零');
                out.push(DIGITS[minute as usize]);
                out.push('分');
            }
            _ => {
                out.push_str(&cardinal(u64::from(minute), script));
                out.push('分');
            }
        }
        Some(out)
    }

    fn unit(&self, amount: &Amount, unit: Unit, locale: &str) -> Option<String> {
        if locale != "zh-CN" {
            return None;
        }
        let script = &SIMPLIFIED;
        let temperature = matches!(unit, Unit::Celsius | Unit::Fahrenheit);
        if temperature && amount.int == 2 && amount.frac.is_none() {
            // `二度` or `两度` depends on context; preserved.
            return None;
        }
        let words = amount_words(amount, script, !temperature);
        Some(match unit {
            Unit::Kilogram => format!("{words}公斤"),
            Unit::Gram => format!("{words}克"),
            Unit::Kilometer => format!("{words}公里"),
            Unit::Meter => format!("{words}米"),
            Unit::Centimeter => format!("{words}厘米"),
            Unit::Millimeter => format!("{words}毫米"),
            Unit::Liter => format!("{words}升"),
            Unit::Milliliter => format!("{words}毫升"),
            Unit::Celsius => format!("{words}摄氏度"),
            Unit::Fahrenheit => format!("华氏{words}度"),
            Unit::KilometersPerHour => format!("{words}公里每小时"),
            Unit::MetersPerSecond => format!("{words}米每秒"),
            Unit::Megahertz => format!("{words}兆赫"),
            Unit::Millisecond => format!("{words}毫秒"),
            Unit::Gigabyte => return None,
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
            (0, "零"),
            (1, "一"),
            (10, "十"),
            (11, "十一"),
            (20, "二十"),
            (99, "九十九"),
            (100, "一百"),
            (105, "一百零五"),
            (110, "一百一十"),
            (1001, "一千零一"),
            (1010, "一千零一十"),
            (1100, "一千一百"),
            (2026, "二千零二十六"),
            (10_000, "一万"),
            (10_001, "一万零一"),
            (100_000, "十万"),
            (150_000, "十五万"),
            (100_010, "十万零一十"),
            (12_345_678, "一千二百三十四万五千六百七十八"),
            (100_000_000, "一亿"),
            (100_000_001, "一亿零一"),
            (100_100_000, "一亿零一十万"),
            (120_000_000, "一亿二千万"),
            (1_000_000_000_000, "一万亿"),
        ];
        for (n, expected) in cases {
            assert_eq!(cardinal(*n, &SIMPLIFIED), *expected, "{n}");
        }
        assert_eq!(cardinal(10_000, &TRADITIONAL), "一萬");
        assert_eq!(cardinal(100_000_000, &TRADITIONAL), "一億");
    }

    #[test]
    fn decimals_percent() {
        let rules = Chinese;
        assert_eq!(
            rules.number(&amount(3, Some("14")), "zh-CN").unwrap(),
            "三点一四"
        );
        assert_eq!(
            rules.number(&amount(0, Some("05")), "zh-CN").unwrap(),
            "零点零五"
        );
        assert_eq!(
            rules.number(&amount(3, Some("14")), "zh-TW").unwrap(),
            "三點一四"
        );
        assert_eq!(
            rules.percent(&amount(12, Some("5")), "zh-CN").unwrap(),
            "百分之十二点五"
        );
        let negative = Amount {
            negative: true,
            int: 3,
            frac: None,
        };
        assert_eq!(rules.percent(&negative, "zh-CN").unwrap(), "负百分之三");
        assert_eq!(rules.number(&negative, "zh-TW").unwrap(), "負三");
    }

    #[test]
    fn dates_times() {
        let rules = Chinese;
        assert_eq!(
            rules.date(2026, 10, 26, "zh-CN").unwrap(),
            "二零二六年十月二十六日"
        );
        assert_eq!(
            rules.date(2000, 1, 1, "zh-CN").unwrap(),
            "二零零零年一月一日"
        );
        assert_eq!(rules.time(15, 30, None, "zh-CN").unwrap(), "十五点三十分");
        assert_eq!(rules.time(9, 5, None, "zh-CN").unwrap(), "九点零五分");
        assert_eq!(rules.time(2, 0, None, "zh-CN").unwrap(), "两点");
        assert_eq!(rules.time(2, 0, None, "zh-TW").unwrap(), "兩點");
        assert_eq!(rules.time(10, 10, None, "zh-CN").unwrap(), "十点十分");
        assert_eq!(rules.time(0, 30, None, "zh-CN").unwrap(), "零点三十分");
        assert_eq!(rules.time(3, 30, Some(Meridiem::Pm), "zh-CN"), None);
    }

    #[test]
    fn currency_units() {
        let rules = Chinese;
        assert_eq!(
            rules.currency(Currency::Usd, 100, None, "zh-CN").unwrap(),
            "一百美元"
        );
        assert_eq!(
            rules.currency(Currency::Eur, 2, None, "zh-CN").unwrap(),
            "两欧元"
        );
        assert_eq!(rules.currency(Currency::Usd, 12, Some(50), "zh-CN"), None);
        assert_eq!(rules.currency(Currency::Usd, 100, None, "zh-TW"), None);
        assert_eq!(rules.currency(Currency::Brl, 100, None, "zh-CN"), None);
        assert_eq!(
            rules
                .unit(&amount(2, None), Unit::Kilometer, "zh-CN")
                .unwrap(),
            "两公里"
        );
        assert_eq!(
            rules
                .unit(&amount(1, Some("5")), Unit::Kilogram, "zh-CN")
                .unwrap(),
            "一点五公斤"
        );
        assert_eq!(
            rules
                .unit(&amount(25, None), Unit::Celsius, "zh-CN")
                .unwrap(),
            "二十五摄氏度"
        );
        assert_eq!(rules.unit(&amount(2, None), Unit::Celsius, "zh-CN"), None);
        assert_eq!(rules.unit(&amount(5, None), Unit::Kilometer, "zh-TW"), None);
        assert_eq!(rules.unit(&amount(5, None), Unit::Gigabyte, "zh-CN"), None);
    }

    #[test]
    fn measure_word_ambiguity() {
        let rules = Chinese;
        assert!(rules.number_gender_ambiguous(2, "个"));
        assert!(rules.number_gender_ambiguous(3, "本书"));
        assert!(!rules.number_gender_ambiguous(3, "apples"));
        assert!(!rules.number_gender_ambiguous(3, "。"));
    }
}
