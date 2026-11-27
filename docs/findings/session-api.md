# Framework-neutral session API SPEC

Status: **reference**.

This page describes the design of the internal `oxpinyin` session surface. It is not a compatibility contract ([approval conditions](../../CONTRIBUTING.md#approval-conditions)).
What binds is the pin's behaviour as the C APIs show it — candidate order, preedit coverage, selection and commit, and errors — under the [compatibility policy](compatibility-policy.md).
The C APIs/ABIs are the only supported external interfaces ([interface ruling](../constitution.md#external-interfaces)).
The code is the reference for the current surface: `src/engine/mod.rs` lists the public items, and `src/engine/session/` defines `Session`.

`docs/architecture.md` states the rules that this surface implements, under **Portability**.

## What this seam is for

`oxpinyin` is the in-tree session layer under the C facades.
The supported external interfaces are the C APIs/ABIs alone (`oxpinyin-capi`, `oxzhuyin-capi`).
The parser, engine and concrete models have internal Rust interfaces.

The engine receives its inputs as data.
`Session::new` takes a dictionary, a language model and the context's option word.
`Session::replace_raw` takes the raw input as text.
The engine has no key dispatch, no display model and no path discovery.

## Deliberately absent

Each item below is a place where a portable API usually leaks:

- **No keys.** The engine has no key type and no key dispatch.
  A caller supplies raw input text.
- **No GSettings, no dconf, no registry.** Settings arrive as the context's option word.
  A settings store is a shell concern outside the engine.
- **No path discovery.** The engine never calls `dirs`, `XDG_*`, `%APPDATA%` or `NSSearchPathForDirectoriesInDomains`.
  The caller supplies the system directory to `Runtime` as data, and `Runtime` holds it.
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
It has no index accessor, and the session selects what the row carries (its token span or its n-best rank), never an index.
A candidate handle is therefore a C ABI case. The C choose reads the row's own scalars and selects on the instance it is passed, whichever instance published it, as the pin does (`tests/it/foreign_candidates.rs`, both C facades). A pointer that is not a row of a live published list is a caller precondition violation ([register entry](upstream-divergences.md#a-candidate-pointer-that-is-not-a-row-of-a-live-published-list)).

`consumed_bytes` states how much of the composition a candidate would absorb.
A shell can then render partial acceptance without re-deriving it.

## Storage and configuration

The session keeps no option word of its own.
The C facades bind each operation under the context's current word, as the pin's instance reads `instance->m_context->m_options` in every parse and guess (`pinyin.cpp:1500`, `:2189`; `zhuyin.cpp:1020`, `:1464` at 074a2219).
`pinyin_set_options` only stores the word (`pinyin.cpp:1299-1307`), so a change reaches every instance at its next operation.
There is no keyed configuration source or layered configuration: the ibus-libpinyin defaults do not belong in oxpinyin (maintainer ruling).

## Session

`Session<D, L, S = SessionState>` is one input context over a dictionary `D` and a language model `L`.
`SessionState` holds the composition, so a facade can keep the state and borrow its storage for each operation.

| Operation | Methods |
|---|---|
| Open | `Session::new(dictionary, model, options) -> Self`, or `SessionState::default()` bound per operation with `SessionState::bind(dictionary, model, options)`; neither can fail |
| Supply input | `replace_raw(&str)`, which replaces the whole raw input; `replace_raw_exact` and `replace_raw_full_pinyin` for the facade parse paths |
| Read | `raw_input`, `candidates_at_for_abi`, `candidates_ending_at_for_abi`, `is_composing`, `composition_offset`, `selected_text`, `selected_tokens` |
| Select | `select_row(start, end, token, text)` for a token row, `select_nbest_row(rank)` for an n-best row, `choose_tokenless()` for a row without a token; `Candidate::span_end_at` places a listed row's span end |
| Reset | `reset`, `discard_composition` |
| Options | the word at `SessionState::bind`, `set_sort_options`, `set_law` |
| Sentences | `guess_sentence`, `sentence_lookup_active`, `sentence_text` ([sentence-surface.md](sentence-surface.md)) |
| Training | `SessionState::train`: each step checked, then trained |

`Selection` is `Refused`, `Continued` or `Completed`.

The bounds name concrete `oxpinyin` types (`SyllableKey`, `PhraseToken`).
The `Display` bounds exist because the [core-trait-seam.md](core-trait-seam.md) traits leave `Error` unbounded.
The engine type-erases backend failures into `EngineError` and does not put an associated type into its public surface.

`Session` owns its dictionary and language model.
To replace the fixture adapters with the table-backed loaders, change the two type arguments and nothing else.

### Behavioural contract

- `replace_raw` replaces the raw input and passes every byte to the decoder; it does not filter characters.
- `reset` discards the input, the selection record, the sentence rows and the constraints.
- The session has no input length limit ([The full-pinyin parse's 16-bit steps](upstream-divergences.md#the-full-pinyin-parses-16-bit-steps)).
- `select_row` forces the chosen token over its span and advances over the bytes that it consumed.
  It returns `Completed` when nothing is left.
- A composition with no dictionary result still offers one candidate: the remaining raw input, as `CandidateKind::Fallback`.
  It has no token, so choosing it runs `choose_tokenless`, which forces nothing.
- Every method is deterministic for the same (input, state, config).

### Candidate windows

The session keeps no candidate list.
Each list is a window that `candidates_at_for_abi` or `candidates_ending_at_for_abi` builds at a lookup offset, because the pin builds candidates only in `pinyin_guess_candidates` (`pinyin.cpp:2184-2308` at 074a2219).
A parse, a sentence guess, a selection and an option or sort-word change build no list and search no table.
`pinyin_set_options` only stores the word (`pinyin.cpp:1299-1307`), and the pin takes the sort word as an argument of the guess (`pinyin.cpp:2198`).

## Errors

`EngineError` is the one error type of the engine.
`src/engine/error.rs` lists the variants.
It implements `Display` and `std::error::Error`.

## Decoder vocabulary types

`oxpinyin` defines the types that the bounds above name:

```rust
pub struct SyllableKey(u16);       // dense id over the fixed key inventory
pub type PhraseToken = u32;        // the pin's phrase_token_t (novel_types.h:33)
pub struct PhraseEntry { /* token, text: PhraseText, pronunciation, unigram */ }
```

The key inventory has 440 keys: the 405 complete syllables of [parser-spec.md](parser-spec.md) in their fixed numeric-ID order, the 23 initial-only keys, two option-only keys (`eng`, `nun`) and ten zhuyin-only syllables.
An initial-only key is a non-empty proper prefix of a complete syllable that contains no vowel byte (`a`, `e`, `i`, `o`, `u`, `v`) and is not itself a complete syllable.
That rule gives exactly 23 keys, and a test asserts it against the fixed inventory.
[segment-graph.md](segment-graph.md) states that these 23 are the pin's incomplete-key set and makes them graph edges.
