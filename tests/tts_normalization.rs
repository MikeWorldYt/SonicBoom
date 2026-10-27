//! Golden-corpus test for the text normalizer.
//!
//! Loads every `tests/fixtures/tts_normalization/*.jsonl` file (one case per
//! line) and checks the spoken text and warning codes. Cases marked
//! `"reviewed_by": "spec"` are taken verbatim from the specification;
//! `"agent-unreviewed"` cases still need review by a fluent speaker.
//! Runs without any server feature:
//! `cargo test --test tts_normalization --no-default-features`.

use std::path::{Path, PathBuf};

use serde_json::Value;
use sonicboom::tts::normalization::{
    NormalizationMode, NormalizeLimits, NormalizeOptions, WarningCode, normalize,
};

const FEATURE_KEYS: [&str; 11] = [
    "cardinal_numbers",
    "decimals",
    "ordinals",
    "percentages",
    "currency",
    "dates",
    "times",
    "units",
    "abbreviations",
    "ranges",
    "fractions",
];

fn fixture_files() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tts_normalization");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("fixture directory exists")
        .map(|entry| entry.expect("readable entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    files.sort();
    files
}

fn code_name(code: WarningCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn str_field<'a>(case: &'a Value, key: &str, id: &str) -> &'a str {
    case[key]
        .as_str()
        .unwrap_or_else(|| panic!("case {id}: missing string field {key}"))
}

struct Case {
    id: String,
    language: String,
    locale: String,
    reviewed_by: String,
    failures: Vec<String>,
}

fn run_case(case: &Value) -> Case {
    let id = str_field(case, "id", "?").to_string();
    let language = str_field(case, "language", &id).to_string();
    let locale = str_field(case, "locale", &id).to_string();
    let mode = NormalizationMode::parse(str_field(case, "mode", &id))
        .unwrap_or_else(|| panic!("case {id}: bad mode"));
    let input = str_field(case, "input", &id);
    let reviewed_by = str_field(case, "reviewed_by", &id).to_string();
    assert!(
        matches!(reviewed_by.as_str(), "spec" | "agent-unreviewed" | "human"),
        "case {id}: unknown reviewed_by {reviewed_by}"
    );
    let features = case["features"]
        .as_array()
        .unwrap_or_else(|| panic!("case {id}: features must be an array"));
    for feature in features {
        let key = feature.as_str().unwrap_or_default();
        assert!(
            FEATURE_KEYS.contains(&key),
            "case {id}: unknown feature {key}"
        );
    }
    let expected_codes: Vec<String> = case["warning_codes"]
        .as_array()
        .unwrap_or_else(|| panic!("case {id}: warning_codes must be an array"))
        .iter()
        .map(|v| v.as_str().unwrap_or_default().to_string())
        .collect();

    let opts = NormalizeOptions::resolve(
        mode,
        &language,
        Some(&locale),
        NormalizeLimits::default(),
        true,
    )
    .unwrap_or_else(|e| panic!("case {id}: options rejected: {e}"));
    let result = normalize(input, &opts).unwrap_or_else(|e| panic!("case {id}: {e}"));

    let mut failures = Vec::new();
    let expected_spoken = match (case.get("expected_spoken"), case.get("expected_unchanged")) {
        (Some(Value::String(spoken)), None) => spoken.as_str(),
        (None, Some(Value::Bool(true))) => input,
        _ => panic!("case {id}: exactly one of expected_spoken / expected_unchanged=true"),
    };
    if result.spoken != expected_spoken {
        failures.push(format!(
            "spoken mismatch\n    expected: {expected_spoken:?}\n    actual:   {:?}",
            result.spoken
        ));
    }
    let actual_codes: Vec<String> = result.warnings.iter().map(|w| code_name(w.code)).collect();
    if actual_codes != expected_codes {
        failures.push(format!(
            "warnings mismatch: expected {expected_codes:?}, actual {actual_codes:?}"
        ));
    }
    // A second pass over a fully spoken result is a no-op.
    if mode == NormalizationMode::Auto && !result.spoken.bytes().any(|b| b.is_ascii_digit()) {
        let again = normalize(&result.spoken, &opts).expect("second pass");
        if again.spoken != result.spoken {
            failures.push(format!("not idempotent: {:?}", again.spoken));
        }
    }
    Case {
        id,
        language,
        locale,
        reviewed_by,
        failures,
    }
}

#[test]
fn golden_corpus() {
    let files = fixture_files();
    assert!(!files.is_empty(), "no fixture files found");
    let mut cases = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    for file in &files {
        let content = std::fs::read_to_string(file).expect("readable fixture");
        for (line_no, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(line).unwrap_or_else(|e| {
                panic!("{}:{}: invalid JSON: {e}", file.display(), line_no + 1)
            });
            let case = run_case(&value);
            assert!(ids.insert(case.id.clone()), "duplicate case id {}", case.id);
            cases.push(case);
        }
    }

    let count = |language: &str| cases.iter().filter(|c| c.language == language).count();
    let negative = cases.iter().filter(|c| c.id.starts_with("neg-")).count();
    let spec = cases.iter().filter(|c| c.reviewed_by == "spec").count();
    println!(
        "golden corpus: {} cases (es {}, en {}, negative {}, spec-verbatim {})",
        cases.len(),
        count("es"),
        count("en"),
        negative,
        spec
    );
    assert!(count("es") >= 30, "need at least 30 Spanish cases");
    assert!(count("en") >= 30, "need at least 30 English cases");
    assert!(negative >= 15, "need at least 15 negative cases");
    for case in &cases {
        if case.language == "es" {
            assert!(case.locale.starts_with("es-") || case.locale == "auto");
        }
    }

    let failed: Vec<String> = cases
        .iter()
        .filter(|c| !c.failures.is_empty())
        .map(|c| format!("[{}] {}\n  {}", c.locale, c.id, c.failures.join("\n  ")))
        .collect();
    assert!(
        failed.is_empty(),
        "{} of {} golden cases failed:\n{}",
        failed.len(),
        cases.len(),
        failed.join("\n")
    );
}
