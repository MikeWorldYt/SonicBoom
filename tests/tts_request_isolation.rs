//! Model-independent end-to-end diagnostic artifact ownership.
fn runner() -> std::process::Command {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_tts-quality-audit"));
    for key in [
        "AUDIT_SEED",
        "AUDIT_TEXTS_JSON",
        "AUDIT_ALT_REFERENCE_WAV",
        "AUDIT_ALT_REFERENCE_TRANSCRIPT",
        "AUDIT_POCKET_STEPS",
        "AUDIT_POCKET_TEMPERATURE",
        "AUDIT_SAMPLING_PRESET",
        "AUDIT_QUALITY",
        "AUDIT_DEVICE",
        "AUDIT_ASR_JSON",
    ] {
        command.env_remove(key);
    }
    command.env(
        "ORT_DYLIB_PATH",
        "sonicboom-deliberately-missing-runtime.so",
    );
    command
}

#[test]
fn mock_sequence_preserves_independent_wavs_and_manifest() {
    let output = std::env::temp_dir().join(format!("sonicboom-audit-{}", uuid::Uuid::new_v4()));
    let status = runner()
        .args(["mock", "mock-v1", "es", "."])
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    let manifest = std::fs::read_to_string(output.join("manifest.jsonl")).unwrap();
    let rows: Vec<serde_json::Value> = manifest
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 11);
    let ids: std::collections::HashSet<_> = rows
        .iter()
        .map(|r| r["request_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), rows.len());
    assert_ne!(rows[0]["wav_hash"], rows[1]["wav_hash"]);
    for index in [3, 4] {
        assert_eq!(rows[index]["wav_hash"], rows[1]["wav_hash"]);
    }
    for row in &rows {
        let wav = hound::WavReader::open(output.join(row["output"].as_str().unwrap())).unwrap();
        assert_eq!(wav.len() as u64, row["sample_count"].as_u64().unwrap());
        assert!(row.get("text").is_none());
        assert_eq!(row["ort_build"], "not used (mock)");
    }
    // A second invocation refuses to overwrite independently produced outputs.
    assert!(
        !runner()
            .args(["mock", "mock-v1", "es", "."])
            .arg(&output)
            .status()
            .unwrap()
            .success()
    );
    std::fs::remove_dir_all(output).unwrap();
}
