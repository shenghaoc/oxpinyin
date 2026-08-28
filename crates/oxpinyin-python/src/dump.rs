//! Corpus-driven engine runs shared by the two parity surfaces.
//!
//! `native-dump` feeds the corpus through this module (pure Rust, no Python)
//! while the pytest suite replays the same cases through the PyO3 binding,
//! then compares the two transcripts structurally (equality of the loaded events). The transcripts are the
//! whole test: any behavioural difference between the native path and the
//! bound path shows up as a diff here.

use std::path::Path;

use serde_json::{Value, json};

use oxpinyin_runtime::{Runtime, RuntimeSession};

/// Runs every case in `corpus` against a fixture-mode runtime over
/// `system_dir` and returns the transcript document.
///
/// # Errors
///
/// Returns an [`std::io::Error`]-flavoured panic-free failure via the
/// serialized [`RunError`] on open or backend errors.
pub fn run_corpus(corpus: &Value, system_dir: &Path) -> Result<Value, RunError> {
    let runtime =
        Runtime::open_fixtures(system_dir, None).map_err(|error| RunError(error.to_string()))?;

    let mut cases = Vec::new();
    for case in corpus["cases"]
        .as_array()
        .ok_or_else(|| RunError("corpus has no cases array".to_owned()))?
    {
        // A fresh session per case, because the pytest driver builds a fresh
        // `Engine` per case. Reusing one session here would leave this side
        // carrying residue from cases 1..N-1 while the Python side starts
        // virgin: the native transcript would be corpus-order-dependent when
        // the replayed one is not, and a future divergence could not be told
        // apart from an incomplete `reset()`. The `Runtime` stays shared —
        // sessions share the table handles, so this costs one session
        // construction per case, not a table reopen.
        let mut session = runtime
            .new_session(&oxpinyin_engine::EmptyConfigSource)
            .map_err(|e| RunError(e.to_string()))?;
        cases.push(run_case(&mut session, case)?);
    }

    Ok(json!({
        "schema": "oxpinyin-native-parity-v1",
        "unigram_source": "flat-export-fixture",
        "cases": cases,
    }))
}

/// Why a transcript could not be produced.
#[derive(Debug)]
pub struct RunError(pub String);

impl core::fmt::Display for RunError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RunError {}

/// Resolves a case's effective input: either the literal `input` or
/// `repeat.unit` repeated `repeat.times` times.
///
/// # Errors
///
/// Returns [`RunError`] when `times` cannot convert to a `usize` or the
/// repeated length overflows — `String::repeat` would panic there, and an
/// adversarial corpus file must not crash the dump.
fn effective_input(case: &Value) -> Result<String, RunError> {
    if let Some(input) = case["input"].as_str() {
        return Ok(input.to_owned());
    }
    if let (Some(unit), Some(times)) = (
        case["repeat"]["unit"].as_str(),
        case["repeat"]["times"].as_u64(),
    ) {
        let times = usize::try_from(times)
            .map_err(|_| RunError(format!("repeat.times {times} exceeds usize")))?;
        let unit_len = unit.len();
        let total = unit_len.checked_mul(times).ok_or_else(|| {
            RunError(format!(
                "repeated input of {unit_len} bytes x {times} overflows usize"
            ))
        })?;
        // Bounded by MAX_INPUT_BYTES anyway; refuse absurd allocations early.
        const INPUT_CAP: usize = 1 << 20;
        if total > INPUT_CAP {
            return Err(RunError(format!(
                "repeated input is {total} bytes, past the {INPUT_CAP}-byte corpus cap"
            )));
        }
        return Ok(unit.repeat(times));
    }
    Ok(String::new())
}

/// Drives one corpus case through the session, recording every observable
/// step. This mirrors the replay procedure documented in the corpus header;
/// the pytest driver implements the same steps through the binding.
fn run_case(session: &mut RuntimeSession, case: &Value) -> Result<Value, RunError> {
    let name = case["name"].as_str().unwrap_or_default().to_owned();
    let input = effective_input(case)?;
    let mut events = Vec::new();

    session.reset();
    match session.type_pinyin(&input) {
        Ok(outcome) => {
            let _ = outcome;
            events.push(json!({
                "type": "lookup",
                "input": session.raw_input(),
                "composing": session.is_composing(),
                "parsed_len": session.full_parsed_len(),
                "candidates": snapshot(session.candidates()),
            }));
        }
        Err(error) => {
            events.push(json!({ "type": "lookup_error", "message": error.to_string() }));
            return Ok(json!({ "name": name, "events": events }));
        }
    }

    if case.get("guess_sentence").and_then(Value::as_bool) == Some(true) {
        match session.guess_sentence() {
            Ok(ran) => {
                let sentences: Vec<String> = (0..=u8::MAX)
                    .map_while(|index| session.sentence_text(index).map(str::to_owned))
                    .collect();
                let top: Vec<String> = top_texts(session);
                events.push(json!({
                    "type": "guess_sentence",
                    "ran": ran,
                    "sentences": sentences,
                    "top_candidate_texts": top,
                }));
            }
            Err(error) => {
                events
                    .push(json!({ "type": "guess_sentence_error", "message": error.to_string() }));
            }
        }
    }

    if let Some(offset) = case.get("candidates_at").and_then(Value::as_u64) {
        match session.candidates_at(usize::try_from(offset).unwrap_or(usize::MAX)) {
            Ok(window) => {
                let all: Vec<serde_json::Value> = window
                    .iter()
                    .map(|c| {
                        json!({
                            "text": c.text(),
                            "kind": kind_label(c.kind()),
                            "consumed_bytes": c.consumed_bytes(),
                        })
                    })
                    .collect();
                events.push(json!({ "type": "candidates_at", "offset": offset, "window": all }));
            }
            Err(error) => {
                events.push(json!({ "type": "candidates_at_error", "message": error.to_string() }));
            }
        }
    }

    for selection in case
        .get("select")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(index) = selection.as_u64() else {
            continue;
        };
        match session.select(usize::try_from(index).unwrap_or(usize::MAX)) {
            Ok(outcome) => {
                let completed = matches!(outcome, oxpinyin_engine::Selection::Completed);
                // Snapshot the post-select state *before* committing: a
                // completing commit clears the composition, so offset,
                // preedit, composing and the top texts must be read here to
                // describe the selection rather than the emptied engine.
                let offset = session.composition_offset();
                let preedit = session.preedit().text().to_owned();
                let composing = session.is_composing();
                let top = top_texts(session);
                let commit_text = if completed {
                    session.commit().ok()
                } else {
                    None
                };
                events.push(json!({
                    "type": "select",
                    "index": index,
                    "result": if completed { "completed" } else { "continued" },
                    "offset": offset,
                    "preedit": preedit,
                    "composing": composing,
                    "top_candidate_texts": top,
                    "commit": commit_text,
                }));
                if completed {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    Ok(json!({ "name": name, "events": events }))
}

/// The stable string for a candidate origin, matching the binding's labels.
fn kind_label(kind: oxpinyin_engine::CandidateKind) -> &'static str {
    match kind {
        oxpinyin_engine::CandidateKind::Phrase => "phrase",
        oxpinyin_engine::CandidateKind::Addon => "addon",
        oxpinyin_engine::CandidateKind::Sentence => "sentence",
        oxpinyin_engine::CandidateKind::Fallback => "fallback",
        _ => "other",
    }
}

fn snapshot(list: &oxpinyin_engine::CandidateList) -> Vec<Value> {
    list.iter()
        .map(|candidate| {
            json!({
                "text": candidate.text(),
                "kind": kind_label(candidate.kind()),
                "consumed_keys": candidate.consumed_keys(),
                "consumed_bytes": candidate.consumed_bytes(),
                "cost": candidate.cost(),
                "nbest_index": candidate.nbest_index(),
            })
        })
        .collect()
}

fn top_texts(session: &RuntimeSession) -> Vec<String> {
    session
        .candidates()
        .iter()
        .map(|c| c.text().to_owned())
        .take(8)
        .collect()
}
