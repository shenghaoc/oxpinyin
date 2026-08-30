# F2: UNIGRAM_TIEBREAK_SCALE sweep — measured

Date: 2026-08-11 · Status: **measured**

## Problem

The first candidate of a fresh composition has no bigram history, so it ranks by
scaled unigram + structure alone. `UNIGRAM_TIEBREAK_SCALE = 16` divides the
unigram cost, which may under-weight frequency for same-length phrases.

The residual analysis shows 2,652 rank-2–5 near-misses (70.6% of all top-1
misses): right candidate in the top 5 but not first.

## Hypothesis

Reducing `UNIGRAM_TIEBREAK_SCALE` will strengthen the unigram signal relative
to structure cost, pushing more correct single-phrase rankings into top-1.

## Method

Changed the constant `UNIGRAM_TIEBREAK_SCALE` in `crates/oxpinyin-data/src/lm/mod.rs`
and re-ran the portable parity test for each value:

```bash
cargo test --release -p pinyin-oracle --test real_tables_integration -- --nocapture
```

Metric: `real_tables_session_reports_parity` over the W2 corpus (10,190 inputs
with oracle candidates) against `fixtures/w4/oracle-candidates.txt` via
`/tmp/oxpinyin-export`. Each run ~10 min.

## Results

| Scale | top-1 | top-5-set | prefix-10 | absent | verdict |
|------:|------:|----------:|----------:|-------:|---------|
| 1 | 21% (2197) | 46% (4775) | 34% (34052/98930) | 1737 | far worse |
| 2 | 25% (2586) | 52% (5381) | 37% (37386/98930) | 1251 | far worse |
| 4 | 33% (3393) | 63% (6516) | 46% (45601/98930) | 356 | worse |
| 8 | 50% (5119) | 86% (8850) | 59% (59307/98930) | 97 | worse than 16 |
| **16** | **64% (6525)** | **90% (9232)** | **66% (65505/98930)** | **70** | **optimal** |
| 32 | 63% (6481) | 93% (9537) | 65% (64354/98930) | 84 | slightly worse than 16 |

Compared: 10,190 inputs in all runs. `compared` is the number of corpus inputs
with oracle candidates; `prefix-10` is overlap of our top-10 vs oracle top-10.

## Conclusion

**16 is optimal for top-1.** Lower scales over-weight unigram frequency and
drown the `phrase_key_bonus` / segmentation signal, collapsing top-1 from 64%
to as low as 21%. Scale 32 is marginally worse than 16 on top-1 (63% vs 64%)
and absent (84 vs 70). No change to `UNIGRAM_TIEBREAK_SCALE` is warranted;
the constant stays at 16.

F2 is therefore a **negative result**: the sweep proves the current value is
already optimal. The rank 2–5 near-misses are not fixed by strengthening the
empty-history unigram alone.

## Implementation constraint

No trait signatures or scoring-spec SPEC were changed. The sweep is local to
the LM's empty-history branch (`lm/mod.rs:441-443`).
