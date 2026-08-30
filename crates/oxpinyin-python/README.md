# oxpinyin (Python)

Python bindings for
[oxpinyin](https://github.com/shenghaoc/oxpinyin), a portable Rust
re-expression of libpinyin. Feed a pinyin string, get the same Chinese
candidate results the native engine produces — the same parser, decoder,
dictionary and ranking code the C ABI frontends use, with no Python-side
algorithm of its own and no dependency on a libpinyin installation.

It serves the use case described in
[libpinyin issue #181](https://github.com/libpinyin/libpinyin/issues/181) —
call a pinyin engine from Python and get Chinese candidates back — without
requiring libpinyin.

```python
import oxpinyin

engine = oxpinyin.Engine.from_fixture_dir("fixtures/w3")
for candidate in engine.lookup("nihao"):
    print(candidate.text)   # 你好, 你, 尼, ...
```

## Install

From a checkout (Rust toolchain required; standard CPython 3.14 or newer,
free-threaded CPython 3.15 or newer):

```sh
pip install maturin
maturin develop            # inside an activated venv, or:
pip install .              # builds a wheel through PEP 517 + maturin
```

The engine needs oxpinyin's own system data: `pinyin_index`,
`phrase_index` and `bigram` tables in the compiled-in backend's
format. The four peer backends produce four distinct extensions — `.kct`
under the default selection (Kyoto Cabinet), `.redb` with
`--no-default-features --features redb`, `.lmdb` with `--features lmdb`,
`.tkt` with `--features tkrzw`. Optional `punct.<ext>` adds
predicted-punctuation rows; its absence simply yields no predicted
punctuation. The repository's committed mini fixture (`fixtures/w3`) works
through `Engine.from_fixture_dir`; production model directories
additionally carry `interpolation2.text` (the real-unigram model) and are
opened with `Engine(system_dir)`. No libpinyin install is required —
only these data files.

## API sketch

- `Engine(system_dir, user_dir=None)` — open over converted data;
  `user_dir` enables learning (`train`/`save` persist to it).
  - Requires `interpolation2.text`; missing or unparsable models raise.
- `Engine.from_fixture_dir(system_dir, user_dir=None)` — fixture semantics
  for development against the committed mini tables.
- `engine.lookup(text)` → `list[Candidate]`, each call a fresh query.
- Stateful workflow: `type_pinyin`, `candidates`, `candidates_at(offset)`,
  `select(index)` → `"continued" | "completed"`, `commit()`, `reset()`.
- Sentences: `guess_sentence()`, `sentences`, `sentence(i)`.
- State inspection: `input`, `composing`, `composition_offset`, `parsed_len`,
  `preedit`.
- Learning: `train()` mirrors native `pinyin_train`; `save()` persists when
  modified.

Errors: missing model/data raise `FileNotFoundError`; unreadable data raises
`OSError`; corrupt content raises `ValueError`; stale candidate indexes raise
`IndexError`; out-of-range offsets raise `ValueError`; backend failures raise
`oxpinyin.OxpinyinError` (a `RuntimeError`). Nothing panics across the
boundary.

Threading: engines may be shared across threads. Calls serialize on an
internal lock and run with the GIL released.

## Tests

```sh
cargo test -p oxpinyin-runtime                      # Rust side (shared assembly)
cargo test -p oxpinyin-python                       # binding crate
cargo run -p oxpinyin-python --bin native-dump -- \
    parity-corpus.json ../../fixtures/w3 native.json  # native transcript
pytest                                              # binding + parity vs above
```

## Licence

GPL-3.0-or-later, matching oxpinyin.
