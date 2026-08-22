# Pin re-freeze — phonetic-initial incomplete expansion (2026-08)

Status: **approved maintainer decision.** The candidate-construction pins are
re-frozen at the values below. They are the frozen pins going forward for every
subsequent Stage-1 measurement.

## Old → new

| Metric | Old frozen pin | New frozen pin | Δ |
|---|---|---|---|
| top-1 | 10136 | 10177 | +41 |
| top-5-set | 10182 | 10189 | +7 |
| prefix-10 overlap | 94456 of 98930 | 94871 of 98930 | +415 |
| absent | 1 | 1 | 0 |
| tie-swaps (order-only) | 1030 | 1036 | +6 |

Measured on the W8 stack's actual base (`feat/capi-interpolation2-unigrams`)
before freezing: exactly these values. Debug and release, serial and parallel
remain bit-identical.

## Cause

`oxpinyin-core::scoring::completions` expanded an initial-only key by string
prefix. That made the incomplete spelling `n` reach every syllable whose text
starts with `n`, including the zero-initial syllable `ng`; it also made `z`,
`c`, and `s` cross into the distinct retroflex initials `zh`, `ch`, and `sh`.

The pinned C++ incomplete index is keyed by `ChewingKey.m_initial`, not by
spelling. The construction copies only the initial into the search key
(`libpinyin 2.11.91`, `src/storage/pinyin_phrase3.h:170-177`), and both
phrase-index search paths dispatch to that construction when the query
contains an incomplete key
(`src/storage/chewing_large_table2.h:136-144`,
`src/storage/chewing_large_table2.cpp:178-184`). The `ng` row is explicitly
zero-initial (`src/storage/pinyin_parser_table.h:4211`), so it is unreachable
from the N-initial spelling `n`.

This was a Stage-1 reproduction bug, not a scoring-taste change. The core fix
expands incomplete keys by phonetic initial. It strictly increased oracle
agreement on every re-frozen metric and moved no import or train differential.

## Frozen pins going forward

`real_tables_session_reports_parity` asserts, and the candidate-construction
SPEC now records (superseded by the 2026-08-22 amendment below):

```text
top-1               10178
top-5-set           10190
prefix-10 overlap   94872 of 98930
absent                  0
tie-swaps             1036
```

A later change that moves any of these is a deliberate re-freeze decision, not
a silent edit.

## 2026-08-21 amendment — doubled apostrophe

The pins above supersede the initial re-freeze table (top-1 10177, top-5-set
10189, prefix-10 94871 of 98930, absent 1). W12 Class B closed on
2026-08-21: the doubled apostrophe now acts as a single separator between
groups (`docs/findings/parser-spec.md` architect correction log 2026-08-21;
`docs/findings/parser-spec-contradiction-incomplete-keys.md` decision 3,
doubled half). `ni''hao` was the sole absent input and the sole top-5 miss;
its `你好` phrase now surfaces, moving every affected metric by exactly one
position. Diagnostic details in `docs/findings/corpus-tail.md`.

| Metric | Prior pin | Amended pin | Δ |
|---|---|---|---|
| top-1 | 10177 | 10178 | +1 |
| top-5-set | 10189 | 10190 | +1 |
| prefix-10 overlap | 94871 of 98930 | 94872 of 98930 | +1 |
| absent | 1 | 0 | −1 |
| tie-swaps (order-only) | 1036 | 1036 | 0 |

## 2026-08-22 amendment — Class A comparator tie law

W12 Class A closed: the candidate comparator's frequency key is now the
pin's `m_freq` — the unigram possibility `(1−λ)·unigram/total` computed
in C `float`, amplified by 2²⁴, truncated to `guint32`
(`pinyin.cpp:1855-1866`; the model20 phrase-index unigram is
interpolation2 count + 1, the index total 51,051,831 = interpolation2
sum + 138,096 items) — and the window scan flushes each window
token-ascending, the array order GLib's stable sort keeps for
comparator-0 pairs. `loses_to` and the n-best trellis are untouched.
Diagnosis and the 12-input table in `docs/findings/corpus-tail.md`
(Class A, CLOSED 2026-08-22).

| Metric | Prior pin | Amended pin | Δ |
|---|---|---|---|
| top-1 | 10178 | 10190 | +12 |
| top-5-set | 10190 | 10190 | 0 |
| prefix-10 overlap | 94872 of 98930 | 98930 of 98930 | +4058 |
| absent | 0 | 0 | 0 |
| tie-swaps (order-only) | 1036 | 0 | −1036 |

The candidate surface agrees with the pinned oracle bit-identically on
every W2 corpus input at depth 10. Sentence pins the same day: row-0 and
full-list agreement hold (488 / 385), first-6 candidate rows rise
370 → 379 (`sentence_surface_reports_parity`).
