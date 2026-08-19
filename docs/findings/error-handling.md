# Error-handling audit

Date: 2026-08-19 · Status: characterization. No API reshape. Pins must not
move.

Workspace-wide inventory of how failures are represented, against the Aug
2026 practice: `thiserror` 2.x in library crates, `anyhow` 1.x only at
binary edges, no `unwrap`/`expect` on public or C-ABI paths. This note
records what is already true. It does not add those crates, invent new
public error enums, or change the session / n-best / sentence surface.

## Policy this audit holds

1. Library crates keep typed, `#[non_exhaustive]` error enums that
   implement `Display` + `std::error::Error`. Callers that must
   distinguish variants already can. Do not add a variant, or a new
   enum, unless a caller has to branch on it.
2. The C ABI stays bool / NULL / 0 / `-1` shaped (`docs/findings/abi-subset.md`
   §7; PR #113). `oxpinyin-capi` maps `Result` to that shape at the
   `extern "C"` boundary. Do not force `Session::sentence_text`, empty
   n-best, or `pinyin_get_sentence` onto `Result`.
3. Bins may use `anyhow::Result` + `.with_context()` at the process
   edge. Pin/oracle fixture loads may `expect` when a failed load must
   abort the tool.
4. Constitution §4: nothing panics on caller input; public Rust APIs
   return `Result`. An empty lookup, a missing phrase, and an optional
   table that is not installed are absences, not errors.
5. Adding `thiserror` or `anyhow` is a dependency add (constitution
   hard-forbid without ask). This audit does not add them.

## Current practice vs the target

Neither `thiserror` nor `anyhow` is a workspace or lockfile
dependency. Every shipping and training crate uses a hand-rolled
`enum …Error` with a `Display` match and `impl std::error::Error`.
That already gives callers the same matchable surface `thiserror`
would derive. Switching to `thiserror` is a later mechanical PR (ask
first for the dep); it is not required to make errors distinguishable.

Binary `main` functions return `Result<(), Box<dyn std::error::Error>>`
or `Result<String, String>` and print `{error}`. That is the slot
`anyhow` would fill. Same rule: later, with an ask, not this change.

## 1. Inventory, crate by crate

### `oxpinyin-core` (library, internal)

| Type | Shape | Callers distinguish? |
|---|---|---|
| `ParseError` | one variant, `TooManyAlternatives` | no — one resource-limit failure |
| `GraphError` | one variant, `InputTooLong` | no |
| `DecodeError` | one variant, `KTooLarge` | no |
| `ScoringError` | `Dictionary(String)` / `LanguageModel(String)` | engine wraps both into `EngineError::Scoring`; backends are string-erased because the frozen `Dictionary` / `LanguageModel` `Error` associated types are unbounded (`core-trait-seam.md`) |
| `FixtureError` | five parse variants | yes — fixture tests and adapters match field/line |

Hand-rolled `Display` + `Error`. `#[non_exhaustive]` on all of the
above. No `io::Error`: the crate performs no I/O.

`Dictionary::lookup` returns `Result<Vec<Entry>, E>`; a miss is
`Ok(Vec::new())`, not `Err`. `LanguageModel::unigram_freq` /
`nbest_step_costs` use `Option` inside `Ok` for “no count / no n-best
cost data”. The default `nbest_step_costs` is
`Ok(NbestStepCosts::default())` — no cost data, not a failure. That is
the W14 / #113 shape: an empty n-best is a successful empty lookup.

### `oxpinyin-engine` (library, supported Rust surface)

| Type | Shape | Callers distinguish? |
|---|---|---|
| `EngineError` | `CandidateIndexOutOfRange { index, len }`, `Dictionary(String)`, `LanguageModel(String)`, `UserModel(String)`, `Graph`, `Decode`, `Scoring` | **yes** — `select` on a stale index is F-E-02; tests match `CandidateIndexOutOfRange`. Backend failures stay strings. |
| `ConfigError` | overlay parse/type mismatch | yes for config-merge callers; `Session::new` does not use it (typed getters fall back to defaults) |

`EngineError` already exists and is the right Rust-side enum. Do not
invent a second one, and do not push it onto methods whose C ABI is
bool-shaped:

| Method | Return | Why not `Result` |
|---|---|---|
| `Session::guess_sentence` | `Result<bool, EngineError>` | `bool` is the lookup-ran flag (empty key matrix → `Ok(false)`; zero rows after a lookup is still `Ok(true)`). Backend failure is `Err`. The C ABI then collapses both `Err` and `Ok(false)` to `false` (PR #113). |
| `Session::sentence_text` | `Option<&str>` | missing row / empty text is absence (`pinyin_get_sentence` → `false` + NULL). |
| `Session::sentence_lookup_active` | `bool` | gate, not a failure. |
| `CandidateList::get` | `Option<&Candidate>` | out-of-range is absence; `select` is the `Err` path. |
| `ConfigSource::get` | `Option<&ConfigValue>` | unset key is absence. |

`Session::select` returning `EngineError::CandidateIndexOutOfRange` is
the one public path that must stay `Result`. `Session::new` is `Result`
because scoring the key-cost table can fail; today that is the only
constructor failure.

### `oxpinyin-data` (library, internal)

| Type | Shape | Notes |
|---|---|---|
| `TableError` | `Io(io::Error)` + four redb wrappers (`Db` / `Table` / `Transaction` / `Storage`) | I/O vs redb. Not `#[non_exhaustive]`. Callers almost never match a variant. |
| `DictError` | `Table(TableError)` / `Parse(String)` | ad-hoc `String` for record layout |
| `LmError` | `Table` / `Parse(String)` / `User(String)` | user overlay is string-erased |
| `InterpolationError` | `Read { path, source: io::Error }` / `Parse { line, detail: String }` / `MissingOneGram` | **yes** — missing section vs bad line vs I/O |
| `LoadError` | six content-file layout variants | **yes** — F-E-09/10 loaders match `TooShort` / `UnsupportedVersion` |
| `Lambda` parse | `Option` | invalid / out-of-range `table.conf` line is treated as absent (fallback to `Lambda::PINNED`) |

`PunctTable::open` is `Result<_, DictError>`. `PunctTable::open_optional`
is a real absence: missing file or a raw HashDBM convert becomes empty,
matching upstream `PunctTable::attach` ignore. Do not make it `Result`.

### `oxpinyin-user` (library, internal)

| Type | Shape | Callers distinguish? |
|---|---|---|
| `UserStoreError` | `Io` + redb wrappers + `InvalidPhrase` + `TokenSpaceExhausted` | **yes** — `InvalidPhrase` vs exhausted id space vs I/O |

`UserStore::phrase` / `token_for_phrase` are `Result<Option<_>, UserStoreError>`:
I/O is `Err`, a missing token is `Ok(None)`. That split is correct.

`UserStore::open` failing at C ABI init degrades to `user: None` rather
than failing `pinyin_init` (system tables still load). The process-global
store registry recovers mutex poison with `into_inner` instead of
panicking.

### `oxpinyin-capi` (library, C ABI)

No crate-level error enum. Every `extern "C"` entry point is bool / NULL
/ `size_t` / `int`, wrapped in `ffi_catch` so a Rust panic cannot unwind
across the ABI. Typical maps:

- `Result<T, _>` → `unwrap_or(false)` / `.is_ok()` / `Err(_) => -1`
- `Option<T>` → NULL / `false`
- invalid UTF-8 / null C string → empty `String` (`cstr_to_string`)

`pinyin_init` returns NULL when the system dir is empty, a required
table fails to open, or `interpolation2.text` is present-but-unparsable.
A missing user dir is not an init failure. A missing `punct.redb` is
`open_optional`.

`pinyin_guess_sentence` is `inst.session.guess_sentence().unwrap_or(false)`:
backend `Err` and `Ok(false)` are `false`. That is the #113 decision, not
a missing `EngineError` at the ABI. `pinyin_get_sentence` answers
decoded-or-nothing from `sentence_text` once a lookup is active, and
`false` past the row count — never a Rust error.

`pinyin_remove_user_candidate` reports `false` instead of panicking when
the token is not a user token (upstream asserts).

### `oxpinyin-segment` / `counter` / `lambda` / `emitter` / `corpus`
(training libraries, never ship)

Each has a hand-rolled `#[non_exhaustive]` enum (`SegmentError`,
`CounterError`, `LambdaError`, `EmitterError`, `CorpusError`) wrapping
the previous stage plus `io::Error` with path. `LambdaError::EmptyDeleted`
is the one variant a caller must distinguish: upstream would print
`-nan` on an empty held-out set; the port must not panic, so estimation
returns this instead of dividing.

### `oxpinyin-dictool` (bin)

| Type | Shape |
|---|---|
| `format::ParseError` | struct `{ line, message: String }` — ad-hoc message, not an enum |
| `ImportError` | `Read(PathBuf, io::Error)` / `Utf8` / `Parse` / `Context(PathBuf, String)` / `Begin` / `Add { line }` / `Save` / `Snapshot` |
| `ExportError` | `Context` / `PhraseSnapshot` / `BigramSnapshot` / `Write(io::Error)` |

`main` is `Result<(), Box<dyn std::error::Error>>`. Import/Export
variants are worth matching (line-numbered ABI rejection vs I/O). The
process edge is the `anyhow` slot.

### `oxpinyin-migrate` (bin)

No crate error enum. `TkrzwReader::open` / `entries` return
`Result<_, String>`. `write_redb` and the four commands return
`Box<dyn std::error::Error>`. `build.rs` `expect`s pkg-config for
libtkrzw (a missing native dep must abort the build).

### `pinyin-oracle` (harness, never ships)

`OracleError` is the largest matchable enum in the tree (manifest,
prefix, FFI, capture). `ModelDirError` is `NotADirectory` /
`Incomplete`. Live FFI maps a C `false`/NULL onto `OracleError::Call` /
`ContextInitFailed` / `InstanceAllocFailed` rather than panicking.

Bins:

- `oracle_sentence_surface`, `oracle_paths`, `oracle_candidates`,
  `oracle_candidate_structure`, `parity_diff`: `Result<_, String>` or
  `match` on `OracleError`, exit 1/2. Appropriate `anyhow` slot.
- `parity_sweep`, `parity_worst`: `expect` on dictionary / LM / session
  / corpus fixture load. A failed load must abort the tool; leave them.
- benches: same fixture `expect`.

## 2. Panic / unwrap / expect on non-test paths

Scan: every `.unwrap()`, `.expect(`, `panic!`, `unreachable!`, `todo!`,
`unimplemented!` in `crates/**/*.rs`, excluding `tests/`, `*_tests.rs`,
`test_support.rs`, `benches/`, and `#[cfg(test)]` / `#[test]` regions.

**Library crates (core, engine, data, user, capi, segment, counter,
lambda, emitter, corpus, dictool): none.**

Non-test sites that remain:

| Site | Kind | Verdict |
|---|---|---|
| `oxpinyin-core` parser `assert_eq!(results.len(), expected)` (complete and fallback enumerators) | release `assert` | Internal count/enumerate mismatch, documented as a parser bug rather than caller input. Constitution §4 is “no panic on input”; this is an invariant. Leave it. |
| `oxpinyin-capi` `const _: () = { assert!(USER_DICTIONARY == 7); … }` | const-eval | Compile-time layout check. |
| `oxpinyin-core::cost::log2_fixed` `debug_assert!(value > 0)` | debug only | Callers (`surprisal`) already return `UNKNOWN_COST` on zero. |
| Mutex locks in `oxpinyin-user` registry and `oxpinyin-capi` `SharedLm` | `unwrap_or_else(into_inner)` | Poison recovery, not a panic. |
| `oxpinyin-migrate/build.rs` `pkg_config::…expect("libtkrzw…")` | build-time | Missing native dep aborts the build. Leave it. |
| `pinyin-oracle` `parity_sweep` / `parity_worst` fixture `expect` | bin | Failed pin/export load must abort. Leave it (policy item 3). |

`unwrap_or` / `unwrap_or_else` / `unwrap_or_default` / `try_from(…).unwrap_or(MAX)`
are saturating fallbacks, not panics. C ABI `guess_sentence().unwrap_or(false)`
is the bool collapse above.

Indexing on the hot paths is checked: `CandidateList::get`,
`SegmentGraph::edge` / `outgoing` (out-of-range → empty),
`sentence_text` via `slice::get`. Trellis `nodes[position]` is in
`0..=bound` by construction.

No public or C-ABI path `unwrap`s. No code PR from this scan.

## 3. `Option` that is absence vs `Option` that should have been `Result`

Real absence (keep `Option` / empty / bool):

| Site | Meaning |
|---|---|
| `CandidateList::get` | no candidate at that index |
| `Session::sentence_text` | no n-best row at `index`, or empty text |
| empty `Vec` from `Dictionary::lookup` / `nbest_sentences` | no phrases / no sentence rows |
| `LanguageModel::unigram_freq` → `Ok(None)` | no unigram table |
| `NbestStepCosts::{blended,unigram}` / `step()` → `None` | branch below epsilon; skip the step |
| `PunctTable::open_optional` | table not installed |
| `UserStore::phrase` / `token_for_phrase` inner `Option` | no such phrase |
| `SyllableKey::from_text` / `from_index` | not in the frozen inventory |
| `FewestKeys::parse` | no complete-key path (also swallows `InputTooLong`) |
| `Lambda::from_decimal` / missing `table.conf` | invalid or absent λ; pinned default stands |
| `ConfigSource::get` | unset key |
| `CapiContext::new` → `Option` | C `pinyin_init` NULL |
| `UserImportContext::open` → `Option` | C `pinyin_begin_add_phrases` NULL |
| `try_promote_addon` → `Option` | not an addon candidate, or promotion write failed (ABI reports a plain select / `false`, not a typed error) |

`FewestKeys::parse` collapsing `GraphError::InputTooLong` into `None`
is the only mild “should have been `Result`” in this list. The only
callers are interchange-format parsers; 64 KiB is not a realistic
user-dict line. Do not reshape.

Not absence — already `Result`:

- table / redb / interpolation2 I/O
- `Session::select` out of range
- `ParseError::TooManyAlternatives`, `DecodeError::KTooLarge`
- `UserStoreError::InvalidPhrase` / `TokenSpaceExhausted`
- `LambdaError::EmptyDeleted`

Do **not** promote empty n-best, `sentence_text`, or
`open_optional` to `Result`. That would fight the C ABI and the #113
session surface.

## 4. `thiserror` — only where a caller must distinguish

Adopt `thiserror` later as a **Display derive** over enums that already
exist. Do not invent types.

Worth the derive (callers match, or will):

- `EngineError` (already public, `#[non_exhaustive]`; keep
  `CandidateIndexOutOfRange` as a struct variant)
- `UserStoreError` (`InvalidPhrase` / `TokenSpaceExhausted` vs I/O)
- `OracleError` / `ModelDirError`
- `InterpolationError`, `LoadError`, `ConfigError`, `FixtureError`
- `LambdaError` (`EmptyDeleted`)
- `ImportError` / `ExportError`
- `CorpusError` (`MalformedXml` / `InvalidUtf8` / `Convert`)

Mechanical only (one or two variants; nobody matches):

- `ParseError`, `GraphError`, `DecodeError`
- `ScoringError`, `DictError`, `LmError`, `TableError`
- `SegmentError`, `CounterError`, `EmitterError`

Do **not** add:

- a new `EngineError` for “no n-best rows” or “no sentence text”
- `Result` on `sentence_text` / `CandidateList::get` /
  `PunctTable::open_optional`
- `thiserror` in `oxpinyin-capi` (no error type; ABI is bool)

If/when the derive lands, keep `to_string()` byte-identical:
`EngineError::CandidateIndexOutOfRange` is asserted as
`"candidate index {index} is out of range 0..{len}"`. `#[source]` on
`Graph` / `Decode` / `Scoring` / redb wrappers would be the one
behavioural improvement (`EngineError` currently does not implement
`Error::source`). Still not this change.

## 5. Bins — `anyhow` slot

Appropriate `anyhow::Result` + `.with_context()` edges, later, with an
ask:

- `oxpinyin-dictool` `main` (`Box<dyn Error>`)
- `oxpinyin-migrate` commands (`Box<dyn Error>` + `String`)
- `oxpinyin-corpus` / `segment` / `counter` / `lambda` / `emitter` CLIs
- `pinyin-oracle` fixture generators (`Result<_, String>`)
- `oracle_sentence_surface` (same)

Leave `expect` on:

- `parity_sweep` / `parity_worst` export + corpus loads
- oracle benches
- `oxpinyin-migrate/build.rs` pkg-config

Bisection C drivers (`tools/bisection/*.c`) are not Rust; they already
print and return a status. No change.

## 6. Decision

No panic path on a public or C-ABI surface. No mass rewrite. No
dependency add. Pins are not involved.

Follow-ups, only if asked:

1. Add `thiserror` 2.x and mechanically derive the existing library
   enums (display strings frozen).
2. Add `anyhow` 1.x at binary `main` functions only, with
   `.with_context()` on path I/O.
3. Optionally implement `Error::source` on `EngineError` /
   `TableError` / `UserStoreError` as part of (1).

Not follow-ups:

- Reshaping `Session` / n-best / `sentence_text` onto `Result`
- A C ABI error code channel
- Replacing `ffi_catch` with typed errors
- Touching pin fixtures or candidate numbers
