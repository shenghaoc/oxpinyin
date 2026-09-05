"""Behavioural tests for the oxpinyin.zhuyin Python binding.

These exercise the built extension through the real package API — no mocks.
The native-vs-Python equality itself lives in test_zhuyin_parity.py; here we
pin the Python-side contract: shapes, error mapping, lifecycle,
thread-safety, and user learning.

Keystrokes are Standard-keyboard codes (``s`` is ㄋ, ``u`` is ㄧ, ``3`` is
the third tone, …), so ``su3cl3`` parses to ni+hao — the chewing twin of
the pinyin suite's ``nihao``.
"""

import threading

import pytest

import oxpinyin.zhuyin
from oxpinyin.zhuyin import Candidate, ChewingKey, Engine
from oxpinyin import OxpinyinError


def test_lookup_chewing_workflow(make_zhuyin_engine):
    engine = make_zhuyin_engine()
    candidates = engine.lookup_chewing("su3cl3")
    assert candidates[0].text == "你好"
    texts = [candidate.text for candidate in candidates]
    assert "你" in texts


def test_candidate_metadata(make_zhuyin_engine):
    (first, *rest) = make_zhuyin_engine().lookup_chewing("su3cl3")
    assert isinstance(first, Candidate)
    assert first.kind in {"phrase", "addon", "sentence", "fallback", "other"}
    # The zhuyin-local list tag — never the pinyin eight-value tag its
    # discriminants collide with at 3 and 4.
    assert first.candidate_type in {
        "best_match",
        "normal_after_cursor",
        "normal_before_cursor",
        "zombie",
    }
    assert first.candidate_type == "normal_after_cursor"
    assert first.consumed_bytes == 6  # su3cl3 parses fully
    assert isinstance(first.cost, int)
    assert first.nbest_index == 0
    assert rest, "multiple candidates offered"
    assert repr(first).startswith('Candidate(text="你好"')
    assert str(first) == "你好"


def test_lookup_calls_are_independent_queries(make_zhuyin_engine):
    engine = make_zhuyin_engine()
    assert [c.text for c in engine.lookup_chewing("su3cl3")] == [
        c.text for c in engine.lookup_chewing("su3cl3")
    ]
    assert engine.composition_offset == 0


def test_lookup_full_pinyin(make_zhuyin_engine):
    engine = make_zhuyin_engine()
    assert engine.lookup_full_pinyin("ni3hao3")[0].text == "你好"


def test_toneless_chewing_parses_to_nothing(make_zhuyin_engine):
    """The pinned FORCE_TONE batch law: toneless keys are refused."""
    engine = make_zhuyin_engine()
    assert engine.parse_chewing("sucl") == 0
    assert engine.lookup_chewing("sucl") == []


def test_stateful_selection_flow(make_zhuyin_engine):
    engine = make_zhuyin_engine()
    assert engine.parse_chewing("su3cl3") == 6
    assert engine.composing is True
    assert engine.input == "su3cl3"
    assert engine.parsed_len == 6

    assert engine.guess_candidates() is True
    candidates = engine.candidates
    assert candidates[0].text == "你好"

    assert engine.select(0) == 6
    assert engine.commit() == "你好"
    assert engine.composing is False
    assert engine.preedit == ""


def test_incremental_parse_continues(make_zhuyin_engine):
    engine = make_zhuyin_engine()
    assert engine.parse_chewing("su3") == 3
    assert engine.parse_chewing("su3cl3") == 6
    assert engine.input == "su3cl3"


def test_reset_discards_the_composition(make_zhuyin_engine):
    engine = make_zhuyin_engine()
    engine.parse_chewing("su3cl3")
    engine.reset()
    assert engine.input == ""
    assert engine.candidates == []
    assert engine.composing is False


def test_guess_sentence_and_rows(make_zhuyin_engine):
    engine = make_zhuyin_engine()
    engine.lookup_chewing("su3cl3")
    assert engine.guess_sentence() is True
    assert engine.sentences[0] == "你好"
    assert engine.sentence(0) == "你好"
    assert engine.sentence(9) is None


def test_guess_sentence_with_prefix(make_zhuyin_engine):
    engine = make_zhuyin_engine()
    engine.lookup_chewing("su3cl3")
    assert engine.guess_sentence_with_prefix("你") is True
    assert engine.sentences[0] == "你好"


def test_guess_before_and_after_cursor(make_zhuyin_engine):
    engine = make_zhuyin_engine()
    engine.parse_chewing("su3cl3")
    assert engine.guess_candidates(3) is True
    assert engine.candidates[0].text == "好"
    assert engine.guess_candidates(6, before_cursor=True) is True
    assert engine.candidates, "before-cursor window at the parse end is non-empty"
    # An out-of-range offset refuses with False, not an exception — the C
    # bool contract — and clears the snapshot.
    assert engine.guess_candidates(99) is False
    assert engine.candidates == []


def test_schemes_round_trip_and_reject(make_zhuyin_engine):
    engine = make_zhuyin_engine()
    assert engine.chewing_scheme == 1
    assert engine.full_pinyin_scheme == 1
    engine.chewing_scheme = 3
    assert engine.chewing_scheme == 3
    engine.chewing_scheme = 1
    engine.full_pinyin_scheme = 2
    assert engine.full_pinyin_scheme == 2
    engine.full_pinyin_scheme = 1
    with pytest.raises(ValueError, match="StandardDvorak"):
        engine.chewing_scheme = 7
    with pytest.raises(ValueError, match="unknown zhuyin keyboard"):
        engine.chewing_scheme = 42
    with pytest.raises(ValueError, match="unknown full-pinyin"):
        engine.full_pinyin_scheme = 5


def test_one_key_probes(make_zhuyin_engine):
    engine = make_zhuyin_engine()
    key = engine.parse_one_chewing("su3")
    assert isinstance(key, ChewingKey)
    assert (key.initial, key.middle, key.final, key.tone) == (11, 1, 0, 3)
    assert key.zhuyin_string() == "ㄋㄧˇ"
    assert key.pinyin_string() == "ni3"
    assert engine.parse_one_chewing("A") is None
    full = engine.parse_one_full_pinyin("ni3")
    assert full is not None
    assert (full.initial, full.middle, full.final, full.tone) == (11, 1, 0, 3)
    assert engine.parse_one_full_pinyin("zzz") is None


def test_chewing_key_values_and_renderers(make_zhuyin_engine):
    engine = make_zhuyin_engine()
    key = ChewingKey.from_pinyin("zhang")
    assert key is not None
    assert key.shengmu_string() == "zh"
    assert key.yunmu_string() == "ang"
    assert key.zhuyin_string() == "ㄓㄤ"
    assert key.table_index() > 0
    # The packed word round-trips through the ABI form.
    clone = ChewingKey.from_packed(key.packed)
    assert (clone.initial, clone.middle, clone.final, clone.tone) == (
        key.initial,
        key.middle,
        key.final,
        key.tone,
    )
    assert repr(key).startswith("ChewingKey(initial=")
    assert str(key) == "ㄓㄤ"
    assert ChewingKey.from_pinyin("zzz") is None
    # The engine-level renderers dispatch on the live full-pinyin scheme;
    # the zero key answers None on both (the C getters' false).
    zero = ChewingKey.from_packed(0)
    assert zero.table_index() == 0
    assert engine.zhuyin_string(zero) is None
    assert engine.pinyin_string(zero) is None
    assert engine.zhuyin_string(key) == "ㄓㄤ"
    assert engine.pinyin_string(key) == "zhang"
    engine.full_pinyin_scheme = 2
    assert engine.pinyin_string(key) == "jhang"
    engine.full_pinyin_scheme = 3
    assert engine.pinyin_string(key) == "jang"
    engine.full_pinyin_scheme = 1


def test_in_keyboard(make_zhuyin_engine):
    engine = make_zhuyin_engine()
    assert engine.in_keyboard("s") == ["ㄋ"]
    assert engine.in_keyboard("A") == []
    with pytest.raises(ValueError, match="single keystroke"):
        engine.in_keyboard("su")


def test_clear_constraint_frees_the_run(make_zhuyin_engine):
    engine = make_zhuyin_engine()
    engine.lookup_chewing("su3cl3")
    engine.select(1)
    assert engine.clear_constraint(0) is True
    assert engine.clear_constraint(0) is False
    # The engine stays usable afterwards.
    assert engine.lookup_chewing("su3cl3")[0].text == "你好"


def test_open_errors_map_to_python_exceptions(fixture_w3):
    with pytest.raises(FileNotFoundError):
        Engine("/nonexistent/oxpinyin-data-dir")
    with pytest.raises(FileNotFoundError):
        Engine.from_fixture_dir("/nonexistent/oxpinyin-data-dir")
    # Production mode opens the same complete mini data directory the
    # fixture constructor does (see test_engine.py).
    with Engine(str(fixture_w3)) as engine:
        assert engine.lookup_chewing("su3cl3")


def test_index_errors_map_cleanly(make_zhuyin_engine):
    engine = make_zhuyin_engine()
    engine.lookup_chewing("su3cl3")
    with pytest.raises(IndexError):
        engine.select(9999)
    with pytest.raises(OverflowError):
        # the native offsets are unsigned; a negative does not fit
        engine.select(-1)


def test_learning_requires_a_user_dir(make_zhuyin_engine):
    engine = make_zhuyin_engine()  # no user_dir
    with pytest.raises(OxpinyinError):
        engine.train()
    assert engine.save() is False


def test_train_and_save_persist_user_state(tmp_path, fixture_w3):
    user_dir = tmp_path / "user-state"
    user_dir.mkdir()  # like capi's TempUserDir: the store opens inside it
    engine = Engine.from_fixture_dir(str(fixture_w3), str(user_dir))
    assert engine.save() is False  # unmodified
    engine.lookup_chewing("su3cl3")
    engine.select(0)
    engine.train()
    assert engine.save() is True  # dirty → saved
    assert engine.save() is False  # now clean again
    # The store file's extension names the compiled-in backend (.tkt by
    # default, .redb under --no-default-features, …); assert a store file
    # exists without pinning which one.
    assert any(user_dir.glob("user_store.*"))

    # a second engine over the same user state loads it cleanly
    reloaded = Engine.from_fixture_dir(str(fixture_w3), str(user_dir))
    first = [c.text for c in reloaded.lookup_chewing("su3cl3")]
    second = [c.text for c in reloaded.lookup_chewing("su3cl3")]
    assert first == second


def test_engines_are_independent(make_zhuyin_engine):
    a = make_zhuyin_engine()
    b = make_zhuyin_engine()
    a.parse_chewing("su3cl3")
    assert b.input == ""  # no cross-talk
    assert b.candidates == []


def test_context_manager_works(make_zhuyin_engine):
    with make_zhuyin_engine() as engine:
        assert engine.lookup_chewing("su3cl3")[0].text == "你好"
    # close() is also directly callable and idempotent-friendly
    make_zhuyin_engine().close()


def test_shared_engine_is_thread_safe(make_zhuyin_engine):
    engine = make_zhuyin_engine()
    expected = [c.text for c in engine.lookup_chewing("su3cl3")]
    errors = []

    def worker():
        try:
            for _ in range(40):
                got = [c.text for c in engine.lookup_chewing("su3cl3")]
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
