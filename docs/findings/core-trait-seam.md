# Core trait seam SPEC

Status: **reference**

This page describes the internal `oxpinyin` trait seam
(`src/model.rs`) and its behavioural rules: totality,
determinism, ordering and growth.
It is not a compatibility contract; the traits and their rules change with the code.
Failure behaviour is set by the [compatibility policy](compatibility-policy.md#failure-behaviour), and candidate order by the C APIs' pin-derived behaviour.
The C APIs/ABIs are the only supported external interfaces ([interface ruling](../constitution.md#external-interfaces)).

## Growth rules

`Dictionary` grows by required methods only.
The engine calls every `Dictionary` method, and every implementor answers each one itself.
A new engine method is required on `Dictionary`, gets one derived body in the `PhraseList` blanket implementation, and is implemented directly by each dictionary that implements `Dictionary` directly.
The `PhraseList` implementors do not change.

`LanguageModel::amplification_lambda` is required. Each implementor supplies its interpolation weight.
New methods on `UserModel` and `LanguageModel` have defaults that preserve the traits' prior behaviour, so existing implementors and fixture doubles compile unchanged.

The associated types are an exception in every trait: an implementor names each one.

## `Dictionary`

```rust
pub trait Dictionary {
    type Syllable;
    type Error;
    type PathMemo: Default;
    // 12 required methods; none has a default
}
```

`Dictionary` is the seam the engine reads dictionaries through.
Its methods are exactly the 12 methods the engine calls:

| Group | Methods | Engine callers |
|---|---|---|
| Sentence trellis | `trellis_ranges_continued`, `trellis_item_with_facts`, `trellis_text_with_facts` | `src/engine/nbest.rs` |
| Candidate window | `candidate_search_into`, `search_addon_into`, `checked_extension_tokens`, `phrase_text_for_token`, `phrase_length_for_token` | `src/engine/session/lookup.rs`, `src/engine/session/selection.rs` |
| Phrase segmentation | `tokens_for_ucs4`, `phrase_span_searches_are_observable`, `phrase_span_continues` | `src/engine/phrase.rs`, `src/engine/char_offset.rs` |
| Forcing | `phrase_item_pronunciations` | `src/engine/nbest.rs`, `src/engine/char_offset.rs` |

An empty search is `Ok` with no entries.
Implementations return entries in a stable, deterministic order.
The trait does not prescribe storage or ranking; the entry type is the core's `PhraseEntry`.

Three types implement `Dictionary` directly:

- `RuntimeDict` (`src/runtime/mod.rs`), the production dictionary: the system table, then the user table, with the addon facade beside them.
- `SystemDictionary` (`src/data/dict.rs`), the system table alone. The nonshipping REPL lookup (`repl/src/lookup.rs`) gives it to the engine.
- Every `PhraseList` implementor, through the blanket implementation.

The other reads of `RuntimeDict` and `SystemDictionary` are inherent methods, for example `lookup`, `trellis_records`, `trellis_ranges`, `trellis_item`, `lookup_into_flagged` and `phrase_prefix_exists`.
The engine does not call them.

### Sentence trellis

- `trellis_ranges_continued(syllables, tones, ranges, scratch)` fills token-only ranges, `PhraseIndexRanges<()>`, as the pin's `convert` does, and returns the `SEARCH_CONTINUED` bit from the same read.
- Every step prices a ranged or forced token through `trellis_item_with_facts(syllables, tones, token, memo, facts) -> Option<TrellisItem>`, with the memo of the path the token was found on.
  `TrellisItem { chars, pronunciation }` carries the item, and the call also records sentence facts.
  This is the n-best search's only pricing path (`src/engine/nbest.rs`, `Pricing::item_on_path`).
  `SpanRanges::push_path` files a memo with every path, so every path has one.
- `trellis_text_with_facts(syllables, tones, token, facts)` reads a result's stored length and text when the result is emitted.
  It takes no path memo.

### Path memo

The n-best caller makes a fresh `PathMemo` for each key path and drops it with the span search that found the path, so nothing in it outlives one guess.
The item `trellis_item_with_facts` answers does not depend on what the memo holds; the memo only keeps what the dictionary would otherwise compute again for each token of the path.
A dictionary with nothing to keep uses `()`.
`TrellisPathMemo<T>` is a fill-once slot for a dictionary that does keep a value.
`RuntimeDict` keeps the path's user lookup and toned keys there (`PathPricing`, `src/runtime/mod.rs`) instead of recomputing them for each token.

### Candidate window

- `candidate_search_into` gives a key path's tone-filtered rows, its unreadable rows and the `SEARCH_CONTINUED` bit from one read per table, into a caller-owned row buffer.
- `search_addon_into` is the addon facade's pass over the same key path.
- `checked_extension_tokens` gives the LONGER candidate's tokens, or `None` where the pin's over-long-key gate aborts `search_suggestion`.
- `phrase_text_for_token` and `phrase_length_for_token` read a token's phrase item.

### Phrase segmentation

`tokens_for_ucs4` gives the phrase tokens of an exact raw UCS-4 span.
`phrase_span_searches_are_observable` and `phrase_span_continues` tell the span search how a failed native handle behaves.

### Forcing

`phrase_item_pronunciations(token)` reads the stored pronunciations of the phrase item behind a token.
It answers `PhraseItemRead::NoItem` for a token without an item, as the pin's `get_phrase_item` fails for it.
A forcing is then revalidated as the pin's `validate_constraint` does it (`phonetic_lookup.cpp:120-165` at 074a2219).

## `PhraseList`

```rust
pub trait PhraseList {
    type Syllable;
    type Error;

    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<PhraseEntry>, Self::Error>;
    // every other method is a defaulted hook
}

impl<T: PhraseList> Dictionary for T { /* PathMemo = () */ }
```

`PhraseList` is the adapter for a dictionary that lists entries: the fixture doubles (`tests/common/fixture_doubles.rs` and the unit-test doubles) and the evaluation tool's `EvalDictionary` (`utils/src/training/eval_correction_rate.rs`).
`lookup` is its one required method.
An implementor overrides only the hooks it models:
`trellis_records`, `trellis_item`, `lookup_into_flagged`, `candidate_search_into`, `phrase_prefix_exists`, `tokens_for_text`, `tokens_for_ucs4`, `lookup_addon`, `suggest_extension_tokens`, `phrase_text_for_token`, `phrase_length_for_token` and `phrase_item_pronunciations`.

The blanket implementation derives every `Dictionary` method from the hooks, with `PathMemo = ()`.
A dictionary that overrides no hook gets these answers, which are weaker than the pin's:

| `Dictionary` method | Derived answer |
|---|---|
| `trellis_ranges_continued` | The ranges of `trellis_records`' tokens (default: `lookup`), then `phrase_prefix_exists`. |
| `trellis_item_with_facts` | `trellis_item` (default: the last `trellis_records` entry for the token); the memo and the facts are not used. |
| `trellis_text_with_facts` | The text of that entry. |
| `candidate_search_into` | `lookup_into_flagged` (default: `lookup`, no unreadable row), then `phrase_prefix_exists`. The query tones are ignored, so no row is tone-filtered. |
| `search_addon_into` | `lookup_addon` (default: empty), with no continuation bit. |
| `checked_extension_tokens` | `Some(suggest_extension_tokens)` (default: empty, so no LONGER candidate); the over-long-key gate is always open. |
| `tokens_for_ucs4` | `tokens_for_text` (default: empty) for an all-scalar span; empty for a span with a non-scalar. |
| `phrase_span_searches_are_observable`, `phrase_span_continues` | `false`, `true`. |
| `phrase_text_for_token`, `phrase_length_for_token` | `None`, and the length of `phrase_text_for_token`'s text. |
| `phrase_item_pronunciations` | `PhraseItemRead::NoItem`. |

`phrase_prefix_exists` defaults to `true`, so `SEARCH_CONTINUED` is always set: the candidate set is the same, and the window scan searches to the end of the input.

A `PhraseList` method and a `Dictionary` method can have the same name.
Where both traits are in scope, a call on a `PhraseList` implementor names its trait, for example `PhraseList::phrase_text_for_token(&inner, token)`.

## `UserModel`

```rust
pub trait UserModel {
    type Error;

    fn observe(&mut self, history: &[PhraseToken], token: &PhraseToken) -> Result<(), Self::Error>;
    // defaulted: observe_with_keys
}
```

`observe` is the explicit learning boundary; callers omit it entirely in learning-off modes.
The defaulted `observe_with_keys(history, token, readings)` delegates to `observe`.
Its lazy tone-bearing readings keep the trained matrix span and the path multiplicity.
The model may not depend on hidden process-global state.

## `LanguageModel`

```rust
pub trait LanguageModel {
    type Error;

    fn amplification_lambda(&self) -> f32;
    // every other method is defaulted
}
```

Every other method has a default. A model that uses all the defaults carries no frequency table.
Each candidate ranks at frequency zero, and no bigram term applies.
The methods are:

- unigram reads: `unigram_freq`, `unigram_total`, `addon_unigram_freq`, `addon_unigram_total`;
- the merged bigram row: `merged_successors(prev) -> Option<MergedGram>` ([`dynamic-adjust.md`](dynamic-adjust.md));
- n-best step pricing: `nbest_step_costs`, `nbest_step_costs_in_row`, `nbest_item_costs_with_facts`, `nbest_item_costs_in_row`, `nbest_successors_in_row`, which return the pre-log `NbestStepCosts` pieces;
- `is_read_after_check` identifies a read failure after a successful size check.
- `amplification_lambda` supplies the interpolation weight. It has no default.

A model reports arithmetic or backend failure through `Result`.

## Cross-trait rules

- Every fallible public method returns `Result`; no method panics on caller input.
- The seam imposes no `Send`, `Sync`, `'static`, serialization or object-store requirement.
- Associated types prevent `oxpinyin` from depending on the concrete data, user or platform implementations.
  The concrete types the engine seam names are in `src/vocab.rs`.
- Implementations must be deterministic for the same explicit input and state.
