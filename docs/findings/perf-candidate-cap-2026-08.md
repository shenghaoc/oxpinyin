# Perf attribution — candidate-cap removal (2026-08)

## Question

Removing `MAX_CANDIDATES` makes the window scan materialise the full lookup
table (`n` → 788, `zh` → 1905, `be` → 1369 observed totals) instead of the
old 64-entry preview.  The first `scan_perf` run reported a large
`keystroke_cycle_20_inputs` regression.  This note attributes that regression
and compares it against the pinned C++ oracle doing the same work.

## Paired criterion run

Same machine, same `PINYIN_EXPORT_DIR` / `PINYIN_MODEL_DIR`, back-to-back:

| Build | `keystroke_cycle_20_inputs` (20 samples) |
|---|---|
| #86, capped at 64 | 55.560–57.733 ms |
| #87, uncapped | 76.006–84.846 ms |

The cap is the cause.  The remaining groups are not part of this attribution:
`prefix_probe` and `parse_interpolation2` do not touch candidate counts, and
their earlier “regression” flags were the saved-baseline comparison shifting
across branches, not a paired code change.  The user-store arms use the flat
export model and are reported separately in PR #87.

## Oracle comparison

A dlopen harness drove the same 20 inputs through the pinned C++ oracle with
the W8 parity profile (`0x18a`, sort `0x1e`): one `pinyin_parse_more_full_pinyins`
per keystroke on the accumulated prefix, then `pinyin_guess_candidates` and
`pinyin_get_n_candidate`.  100 measured cycles:

- C++ oracle: **38.169 ms/cycle**
- oxpinyin #87: **~76–85 ms/cycle**

Conclusion: the regression versus the capped preview is legitimate — the C++
oracle also constructs the full table, so the old 64-candidate benchmark was
measuring a cheaper, wrong behaviour.  However oxpinyin is currently about
**2.0–2.2×** the oracle on the same keystroke cycle.  That is the first
concrete Stage-2 optimisation target, recorded here and not optimised in the
cap-removal change.
