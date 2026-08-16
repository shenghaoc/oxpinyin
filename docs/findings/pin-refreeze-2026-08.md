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
SPEC now records:

```text
top-1               10177
top-5-set           10189
prefix-10 overlap   94871 of 98930
absent                  1
tie-swaps             1036
```

A later change that moves any of these is a deliberate re-freeze decision, not
a silent edit.
