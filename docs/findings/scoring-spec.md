# Scoring SPEC

Date: 2026-08-09 · Status: **frozen for W4-T3, constants provisional**
(amended 2026-08-15: λ is now read from the model config, no longer provisional
`1/2`; see the Architect correction log at the end of this file)

> **The constants in this SPEC are provisional.** The functional form, the
> cost scale, the sign convention, the tie-break and the totality rules are
> normative and implementations must follow them. The numeric weights are
> *not* claimed to be upstream's. They are chosen to satisfy orderings
> observed in the frozen captures, and W3+W4 integration against real tables
> is what will settle them. This banner is part of the freeze.

## Why the constants cannot be derived yet

`docs/findings/spec-derivation.md` lists "interpolation formula + constants"
under *what still requires reading*, with the reason: internal arithmetic. The
pinned oracle exposes candidate **order** at its public API and never a
probability. Its probabilities live in `interpolation2.text`, which
`model-provenance.md` classifies as not redistributable, so they cannot be
committed here either.

That leaves ordinal evidence: for a given input, which candidate the pin puts
first, second, third. Ordinal evidence constrains the constants — it produces
inequalities — but does not determine them. This SPEC states the inequalities
it can prove and picks values satisfying them.

Reading upstream C++ to recover the numbers is expected under AGENTS.md's
Source policy. The values below are still marked provisional: they were
settled by the parity sweep, not yet source-verified against the pinned
upstream implementation.

## Cost scale

Costs are `i64` on the fixed-point negative-log₂ scale of
`oxpinyin_core::cost`:

- `COST_PER_BIT` = 1,000. One bit of surprisal costs 1,000 units; an event the
  model gives half its mass to costs 1,000, a quarter 2,000.
- **Lower is better.** Costs accumulate by addition along a path.
- `UNKNOWN_COST` = 40,000 (40 bits) is charged for an event with no mass.
  Finite, not infinite: a path through an unknown token has to stay comparable
  rather than poisoning the arithmetic.
- All accumulation saturates. No addition, multiplication or subtraction in
  the scorer may panic, whatever a backend returns.

**No floating point anywhere.** `f64::ln` is not required to be bit-identical
across platforms and libms, and constitution item 6 makes engine output a pure
function of (input, user state, config) on every operating system. The
fixed-point logarithm is integer squaring and shifting.

## The interpolated bigram

The language-model term is the classic interpolation, stated as a
parameterised family:

```text
P(w_n | w_n-1) = λ · P_bigram(w_n | w_n-1) + (1 − λ) · P_unigram(w_n)
model_cost     = −log₂ P(w_n | w_n-1) × COST_PER_BIT
```

Evaluated as **one exact integer ratio** — numerator and denominator combined
before the logarithm — so no intermediate rounding enters and the result is
identical on every platform.

| Constant | Value | Status |
|---|---|---|
| λ | `table.conf` (`0.312699` pinned) | superseded the provisional `1/2` — see the correction log |

The `LanguageModel` implementation owns this term; the seam in
`core-trait-seam.md` requires it to combine the caller's `edge_cost`
deterministically, which it does.

## The log-linear combination

The decoder's cost for one phrase is a weighted sum of the model term and a
small set of structural features over the graph:

```text
cost(phrase) = w_lm · model_cost(history, token, Σ edge_penalty(kind))
             − phrase_key_bonus × (keys − 1)
```

The structural penalties are handed to the model as its `edge_cost`, which is
the seam's stated purpose. The coverage credit is applied afterwards, because
it is a property of the decoder's preference and not of the model.

| Weight | Provisional value | Meaning |
|---|---:|---|
| `lm_weight` | 1.00 | weight of the model term, over `WEIGHT_SCALE` = 100 |
| `exact_penalty` | 0 | an `Exact` edge is the reference |
| `segmentation_penalty` | 500 | half a bit for taking a non-greedy split |
| `incomplete_penalty` | 1,000 | one bit for an initial-only key |
| `phrase_key_bonus` | 2,000 | credit per key covered beyond the first |
| `expansion_limit` | 64 | see *Incomplete keys* below |

### The inequalities the captures actually prove

Each of these is measured on `fixtures/foundation/f-a.txt` or `f-c.txt`, and
each is asserted by a test:

1. **`phrase_key_bonus > 0`.** For `nihao` the pin lists `你好` before `你`;
   for `zhongguoren` it lists `中国人`, then `中国`, then `中`. A phrase
   covering more keys must be able to beat its own first syllable.
2. **`segmentation_penalty > 0`.** For `fangan` the pin lists `方案`
   (`fang` + `an`, both exact) before `反感` (`fan` + `gan`, where `fan` is
   the shorter split at position 0). Two phrases of equal weight and equal
   coverage are separated only by the edge kinds beneath them.
3. **`incomplete_penalty < phrase_key_bonus`.** For `nih` the pin lists `你好`
   before `你`, and for `zhongg` it lists `中国` before `中`. Covering one
   more key *through an initial-only edge* still has to win.

Inequality 3 is why the first draft of this configuration was wrong: an
incomplete penalty of 3,000 against a bonus of 2,000 put `你` ahead of `你好`
and contradicted the capture. The fixture report is what caught it.

Nothing here proves the *magnitudes*. 500 and 1,000 and 2,000 are one point in
the space the inequalities allow.

## Incomplete keys at lookup

An incomplete key stands for every complete key with the same phonetic
initial (`ChewingKey.m_initial`), in frozen inventory order. This is the
pin's behaviour: `nih` offers `你好`, `霓虹`, `拟合`, `泥孩`, `你还`, `你和`,
`你很`, `你会` — two-key phrases whose second syllable begins with `h`.
String-prefix expansion is wrong: `n` must not reach zero-initial `ng`,
and `z`/`c`/`s` must not reach `zh`/`ch`/`sh`.

Expansion is a Cartesian product and therefore bounded. Above
`expansion_limit` (64) sequences the key sequence yields **nothing**, not a
truncated subset: a caller must not be able to mistake a truncation for the
whole answer. `zzzzzzzz` expands past the limit and is correctly barren.

## Determinism and totality

- Per-key costs are precomputed once, at scorer construction, so the
  `EdgeCost` the k-best sweep calls is a table lookup that cannot fail. A
  backend failure surfaces at construction, where it can be reported, rather
  than inside a search that has no way to express it.
- An empty dictionary is not an error. It yields no phrases and
  `UNKNOWN_COST` per key.
- A backend that does fail is reported as `ScoringError`, never swallowed.
- Ranking is a stable sort by cost, so equal-cost phrases keep the fixture's
  (captured) order.

## Acceptance

Per-formula unit tests in `crates/oxpinyin-core/src/scoring.rs` cover each
worked example above.

`crates/oxpinyin-core/tests/scoring_fixture.rs` reports the fixture pass rate.
Measured at this freeze:

```text
capture records with candidates    55
covered by the mini vocabulary     23
needing another segmentation (T4)   4
comparable at this layer           19
top-1 agreeing with the pin        19
rate                            1.000
```

Read that honestly. The vocabulary is 90 authored phrases against the pin's
tens of thousands, so 32 of 55 records have nothing to find. Four more — the
`xian` and `fangan` families — have a first candidate the pin reached through
a *different* segmentation than the one it selected, which one-path scoring
cannot produce and is not meant to: ranking across the graph is W4-T4. Of the
19 records this layer can actually be compared on, it agrees with the pin on
all 19.

The test fails on any disagreement among comparable records, so a weight
change that breaks an observed ordering breaks the build rather than quietly
moving a number.

## Architect correction log

**2026-08-15 — λ read from config (was provisional `1/2`).** The λ row above
listed a provisional `1/2` ("authored, deliberately neutral", per the freeze
banner's "not claimed to be upstream's"). The decoder now reads λ from the
model's `table.conf` rather than hardcoding it; the pinned value is `0.312699`,
recorded in `data-formats.md` §3 (verified against the oracle's installed
copy). It is held as the exact decimal rational the file denotes, reduced to
lowest terms (`312699 / 1_000_000`), preserving the "one exact integer ratio"
evaluation above (a value `> 1` is rejected and `model_cost` uses checked
arithmetic, so a malformed config floors at `UNKNOWN_COST`, never panics).
Ranking impact is path-specific — the real-unigram three-key order is
λ-insensitive (the parity pins hold), while the export-ABI `model_cost` path is
not; see `scoring-constant-sweep.md`. The functional form, cost scale, sign
convention, tie-break and totality rules in this SPEC are unchanged.

**2026-08-16 — incomplete expansion is phonetic initial, not string prefix.**
The paragraph above said an incomplete key stood for every complete key it is
a proper prefix of. That leaked `n` into `ng` and `z`/`c`/`s` into
`zh`/`ch`/`sh`. Lookup now uses `phonetic_initial` (the `m_initial` index);
see `docs/findings/pin-refreeze-2026-08.md`. The Cartesian-product bound and
empty-on-overflow rule are unchanged.
