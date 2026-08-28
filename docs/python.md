# Python API

oxpinyin ships first-class Python bindings (`oxpinyin`) that expose the same
engine C frontends use. The binding contains no algorithm of its own:
candidate lists come from `oxpinyin-engine`'s session API through a thin
PyO3 wrapper over `oxpinyin-runtime` — the same concrete assembly the C ABI
consumes, so the native, C, and Python paths cannot silently diverge.
It serves the use case described in
[libpinyin issue #181](https://github.com/libpinyin/libpinyin/issues/181) —
call a pinyin engine from Python and get Chinese candidates back — without
requiring libpinyin.

## Installation

Free-threaded Python 3.14 with a Rust toolchain (the pinned one in
`rust-toolchain.toml` works):

```sh
cd crates/oxpinyin-python  # this package lives inside the workspace
pip install maturin        # only needed for the develop loop below
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
| Backend failure (dictionary/model/user-store read or save/decode) | `oxpinyin.OxpinyinError` (subclass of `RuntimeError`) |

Nothing panics across the boundary: PyO3 converts any unwinding into
`pyo3_runtime.PanicException`, and the underlying crates are panic-free by
constitution. Binding code uses no explicit `unsafe`.

## Thread-safety

Engines may be shared across Python threads: every call takes an internal
mutex, so calls from different threads serialize rather than interleave.

**The guarantee is per call, and only per call.** One call — `lookup(...)`,
`select(...)`, a property read — takes the lock, does its work and releases
it, so it sees a consistent session and returns a snapshot nothing can
mutate afterwards. A *sequence* of calls is not atomic, because the lock is
released between them:

```python
engine.lookup("nihao")
print(engine.preedit)      # not necessarily the preedit for "nihao"
```

Another thread's `lookup` can land in that gap. Under a GIL the gap was
narrow enough to miss; without one it is a live footgun. It is also why
`test_shared_engine_is_thread_safe` reads `composition_offset` and `preedit`
between lookups without asserting on them — those values mean something only
to a thread that owns the engine for the whole sequence.

A caller needing a consistent view across several members must therefore
either hold its own lock around the whole sequence, or give each thread its
own private `Engine`. `lookup(...)` is shaped to avoid the question
entirely: it resets, types and returns the candidate list inside one locked
call, so the batch-query workflow needs no caller-side locking.

Operations that decode — `lookup`, `type_pinyin`, `select`, `commit`,
`guess_sentence`, `train`, `save`, `candidates_at`, and the `parsed_len`
property — release the GIL around the engine call, as does `reset`;
snapshot-style reads (`candidates`, `sentences`, `sentence`, and the
remaining property getters) hold it while guarding. `parsed_len` sits with
the decoders because `Session::full_parsed_len` rebuilds the segment graph
on every call rather than reading a stored value. Sharing one
engine at all deliberately exceeds what a TSF/IMK main-thread shell needs;
it costs one lock acquisition per call.

## Learning

When the user store opens successfully — which needs a usable `user_dir` —
`train()` records the composed/sentence history into it exactly like the
native `pinyin_train`, and `save()` persists when anything changed — `True`
when it wrote, `False` when there was nothing to persist. When it does not
open — no `user_dir`, or one that cannot be opened — `train()` refuses and
`save()` returns `False`. `False` is reserved for those "nothing to persist"
cases (no user store, or an unmodified one); a store-level persistence failure
raises `OxpinyinError` rather than being swallowed into a `False`. A user
directory must already exist; opening it is best-effort like the C ABI —
failures degrade to "no user state" rather than failing construction, so
candidate computation never depends on writable storage.

## Supported platforms

Free-threaded CPython 3.14, on any platform PyO3 + maturin build for. CI
exercises Linux; macOS and Windows run the same portable crates
(`oxpinyin-core/data/user/engine`) that the portable CI job covers, but
wheel builds there are currently untested.

CI runs exactly one interpreter — free-threaded CPython 3.14 on Linux — and
it builds from source there (`pip install .` through maturin), so that one
source-built configuration is the whole of what these tests prove.
`requires-python = ">=3.14"` only gates where pip will *install* the package;
it does not describe what a build is binary-compatible with. That is fixed at
build time by pyo3's stable-ABI settings — `abi3-py310` for standard CPython,
`abi3t-py315` for the free-threaded build — not inferred from the version
range. This project publishes and tests no pre-built wheels, so nothing beyond
the source build on free-threaded 3.14 is claimed here.

Free-threaded 3.14 is the platform this binding is written for. `Engine.lookup`
runs through `Engine::with_session`, which releases the GIL before it acquires
the shared-`Engine` mutex and performs the native operation, so the
shared-engine thread-safety test contends that mutex for real even on a GIL
build — only the Python-side work outside that detached region stays
serialized. GIL builds are neither claimed nor tested here.

## Testing strategy

The corpus lives at `crates/oxpinyin-python/parity-corpus.json` together with
its replay procedure. One binary (`native-dump`) replays it through the pure
Rust API; the pytest suite replays it through the binding;
`test_replayed_corpus_matches_the_native_transcript` then asserts structural
equality of the loaded event objects (not of serialized bytes) per case.
The shipping data path always uses redb — `oxpinyin-runtime` opens tables
through `GenericLookupTable<DefaultStore>` and learning through
`UserStore = GenericUserStore<DefaultStore>`, where
`DefaultStore = RedbStore`. LMDB and Tkrzw are optional backends behind
cargo features that `oxpinyin-python` does not enable, so their coverage
belongs to the separate store/backend differential tests, not to Python
parity.

Known gap: no corpus case opens a `user_dir`. All 18 cases run against
`fixtures/w3` with user learning off, on both sides — `native-dump` calls
`Runtime::open_fixtures(system_dir, None)` and the pytest driver calls
`Engine.from_fixture_dir(system_dir)` with `user_dir` defaulting to `None`
— so the user-overlay ranking path is never compared native-vs-Python.
That path is covered only in part.
`test_engine.py::test_train_and_save_persist_user_state` exercises
persistence and reload — `save()` flipping dirty→clean, the store file
appearing, a second engine over the same `user_dir` loading it and
serving stable lookups — but it selects candidate 0, the already-top
entry, so a no-op learning update would pass it too. That an overlay
*changes* ranking is pinned at the native layer instead, by
`oxpinyin-capi::e2e_tests::populated_store_cheapens_the_trained_candidate`,
which asserts the trained token's decoder cost drops strictly below its
empty-store cost — over the same `Session::train` this binding calls.
What no test covers is that the binding and the native API rank an
overlaid candidate list *identically*. Closing that means adding corpus
cases, which changes the corpus, so it is recorded here rather than done.

## Deliberate omissions (v0)

- Addon phrase libraries and the punctuation table remain C-ABI-only.
- Double-pinyin/Zhuyin input schemes are not bound (parse-level features the
  capi drives separately).
- Live configuration (page size, incomplete-pinyin toggles) follows the
  captured upstream defaults; no knobs yet.
