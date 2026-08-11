# F3: bigram-in-kbest — negative result

Date: 2026-08-11 · Status: **measured — negative result**

## Hypothesis

Move the bigram transition cost from `Session::refresh` (post-k-best ranking)
into `Scorer::cost` (inside k-best) so the k-best search prefers paths where
bigram transitions are cheap. The `EdgeCost` trait already has the right
signature:

```rust
fn cost(prev: Option<&Edge>, edge: &Edge) -> Cost
```

The `BigramLanguageModel` is already loaded in the `Scorer` and phrase tokens
are available via dictionary lookup. The change is to charge
`bigram(prev,curr) - unigram(curr)` per edge when `prev` is `Some`.

## Implementation

- `Scorer` gained `best_tokens: Vec<Option<PhraseToken>>` — the cheapest
  phrase token per `SyllableKey` (428 keys), built in `key_cost_table` via
  `dictionary.lookup(&[key])` + `model.score(&[], token, 0)`.
- `Scorer::with_key_costs` changed from `const fn` to `fn` to carry the
  token table.
- `EdgeCost::cost` computes `base = key_costs[edge.key.index()] + edge_penalty`,
  then when `prev` exists looks up `best_tokens` for both edges, scores
  `bigram = model.score(&[prev_token], curr_token, 0)` and
  `unigram = model.score(&[], curr_token, 0)`, and adds
  `delta = bigram - unigram` to `base`.
- Guard added: if `bigram == UNKNOWN_COST || unigram == UNKNOWN_COST`
  (40000), return `base` unchanged — the bigram has no information about
  this transition, so delta must be 0, not a ~37,000 penalty.

No `EdgeCost`, `Dictionary`, or `LanguageModel` trait signatures were changed.

## Method

Portable parity test on `feat/parity-climb-2` after F1 (64% top-1 baseline):

```
cargo test --release -p pinyin-oracle --test real_tables_integration -- --nocapture
```

Metric: `real_tables_session_reports_parity` over the W2 corpus (10,190 inputs
with oracle candidates, 98,930 prefix-10 depth) against
`fixtures/w4/oracle-candidates.txt` via `/tmp/pinyin-rs-export`.

## Results

| variant | top-1 | top-5-set | prefix-10 | absent | delta vs F1 |
|---------|------:|----------:|----------:|-------:|-------------|
| F1 baseline | 64% (6525/10190) | 90% (9232) | 66% (65505/98930) | 70 | — |
| F3 without guard | 63% (6519) | 90% (9235) | 66% (65400/98930) | 71 | -1pp, +1 absent |
| F3 with guard | 63% (6519) | 90% (9235) | 66% (65397/98930) | 70 | -1pp, unchanged |

Both variants regress top-1 by 6 cases (-1pp). The guard fixes the absent
regression (71 → 70) but does not recover top-1.

## Analysis

Single-key token proxies are too noisy. The cheapest token per syllable
(e.g. the most frequent phrase for key `ni`) rarely matches the phrase the
decoder actually selects in a multi-key path. Charging a per-edge delta based
on this proxy adds noise that outweighs the bigram signal. The correct fix
requires phrase-level lattice edges where each edge carries its own phrase
token, so the bigram is scored on the actual phrase sequence — an
architectural change deferred to Stage 2.

## Conclusion

F3 is a **negative result**. No code change is kept. The finding is recorded
here and the branch retains only the F1 code change (64%/90%/70 floor).

## References

- `crates/pinyin-core/src/scoring.rs` — `Scorer`, `key_cost_table`, `EdgeCost`
- `crates/pinyin-core/src/kbest.rs` — `EdgeCost` trait (frozen)
- `crates/pinyin-data/src/lm.rs` — `BigramLanguageModel`, `UNKNOWN_COST`
