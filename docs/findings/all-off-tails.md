# Findings — W12 all-off TEXT-set tails (the six option-sweep residuals)

Date: 2026-08-22 · Status: **CLOSED 2026-08-24 — all six rows class (iii),
not a divergence; no fix needed.** The tails were the pre-port comparator
species the Class A port (`e941090`) closed on the corpus; re-measurement
after the port shows bit-identical lists (§"Closure").

`docs/findings/option-bits.md` §"TEXT-set STOP triage" froze six inputs
as W12 residuals measured by the live-oracle W10 control under
ALL-BITS-OFF (`0x0`). This finding names each input's all-off behaviour
on the corpus-tail scoring path — no oracle FFI, the frozen fixture and
the shared tables only — and assigns each row its class. It does not
touch ranking, the parser, or any pin.

## Reproduction

```bash
PINYIN_EXPORT_DIR=/tmp/oxpinyin-export \
PINYIN_MODEL_DIR=<extracted-model20> \
cargo run -p pinyin-oracle --release --bin corpus-tail -- --all-off-tails
```

The `--all-off-tails` flag is this change's addition to the existing
`bin/corpus-tail`: same setup as the corpus pass (exported redb tables,
real unigrams from `interpolation2.text`, read-only session, the
`fixtures/w4/oracle-candidates.txt` fixture), driven at
`OptionBits::default()` (`0x0`). Read-only, `Result` on I/O, no panic
path.

**2026-08-24 re-measurement (branch `fix/all-off-tails`, post-Class-A
code).** The W10-era numbers this finding froze — shared prefix 8 / 6 / 4
and the diverging tail glyphs below — were measured against the
**pre-port** engine and reproduce only on pre-`e941090` code. On current
`origin/main` every fixture-backed row is bit-identical at depth 10:
shared prefix 10 of 10, `pin_not_ours=0`, `ours_not_pin=0` for all of
`cang` / `sang` / `lve` / `ang` (`n=31/16/22/21` unchanged, top-1
unchanged), and the correction-alias and option-word-invariance verdicts
below still hold. The stale-prose claim that the Class A port left these
tails "unchanged" was asserted at `e941090` without re-running this pass;
it was wrong — the port closed them with the corpus residual.

No zhuyin scheme 7, double scheme 30, or toned incomplete key is sent
anywhere — the inputs are bare latin words.

### Why the 0x18a fixture row is the 0x0 answer

The fixture was captured at the parity word `0x18a`
(`oracle-candidates-v1`); the W10 control ran the oracle at `0x0`. For
the four fixture-backed inputs the two words admit the same answer, so
the fixture row is the pin's all-off list:

1. `cang`, `sang`, `lve`, `ang` are unflagged `pinyin_index` entries
   (`pinyin_parser_table.h:46,463,311,15`) — `check_pinyin_options`
   (`pinyin_parser2.cpp:38-58`) passes them under any option word.
2. `PINYIN_INCOMPLETE` gates only the initial-only entries (`:41-46`),
   and the full-pinyin DP prefers the fewest keys at equal length
   (`:301-303`), so `cang` selects `[cang]`, never `[c][ang]`, under
   either word.
3. None of the four keys is a divided-table or resplit-table orig
   (`special_table.h:9-118`), so the `0x80`/`0x100` bits are inert.
4. The guess path never reads `PINYIN_INCOMPLETE` (no occurrence in
   `pinyin.cpp`).

Engine-side, our lists for all four are INVARIANT between `0x0` and the
parity word (measured below), and the fixture comparison lands on the
same prefix/`n` the live-oracle W10 control froze — the strongest check
available without re-linking the oracle.

## The six rows

`ang` is carried as a seventh row: it is the canonical key the
`agn`/`amg` correction aliases resolve to (`pinyin_parser_table.h:11,13,15`
share table index 4), so the alias rows are read against it.

The "first divergence" column is the **pre-port** measurement (frozen
2026-08-22 against pre-`e941090` code) and is kept as history; post-port
the divergences are gone (§"Closure").

| input | all-off parse (consumed) | our top-1 | pin top-1 | first divergence / `n` (pre-port) | class |
|---|---|---|---|---|---|
| `cang` | `cang` (4/4) | 藏 | 藏 | rank 9; `n=31` both | (iii) closed |
| `sang` | `sang` (4/4) | 桑 | 桑 | rank 7; `n=16` both | (iii) closed (same mechanism as `cang`) |
| `lve` | `lve` (3/3) | 略 | 略 | rank 5; `n=22` both | (iii) closed (same mechanism as `cang`) |
| `lue` | `lu'e` (3/3) | 路 | — no fixture row; all-off cross-engine IDENTICAL (frozen W10 control) | — | (iii) closed — correction-bit alias of `lve` |
| `agn` | `a` (1/3; no complete parse) | 阿 | — same | — | (iii) closed — correction-bit alias of `ang` |
| `amg` | `a` (1/3) | 阿 | — same | — | (iii) closed — same mechanism as `agn`, other bit, same canonical |
| `ang` *(canonical twin)* | `ang` (3/3) | 昂 | 昂 | rank 4; `n=21` both | (iii) closed (same mechanism as `cang`) |

## Shared mechanisms

Three mechanisms cover the seven rows; each is stated once.

**Native-key tail (cang, sang, lve, ang).** One mechanism: a single
native syllable, parsed to the same one-key matrix by both engines under
every option word; top-1 and the count-backed head agree. Pre-port, the
depth-10 window diverged only in the rare-glyph tail (positions at
depth 10 missing from the other side's top-10):

| input | shared prefix (pre-port) | ours only | pin only |
|---|---:|---|---|
| `cang` | 8 | 鸧 嵢 | 螥 鶬 |
| `sang` | 6 | 桒 槡 | 䘮 褬 |
| `lve` | 4 | 畧 㔀 䌎 圙 | 擽 㗕 攊 䤚 |
| `ang` | 3 | 枊 䇦 骯 | 昻 䩕 䍩 |

Every glyph across the eight diverging sets has an `interpolation2.text`
single-character count of 0–3 (鸧 3, 嵢 2, 桒/畧/枊 3, the rest 0), while
the agreeing heads are count-backed (藏 10122, 桑 4382, 略 3501, 昂 1494,
盎 32, 卬 26). The pre-port divergence was the comparator, not the data:
our engine ordered the near-zero tail by raw `interpolation2.text` count,
while the pin's `_compute_frequency_of_items` (`pinyin.cpp:1858-1866`)
amplifies `(1−λ)·unigram/total` by 2²⁴ in C `float` and truncates to
`guint32` — and for every baked unigram ≤ 4 (count 0–3; total
51,051,831, λ = 0.312699) that product is < 1, so the whole tail
collapses to `m_freq` 0 and the order falls to the stable sort's array
order, which `_append_items` (`pinyin.cpp:1768-1791`) lays down
library-ascending then token-ascending. That is the same species as the
corpus 4,058 prefix-10 residual, and the Class A port of that law
(`e941090`) closed both: re-measured 2026-08-24, all four rows are
bit-identical to the pin at depth 10.

**Correction alias, same canonical (agn, amg).** One mechanism with two
bits: under all-off neither `agn` nor `amg` completes a parse
(`pinyin_index` has no bare `gn`/`mg`), both fall back to `[a]`, and the
all-off cross-engine verdict is IDENTICAL (frozen W10 control; our
all-off list is 阿…, `n=8`). The W12 tail exists only when the bit is
set: measured same-engine, `agn`+`CORRECT_GN_NG` and `amg`+`CORRECT_MG_NG`
each produce a list EQUAL to native `ang` (`n=21` both) — the corrected
parse is the canonical key (`content_table` index 4), so the fork-default
cross-engine tail is exactly `ang`'s native-key tail above.

**Correction alias, distinct canonical (lue).** The `CORRECT_UE_VE`
alias of `lve`: under all-off it parses `lu'e` (cross-engine IDENTICAL,
frozen W10 control; our list starts 路, `n=206`), and with the bit set
the measured same-engine list EQUALS native `lve` (`n=22`) — the tail it
contributes under fork-default is `lve`'s native-key tail.

## Scope and pins

No parser behaviour, scheme table, `loses_to`, Class A material,
init/slurp, apostrophe, offset-decode, or `pinyin_clear_constraint` code
is touched. At freeze time (2026-08-17) the default-profile pins stood at
12 top-1 misses (10,178 top-1), 0 top-5 misses / 0 absent (10,190 /
10,178), 1,036 order-only, and 4,058 of 98,930 (94,872 overlap), sentence
pins 488/385/370. The 2026-08-22 Class A comparator port
(`docs/findings/corpus-tail.md`) later closed the corpus residual to
10,190 / 10,190 / 98,930 / absent 0 / tie-swaps 0 — and, re-measured
2026-08-24, closed these six tails with it; the `e941090`-era prose
claim that the tails were "unchanged" by the port was made without
re-running this pass and is corrected here. The `run-option-sweep.sh`
W12 exclusion list still carries the inputs untouched (no gate change);
re-measured 2026-08-24 it never fires — all 21 sweep cases, baseline and
fork-default included, report TEXT/ORDER identical with zero W12 lines.
Retiring the now-inert exclusion list is a separate gate-policy change,
deliberately not made in this docs-only closure.

## Closure — all six rows class (iii), not a divergence (2026-08-24)

Explain-back against the pin (`0c5e80e`) with the live oracle, on branch
`fix/all-off-tails`, no code changes. Three independent controls, all
green:

1. **Fixture path** (`bin/corpus-tail --all-off-tails`, full redb export
   + `interpolation2.text`): every fixture-backed row bit-identical at
   depth 10 — shared prefix 10 of 10, `pin_not_ours=0`, `ours_not_pin=0`
   for `cang`/`sang`/`lve`/`ang`; the three alias rows' all-off
   equivalence and option-word invariance re-confirm as frozen.
2. **Live pin at `0x0`** (`tools/bisection/option-sweep` driven directly
   with option word `0` against the pin-built
   `libpinyin.so` and `oxpinyin-capi`): parse, `aux`, `n`, and every
   candidate text in order identical on all seven inputs.
3. **Option sweep gate** (`tools/bisection/run-option-sweep.sh`, full
   export as `OPTION_SWEEP_CAPI_DATA`): exit 0, 21 of 21 cases
   `TEXT/ORDER identical`, zero `W12`/`STOP`/`TIE-ORDER` lines — the
   exclusion list never fires.

Per-input verdict with pin cite (`pinyin_parser_table.h` and
`pinyin_parser2.cpp` cited at `0c5e80e`):

| input | verdict | evidence |
|---|---|---|
| `cang` | pin behaves the same → **(iii)** | `pinyin_parser_table.h:46` `{"cang", IS_ZHUYIN\|IS_PINYIN, 27, 0}` is unflagged, so `check_pinyin_options` (`pinyin_parser2.cpp:38-58`) admits it under any option word incl. `0x0`; the pre-port tail gap was our comparator's raw-count ordering, closed by the amplified-law port (see §"Shared mechanisms"). Live `0x0`: identical, `n=31`. |
| `sang` | pin behaves the same → **(iii)** | same shape: `pinyin_parser_table.h:463`, table index 307, unflagged. Live `0x0`: identical, `n=16`. |
| `lve` | pin behaves the same → **(iii)** | `pinyin_parser_table.h:311` `{"lve", IS_ZHUYIN\|IS_PINYIN, 203, 0}`, unflagged. Live `0x0`: identical, `n=22`. |
| `ang` *(twin)* | pin behaves the same → **(iii)** | `pinyin_parser_table.h:15` `{"ang", IS_ZHUYIN\|IS_PINYIN, 4, 0}`, unflagged; canonical of the two `g`-correction aliases. Live `0x0`: identical, `n=21`. |
| `lue` | no `0x0` divergence exists → **(iii)** | `pinyin_parser_table.h:306` carries `PINYIN_CORRECT_UE_VE`, which `0x0` fails in `check_pinyin_options`, so both engines parse `lu'e` and agree bit-for-bit (live `0x0`: identical, `n=206`). The "tail" existed only under the bit, where `search_pinyin_index` (`pinyin_parser2.cpp:93-116`) maps the alias through `content_table[203]` onto `lve`'s key — a closed row. |
| `agn` | no `0x0` divergence exists → **(iii)** | `pinyin_parser_table.h:11` carries `PINYIN_CORRECT_GN_NG`; at `0x0` both engines fall back to `[a]` and agree bit-for-bit (live `0x0`: identical, `n=8`). Under the bit the alias resolves via `content_table[4]` to `ang`'s key — a closed row. |
| `amg` | no `0x0` divergence exists → **(iii)** | `pinyin_parser_table.h:13` carries `PINYIN_CORRECT_MG_NG`; identical `[a]` fallback at `0x0`, and the bit resolves to `ang` — a closed row. |

Classification: **all six are (iii)** — no class (i) row exists, so no
fix, no re-freeze, and no gate change follow from this finding. The
corrective record: the pre-port TEXT-set gap was a real comparator
divergence on our side of exactly the Class A species, and the
`e941090` port resolved it; what was missing was only the re-measurement
this closure now supplies.

## What this finding closes

The six W12 all-off TEXT-set tails are named, reproduced on the
fixture-based scoring path (no live oracle), classed — and now **closed
as class (iii), not divergences**: four rows were the pre-port
comparator's raw-count ordering of the count-0–3 tail (closed by the
Class A port `e941090`, bit-identical since), and three rows are
correction-bit aliases that never diverge at `0x0` and whose bit-set
tails are two of the closed native rows (`lue`→`lve`,
`agn`/`amg`→`ang`). W12's option-sweep residual is fully resolved; the
`run-option-sweep.sh` W12 exclusion list is provably inert and its
retirement is the only loose end, a separate gate-policy change.

**Out of scope, parked (observed while re-measuring, 2026-08-24):** at
the literal `0x0` word on *non-W12* sweep inputs the pin and we differ —
`jv`/`zon` empty-guess (`n=0` vs our raw-text fallback `n=1`) and
`xian`/`fanan`/`fangan`/`tian` divided-table inventory (oracle `n=337`
vs our `n=756` for `xian`; the pin drops `xi'an`-style phrases without
`USE_DIVIDED_TABLE`). No frontend can produce this word (the fork ORs
`USE_DIVIDED_TABLE | USE_RESPLIT_TABLE` unconditionally,
`src/PYLibPinyin.cc:196-198`) and no frozen gate runs it; recorded here
only so a future option-bits pass knows it exists.
