//! Contract tests for the pure-Rust parity transcript driver.

use std::path::PathBuf;

use serde_json::{Value, json};

use oxpinyin_python::dump::run_corpus;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/w3")
}

fn cases(transcript: &Value) -> &[Value] {
    transcript["cases"]
        .as_array()
        .expect("a successful transcript always contains cases")
}

#[test]
fn literal_and_repeated_inputs_produce_the_same_events() {
    let corpus = json!({
        "cases": [
            { "name": "literal", "input": "nini" },
            { "name": "repeated", "repeat": { "unit": "ni", "times": 2 } },
        ]
    });

    let transcript = run_corpus(&corpus, &fixture_dir()).expect("fixture replay succeeds");
    let cases = cases(&transcript);

    assert_eq!(cases[0]["events"], cases[1]["events"]);
    assert_eq!(transcript["schema"], "oxpinyin-native-parity-v1");
    assert_eq!(transcript["unigram_source"], "flat-export-fixture");
}

#[test]
fn each_case_starts_with_fresh_engine_state() {
    let corpus = json!({
        "cases": [
            { "name": "mutates-state", "input": "nihao", "select": [0] },
            { "name": "fresh", "input": "" },
        ]
    });

    let transcript = run_corpus(&corpus, &fixture_dir()).expect("fixture replay succeeds");
    let fresh_lookup = &cases(&transcript)[1]["events"][0];

    assert_eq!(fresh_lookup["type"], "lookup");
    assert_eq!(fresh_lookup["input"], "");
    assert_eq!(fresh_lookup["composing"], false);
    assert_eq!(fresh_lookup["parsed_len"], 0);
    assert_eq!(fresh_lookup["candidates"], json!([]));
}

#[test]
fn completing_selection_is_snapshotted_before_commit_clears_state() {
    let corpus = json!({
        "cases": [
            { "name": "complete", "input": "nihao", "select": [0] },
        ]
    });

    let transcript = run_corpus(&corpus, &fixture_dir()).expect("fixture replay succeeds");
    let selection = &cases(&transcript)[0]["events"][1];

    assert_eq!(selection["type"], "select");
    assert_eq!(selection["result"], "completed");
    assert_eq!(selection["preedit"], "你好");
    assert_eq!(selection["commit"], "你好");
    assert_ne!(selection["preedit"], "");
}

#[test]
fn invalid_probe_offsets_are_recorded_without_aborting_the_case() {
    let corpus = json!({
        "cases": [
            { "name": "bad-offset", "input": "ni", "candidates_at": 99 },
        ]
    });

    let transcript = run_corpus(&corpus, &fixture_dir()).expect("probe errors are events");
    let events = cases(&transcript)[0]["events"]
        .as_array()
        .expect("events is an array");

    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["type"], "lookup");
    assert_eq!(events[1]["type"], "candidates_at_error");
    assert!(
        events[1]["message"]
            .as_str()
            .expect("errors have messages")
            .contains("offset 99")
    );
}

#[test]
fn malformed_corpora_fail_cleanly() {
    let missing_cases = run_corpus(&json!({}), &fixture_dir())
        .expect_err("the cases array is required")
        .to_string();
    assert_eq!(missing_cases, "corpus has no cases array");

    let oversized_repeat = json!({
        "cases": [{
            "name": "oversized",
            "repeat": { "unit": "x", "times": (1_u64 << 20) + 1 },
        }]
    });
    let error = run_corpus(&oversized_repeat, &fixture_dir())
        .expect_err("absurd repeats are refused before allocation")
        .to_string();
    assert!(
        error.contains("past the 1048576-byte corpus cap"),
        "{error}"
    );

    let overflowing_repeat = json!({
        "cases": [{
            "name": "overflow",
            "repeat": { "unit": "xx", "times": u64::MAX },
        }]
    });
    let error = run_corpus(&overflowing_repeat, &fixture_dir())
        .expect_err("overflowing repeats are refused")
        .to_string();
    assert!(error.contains("overflows usize"), "{error}");
}
