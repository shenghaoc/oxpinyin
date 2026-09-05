"""oxpinyin.zhuyin — the libzhuyin-compatible chewing engine, from Python.

Feed chewing keystrokes (bopomofo keyboard codes), get the same Chinese
candidates the native zhuyin facade produces::

    import oxpinyin.zhuyin

    with oxpinyin.zhuyin.Engine.from_fixture_dir("fixtures/w3/tkt") as engine:
        for candidate in engine.lookup_chewing("su3cl3"):
            print(candidate.text)

Keystrokes are Standard-keyboard codes by default (``s`` is ㄋ, ``u`` is
ㄧ, ``3`` is the third tone, …); other keyboards arrive through
``chewing_scheme``. Tones are mandatory under the facade's default
``USE_TONE | FORCE_TONE`` options — toneless input parses to nothing, which
is the pinned upstream batch law, not a binding limitation.

See ``docs/python.md`` for model requirements, thread-safety notes, and the
full workflow (selection, sentences, learning).
"""

from oxpinyin._native.zhuyin import (
    Candidate,
    ChewingKey,
    Engine,
)

__all__ = ["Candidate", "ChewingKey", "Engine"]
