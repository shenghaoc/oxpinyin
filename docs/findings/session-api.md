# Framework-neutral session API SPEC

Date: 2026-08-09 · Status: **current reference**.

This page describes the design of the internal `oxpinyin-engine` session surface. It is not a compatibility contract ([frozen-SPEC ruling](compatibility-policy.md#frozen-specs)).
What binds is the pin's behaviour as the C APIs show it — candidate order, preedit coverage, selection and commit, and errors — under the [compatibility policy](compatibility-policy.md).
The C APIs/ABIs are the only supported external interfaces ([interface ruling](../constitution.md#external-interfaces), 2026-10-18 UTC).
The code is the reference for the current surface: `crates/oxpinyin-engine/src/lib.rs` lists the public items, and `crates/oxpinyin-engine/src/session/` defines `Session`.

`docs/architecture.md` states the rules that this surface implements, under **Portability**.

## What this seam is for

`oxpinyin-engine` is the in-tree session layer under the C facades.
The supported external interfaces are the C APIs/ABIs alone (`oxpinyin-capi`, `oxpinyin-zhuyin-capi`).
This crate and the crates below it (`oxpinyin-core`, `oxpinyin-data`, `oxpinyin-user`) are internal and carry no stability promise.

The engine receives its inputs as data.
`Session::new` takes a `SessionSettings` value, a dictionary and a language model.
`Session::replace_raw` takes the raw input as text.
The engine has no key dispatch, no display model and no path discovery.

## Deliberately absent

Each item below is a place where a portable API usually leaks:

- **No keys.** The engine has no key type and no key dispatch.
  A caller supplies raw input text.
- **No GSettings, no dconf, no registry.** Settings arrive as a `SessionSettings` value.
  A settings store is a shell concern outside the engine.
- **No path discovery.** The engine never calls `dirs`, `XDG_*`, `%APPDATA%` or `NSSearchPathForDirectoriesInDomains`.
  `StoragePaths` carries the directories as data, and `oxpinyin-runtime` holds the value.
- **No `cfg(target_os)` and no platform types** anywhere in the portable crates.
- **No threading contract.** `Session` is not required to be `Send` or `Sync`.
  Sessions are instance-per-context and main-thread-friendly, which is what the TSF, IMK and ArkTS models want.
- **No clock, no locale, no environment reads.** Output stays a pure function of (input, user state, config), per constitution item 6.

## Candidates

```rust
pub enum CandidateKind { Phrase, Addon, Sentence, Fallback }
pub struct Candidate { /* text, kind, consumed keys and bytes, cost */ }
pub struct CandidateList { /* ordered, deterministic */ }
```

`CandidateList` supports `len`, `is_empty` and `iter`.
It has no index accessor, and the session selects a row it is given (`Session::select_target`), never an index.
F-E-02 ([robustness-evidence.md](robustness-evidence.md)), a candidate that outlived its list, is therefore a C ABI case: a stale candidate pointer, which the snapshot resolves by address.

`consumed_keys` and `consumed_bytes` state how much of the composition a candidate would absorb.
A shell can then render partial acceptance without re-deriving it.

## Storage and configuration

```rust
pub struct StoragePaths { user_data_dir: PathBuf, system_data_dirs: Vec<PathBuf> }

pub struct SessionSettings { pub incomplete_pinyin: bool }   // Default: true
```

A session opens under `PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE`, with `PINYIN_INCOMPLETE` taken from `incomplete_pinyin`.
Every other option bit arrives through `Session::set_options` from the C ABI's option word.
There is no keyed configuration source or layered configuration; see the correction log.

## Session

`Session<D, L, S = SessionState>` is one input context over a dictionary `D` and a language model `L`.
`SessionState` holds the composition, so a facade can keep the state and borrow its storage for each operation.

| Operation | Methods |
|---|---|
| Open | `Session::new(settings, dictionary, model) -> Self`; it cannot fail |
| Supply input | `replace_raw(&str)`, which replaces the whole raw input; `replace_raw_exact` and `replace_raw_full_pinyin` for the facade parse paths |
| Read | `raw_input`, `candidates`, `candidates_at`, `is_composing`, `composition_offset`, `selected_text`, `selected_tokens` |
| Select | `select_target(&SelectionTarget) -> Result<Selection, EngineError>`, with `SelectionTarget::of_candidate` for a listed row |
| Reset | `reset`, `reset_composition` |
| Options | `set_options`, `set_sort_options`, `set_candidate_ranking`, `set_nbest_shape` |
| Sentences | `guess_sentence`, `sentence_lookup_active`, `sentence_text` ([sentence-surface.md](sentence-surface.md)) |
| Deferred list | `set_defer_list_builds`, `refresh_candidates` |
| Training | `prepare_training`, then `TrainingPlan::observe` |

`Selection` is `Refused`, `Continued` or `Completed`.

The bounds name concrete `oxpinyin-core` types (`SyllableKey`, `PhraseToken`).
The `Display` bounds exist because the [core-trait-seam.md](core-trait-seam.md) traits leave `Error` unbounded.
The engine type-erases backend failures into `EngineError` and does not put an associated type into its public surface.

`Session` owns its dictionary and language model.
To replace the fixture adapters with the table-backed loaders, change the two type arguments and nothing else.

### Behavioural contract

- `replace_raw` replaces the raw input and passes every byte to the decoder; it does not filter characters.
- `reset` discards the input, the selection record, the sentence rows and the constraints.
- The session has no input length limit (#1501; [1501-gint16-input-cap](1501-gint16-input-cap.md)).
- `select_target` appends the chosen candidate to the selected prefix and advances over the bytes that it consumed.
  It returns `Completed` when nothing is left.
- A composition with no dictionary result still offers one candidate: the remaining raw input, as `CandidateKind::Fallback`.
  This keeps `select_target` meaningful in every state.
- Every method is deterministic for the same (input, state, config).

`SessionSettings::default()` serves callers that have no settings of their own: incomplete pinyin on, with the divided and resplit tables.

## Errors

`EngineError` is the one error type of the engine.
`crates/oxpinyin-engine/src/error.rs` lists the variants.
It implements `Display` and `std::error::Error`.

## Decoder vocabulary types

`oxpinyin-core` defines the types that the bounds above name:

```rust
pub struct SyllableKey(u16);       // dense id over the frozen key inventory
pub struct PhraseToken(u32);
pub struct PhraseEntry { /* token, text: PhraseText, pronunciation, unigram */ }
```

The key inventory has 440 keys in Stage 1: the 405 complete syllables of [parser-spec.md](parser-spec.md) in their frozen numeric-ID order, the 23 initial-only keys, two option-only keys (`eng`, `nun`) and ten zhuyin-only syllables.
An initial-only key is a non-empty proper prefix of a complete syllable that contains no vowel byte (`a`, `e`, `i`, `o`, `u`, `v`) and is not itself a complete syllable.
That rule gives exactly 23 keys, and a test asserts it against the frozen inventory.
[segment-graph.md](segment-graph.md) records the oracle evidence that these 23 are the pin's incomplete-key set and makes them graph edges.

## Acceptance

- `oxpinyin-engine` compiles on Linux, macOS and Windows, with no `cfg(target_os)` and no platform dependency.
- Every public item has a doc comment.
- Unit tests cover candidate indexing.

## Architect correction log

**2026-10-16 — #757 deferred candidate list (human approval, 2026-10-16).**
`Session` gained `set_defer_list_builds` and `refresh_candidates`.
The default keeps the eager list, its parse length and the errors that the parse seams and sentence guesses return.
The C facades turn the mode on, because the pin builds candidates only in `pinyin_guess_candidates` (`pinyin.cpp:2184-2308` at 074a2219).
The C ABI answers from the window that it re-anchors there.
So in that mode, a sort-word change on a composing session (`set_sort_options`) also leaves the list stale: the pin takes the word as an argument of that call (`pinyin.cpp:2198`).

**2026-11-07 UTC — configuration framework removed (maintainer ruling).**
The ibus-libpinyin defaults do not belong in oxpinyin.
`Config`, `ConfigValue`, `ConfigSource`, `EmptyConfigSource` and the 69 captured upstream defaults are removed.
`Session::new` and `Runtime::new_session` take a `SessionSettings` value.
These do not change: the session's default options, each C facade's initial option word, and the point at which an option change reaches existing instances.
