# Core trait seam SPEC

Date: 2026-08-09 · Status: frozen for Foundation. **Superseded 2026-10-18 UTC
in part:** the trait *signatures*, unsealed shape, object safety and
"grow by defaulted methods only" rule were a Rust-source-compatibility
freeze, retired by the maintainer ruling that the C APIs/ABIs are the only
supported external interfaces ([interface ruling](../constitution.md#external-interfaces)). The
behavioural rules below (totality, determinism, ordering, costs) remain.

This SPEC freezes the dependency-free `oxpinyin-core` seam before downstream
implementation. The traits are public and unsealed. Existing required methods
are the minimum contract; future growth must use methods with default
implementations unless an Architect correction is merged first.

## Shared cost

```rust
pub type Cost = i64;
```

Costs are signed integers so ordering and accumulation remain deterministic.
The scale is owned by the decoder/scoring SPEC; this seam fixes only the
representation.

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

An empty lookup is `Ok(Vec::new())`. Implementations return entries in stable,
deterministic order. The trait does not prescribe storage, token layout or
ranking.

2026-10-16: #825 adds defaulted `Dictionary::phrase_item_pronunciations(token)`, answering `PhraseItemRead::Unsupported` unless overridden; it reads the stored pronunciations of the phrase item behind a token so a forcing is revalidated as the pin's `validate_constraint` does (`phonetic_lookup.cpp:120-165` at 074a2219). A defaulted addition under the paragraph above (human approval, 2026-10-16).

2026-10-16: #821 (#757) changes `Dictionary::trellis_ranges` to fill token-only ranges (`PhraseIndexRanges<()>` in place of `PhraseIndexRanges<Self::Entry>`, as the pin's `convert` does), a signature change. A step prices a ranged token through the new defaulted `trellis_item(syllables, tones, token) -> Option<TrellisItem>`, and a result's text is read through the new defaulted `trellis_text(syllables, tones, token)` when it is emitted; `TrellisItem { chars, pronunciation }` is new. The helper trait `PhraseIndexToken` gains two required methods, `phrase_index_item` and `phrase_index_text`, which `PhraseEntry` implements. The defaults read `trellis_records`, so dictionaries that override none of them need no change. It also adds defaulted `trellis_ranges_continued`, the ranges and the `SEARCH_CONTINUED` probe from one read, whose default is `trellis_ranges` then `phrase_prefix_exists` (human approval, 2026-10-16).

2026-10-16: #831 (#757) adds defaulted `Dictionary::trellis_item_on_path(syllables, tones, token, memo)`, whose default is `trellis_item`, and `TrellisPathMemo`, a fill-once slot the n-best caller holds per key path for one span search. `RuntimeDict` keeps the path's user lookup and tone-less keys there in place of recomputing them per token; a memo holding another type is not read, and the answer is `trellis_item`'s (human approval, 2026-10-16).

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

2026-10-01: #615 adds defaulted `UserModel::observe_with_keys(history, token, readings)`, delegating to `observe`; its lazy tone-bearing readings preserve the trained matrix span and path multiplicity (human approval, 2026-10-01).

`score` is read-only. `observe` is the explicit learning boundary; callers can
omit it entirely in learning-off modes. Neither method may depend on hidden
process-global state.

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

The edge cost is present from the first signature. A model may combine it with
its own cost but must do so deterministically and report arithmetic or backend
failure through `Result`.

## `InputParser`

```rust
pub trait InputParser {
    type Parse;
    type Error;

    fn parse(&self, input: &[u8]) -> Result<Vec<Self::Parse>, Self::Error>;
}
```

The byte-slice input makes the totality domain explicit. Malformed, junk and
partial bytes are represented by parse outputs and remainders rather than a
panic. The returned vector contains every valid segmentation in the frozen
path-set order; selection belongs to the decoder. The parser/path-set SPEC
freezes the concrete parse type and ordering before implementation.

## Cross-trait rules

- Every fallible public method returns `Result`; no method panics on caller
  input.
- The seam imposes no `Send`, `Sync`, `'static`, serialization or object-store
  requirement.
- Associated types prevent `oxpinyin-core` from depending on data, user, engine
  or platform crates.
- Implementations must be deterministic for the same explicit input and
  state.
