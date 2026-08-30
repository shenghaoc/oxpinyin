# Parity-climb residual analysis

Date: 2026-08-10 · Status: **measured after constant sweep**

After bigram integration and the scoring-constant sweep
(`docs/findings/scoring-constant-sweep.md`), full-corpus rates against
`fixtures/w4/oracle-candidates.txt` are:

```text
top-1      63%   (6435 / 10190)
top-5-set  89%   (9087 / 10190)
prefix-10  65%
absent     177   (1.7%)
```

**Post-F1 (see docs/testing/f1-junk-aware-parse.md):** top-1 64% (6525/10190), absent 70 on fixtures/w4/oracle-candidates.txt. The tables and residual bucket counts below are the pre-F1 snapshot that generated the F-task plan.

Top-1 is still below 80%. This finding classifies the residual top-1
misses and proposes targeted fixes. Re-run with:

```bash
cargo run -p pinyin-oracle --release --bin parity-worst
```

## All top-1 misses (3,755 of 10,190)

| Bucket | Count | Share of misses | Share of corpus |
|---|---:|---:|---:|
| rank 2–5 | 2,652 | 70.6% | 26.0% |
| rank 6–9 | 342 | 9.1% | 3.4% |
| rank 10+ | 584 | 15.6% | 5.7% |
| absent | 177 | 4.7% | 1.7% |

The dominant residual is **near-miss ranking** (oracle's #1 is in our top
five but not first). Absent is already near the data-layer floor reported
before the climb (3.8% → 1.7%).

## Worst 50 (oracle #1 is absent from our list)

Sorted by severity (absent first). Almost all are **corpus noise /
junk-handling** cases, not clean pinyin:

| Pattern | Examples | What we do | What the pin does |
|---|---|---|---|
| Punctuation inside the syllable | `a\|i`, `b#ing`, `b.o`, `d+ui`, `c:unla` | `type_pinyin` drops non `a-z`/`'`, so `b#ing` → `bing` → 并… | Treats junk as a hard break / partial; often returns a different first syllable (不 for `b#ing`) |
| Digits / symbols as tone or noise | `b3i`, `h8aizuo`, `be~ng` | Dropped; remainder re-parsed | Partial match on the leading letter(s) |
| Mixed case | `cDhubogai`, `haNi`, `cSunluosai` | Uppercase is dropped (not `a-z`), so `cDhu…` → `chu…` | Case-folds or truncates differently |
| Long incomplete / oversegmented | `bair,uanzong`, `biangangruiguang…` | First complete keys only; comma dropped | Different first-phrase cut |

**Category counts over the worst 50:** missing-candidate 43, prefix-only 7,
wrong-scoring 0, wrong-segmentation 0.

So the tail of the residual is **not** a ranking problem — it is input
normalisation and the incomplete-key expansion ceiling
(`scoring-spec.md` / `expand_keys` empty product).

## Rank 2–5 near-misses (2,652)

These have the right candidate and lose only on order. Likely causes,
in priority order for follow-up:

1. **Unigram scale.** Empty-history unigram is divided by
   `UNIGRAM_TIEBREAK_SCALE = 16` so coverage survives; same-length frequency
   signal may be too weak against the pin's order.
2. **Bigram only after a selection.** Multi-phrase sentences use bigram
   history inside `collect_sentence`, but the first candidate of a fresh
   composition has empty history — pure (scaled) unigram + structure.
3. **No user model / no context.** The pin's session is also fresh per
   observe, so this is not user-learning; remaining gap is model form
   (interpolation λ, or features we do not have).
4. **Segmentation path cost vs phrase cost.** k-best path costs use
   precomputed key unigrams + edge penalties only; phrase ranking is a
   second pass. Path order can starve the segmentation that holds the
   pin's top phrase.

## Proposed fixes (each a separate commit with before/after rates)

| # | Fix | Targets | Risk | Landed |
|---|---|---|---|---|
| F1 | **Junk-aware parse:** map non `a-z`/`'` to a hard boundary (or drop with a zero-width break) matching the pin's observe behaviour on the noise strata | absent + worst-50 | Needs a small SPEC of observed junk rules; no upstream read | landed (`eb00eff`) — top-1 63%→64%, absent 177→70 |
| F2 | **Stronger empty-history unigram** (lower `UNIGRAM_TIEBREAK_SCALE`, or secondary stable key by raw frequency after cost) | rank 2–5 | Must re-check multi-key vs first-syllable inequalities | measured negative (`750bd6c`) — scale 16 remains optimal |
| F3 | **Path-aware phrase bonus:** charge coverage only on the winning segmentation, or fold bigram into path cost for multi-key paths | rank 2–5 / 6–9 | Touches k-best cost seam; keep trait signatures | attempted negative (`ab657a8`) — single-key token proxy too noisy; see f3-bigram-kbest.md |
| F4 | **Prefix-query dictionary** for multi-initial incomplete sequences (closes the `expand_keys` empty-product gap) | prefix-only / long incomplete | Data API extension (defaulted method only) | skipped — trigger absent > 100 not met (absent = 70 after F1) |

F1 is the largest absent win. F2 is the largest top-1 win. F3/F4 are
structural and should wait until F1–F2 are measured.

## What not to do

- Do not raise `phrase_key_bonus` back toward 2,000 without a new full-corpus
  number — the sweep showed that hurts top-1.
- Change junk-rule handling without re-running the noise-strata differential;
  the pin's public output on those strata is the evidence.
