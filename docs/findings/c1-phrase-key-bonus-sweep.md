# C1: phrase_key_bonus re-sweep — negative result

Date: 2026-08-14 · Status: **measured — negative result**

Stage-4 calibration (C1) under the frozen construction contract
(`docs/findings/candidate-construction.md` §8). One lever only:
`ScoringConfig::phrase_key_bonus`, currently frozen at 1,000.

## Hypothesis

The residual is dominated by rank 2–5 near-misses (the oracle's rank-1 is in
our list but ordered lower). `phrase_key_bonus` scales the coverage credit
`bonus × (keys − 1)` subtracted from the weighed model cost, so retuning it
changes how strongly long phrases outrank their own first syllables. A full-
corpus re-sweep around the frozen value tests whether some other value moves
more oracle rank-1s into first position. This also settles the open warning
in `docs/findings/parity-climb-residual.md` ("do not raise
`phrase_key_bonus` back toward 2,000 without a new full-corpus number").

## Method

Edited only `phrase_key_bonus` in `ScoringConfig::default`
(`crates/pinyin-core/src/scoring.rs`) and re-ran the full portable parity
suite per value:

```
cargo test --release -p pinyin-oracle --test real_tables_integration -- --nocapture
```

`real_tables_session_reports_parity` over the W2 corpus (10,190 inputs with
oracle candidates, 98,930 prefix-10 depth) against
`fixtures/w4/oracle-candidates.txt` via `/tmp/pinyin-rs-export`
(pin-verified `pinyin-migrate export`). Each run ≈ 3.5 min. The frozen
baseline reproduced all five §0 pins bit-exactly before the sweep; non-frozen
values trip the pins by design (`candidate-construction.md` §4).

The admissible region is upward-only: the capture inequality
`incomplete_penalty (999) < phrase_key_bonus` is asserted by a test, so the
frozen 1,000 is already the minimum integer value. The sweep maps the ray
[1,000, 2,000].

## Results

| bonus | top-1 | top-5-set | prefix-10 | absent | verdict |
|------:|------:|----------:|----------:|-------:|---------|
| **1,000 (frozen)** | **64% (6525)** | **90% (9232)** | **66% (65505/98930)** | **70** | **only admissible value** |
| 1,025 | 63% (6520) | 90% (9265) | 65% (64959/98930) | 65 | top-1 −5, prefix-10 −546 |
| 1,050 | 63% (6519) | 91% (9276) | 65% (64653/98930) | 64 | top-1 −6, prefix-10 −852 |
| 1,075 | 64% (6538) | 91% (9292) | 65% (64408/98930) | 63 | top-1 +13, prefix-10 −1097 |
| 1,100 | 64% (6534) | 91% (9349) | 64% (64077/98930) | 63 | top-1 +9, prefix-10 −1428 |
| 1,125 | 64% (6530) | 91% (9352) | 64% (63654/98930) | 60 | top-1 +5, prefix-10 −1851 |
| 1,200 | 63% (6453) | 92% (9471) | 62% (62324/98930) | 57 | top-1 −72 |
| 1,500 | 55% (5656) | 86% (8826) | 52% (51858/98930) | 70 | far worse |
| 2,000 | 44% (4583) | 74% (7619) | 47% (46986/98930) | 100 | far worse |

## Analysis

The five metrics split into two responses:

- **prefix-10 overlap is monotone decreasing in the bonus** across the whole
  measured ray (65,505 → 46,986). A higher coverage credit lifts every
  multi-key phrase while sinking single-key candidates in our ordered top-10;
  the oracle's own top-10 mixes short and long forms, so overlap falls at
  every step — already −546 at the smallest 25-unit step.
- **top-1 is jagged**, with a shallow peak of +13 at 1,075 and +9 at 1,100,
  before collapsing (−72 at 1,200, −1,942 at 2,000). The rank-1 comparison
  only needs one correct reordering per input, and a modest boost achieves a
  few hundred of those — at the cost of thousands of prefix-10 hits.

The selection rule is: maximise top-1 with **no regressions** on the other
four metrics. Every value above 1,000 regresses the prefix-10 pin, and no
value below 1,000 is admissible (the 999 < bonus inequality). The frozen
1,000 therefore remains the rule's optimum. The 2,000 warning from
`parity-climb-residual.md` is now backed by full-corpus numbers: top-1 44%
and absent 100.

## Conclusion

C1 is a **negative result**. No code change is kept;
`phrase_key_bonus` stays at 1,000 and the five §0 pins are untouched. The
coverage-credit lever trades top-10 breadth for rank-1 alignment and cannot
beat the frozen value without regressing prefix-10. C2 (secondary frequency
key / tie-break) and C3 (`UNIGRAM_TIEBREAK_SCALE` neighbourhood) remain
un-started pending instruction.

## References

- `crates/pinyin-core/src/scoring.rs` — `ScoringConfig::default`,
  `coverage_bonus`, `phrase_cost`
- `docs/findings/scoring-constant-sweep.md` — the sweep that froze 1,000
- `docs/findings/scoring-spec.md` — functional form; magnitudes provisional
- `docs/findings/parity-climb-residual.md` — the 2,000 warning settled here
- `docs/findings/candidate-construction.md` §0, §4, §8 — pins and gates
