//! Pipeline-level unit and deterministic fuzz tests for the normalizer.

use super::*;

fn opts(language: &str, locale: Option<&str>) -> NormalizeOptions {
    NormalizeOptions::resolve(
        NormalizationMode::Auto,
        language,
        locale,
        NormalizeLimits::default(),
        true,
    )
    .expect("valid options")
}

fn spoken(text: &str, language: &str, locale: Option<&str>) -> String {
    normalize(text, &opts(language, locale)).unwrap().spoken
}

fn warning_codes(text: &str, language: &str, locale: Option<&str>) -> Vec<WarningCode> {
    normalize(text, &opts(language, locale))
        .unwrap()
        .warnings
        .iter()
        .map(|w| w.code)
        .collect()
}

#[test]
fn mode_parse_and_names() {
    assert_eq!(
        NormalizationMode::parse(" AUTO "),
        Some(NormalizationMode::Auto)
    );
    assert_eq!(
        NormalizationMode::parse("off"),
        Some(NormalizationMode::Off)
    );
    assert_eq!(NormalizationMode::parse("strict"), None);
    assert_eq!(NormalizationMode::Auto.as_str(), "auto");
    assert_eq!(
        serde_json::to_string(&NormalizationMode::Off).unwrap(),
        "\"off\""
    );
    assert_eq!(
        serde_json::to_string(&WarningCode::AmbiguousNumber).unwrap(),
        "\"ambiguous_number\""
    );
    assert_eq!(WarningCode::NumberTooLarge.as_str(), "number_too_large");
    assert_eq!(
        serde_json::to_string(&ChangeKind::Percentage).unwrap(),
        "\"percentage\""
    );
}

#[test]
fn resolve_defaults_and_explicit() {
    let o = opts("es", None);
    assert_eq!((o.locale.as_str(), o.locale_explicit), ("es-ES", false));
    let o = opts("ES", Some("auto"));
    assert_eq!(
        (o.language.as_str(), o.locale.as_str(), o.locale_explicit),
        ("es", "es-ES", false)
    );
    let o = opts("es", Some(""));
    assert!(!o.locale_explicit);
    let o = opts("es", Some("es-pe"));
    assert_eq!((o.locale.as_str(), o.locale_explicit), ("es-PE", true));
    let o = opts("en", None);
    assert_eq!(o.locale, "en-US");
    // Unknown but well-formed language passes through.
    let o = opts("nl", None);
    assert_eq!(o.locale, "nl");
}

#[test]
fn resolve_errors() {
    let resolve = |language: &str, locale: Option<&str>| {
        NormalizeOptions::resolve(
            NormalizationMode::Auto,
            language,
            locale,
            NormalizeLimits::default(),
            true,
        )
        .map(|o| o.locale)
    };
    for bad in [
        "es_PE",
        "e",
        "es-",
        "123",
        "-PE",
        "es--PE",
        "es-PE-toolongsubtag",
    ] {
        assert_eq!(
            resolve("es", Some(bad)),
            Err(NormalizeError::InvalidLocale),
            "{bad}"
        );
    }
    assert_eq!(
        resolve("es", Some("es-PE-x-private")),
        Err(NormalizeError::UnsupportedLocale)
    );
    assert_eq!(
        resolve("es", Some("es-AR")),
        Err(NormalizeError::UnsupportedLocale)
    );
    assert_eq!(
        resolve("es", Some("en-US")),
        Err(NormalizeError::LocaleLanguageMismatch)
    );
    assert_eq!(
        resolve("pt", Some("es-PE")),
        Err(NormalizeError::LocaleLanguageMismatch)
    );
    assert_eq!(
        resolve("nl", Some("nl-NL")),
        Err(NormalizeError::UnsupportedLocale)
    );
    for bad in ["", "e", "español", "e5", "es-PE", "1"] {
        assert_eq!(
            resolve(bad, None),
            Err(NormalizeError::InvalidLanguage),
            "{bad}"
        );
    }
    // Error messages never contain user text.
    let message = NormalizeError::InputTooLong { max_chars: 3 }.to_string();
    assert_eq!(message, "input exceeds 3 characters");
}

#[test]
fn locale_helpers() {
    assert!(is_supported_locale("es-PE"));
    assert!(is_supported_locale("ES-pe"));
    assert!(is_supported_locale("zh-tw"));
    for bad in [
        "",
        "es_PE",
        "e",
        "es-",
        "123",
        "es-PE-x-private",
        "es-AR",
        "auto",
    ] {
        assert!(!is_supported_locale(bad), "{bad}");
    }
    assert_eq!(default_locale_for("pt"), Some("pt-BR"));
    assert_eq!(default_locale_for("xx"), None);
    assert_eq!(locales_for("es"), &["es-ES", "es-MX", "es-PE"]);
    assert!(locales_for("xx").is_empty());
    assert!(locales_for("").is_empty());
}

#[test]
fn coverage_reporting() {
    let all = language_coverage();
    assert_eq!(all.len(), 10);
    for entry in &all {
        assert_eq!(entry.features.len(), FEATURE_KEYS.len());
        for key in FEATURE_KEYS {
            assert!(entry.features.contains_key(key));
        }
        match entry.language {
            "es" | "en" => {
                assert_eq!(entry.rules, CoverageLevel::Partial);
                assert_eq!(entry.features["cardinal_numbers"], CoverageLevel::Full);
            }
            "pt" | "fr" | "de" | "it" | "ru" | "zh" | "ja" | "ko" => {
                assert_eq!(entry.rules, CoverageLevel::Partial, "{}", entry.language);
            }
            other => panic!("unexpected language {other}"),
        }
        assert!(entry.locales.contains(&entry.default_locale));
    }
    let json = serde_json::to_value(&all[0]).unwrap();
    assert_eq!(json["language"], "es");
    assert_eq!(json["rules"], "partial");
    let result = normalize("x", &opts("nl", None)).unwrap();
    assert_eq!(result.coverage.len(), 11);
    assert!(result.coverage.values().all(|l| *l == CoverageLevel::None));
}

#[test]
fn off_mode_is_byte_identical() {
    let mut o = opts("es", Some("es-PE"));
    o.mode = NormalizationMode::Off;
    let text = "  El 26/10/2026 cuesta S/ 150,50.\n😊👨\u{200D}👩\u{200D}👧  ";
    let result = normalize(text, &o).unwrap();
    assert_eq!(result.spoken, text);
    assert!(result.changes.is_empty() && result.warnings.is_empty());
    assert_eq!(result.mode, NormalizationMode::Off);
}

#[test]
fn spec_preview_example() {
    let text = "La reunión es el 26/10/2026 a las 15:30 y cuesta S/ 150,50.";
    let result = normalize(text, &opts("es", Some("es-PE"))).unwrap();
    assert_eq!(
        result.spoken,
        "La reunión es el veintiséis de octubre de dos mil veintiséis a las quince treinta y \
         cuesta ciento cincuenta soles con cincuenta céntimos."
    );
    let kinds: Vec<ChangeKind> = result.changes.iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        vec![ChangeKind::Date, ChangeKind::Time, ChangeKind::Currency]
    );
    assert_eq!(
        &text[result.changes[0].start..result.changes[0].end],
        "26/10/2026"
    );
    assert_eq!(
        &text[result.changes[2].start..result.changes[2].end],
        "S/ 150,50"
    );
    assert!(result.warnings.is_empty());
    assert_eq!(result.ruleset_version, "tn-v1");
}

#[test]
fn spanish_features() {
    let es = |t: &str| spoken(t, "es", Some("es-PE"));
    assert_eq!(es("12,5%"), "doce coma cinco por ciento");
    assert_eq!(
        es("1.234,56"),
        "mil doscientos treinta y cuatro coma cincuenta y seis"
    );
    assert_eq!(es("Tengo 2026 ideas."), "Tengo dos mil veintiséis ideas.");
    assert_eq!(es("-5"), "menos cinco");
    assert_eq!(es("0"), "cero");
    assert_eq!(es("el 1º de mayo"), "el primero de mayo");
    assert_eq!(es("el 1º piso"), "el primer piso");
    assert_eq!(es("la 2ª vez"), "la segunda vez");
    assert_eq!(es("S/ 1"), "un sol");
    assert_eq!(es("S/21"), "veintiún soles");
    assert_eq!(es("12,50 €"), "doce euros con cincuenta céntimos");
    assert_eq!(es("150 USD"), "ciento cincuenta dólares");
    assert_eq!(es("S/ 1.500"), "mil quinientos soles");
    assert_eq!(es("25 °C"), "veinticinco grados Celsius");
    assert_eq!(es("1 kg"), "un kilogramo");
    assert_eq!(es("21 km"), "veintiún kilómetros");
    assert_eq!(es("100km/h"), "cien kilómetros por hora");
    assert_eq!(es("1,5 L"), "uno coma cinco litros");
    assert_eq!(es("10–15"), "diez a quince");
    assert_eq!(es("10–15 kg"), "diez a quince kilogramos");
    assert_eq!(es("El Dr. García llegó."), "El doctor García llegó.");
    assert_eq!(es("Av. Arequipa 123"), "avenida Arequipa ciento veintitrés");
    assert_eq!(
        es("perros, gatos, etc., todos"),
        "perros, gatos, etcétera, todos"
    );
    assert_eq!(es("Fin etc. Otro."), "Fin etc. Otro.");
    assert_eq!(
        es("2026-10-26"),
        "veintiséis de octubre de dos mil veintiséis"
    );
    assert_eq!(
        es("29/02/2024"),
        "veintinueve de febrero de dos mil veinticuatro"
    );
}

#[test]
fn spanish_ambiguity() {
    let es = |t: &str| warning_codes(t, "es", Some("es-PE"));
    assert_eq!(spoken("3.14", "es", Some("es-PE")), "3.14");
    assert_eq!(es("3.14"), vec![WarningCode::AmbiguousNumber]);
    assert_eq!(es("1.234"), vec![WarningCode::AmbiguousNumber]);
    assert_eq!(es("1,234"), vec![WarningCode::AmbiguousNumber]);
    assert_eq!(es("21 casas"), vec![WarningCode::AmbiguousNumber]);
    assert_eq!(spoken("21 casas", "es", Some("es-PE")), "21 casas");
    assert_eq!(es("$ 20"), vec![WarningCode::AmbiguousCurrency]);
    assert_eq!(es("29/02/2026"), vec![WarningCode::InvalidDate]);
    assert_eq!(es("1/2/26"), vec![WarningCode::AmbiguousDate]);
    // Both readings valid: default locale preserves, explicit converts.
    assert_eq!(
        warning_codes("01/02/2026", "es", None),
        vec![WarningCode::AmbiguousDate]
    );
    assert_eq!(
        spoken("01/02/2026", "es", Some("es-PE")),
        "primero de febrero de dos mil veintiséis"
    );
    assert_eq!(es("3:30 pm"), vec![WarningCode::AmbiguousTime]);
    assert_eq!(es("25:00"), vec![WarningCode::AmbiguousTime]);
    assert_eq!(es("10:30:15"), vec![WarningCode::AmbiguousTime]);
}

#[test]
fn english_features() {
    let us = |t: &str| spoken(t, "en", Some("en-US"));
    let gb = |t: &str| spoken(t, "en", Some("en-GB"));
    assert_eq!(us("12/31/2026"), "December thirty-first, twenty twenty-six");
    assert_eq!(
        gb("31/12/2026"),
        "the thirty-first of December, twenty twenty-six"
    );
    assert_eq!(us("$12.50"), "twelve dollars and fifty cents");
    assert_eq!(us("12.5%"), "twelve point five percent");
    assert_eq!(us("3.140"), "three point one four zero");
    assert_eq!(us("1,234"), "one thousand two hundred thirty-four");
    assert_eq!(gb("105"), "one hundred and five");
    assert_eq!(us("the 21st time"), "the twenty-first time");
    assert_eq!(us("15:30"), "fifteen thirty");
    assert_eq!(us("3:30 PM"), "three thirty P M");
    assert_eq!(us("at 3:30 p.m. today"), "at three thirty P M today");
    assert_eq!(us("at 3:30 p.m. Then"), "at three thirty P M. Then");
    assert_eq!(us("5 km"), "five kilometers");
    assert_eq!(gb("5 km"), "five kilometres");
    assert_eq!(us("1 kg"), "one kilogram");
    assert_eq!(us("-1 °C"), "minus one degree Celsius");
    assert_eq!(us("10–15"), "ten to fifteen");
    assert_eq!(us("Dr. Smith"), "doctor Smith");
    assert_eq!(gb("£5.20"), "five pounds and twenty pence");
    assert_eq!(us("€3"), "three euros");
}

#[test]
fn english_ambiguity() {
    assert_eq!(
        warning_codes("$5", "en", Some("en-GB")),
        vec![WarningCode::AmbiguousCurrency]
    );
    assert_eq!(
        warning_codes("3,14", "en", None),
        vec![WarningCode::AmbiguousNumber]
    );
    assert_eq!(
        warning_codes("01/02/2026", "en", None),
        vec![WarningCode::AmbiguousDate]
    );
    assert_eq!(
        spoken("01/02/2026", "en", Some("en-US")),
        "January second, twenty twenty-six"
    );
    assert_eq!(
        spoken("01/02/2026", "en", Some("en-GB")),
        "the first of February, twenty twenty-six"
    );
    // Only one valid reading converts even under the default locale.
    assert_eq!(
        spoken("31/12/2026", "en", None),
        "December thirty-first, twenty twenty-six"
    );
    assert_eq!(spoken("1th", "en", None), "1th");
    assert_eq!(
        warning_codes("$1.5 million", "en", None),
        vec![WarningCode::AmbiguousCurrency]
    );
    assert_eq!(
        warning_codes("13:30 PM", "en", None),
        vec![WarningCode::AmbiguousTime]
    );
}

#[test]
fn protected_tokens_are_untouched() {
    for text in [
        "Minecraft 1.21.11",
        "10.0.0.1:8080",
        "https://example.org/a?b=3.14",
        "v1.2.3-beta.2",
        "user@example.org",
        "@user123",
        "#42",
        "C:\\data\\file2.txt",
        "/usr/lib/x86_64",
        "2001:db8::1",
        "550e8400-e29b-41d4-a716-446655440000",
        "deadbeef1234",
        "Qwen3 4K 3D",
        "987 654 321",
        "+51 987 654 321",
        "123-456",
        "`x = 3.14 + 2`",
        "<span id=\"3\">",
        "COVID-19",
    ] {
        for (language, locale) in [("es", "es-PE"), ("en", "en-US")] {
            let result = normalize(text, &opts(language, Some(locale))).unwrap();
            assert_eq!(result.spoken, text, "{text} ({locale})");
            assert!(result.changes.is_empty(), "{text}");
        }
    }
}

#[test]
fn fractions_and_leading_zeros_warn() {
    for text in ["½", "3/4", "1/2", "1:2", "16:9"] {
        assert_eq!(spoken(text, "es", Some("es-PE")), text);
        assert_eq!(
            warning_codes(text, "es", Some("es-PE")),
            vec![WarningCode::AmbiguousFraction],
            "{text}"
        );
    }
    assert_eq!(spoken("007", "en", None), "007");
    assert_eq!(
        warning_codes("007", "en", None),
        vec![WarningCode::AmbiguousNumber]
    );
}

#[test]
fn large_numbers_are_preserved() {
    let text = "1000000000000000";
    assert_eq!(spoken(text, "es", None), text);
    assert_eq!(
        warning_codes(text, "es", None),
        vec![WarningCode::NumberTooLarge]
    );
    let digits = "7".repeat(10_000);
    let result = normalize(&digits, &opts("en", None)).unwrap();
    assert_eq!(result.spoken, digits);
    assert_eq!(result.warnings.len(), 1);
}

#[test]
fn report_ambiguities_false_hides_warnings() {
    let mut o = opts("es", Some("es-PE"));
    o.report_ambiguities = false;
    let result = normalize("3.14", &o).unwrap();
    assert!(result.warnings.is_empty());
    assert_eq!(result.spoken, "3.14");
}

#[test]
fn unicode_is_preserved() {
    let text = "Pingüino, niño, acción, áéíóú 😊👨\u{200D}👩\u{200D}👧\u{200D}👦 日本語 e\u{301}";
    for (language, locale) in [("es", Some("es-PE")), ("en", None), ("ja", None)] {
        assert_eq!(spoken(text, language, locale), text);
    }
    // Replacements keep the surrounding bytes intact.
    assert_eq!(spoken("¡Ñandú 5 😊!", "es", None), "¡Ñandú cinco 😊!");
}

#[test]
fn pass_through_languages() {
    // Inputs that stay unchanged: an unknown language, and the attached CJK
    // dates that the zh/ja/ko rules deliberately preserve.
    for (language, text) in [
        ("zh", "2026年10月26日"),
        ("ja", "2026年10月26日"),
        ("ko", "2026년 10월 26일"),
        ("nl", "12 euro"),
    ] {
        let result = normalize(text, &opts(language, None)).unwrap();
        assert_eq!(result.spoken, text, "{language}");
        assert!(result.changes.is_empty() && result.warnings.is_empty());
    }
}

#[test]
fn input_and_output_budgets() {
    let mut o = opts("es", None);
    o.limits.max_input_chars = 3;
    assert_eq!(
        normalize("ñañá", &o).unwrap_err(),
        NormalizeError::InputTooLong { max_chars: 3 }
    );
    assert!(normalize("ñañ", &o).is_ok());

    let mut o = opts("es", None);
    o.limits.max_output_chars = 5;
    assert_eq!(
        normalize("5 5,5", &o).unwrap_err(),
        NormalizeError::OutputTooLong { max_chars: 5 }
    );
    let mut o = opts("es", None);
    o.limits.max_expansion_ratio = 1.0;
    // Short inputs use the 32-char floor.
    assert_eq!(normalize("5", &o).unwrap().spoken, "cinco");
    // Long expanding inputs are still rejected.
    assert_eq!(
        normalize(&"5% ".repeat(20), &o).unwrap_err(),
        NormalizeError::ExpansionTooLarge
    );
    assert_eq!(normalize("", &o).unwrap().spoken, "");
}

#[test]
fn short_inputs_are_not_rejected_by_the_ratio() {
    // Default ratio 8: 3 input chars, 26 spoken chars.
    assert_eq!(
        spoken("77%", "es", Some("es-ES")),
        "setenta y siete por ciento"
    );
    assert_eq!(
        spoken("77°F", "en", None),
        "seventy-seven degrees Fahrenheit"
    );
    // Pathological expansion at the default ratio is still rejected.
    let mut o = opts("es", Some("es-ES"));
    o.limits.max_expansion_ratio = 4.0;
    assert_eq!(
        normalize(&"77% ".repeat(100), &o).unwrap_err(),
        NormalizeError::ExpansionTooLarge
    );
}

#[test]
fn ten_thousand_digit_input() {
    // Long digit runs are never expanded: preserved with a warning, so they
    // pass the budget unchanged. One character above the input limit is
    // rejected.
    let o = opts("es", Some("es-ES"));
    let digits = "7".repeat(10_000);
    let result = normalize(&digits, &o).unwrap();
    assert_eq!(result.spoken, digits);
    assert!(result.changes.is_empty());
    assert_eq!(
        result.warnings.iter().map(|w| w.code).collect::<Vec<_>>(),
        vec![WarningCode::NumberTooLarge]
    );
    assert_eq!(
        normalize(&"7".repeat(10_001), &o).unwrap_err(),
        NormalizeError::InputTooLong { max_chars: 10_000 }
    );
}

#[test]
fn speech_text_identity() {
    let o = opts("es", Some("es-PE"));
    let prepared = SpeechText::prepare("Son 5.", &o).unwrap();
    assert_eq!(prepared.as_str(), "Son cinco.");
    assert_eq!(prepared.identity(), "tn-v1|auto|es|es-PE");
    assert!(prepared.changed());
    let mut off = o.clone();
    off.mode = NormalizationMode::Off;
    let raw = SpeechText::prepare("Son 5.", &off).unwrap();
    assert_eq!(raw.identity(), "tn-v1|off|es|es-PE");
    assert!(!raw.changed());
    assert_eq!(raw.into_string(), "Son 5.");
}

#[test]
fn changes_are_ordered_and_on_boundaries() {
    let text = "¿El 26/10/2026? Sí: S/ 150,50, 12,5% y 3.14 ñ 25 °C…";
    let result = normalize(text, &opts("es", Some("es-PE"))).unwrap();
    assert_invariants(text, &result);
    assert_eq!(result.changes.len(), 4);
    assert_eq!(result.warnings.len(), 1);
}

// ---------------------------------------------------------------------------
// Deterministic fuzzing (no external crates).
// ---------------------------------------------------------------------------

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const FUZZ_PIECES: &[&str] = &[
    "0",
    "1",
    "2",
    "3",
    "5",
    "7",
    "9",
    "00",
    "12",
    "31",
    "2026",
    "999999999999999",
    "1000000000000000",
    ".",
    ",",
    ":",
    "/",
    "-",
    "–",
    "%",
    "$",
    "€",
    "S/",
    "S/.",
    "US$",
    "R$",
    "£",
    "¥",
    "°",
    "°C",
    "½",
    "³",
    "º",
    "ª",
    " ",
    " ",
    " ",
    "\n",
    "\u{a0}",
    "a",
    "e",
    "m",
    "kg",
    "km/h",
    "L",
    "st",
    "th",
    "PM",
    "p.m.",
    "Dr.",
    "etc.",
    "Av.",
    "ñ",
    "á",
    "ü",
    "e\u{301}",
    "\u{300}",
    "😊",
    "👨\u{200D}👩\u{200D}👧",
    "\u{200D}",
    "日本",
    "年",
    "¿",
    "¡",
    "(",
    ")",
    "\"",
    "`",
    "<",
    ">",
    "@",
    "http://",
    "x",
    "Q",
    "USD",
    "+",
];

fn random_text(rng: &mut XorShift) -> String {
    let len = rng.below(24);
    (0..len)
        .map(|_| FUZZ_PIECES[rng.below(FUZZ_PIECES.len())])
        .collect()
}

fn assert_invariants(text: &str, result: &NormalizedText) {
    let mut cursor = 0usize;
    for change in &result.changes {
        assert!(change.start >= cursor, "overlapping changes in {text:?}");
        assert!(change.end > change.start, "empty change in {text:?}");
        assert!(text.is_char_boundary(change.start) && text.is_char_boundary(change.end));
        assert!(!change.replacement.is_empty());
        cursor = change.end;
    }
    let mut cursor = 0usize;
    for warning in &result.warnings {
        assert!(warning.start >= cursor, "overlapping warnings in {text:?}");
        assert!(warning.end > warning.start && warning.end <= text.len());
        assert!(text.is_char_boundary(warning.start) && text.is_char_boundary(warning.end));
        cursor = warning.end;
    }
    // Unchanged bytes are preserved: rebuild the original from the spoken
    // text by reversing the splice.
    let mut rebuilt = String::new();
    let mut spoken_cursor = 0usize;
    let mut original_cursor = 0usize;
    for change in &result.changes {
        let keep = change.start - original_cursor;
        rebuilt.push_str(&result.spoken[spoken_cursor..spoken_cursor + keep]);
        rebuilt.push_str(&text[change.start..change.end]);
        spoken_cursor += keep + change.replacement.len();
        original_cursor = change.end;
    }
    rebuilt.push_str(&result.spoken[spoken_cursor..]);
    assert_eq!(rebuilt, text, "bytes outside changes were modified");
}

#[test]
fn fuzz_random_inputs() {
    let mut rng = XorShift(0x5EED_CAFE_F00D_D00D);
    let configs: Vec<NormalizeOptions> = [
        ("es", Some("es-PE")),
        ("es", None),
        ("es", Some("es-MX")),
        ("en", Some("en-US")),
        ("en", Some("en-GB")),
        ("ja", None),
    ]
    .iter()
    .map(|(l, loc)| opts(l, *loc))
    .collect();
    let mut idempotence_checks = 0usize;
    for round in 0..6000 {
        let text = random_text(&mut rng);
        let o = &configs[round % configs.len()];
        let input_chars = text.chars().count();
        match normalize(&text, o) {
            Ok(result) => {
                assert_invariants(&text, &result);
                let out_chars = result.spoken.chars().count();
                assert!(
                    out_chars as f64 <= input_chars.max(32) as f64 * 8.0,
                    "expansion bound violated for {text:?}"
                );
                if !result.spoken.bytes().any(|b| b.is_ascii_digit()) {
                    let again = normalize(&result.spoken, o).unwrap();
                    assert_eq!(again.spoken, result.spoken, "not idempotent for {text:?}");
                    idempotence_checks += 1;
                }
            }
            Err(NormalizeError::ExpansionTooLarge | NormalizeError::OutputTooLong { .. }) => {}
            Err(other) => panic!("unexpected error {other:?} for {text:?}"),
        }
        let mut off = o.clone();
        off.mode = NormalizationMode::Off;
        assert_eq!(normalize(&text, &off).unwrap().spoken, text);
    }
    assert!(
        idempotence_checks > 1000,
        "only {idempotence_checks} idempotence checks"
    );
}

#[test]
fn fuzz_adversarial_long_inputs() {
    let o = opts("es", Some("es-PE"));
    let mut limits = o.limits;
    limits.max_input_chars = 100_000;
    limits.max_output_chars = 1_000_000;
    let o = NormalizeOptions { limits, ..o };
    for text in [
        "1.".repeat(5000),
        "1,1".repeat(3000),
        "`".repeat(5000),
        "<a".repeat(5000),
        "S/ ".repeat(5000),
        "9".repeat(50_000),
        "–".repeat(5000),
        "12:".repeat(5000),
        "1/".repeat(5000),
        "\u{301}".repeat(5000),
        "👨\u{200D}".repeat(5000),
        "1 ".repeat(5000),
    ] {
        let result = normalize(&text, &o).unwrap();
        assert_invariants(&text, &result);
    }
}

#[test]
fn malformed_dates_never_become_dates() {
    for text in [
        "31/02/2026",
        "29/02/2025",
        "00/01/2026",
        "32/01/2026",
        "2026-02-30",
        "2026-13-01",
    ] {
        for (language, locale) in [
            ("es", Some("es-PE")),
            ("en", Some("en-US")),
            ("en", Some("en-GB")),
        ] {
            let result = normalize(text, &opts(language, locale)).unwrap();
            assert_eq!(result.spoken, text);
            assert_eq!(
                result.warnings.iter().map(|w| w.code).collect::<Vec<_>>(),
                vec![WarningCode::InvalidDate],
                "{text}"
            );
        }
    }
}
