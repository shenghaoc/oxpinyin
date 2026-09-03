"""oxpinyin — the libpinyin-compatible pinyin engine, from Python.

Feed a pinyin string, get the same Chinese candidates the native oxpinyin
engine produces::

    import oxpinyin

    with oxpinyin.Engine.from_fixture_dir("fixtures/w3/kct") as engine:
        for candidate in engine.lookup("nihao"):
            print(candidate.text)

See this package's ``README.md`` for model requirements, thread-safety
notes, and the full workflow (selection, sentences, learning).
"""

from oxpinyin._native import (
    Candidate,
    Engine,
    OxpinyinError,
    __version__,
)

__all__ = ["Candidate", "Engine", "OxpinyinError", "__version__"]
