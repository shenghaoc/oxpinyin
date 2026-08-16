# F1: split `process_key` / `type_pinyin` accept sets for oracle parity

Date: 2026-08-11 · Status: **landed — top-1 63%→64%, absent 177→70 on fixtures/w4/oracle-candidates.txt (10,190 inputs)**

## Observed oracle behaviour (fixture only)

From `fixtures/w4/oracle-candidates.txt` (rank-1 lines, verbatim):

| Input | Oracle rank 1 | Reads as |
|---|---|---|
| `b#ing` | 不 | `b`-family — **not** `bing`→并 |
| `ai.hang` | 爱 | same top list as pure `ai` |
| `haNi` | 哈 | like pure `ha`, not multi-char |
| `c:unla` | 从 | incomplete `c` family |
| `cDhubogai` | 从 | incomplete `c` family |
| `a\|i` | 阿 | pure `a` — **not** `ai`→爱 |
| `b3i` | 不 | `b`-family — **not** `bi` |

Contrast clean inputs in the same fixture:

- `bing` → 并 (rank 1)
- `ai` → 爱 (rank 1)

**What the fixture supports.** Rank-1 for junk-bearing inputs matches a **hard stop** at the first non-`a-z`/`'` byte (reachable prefix only). It does **not** match stripping junk (`b#ing`→`bing`→并), and candidate texts alone do not prove multi-key continuation past the junk byte.

## Why the first F1 attempt was wrong

The first attempt widened `is_input_character` so `process_key` accepted all ASCII except space. That:

1. **Contradicted frozen `docs/findings/session-api.md`** (`process_key` only appends a-z/`'`).
2. Admitted control ASCII into preedit via `char::is_ascii()`.
3. Put the fix on the interactive key gate rather than the batch path the parity harness uses.

It recovered ~107 absent cases only because putting junk into `raw` let **`SegmentGraph`** hard-stop at the junk byte — the graph already implements the fixture-aligned boundary. The number was right; the layer was wrong.

## Fix

| Path | Accept set | Role |
|---|---|---|
| `process_key` | `a-z` and `'` only | Interactive typing; frozen `session-api.md` |
| `type_pinyin` | printable ASCII (`char::is_ascii_graphic()`) | Parity / batch harness; keeps junk in `raw` |

No change to `FullPinyinParser`, `parser-spec.md`, or `session-api.md`. The decoder (`SegmentGraph::build` on `raw`) already stops the reachable prefix at the first non-syntax byte.

## Design: split accept sets for interactive vs batch

The split is intentional and introduces a real divergence on junk-bearing inputs:

| Call sequence | `raw` | Decoder sees | Rank-1 family |
|---|---|---|---|
| `process_key` for `b`, `#`, `i`, `n`, `g` | `"bing"` | complete `bing` | 并-family |
| `type_pinyin("b#ing")` | `"b#ing"` | hard-stop after `b` | 不-family |

That is fine:

- **Interactive (IBus-class shells).** Printable junk such as `#` does not arrive as `LogicalKey::Character` while composing. The frontend typically auto-commits the composition on punctuation before the session buffer would see it. `process_key` therefore never needs to compose junk; its accept set stays the frozen a-z/`'` contract.
- **Batch (`type_pinyin`).** The parity harness feeds the corpus strings the oracle fixture was captured with — including embedded junk — in one shot. Keeping those bytes in `raw` is what matches fixture rank-1 (`b#ing` → 不). Re-filtering junk inside `type_pinyin` would collapse `b#ing`→`bing` and re-regress absent toward 177.

Do not “fix” the divergence by making the two paths agree on junk; that would either break the frozen session contract or break oracle parity.

## Evidence

- Fixture lines above (oracle-candidates.txt).
- Graph hard-stop: `crates/oxpinyin-core/src/graph.rs` (`junk_stops_the_reachable_prefix_without_stopping_the_build`).
- Measured on this tree:

```text
cargo test --release -p pinyin-oracle --test real_tables_integration -- --nocapture

compared                10190
top-1                     6525  64%
top-5-set                 9232  90%
prefix-10 overlap        65505 of 98930  66%
absent                      70
```

Before F1 (post constant-sweep residual): top-1 63% (6435/10190), absent 177.

## Implementation notes

1. Restore `is_input_character` to `is_ascii_lowercase() || character == '\''`.
2. `type_pinyin` uses `is_batch_input_character` = `is_ascii_graphic()` (no space, no controls).
3. Regression: `only_parser_syntax_extends_the_composition`; batch/key split tests; `type_pinyin("b#ing")` top-1 is 不-family not 并-family.
