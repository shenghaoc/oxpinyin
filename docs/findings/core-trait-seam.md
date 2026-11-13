# Core trait seam SPEC

Status: **reference**

This page describes the internal `oxpinyin-core` trait seam
(`crates/oxpinyin-core/src/lib.rs`) and its behavioural rules: totality,
determinism, ordering and growth.
It is not a compatibility contract; the traits and their rules change with the code.
Failure behaviour is set by the [compatibility policy](compatibility-policy.md#failure-behaviour), and candidate order by the C APIs' pin-derived behaviour.
The C APIs/ABIs are the only supported external interfaces ([interface ruling](../constitution.md#external-interfaces)).

## Shared cost

```rust
pub type Cost = i64;
```

Costs are signed integers, so ordering and accumulation stay deterministic.
The decoder and scoring SPEC owns the scale ([scoring-spec.md](scoring-spec.md)); this seam fixes only the representation.

## Growth rule

The seam grows by defaulted methods only.
A new method has a default that answers as the trait did before it existed, so existing implementors and fixture doubles compile unchanged.
The associated types are the exception: an implementor names each one.

## `Dictionary`

```rust
pub trait Dictionary {
    type Syllable;
    type Error;
    type PathMemo: Default;

    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<PhraseEntry>, Self::Error>;
    // every other method is defaulted
}
```

`lookup` is the one required method.
An empty lookup is `Ok(Vec::new())`.
Implementations return entries in a stable, deterministic order.
The trait does not prescribe storage or ranking; the entry type is the core's `PhraseEntry`.

The defaulted methods form these groups:

- **Sentence trellis.**
  `trellis_records(syllables, tones)` returns every matching pronunciation record in library, range and item order (default: `lookup`).
  `trellis_ranges` fills token-only ranges, `PhraseIndexRanges<()>`, as the pin's `convert` does.
  `trellis_ranges_continued` gives the ranges and the `SEARCH_CONTINUED` probe from one read (default: `trellis_ranges`, then `phrase_prefix_exists`).
  A step prices a ranged token through `trellis_item(syllables, tones, token) -> Option<TrellisItem>`, where `TrellisItem { chars, pronunciation }` carries the item, and a result's text is read through `trellis_text` when it is emitted.
  The defaults read `trellis_records`, so a dictionary that overrides none of them needs no change.
- **Path memo.**
  `trellis_item_on_path(syllables, tones, token, memo: &Self::PathMemo)` answers exactly as `trellis_item`; the default ignores the memo.
  The n-best caller makes a fresh `PathMemo` per key path and drops it with the span search that found the path, so nothing in it outlives one guess.
  A dictionary with nothing to keep uses `()`.
  `TrellisPathMemo<T>` is a fill-once slot for a dictionary that does keep a value; `RuntimeDict` keeps the path's user lookup and tone-less keys there (`PathPricing`, `crates/oxpinyin-runtime/src/lib.rs`) instead of recomputing them per token.
  `trellis_item_with_facts` and `trellis_text_with_facts` are the sentence-only variants that also record sentence facts.
- **Candidate lookup.**
  `lookup_into`, `lookup_into_flagged`, `lookup_candidates_into`, `candidate_search_into` and the addon variants (`lookup_addon`, `lookup_addon_into`, `lookup_addon_candidates_into`, `phrase_prefix_exists_addon`) fill caller-owned buffers or read the addon facade.
- **Phrase index reads.**
  `phrase_prefix_exists`, `tokens_for_text`, `tokens_for_ucs4`, `phrase_span_searches_are_observable`, `phrase_span_continues`, `phrase_index_item_count`, `handles_partial_keys`, `suggest_extension_tokens`, `checked_extension_tokens`, `phrase_text_for_token`, `phrase_length_for_token` and `overlong_extension_gate`.
- **`phrase_item_pronunciations(token)`** reads the stored pronunciations of the phrase item behind a token; the default answers `PhraseItemRead::Unsupported`.
  A forcing is then revalidated as the pin's `validate_constraint` does it (`phonetic_lookup.cpp:120-165` at 074a2219).

## `UserModel`

```rust
pub trait UserModel {
    type Token;
    type Error;

    fn observe(&mut self, history: &[Self::Token], token: &Self::Token) -> Result<(), Self::Error>;
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
    type Token;
    type Error;
    // every method is defaulted
}
```

Every method is defaulted, and a model that overrides none of them carries no frequency table: each candidate ranks at frequency zero and no bigram term applies.
The methods are:

- unigram reads: `unigram_freq`, `unigram_total`, `addon_unigram_freq`, `addon_unigram_total`;
- the merged bigram row: `merged_successors(prev) -> Option<MergedGram>` ([`dynamic-adjust.md`](dynamic-adjust.md));
- n-best step pricing: `nbest_step_costs`, `nbest_step_costs_in_row`, `nbest_item_costs_with_facts`, `nbest_item_costs_in_row`, `begin_successor_span`, `nbest_successors_in_row`, which return the pre-log `NbestStepCosts` pieces;
- `is_read_after_check` and `amplification_lambda` (default: the pinned model's `0.312699`).

A model reports arithmetic or backend failure through `Result`.

## Cross-trait rules

- Every fallible public method returns `Result`; no method panics on caller input.
- The seam imposes no `Send`, `Sync`, `'static`, serialization or object-store requirement.
- Associated types prevent `oxpinyin-core` from depending on data, user, engine or platform crates.
  The concrete types the engine seam names are in `crates/oxpinyin-core/src/vocab.rs`.
- Implementations must be deterministic for the same explicit input and state.
