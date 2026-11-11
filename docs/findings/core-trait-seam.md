# Core trait seam SPEC

Date: 2026-08-09 · Status: **current reference**.

This page describes the internal `oxpinyin-core` trait seam and its behavioural rules: totality, determinism, ordering and costs.
It is not a compatibility contract ([frozen-SPEC ruling](compatibility-policy.md#frozen-specs)); the traits and their rules change with the code.
Failure behaviour is set by the [compatibility policy](compatibility-policy.md#failure-behaviour), and candidate order by the C APIs' pin-derived behaviour.
The C APIs/ABIs are the only supported external interfaces ([interface ruling](../constitution.md#external-interfaces), 2026-10-18 UTC).

## Shared cost

```rust
pub type Cost = i64;
```

Costs are signed integers, so ordering and accumulation stay deterministic.
The decoder and scoring SPEC owns the scale ([scoring-spec.md](scoring-spec.md)); this seam fixes only the representation.

## `Dictionary`

```rust
pub trait Dictionary {
    type Syllable;
    type Entry;
    type Error;

    fn lookup(
        &self,
        syllables: &[Self::Syllable],
    ) -> Result<Vec<Self::Entry>, Self::Error>;
}
```

An empty lookup is `Ok(Vec::new())`.
Implementations return entries in a stable, deterministic order.
The trait does not prescribe storage, token layout or ranking.

The trait also has these methods:

- **`phrase_item_pronunciations(token)`** (#825, defaulted; the default answers `PhraseItemRead::Unsupported`).
  It reads the stored pronunciations of the phrase item behind a token.
  A forcing is then revalidated as the pin's `validate_constraint` does it (`phonetic_lookup.cpp:120-165` at 074a2219).
- **`trellis_ranges`** (#821, #757) fills token-only ranges: `PhraseIndexRanges<()>` in place of `PhraseIndexRanges<Self::Entry>`, as the pin's `convert` does.
  This is a signature change.
  - A step prices a ranged token through the defaulted `trellis_item(syllables, tones, token) -> Option<TrellisItem>`.
  - When a result is emitted, its text is read through the defaulted `trellis_text(syllables, tones, token)`.
  - `TrellisItem { chars, pronunciation }` carries the item.
  - The helper trait `PhraseIndexToken` has two required methods, `phrase_index_item` and `phrase_index_text`, which `PhraseEntry` implements.
  - The defaults read `trellis_records`, so a dictionary that overrides none of them needs no change.
  - The defaulted `trellis_ranges_continued` gives the ranges and the `SEARCH_CONTINUED` probe from one read.
    Its default is `trellis_ranges` then `phrase_prefix_exists`.
- **`trellis_item_on_path(syllables, tones, token, memo)`** (#831, #757, defaulted; the default is `trellis_item`).
  `TrellisPathMemo` is a fill-once slot that the n-best caller holds per key path for one span search.
  `RuntimeDict` keeps the path's user lookup and tone-less keys there, and does not recompute them per token.
  A memo that holds another type is not read, and the answer is then the answer of `trellis_item`.

## `UserModel`

```rust
pub trait UserModel {
    type Token;
    type Error;

    fn score(
        &self,
        history: &[Self::Token],
        token: &Self::Token,
    ) -> Result<Cost, Self::Error>;

    fn observe(
        &mut self,
        history: &[Self::Token],
        token: &Self::Token,
    ) -> Result<(), Self::Error>;
}
```

The trait also has the defaulted `observe_with_keys(history, token, readings)` (#615), which delegates to `observe`.
Its lazy tone-bearing readings keep the trained matrix span and the path multiplicity.

`score` is read-only.
`observe` is the explicit learning boundary; callers can omit it entirely in learning-off modes.
Neither method may depend on hidden process-global state.

## `LanguageModel`

```rust
pub trait LanguageModel {
    type Token;
    type Error;

    fn score(
        &self,
        history: &[Self::Token],
        token: &Self::Token,
        edge_cost: Cost,
    ) -> Result<Cost, Self::Error>;
}
```

The edge cost is part of the signature.
A model may combine it with its own cost, but it must do so deterministically.
It must report arithmetic or backend failure through `Result`.

## `InputParser`

```rust
pub trait InputParser {
    type Parse;
    type Error;

    fn parse(&self, input: &[u8]) -> Result<Vec<Self::Parse>, Self::Error>;
}
```

The byte-slice input makes the totality domain explicit.
Parse outputs and remainders represent malformed, junk and partial bytes; these bytes never cause a panic.
The returned vector contains every valid segmentation in the frozen path-set order, and selection belongs to the decoder.
[parser-spec.md](parser-spec.md) and [parser-path-set.md](parser-path-set.md) freeze the concrete parse type and the order.

## Cross-trait rules

- Every fallible public method returns `Result`; no method panics on caller input.
- The seam imposes no `Send`, `Sync`, `'static`, serialization or object-store requirement.
- Associated types prevent `oxpinyin-core` from depending on data, user, engine or platform crates.
- Implementations must be deterministic for the same explicit input and state.
