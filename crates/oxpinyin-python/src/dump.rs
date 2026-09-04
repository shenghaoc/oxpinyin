//! Corpus-driven engine runs shared by the two parity surfaces.
//!
//! `native-dump` feeds the corpus through this module (pure Rust, no Python)
//! while the pytest suite replays the same cases through the `PyO3` binding,
//! then compares the two transcripts structurally (equality of the loaded events). The transcripts are the
//! whole test: any behavioural difference between the native path and the
//! bound path shows up as a diff here.

use std::path::Path;

use serde_json::{Value, json};

use oxpinyin_runtime::{Runtime, RuntimeSession};

use crate::zhuyin::{
    ZhuyinSession, chewing_scheme_from_value, dvorak_scheme_message, full_scheme_from_value,
    in_keyboard_arity_message, unknown_chewing_scheme_message, unknown_full_scheme_message,
};

/// Runs every case in `corpus` against a runtime over `system_dir` and
/// returns the transcript document.
///
/// # Errors
///
/// Returns an [`std::io::Error`]-flavoured panic-free failure via the
/// serialized [`RunError`] on open or backend errors.
pub fn run_corpus(corpus: &Value, system_dir: &Path) -> Result<Value, RunError> {
    let runtime = Runtime::open(system_dir, None).map_err(|error| RunError(error.to_string()))?;

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
        // Bounded by MAX_INPUT_BYTES anyway; refuse absurd allocations early.
        const INPUT_CAP: usize = 1 << 20;
        let times = usize::try_from(times)
            .map_err(|_| RunError(format!("repeat.times {times} exceeds usize")))?;
        let unit_len = unit.len();
        let total = unit_len.checked_mul(times).ok_or_else(|| {
            RunError(format!(
                "repeated input of {unit_len} bytes x {times} overflows usize"
            ))
        })?;
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
            Err(error) => {
                events.push(json!({ "type": "select_error", "message": error.to_string() }));
                break;
            }
        }
    }

    Ok(json!({ "name": name, "events": events }))
}

/// The stable string for a candidate origin, matching the binding's labels.
const fn kind_label(kind: oxpinyin_engine::CandidateKind) -> &'static str {
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

/// Runs every case in a zhuyin parity corpus against a runtime over
/// `system_dir` and returns the transcript document.
///
/// The zhuyin twin of [`run_corpus`]: a fresh [`ZhuyinSession`] per case —
/// the same session type the binding wraps — replayed through the
/// corpus-header procedure the pytest driver mirrors through
/// `oxpinyin.zhuyin`.
pub fn run_zhuyin_corpus(corpus: &Value, system_dir: &Path) -> Result<Value, RunError> {
    let runtime = Runtime::open(system_dir, None).map_err(|error| RunError(error.to_string()))?;

    let mut cases = Vec::new();
    for case in corpus["cases"]
        .as_array()
        .ok_or_else(|| RunError("corpus has no cases array".to_owned()))?
    {
        // A fresh facade per case, because the pytest driver builds a fresh
        // `zhuyin.Engine` per case — the same order-independence argument as
        // `run_corpus`. The `Runtime` stays shared.
        let session = runtime
            .new_session(&oxpinyin_engine::Config::default())
            .map_err(|e| RunError(e.to_string()))?;
        let mut facade = ZhuyinSession::open(&runtime, session);
        cases.push(run_zhuyin_case(&mut facade, case)?);
    }

    Ok(json!({
        "schema": "oxpinyin-zhuyin-native-parity-v1",
        "cases": cases,
    }))
}

/// Drives one zhuyin corpus case through the facade, recording every
/// observable step. Mirrors the replay procedure documented in the zhuyin
/// corpus header; the pytest driver implements the same steps through the
/// binding.
fn run_zhuyin_case(facade: &mut ZhuyinSession, case: &Value) -> Result<Value, RunError> {
    let name = case["name"].as_str().unwrap_or_default().to_owned();
    let mut events = Vec::new();

    // Optional scheme setup. A rejection ends the case with a `scheme_error`
    // event, the way a failed open ends a pinyin case with `lookup_error`:
    // the facade keeps its previous scheme, so continuing would describe a
    // different engine than the replaying side's refused one.
    if let Some(value) = case.get("chewing_scheme").and_then(Value::as_u64) {
        // Corpus-authored values fit in a byte (see the corpus header); an
        // out-of-range value saturates into the unknown-scheme refusal.
        let byte = u8::try_from(value).unwrap_or(u8::MAX);
        match chewing_scheme_from_value(byte) {
            Some(scheme) => {
                if !facade.set_chewing_scheme(scheme) {
                    events.push(json!({
                        "type": "scheme_error",
                        "message": dvorak_scheme_message(),
                    }));
                    return Ok(json!({ "name": name, "events": events }));
                }
            }
            None => {
                events.push(json!({
                    "type": "scheme_error",
                    "message": unknown_chewing_scheme_message(byte),
                }));
                return Ok(json!({ "name": name, "events": events }));
            }
        }
    }
    if let Some(value) = case.get("full_scheme").and_then(Value::as_u64) {
        let byte = u8::try_from(value).unwrap_or(u8::MAX);
        match full_scheme_from_value(byte) {
            Some(scheme) => {
                facade.set_full_scheme(scheme);
            }
            None => {
                events.push(json!({
                    "type": "scheme_error",
                    "message": unknown_full_scheme_message(byte),
                }));
                return Ok(json!({ "name": name, "events": events }));
            }
        }
    }

    // Optional one-key probes.
    if let Some(text) = case.get("probe_chewing").and_then(Value::as_str) {
        events.push(json!({
            "type": "probe_chewing",
            "input": text,
            "key": facade.parse_one_chewing(text).map(key_snapshot),
        }));
    }
    if let Some(text) = case.get("probe_full").and_then(Value::as_str) {
        events.push(json!({
            "type": "probe_full",
            "input": text,
            "key": facade.parse_one_full_pinyin(text).map(key_snapshot),
        }));
    }

    // Optional keyboard-membership probe.
    if let Some(key) = case.get("in_keyboard").and_then(Value::as_str) {
        if let &[byte] = key.as_bytes() {
            events.push(json!({
                "type": "in_keyboard",
                "key": key,
                "symbols": facade.in_keyboard(byte),
            }));
        } else {
            events.push(json!({
                "type": "in_keyboard_error",
                "message": in_keyboard_arity_message(),
            }));
        }
    }

    // The batch parse: fresh facade, so no reset precedes it.
    let mode = case
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("chewing");
    let input = effective_input(case)?;
    events.push(parse_event(facade, "parse", mode, &input));

    // An optional second parse without resetting exercises the
    // begin-parse continuation law (an extending buffer continues, a
    // divergent one restarts).
    if let Some(second) = case.get("parse2") {
        let mode = second
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("chewing");
        let input = second
            .get("input")
            .and_then(Value::as_str)
            .unwrap_or_default();
        events.push(parse_event(facade, "parse2", mode, input));
    }

    if case.get("guess_sentence").and_then(Value::as_bool) == Some(true) {
        match facade.guess_sentence() {
            Ok(ran) => events.push(sentence_event(facade, "guess_sentence", ran)),
            Err(error) => {
                events
                    .push(json!({ "type": "guess_sentence_error", "message": error.to_string() }));
            }
        }
    }
    if let Some(prefix) = case.get("sentence_prefix").and_then(Value::as_str) {
        match facade.guess_sentence_with_prefix(prefix) {
            Ok(ran) => events.push(sentence_event(facade, "guess_sentence_with_prefix", ran)),
            Err(error) => {
                events.push(
                    json!({ "type": "guess_sentence_with_prefix_error", "message": error.to_string() }),
                );
            }
        }
    }

    if let Some(spec) = case.get("guess") {
        let offset = spec
            .get("offset")
            .and_then(Value::as_u64)
            .map(|offset| usize::try_from(offset).unwrap_or(usize::MAX))
            .unwrap_or(0);
        let before = spec.get("before").and_then(Value::as_bool).unwrap_or(false);
        let ran = facade.guess_candidates(offset, before);
        events.push(json!({
            "type": "guess",
            "offset": offset,
            "before": before,
            "ran": ran,
            "candidates": zhuyin_snapshot(facade.candidates()),
        }));
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
        match facade.choose(usize::try_from(index).unwrap_or(usize::MAX)) {
            Ok(cursor) => {
                events.push(json!({
                    "type": "select",
                    "index": index,
                    "cursor": cursor,
                    "offset": facade.composition_offset(),
                    "composing": facade.is_composing(),
                    "top_candidate_texts": zhuyin_top_texts(facade),
                }));
            }
            Err(error) => {
                events.push(json!({ "type": "select_error", "message": error.to_string() }));
                break;
            }
        }
    }

    // An explicit constraint clear: records whether a run was freed and
    // the snapshot head afterwards.
    if let Some(offset) = case.get("clear").and_then(Value::as_u64) {
        let offset = usize::try_from(offset).unwrap_or(usize::MAX);
        let cleared = facade.clear_constraint(offset);
        events.push(json!({
            "type": "clear_constraint",
            "offset": offset,
            "cleared": cleared,
            "top_candidate_texts": zhuyin_top_texts(facade),
        }));
    }

    // An explicit commit step: unlike the pinyin surface, choose answers a
    // cursor rather than a completed/continued verdict, so the corpus
    // commits by instruction, not by signal.
    if case.get("commit").and_then(Value::as_bool) == Some(true) {
        match facade.commit() {
            Ok(text) => events.push(json!({ "type": "commit", "text": text })),
            Err(error) => {
                events.push(json!({ "type": "commit_error", "message": error.to_string() }));
            }
        }
    }

    Ok(json!({ "name": name, "events": events }))
}

/// The parse-plus-guess step both drivers run: batch-parse `input` in
/// `mode`, guess the after-cursor window at 0, and snapshot everything the
/// API returned.
fn parse_event(facade: &mut ZhuyinSession, kind: &str, mode: &str, input: &str) -> Value {
    let consumed = if mode == "full" {
        facade.parse_full_pinyin(input)
    } else {
        facade.parse_chewing(input)
    };
    let ran = facade.guess_candidates(0, false);
    json!({
        "type": kind,
        "mode": mode,
        "consumed": consumed,
        "input": facade.input(),
        "parsed_len": facade.parsed_len(),
        "composing": facade.is_composing(),
        "ran": ran,
        "candidates": zhuyin_snapshot(facade.candidates()),
    })
}

/// The sentence-decode step both drivers run.
fn sentence_event(facade: &ZhuyinSession, kind: &str, ran: bool) -> Value {
    let sentences: Vec<String> = (0..=u8::MAX)
        .map_while(|index| facade.sentence_text(index).map(str::to_owned))
        .collect();
    json!({
        "type": kind,
        "ran": ran,
        "sentences": sentences,
        "top_candidate_texts": zhuyin_top_texts(facade),
    })
}

/// One parsed key as transcript data.
fn key_snapshot(key: oxpinyin_core::ChewingKey) -> Value {
    json!({
        "initial": key.initial,
        "middle": key.middle,
        "final": key.final_,
        "tone": key.tone,
        "packed": key.to_packed(),
        "zhuyin": key.zhuyin_string(),
        "pinyin": key.pinyin_string(),
    })
}

fn zhuyin_snapshot(candidates: &[crate::zhuyin::ZhuyinCandidate]) -> Vec<Value> {
    candidates
        .iter()
        .map(|candidate| {
            json!({
                "text": candidate.text(),
                "kind": kind_label(candidate.kind()),
                "candidate_type": candidate.candidate_type().label(),
                "consumed_bytes": candidate.consumed_bytes(),
                "cost": candidate.cost(),
                "nbest_index": candidate.nbest_index(),
            })
        })
        .collect()
}

fn zhuyin_top_texts(facade: &ZhuyinSession) -> Vec<String> {
    facade
        .candidates()
        .iter()
        .map(|c| c.text().to_owned())
        .take(8)
        .collect()
}
