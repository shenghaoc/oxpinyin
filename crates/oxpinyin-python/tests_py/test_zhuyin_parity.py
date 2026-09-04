"""Native-vs-Python parity for the zhuyin binding.

Replays the shared zhuyin corpus through the Python binding and compares the
transcript against the one produced by ``zhuyin-dump``, which runs the same
corpus through the pure-Rust facade in a process that never touches Python.
The comparison is structural — both transcripts are loaded as Python objects
and their ``events`` compared with ``==`` — not a byte-comparison of the
serialized files. Any behavioural difference between the two surfaces —
parsed lengths, candidate ordering, metadata, offsets, sentence rows, commit
text — shows up as a diff here. See the replay procedure documented at the
top of ``parity-corpus-zhuyin.json``.
"""

import pytest

import oxpinyin.zhuyin


def resolve_input(case: dict) -> str:
    if "input" in case:
        return case["input"]
    repeat = case.get("repeat") or {}
    unit, times = repeat.get("unit", ""), int(repeat.get("times", 0))
    return unit * times


def snapshot_candidates(candidates) -> list[dict]:
    return [
        {
            "text": candidate.text,
            "kind": candidate.kind,
            "candidate_type": candidate.candidate_type,
            "consumed_bytes": candidate.consumed_bytes,
            "cost": candidate.cost,
            "nbest_index": candidate.nbest_index,
        }
        for candidate in candidates
    ]


def snapshot_key(key) -> dict | None:
    if key is None:
        return None
    return {
        "initial": key.initial,
        "middle": key.middle,
        "final": key.final,
        "tone": key.tone,
        "packed": key.packed,
        "zhuyin": key.zhuyin_string(),
        "pinyin": key.pinyin_string(),
    }


def top_texts(engine) -> list[str]:
    return [c.text for c in engine.candidates][:8]


def parse_event(engine, kind: str, mode: str, input_text: str) -> dict:
    """The parse-plus-guess step both drivers run."""
    if mode == "full":
        consumed = engine.parse_full_pinyin(input_text)
    else:
        consumed = engine.parse_chewing(input_text)
    ran = engine.guess_candidates(0)
    return {
        "type": kind,
        "mode": mode,
        "consumed": consumed,
        "input": engine.input,
        "parsed_len": engine.parsed_len,
        "composing": engine.composing,
        "ran": ran,
        "candidates": snapshot_candidates(engine.candidates),
    }


def sentence_event(engine, kind: str, ran: bool) -> dict:
    return {
        "type": kind,
        "ran": ran,
        "sentences": engine.sentences,
        "top_candidate_texts": top_texts(engine),
    }


def replay_case(case: dict) -> dict:
    """Implements the corpus replay procedure through the binding."""
    engine = oxpinyin.zhuyin.Engine.from_fixture_dir(case["_system_dir"])
    events: list[dict] = []

    # 1. optional scheme setup; a rejection ends the case
    if "chewing_scheme" in case and case["chewing_scheme"] is not None:
        try:
            engine.chewing_scheme = int(case["chewing_scheme"])
        except Exception as error:  # noqa: BLE001 - recorded for comparison
            events.append({"type": "scheme_error", "message": str(error)})
            return {"name": case["name"], "events": events}
    if "full_scheme" in case and case["full_scheme"] is not None:
        try:
            engine.full_pinyin_scheme = int(case["full_scheme"])
        except Exception as error:  # noqa: BLE001 - recorded for comparison
            events.append({"type": "scheme_error", "message": str(error)})
            return {"name": case["name"], "events": events}

    # 2. optional one-key probes
    if "probe_chewing" in case and case["probe_chewing"] is not None:
        text = case["probe_chewing"]
        events.append(
            {
                "type": "probe_chewing",
                "input": text,
                "key": snapshot_key(engine.parse_one_chewing(text)),
            }
        )
    if "probe_full" in case and case["probe_full"] is not None:
        text = case["probe_full"]
        events.append(
            {
                "type": "probe_full",
                "input": text,
                "key": snapshot_key(engine.parse_one_full_pinyin(text)),
            }
        )

    # 3. optional keyboard-membership probe
    if "in_keyboard" in case and case["in_keyboard"] is not None:
        key = case["in_keyboard"]
        try:
            events.append(
                {"type": "in_keyboard", "key": key, "symbols": engine.in_keyboard(key)}
            )
        except Exception as error:  # noqa: BLE001 - recorded for comparison
            events.append({"type": "in_keyboard_error", "message": str(error)})

    # 4. the batch parse over a fresh facade
    mode = case.get("mode") or "chewing"
    events.append(parse_event(engine, "parse", mode, resolve_input(case)))

    # 5. optional second parse without resetting
    if "parse2" in case and case["parse2"] is not None:
        second = case["parse2"]
        events.append(
            parse_event(
                engine,
                "parse2",
                second.get("mode") or "chewing",
                second.get("input") or "",
            )
        )

    # 6. optional sentence decodes
    if case.get("guess_sentence"):
        try:
            events.append(
                sentence_event(engine, "guess_sentence", engine.guess_sentence())
            )
        except Exception as error:  # noqa: BLE001
            events.append(
                {"type": "guess_sentence_error", "message": str(error)}
            )
    if "sentence_prefix" in case and case["sentence_prefix"] is not None:
        prefix = case["sentence_prefix"]
        try:
            events.append(
                sentence_event(
                    engine,
                    "guess_sentence_with_prefix",
                    engine.guess_sentence_with_prefix(prefix),
                )
            )
        except Exception as error:  # noqa: BLE001
            events.append(
                {
                    "type": "guess_sentence_with_prefix_error",
                    "message": str(error),
                }
            )

    # 7. optional re-anchored window probe
    if "guess" in case and case["guess"] is not None:
        spec = case["guess"]
        offset = int(spec.get("offset") or 0)
        before = bool(spec.get("before", False))
        ran = engine.guess_candidates(offset, before)
        events.append(
            {
                "type": "guess",
                "offset": offset,
                "before": before,
                "ran": ran,
                "candidates": snapshot_candidates(engine.candidates),
            }
        )

    # 8. selections against the current snapshot; stop on error
    for index in case.get("select") or []:
        try:
            cursor = engine.select(int(index))
        except Exception as error:  # noqa: BLE001 - recorded for comparison
            events.append({"type": "select_error", "message": str(error)})
            break
        events.append(
            {
                "type": "select",
                "index": int(index),
                "cursor": cursor,
                "offset": engine.composition_offset,
                "composing": engine.composing,
                "top_candidate_texts": top_texts(engine),
            }
        )

    # 9. optional constraint clear
    if "clear" in case and case["clear"] is not None:
        offset = int(case["clear"])
        events.append(
            {
                "type": "clear_constraint",
                "offset": offset,
                "cleared": engine.clear_constraint(offset),
                "top_candidate_texts": top_texts(engine),
            }
        )

    # 10. optional explicit commit
    if case.get("commit"):
        try:
            events.append({"type": "commit", "text": engine.commit()})
        except Exception as error:  # noqa: BLE001
            events.append({"type": "commit_error", "message": str(error)})

    return {"name": case["name"], "events": events}


def test_replayed_corpus_matches_the_native_transcript(
    zhuyin_parity_corpus, zhuyin_native_transcript, fixture_w3
):
    system_dir = str(fixture_w3)
    cases = [
        dict(case, _system_dir=system_dir) for case in zhuyin_parity_corpus["cases"]
    ]

    python_cases = [replay_case(case) for case in cases]
    native_cases = zhuyin_native_transcript["cases"]

    assert len(python_cases) == len(native_cases)
    for python_case, native_case in zip(python_cases, native_cases):
        assert python_case["name"] == native_case["name"], "case order drifted"
        assert (
            python_case["events"] == native_case["events"]
        ), f"parity divergence in case {python_case['name']!r}"


def test_transcript_schema_is_versioned(zhuyin_native_transcript):
    assert zhuyin_native_transcript["schema"] == "oxpinyin-zhuyin-native-parity-v1"


def test_corpus_anchors_survive_refactors(zhuyin_parity_corpus):
    names = {case["name"] for case in zhuyin_parity_corpus["cases"]}
    assert {
        "empty-input",
        "nihao",
        "nihao-toneless-refused",
        "single-ni",
        "scheme-dvorak-rejected",
        "nihao-guess-sentence",
        "select-first-commits",
        "select-stale",
    } <= names
