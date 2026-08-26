# Python API

oxpinyin ships first-class Python bindings (`oxpinyin`) that expose the same
engine C frontends use. The binding contains no algorithm of its own: candidate lists come from
`oxpinyin-engine`'s session API through a thin PyO3 wrapper over
`oxpinyin-runtime` — the same concrete assembly the C ABI consumes, so the
native, C, and Python paths cannot silently diverge. This is also how
the libpinyin-side request for a Python binding (libpinyin issue #181) is
answered — without needing libpinyin installed at all.

## Installation

Python ≥ 3.10 with a Rust toolchain (the pinned one in `rust-toolchain.toml`
works):

```sh
pip install maturin
maturin develop            # inside an activated virtualenv, or:
pip install .              # PEP 517 wheel build through maturin
```

Development tests:

```sh
cargo test -p oxpinyin-python    # Rust runtime tests
pytest                           # binding tests + native-vs-Python parity
```

`pytest` regenerates the native transcript itself via the `native-dump`
binary, so the comparison can never go stale against a golden file.

## Model and data requirements

The engine opens *converted* oxpinyin data files; there is no dependency on a
libpinyin installation:

| File | Required | Purpose |
|---|---|---|
| `pinyin_index.redb` | yes | syllable index |
| `phrase_index.redb` | yes | phrase table |
| `bigram.redb` | yes | bigram language model |
| `interpolation2.text` | production mode | real unigram frequencies driving the pinned candidate ranking |
| `table.conf` | optional | λ override; pinned default otherwise |
| `user_store.redb` | created in `user_dir` | learning persistence |

A directory is converted from libpinyin-format sources by the repository's
usual toolchain (`tools/model/fetch-model.sh` fetches the pinned model;
`oxpinyin-dictool` converts). The committed mini fixture `fixtures/w3` has no
`interpolation2.text`; open it through `Engine.from_fixture_dir`, which falls
back to flat counts derived from the phrase index. That mode exists for
development and tests — production engines need the real-unigram model,
which is why `Engine(system_dir)` raises `FileNotFoundError` without it.

## Basic usage

```python
import oxpinyin

engine = oxpinyin.Engine("/path/to/converted-data")

for candidate in engine.lookup("nihao"):
    print(candidate.text)     # 你好 first, then 你, 尼, ...
```

`lookup(text)` resets the composition each call — independent queries, which
is exactly the issue-#181 batch shape. Input filtering matches the native
batch API: characters outside `a-z` and `'` are dropped silently, and input
stops extending past 4096 bytes.

## Candidates and metadata

Each element of `lookup(...)` / `engine.candidates` is a `Candidate`:

| Attribute | Meaning |
|---|---|
| `text` | Chinese text this candidate inserts |
| `kind` | `"phrase"`, `"addon"`, `"sentence"`, `"fallback"` (future kinds render `"other"`) |
| `consumed_keys` | pinyin keys absorbed |
| `consumed_bytes` | raw-input bytes absorbed |
| `cost` | decoder cost that ranked it — opaque; trust list order |
| `nbest_index` | tail rank when this is an n-best sentence row, else 0 |

Ordering is the engine's rank order, best first.

## Selection workflow (stateful)

```python
from pathlib import Path

data_dir = "/path/to/converted-data"

user_dir = Path("~/.local/share/oxpinyin").expanduser()
user_dir.mkdir(parents=True, exist_ok=True)

with oxpinyin.Engine(data_dir, user_dir=str(user_dir)) as engine:
    engine.type_pinyin("ni'hao")
    print(engine.preedit)            # current display text
    choice = engine.select(0)        # "continued" or "completed"
    if choice == "completed":
        text = engine.commit()
    engine.train()                   # learn the recorded sentence
    engine.save()                    # persist user state when modified
```

The directory must already exist — as with the native init, an unusable
user dir silently degrades to "no learning" instead of failing construction;
the typo surfaces only later, as `train()` refusing and `save()` returning
False.

Extra surface: `candidates_at(offset)` builds the per-offset window the C ABI
offers without disturbing engine state; `input`, `composing`,
`composition_offset` and `parsed_len` report state; `reset()` discards the
composition.

## Sentences

```python
engine.lookup("nihao")
if engine.guess_sentence():
    print(engine.sentences)          # n-best rows, best first
```

Sentence rows prepend to `engine.candidates` while they live, mirroring the
native behaviour.

## Errors

| Condition | Exception |
|---|---|
| Missing data dir / required file / missing model | `FileNotFoundError` |
| Unreadable file | `OSError` |
| Corrupt content that parses badly | `ValueError` |
| Stale/out-of-range candidate index | `IndexError` |
| Out-of-range lookup offset | `ValueError` |
| Backend failure (dictionary/model/user store/decode) | `oxpinyin.OxpinyinError` (subclass of `RuntimeError`) |

Nothing panics across the boundary: PyO3 converts any unwinding into
`pyo3_runtime.PanicException`, and the underlying crates are panic-free by
constitution. Binding code uses no explicit `unsafe`.

## Thread-safety

Engines may be shared across Python threads. Every operation serializes on
an internal mutex and runs with the GIL released, so concurrent `lookup(...)`
calls are correct and deterministic (the sequence interleaves, the results do
not change). This deliberately exceeds what a TSF/IMK main-thread shell
needs; it costs one lock acquisition per call.

## Learning

With `user_dir` given, `train()` records the composed/sentence history into
the user store exactly like the native `pinyin_train`, and `save()` persists
when anything changed (returns `bool`). Without `user_dir`, `train()` refuses
and `save()` returns `False`. A user directory must already exist; opening it
is best-effort like the C ABI — failures degrade to "no user state" rather
than failing construction, so candidate computation never depends on writable
storage.

## Supported platforms

Any platform PyO3 + maturin build for (CPython 3.10–3.14, built with the
stable `abi3`-style single extension). CI exercises Linux; macOS and Windows
run the same portable crates (`oxpinyin-core/data/user/engine`) that the
portable CI job covers, but wheel builds there are currently untested.

## Testing strategy

The corpus lives at `crates/oxpinyin-python/parity-corpus.json` together with
its replay procedure. One binary (`native-dump`) replays it through the pure
Rust API; the pytest suite replays it through the binding; the transcripts
must match byte-for-byte. Because all three DB backends funnel the shipping
data path through the same redb tables (LMDB/Tkrzw are compile-time
alternatives behind cargo features), the Python surface inherits backend
behaviour automatically rather than selecting paths of its own.

## Deliberate omissions (v0)

- Addon phrase libraries and the punctuation table remain C-ABI-only.
- Double-pinyin/Zhuyin input schemes are not bound (parse-level features the
  capi drives separately).
- Live configuration (page size, incomplete-pinyin toggles) follows the
  captured upstream defaults; no knobs yet.
