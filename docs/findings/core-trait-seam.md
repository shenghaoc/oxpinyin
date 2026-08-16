# Core trait seam SPEC

Date: 2026-08-09 · Status: frozen for Foundation

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
