"""Behavioural tests for the oxpinyin Python binding.

These exercise the built extension through the real package API — no mocks.
The native-vs-Python equality itself lives in test_parity.py; here we pin
the Python-side contract: shapes, error mapping, lifecycle, thread-safety,
and user learning.
"""

import threading

import pytest

import oxpinyin
from oxpinyin import Candidate, Engine, OxpinyinError


def test_issue_181_workflow(make_engine):
    """The motivating use case, verbatim in spirit."""
    engine = make_engine()
    candidates = engine.lookup("nihao")
    assert candidates[0].text == "你好"
    texts = [candidate.text for candidate in candidates]
    assert "你" in texts


def test_candidate_metadata(make_engine):
    (first, *rest) = make_engine().lookup("nihao")
    assert isinstance(first, Candidate)
    assert first.kind in {"phrase", "addon", "sentence", "fallback", "other"}
    assert first.consumed_bytes == 5  # nihao parses fully
    assert isinstance(first.consumed_keys, int)
    assert isinstance(first.cost, int)
    assert first.nbest_index == 0
    assert rest, "multiple candidates offered"
    assert repr(first).startswith('Candidate(text="你好"')
    assert str(first) == "你好"


def test_lookup_calls_are_independent_queries(make_engine):
    engine = make_engine()
    assert [c.text for c in engine.lookup("nihao")] == [
        c.text for c in engine.lookup("nihao")
    ]
    assert engine.composition_offset == 0


def test_stateful_selection_flow(make_engine):
    engine = make_engine()
    assert engine.type_pinyin("ni") is True
    assert engine.composing is True
    assert engine.preedit == "ni"
    assert engine.type_pinyin("hao") is True

    candidates = engine.candidates
    assert candidates[0].text == "你好"

    assert engine.select(0) == "completed"
    assert engine.commit() == "你好"
    assert engine.composing is False
    assert engine.preedit == ""


def test_reset_discards_the_composition(make_engine):
    engine = make_engine()
    engine.type_pinyin("nihao")
    engine.reset()
    assert engine.input == ""
    assert engine.candidates == []
    assert engine.composing is False


def test_guess_sentence_and_rows(make_engine):
    engine = make_engine()
    engine.lookup("nihao")
    assert engine.guess_sentence() is True
    assert engine.sentences[0] == "你好"
    assert engine.sentence(0) == "你好"
    assert engine.sentence(9) is None
    # rows prepend to the candidate list head
    assert engine.candidates[0].text == "你好"


def test_parsed_len_and_input_reflect_filtering(make_engine):
    engine = make_engine()
    engine.lookup("nǐ hǎo")
    # ǐ, ǎ and the space are filtered; n, h, o stay
    assert engine.input == "nho"
    filtered = [c.text for c in engine.lookup("nǐhǎo")]
    raw = [c.text for c in engine.lookup("nho")]
    assert filtered == raw


def test_input_stops_at_the_native_cap(make_engine):
    engine = make_engine()
    engine.lookup("ni" * 3000)
    assert len(engine.input) <= 4096
    assert len(engine.input) % 2 == 0


def test_candidates_at_reanchors_without_disturbing_state(make_engine):
    engine = make_engine()
    engine.type_pinyin("nini")
    window = engine.candidates_at(2)
    assert window, "window at offset 2 has candidates"
    for candidate in window:
        assert candidate.consumed_bytes > 0
    # the cached list and cursor are untouched
    assert engine.composition_offset == 0


def test_open_errors_map_to_python_exceptions(fixture_w3):
    with pytest.raises(FileNotFoundError):
        Engine("/nonexistent/oxpinyin-data-dir")
    with pytest.raises(FileNotFoundError):
        Engine.from_fixture_dir("/nonexistent/oxpinyin-data-dir")
    # production mode requires interpolation2.text; the fixture lacks it
    with pytest.raises(FileNotFoundError, match="interpolation2"):
        Engine(str(fixture_w3))


def test_index_and_offset_errors_map_cleanly(make_engine):
    engine = make_engine()
    engine.lookup("nihao")
    with pytest.raises(IndexError):
        engine.select(999)
    with pytest.raises(ValueError):
        engine.candidates_at(99)
    with pytest.raises(OverflowError):
        # the native offsets are unsigned; a negative does not fit
        engine.select(-1)


def test_learning_requires_a_user_dir(make_engine):
    engine = make_engine()  # no user_dir
    with pytest.raises(OxpinyinError):
        engine.train()
    assert engine.save() is False


def test_train_and_save_persist_user_state(tmp_path, fixture_w3):
    user_dir = tmp_path / "user-state"
    user_dir.mkdir()  # like capi's TempUserDir: the store opens inside it
    engine = Engine.from_fixture_dir(str(fixture_w3), str(user_dir))
    assert engine.save() is False  # unmodified
    engine.lookup("nihao")
    engine.select(0)
    engine.train()
    assert engine.save() is True  # dirty → saved
    assert engine.save() is False  # now clean again
    # The store file's extension names the compiled-in backend (.redb by
    # default, the others by explicit feature selection); assert a store
    # file exists without pinning which one.
    assert any(user_dir.glob("user_store.*"))

    # a second engine over the same user state loads it cleanly
    reloaded = Engine.from_fixture_dir(str(fixture_w3), str(user_dir))
    first = [c.text for c in reloaded.lookup("nihao")]
    second = [c.text for c in reloaded.lookup("nihao")]
    assert first == second


def test_engines_are_independent(make_engine):
    a = make_engine()
    b = make_engine()
    a.type_pinyin("nihao")
    assert b.input == ""  # no cross-talk
    assert b.candidates == []


def test_context_manager_works(make_engine):
    with make_engine() as engine:
        assert engine.lookup("nihao")[0].text == "你好"
    # close() is also directly callable and idempotent-friendly
    make_engine().close()


def test_shared_engine_is_thread_safe(make_engine):
    engine = make_engine()
    expected = [c.text for c in engine.lookup("nihao")]
    errors = []

    def worker():
        try:
            for _ in range(40):
                got = [c.text for c in engine.lookup("nihao")]
                assert got == expected, "concurrent lookups diverged"
                _ = engine.composition_offset
                _ = engine.preedit
        except Exception as error:  # noqa: BLE001 - collected below
            errors.append(error)

    threads = [threading.Thread(target=worker) for _ in range(8)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    assert not errors, errors


def test_version_is_exposed():
    assert oxpinyin.__version__.startswith("0.1.")
