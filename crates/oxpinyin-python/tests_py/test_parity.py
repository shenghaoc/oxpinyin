"""Native-vs-Python parity.

Replays the shared corpus through the Python binding and compares the
transcript against the one produced by ``native-dump``, which runs the same
corpus through the pure-Rust public API in a process that never touches
Python. The comparison is structural — both transcripts are loaded as
Python objects and their ``events`` compared with ``==`` — not a
byte-comparison of the serialized files. Any behavioural difference between
the two surfaces — candidate ordering, metadata, offsets, sentence rows,
commit text — shows up as a diff here. See the replay procedure documented
at the top of ``parity-corpus.json``.
"""

import json

import pytest

import oxpinyin


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
            "consumed_keys": candidate.consumed_keys,
            "consumed_bytes": candidate.consumed_bytes,
            "cost": candidate.cost,
            "nbest_index": candidate.nbest_index,
        }
        for candidate in candidates
    ]


def top_texts(engine) -> list[str]:
    return [c.text for c in engine.candidates][:8]


def replay_case(case: dict) -> dict:
    """Implements the corpus replay procedure through the binding."""
    engine = oxpinyin.Engine.from_fixture_dir(case["_system_dir"])
    events: list[dict] = []
    input_text = resolve_input(case)

    # 1. batch lookup over a fresh composition
    try:
        engine.reset()
        engine.type_pinyin(input_text)
        events.append(
            {
                "type": "lookup",
                "input": engine.input,
                "composing": engine.composing,
                "parsed_len": engine.parsed_len,
                "candidates": snapshot_candidates(engine.candidates),
            }
        )
    except Exception as error:  # noqa: BLE001 - recorded for comparison
        events.append({"type": "lookup_error", "message": str(error)})
        return {"name": case["name"], "events": events}

    # 2. optional n-best sentence decode
    if case.get("guess_sentence"):
        try:
            ran = engine.guess_sentence()
            events.append(
                {
                    "type": "guess_sentence",
                    "ran": ran,
                    "sentences": engine.sentences,
                    "top_candidate_texts": top_texts(engine),
                }
            )
        except Exception as error:  # noqa: BLE001
            events.append(
                {"type": "guess_sentence_error", "message": str(error)}
            )

    # 3. optional re-anchored window probe
    if "candidates_at" in case and case["candidates_at"] is not None:
        offset = int(case["candidates_at"])
        try:
            window = engine.candidates_at(offset)
            events.append(
                {
                    "type": "candidates_at",
                    "offset": offset,
                    "window": [
                        {
                            "text": c.text,
                            "kind": c.kind,
                            "consumed_bytes": c.consumed_bytes,
                        }
                        for c in window
                    ],
                }
            )
        except Exception as error:  # noqa: BLE001
            events.append(
                {"type": "candidates_at_error", "message": str(error)}
            )

    # 4. selections against the current window; stop once completed
    for index in case.get("select") or []:
        try:
            result = engine.select(int(index))
        except Exception:  # noqa: BLE001 - native side stops silently here
            break
        completed = result == "completed"
        # Snapshot the post-select state before commit clears the
        # composition, mirroring native-dump so the transcripts still line up.
        offset = engine.composition_offset
        preedit = engine.preedit
        composing = engine.composing
        top = top_texts(engine)
        commit_text = engine.commit() if completed else None
        events.append(
            {
                "type": "select",
                "index": int(index),
                "result": result,
                "offset": offset,
                "preedit": preedit,
                "composing": composing,
                "top_candidate_texts": top,
                "commit": commit_text,
            }
        )
        if completed:
            break

    return {"name": case["name"], "events": events}


def test_replayed_corpus_matches_the_native_transcript(
    parity_corpus, native_transcript, fixture_w3
):
    system_dir = str(fixture_w3)
    cases = [dict(case, _system_dir=system_dir) for case in parity_corpus["cases"]]

    python_cases = [replay_case(case) for case in cases]
    native_cases = native_transcript["cases"]

    assert len(python_cases) == len(native_cases)
    for python_case, native_case in zip(python_cases, native_cases):
        assert python_case["name"] == native_case["name"], "case order drifted"
        assert (
            python_case["events"] == native_case["events"]
        ), f"parity divergence in case {python_case['name']!r}"


def test_transcript_schema_is_versioned(native_transcript):
    assert native_transcript["schema"] == "oxpinyin-native-parity-v1"
    assert native_transcript["unigram_source"] == "flat-export-fixture"


def test_corpus_anchors_survive_refactors(parity_corpus):
    names = {case["name"] for case in parity_corpus["cases"]}
    assert {
        "empty-input",
        "nihao",
        "nihao-apostrophe",
        "incomplete-nih",
        "unicode-filtered",
        "nihao-guess-sentence",
        "select-first-completes",
    } <= names
