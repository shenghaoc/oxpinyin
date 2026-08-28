# Findings — option bits

Date: 2026-08-17 · Status: W10 Phase 0 characterisation.

This finding pins where each `pinyin_option_t` bit class acts in the
libpinyin 2.11.91 source, how the ibus-libpinyin fork maps GSettings keys to
those bits, and what the current oxpinyin inventory omits. Upstream line
numbers are from the pinned trees:

- libpinyin `2.11.91` / `0c5e80e1200f84fab185d1c5bde458b770a0636c`;
- ibus-libpinyin `1.16.5` / `2d2cdac0187101aa0cd7ac06694a8340721ddfbb`.

## Bit values

From `src/storage/pinyin_custom2.h:31-79` (already mirrored in
`crates/oxpinyin-capi/pinyin.h`):

| Class | Bits |
|---|---|
| `PINYIN_INCOMPLETE` | `1 << 3` |
| `DYNAMIC_ADJUST` | `1 << 9` |
| `PINYIN_AMB_*` | `1 << 10` .. `1 << 19`, `PINYIN_AMB_ALL = 0x3ff << 10` |
| `PINYIN_CORRECT_*` | `1 << 21` .. `1 << 28`, `PINYIN_CORRECT_ALL = 0xff << 21` |

## Correction bits — parser-table entry flags

`FullPinyinParser2` filters each `pinyin_index` entry against the context
options in `check_pinyin_options`
(`src/storage/pinyin_parser2.cpp:38-58`):

```cpp
flags &= PINYIN_CORRECT_ALL | PINYIN_AMB_ALL;
options &= PINYIN_CORRECT_ALL | PINYIN_AMB_ALL;
if (flags) {
    if ((flags & options) != flags)
        return false;
}
```

The generated `pinyin_index` marks the correction spellings with exactly one
correction bit. For example `src/storage/pinyin_parser_table.h:11` is
`{"agn", IS_PINYIN|PINYIN_CORRECT_GN_NG, 4, 1}` and `:306` is
`{"lue", IS_PINYIN|PINYIN_CORRECT_UE_VE, 203, 1}`. Each such entry reuses the
canonical `content_table` id, so `lue` parses to the same `ChewingKey` as
`lve` and `pinyin_get_pinyin_string` returns the canonical spelling.

Mechanically extracting the flagged entries from `pinyin_index` gives **232
correction aliases** across the eight bits:

| Bit | Aliases | Canonical targets |
|---|---:|---|
| `PINYIN_CORRECT_GN_NG` | 80 | `ang`, `bang`, `beng`, …, `zong` |
| `PINYIN_CORRECT_MG_NG` | 80 | `ang`, `bang`, `beng`, …, `zong` |
| `PINYIN_CORRECT_IOU_IU` | 7 | `diu`, `jiu`, `liu`, `miu`, `niu`, `qiu`, `xiu` |
| `PINYIN_CORRECT_UEI_UI` | 12 | `chui`, `cui`, `dui`, `gui`, `hui`, `kui`, `rui`, `shui`, `sui`, `tui`, `zhui`, `zui` |
| `PINYIN_CORRECT_UEN_UN` | 18 | `chun`, `cun`, `dun`, `gun`, `hun`, `jun`, `kun`, `lun`, `nun`, `qun`, `run`, `shun`, `sun`, `tun`, `xun`, `yun`, `zhun`, `zun` |
| `PINYIN_CORRECT_UE_VE` | 2 | `lve`, `nve` |
| `PINYIN_CORRECT_V_U` | 16 | `ju`, `juan`, `jue`, `jun`, `qu`, `quan`, `que`, `qun`, `xu`, `xuan`, `xue`, `xun`, `yu`, `yuan`, `yue`, `yun` |
| `PINYIN_CORRECT_ON_ONG` | 17 | `chong`, `cong`, `dong`, `gong`, `hong`, `jiong`, `kong`, `long`, `nong`, `qiong`, `rong`, `song`, `tong`, `xiong`, `yong`, `zhong`, `zong` |

Two correction targets are **not** in oxpinyin's current 405-syllable
inventory: `eng` (target of `egn`/`emg` under `GN_NG`/`MG_NG`) and `nun`
(target of `nuen` under `UEN_UN`). Upstream can produce them only through
those correction spellings, not as raw input: `pinyin_index` has no `eng` or
`nun` entry. The implementation therefore carries `eng` and `nun` as
option-only canonical keys appended after the frozen id space rather than
changing `FULL_PINYIN_SYLLABLES`.

## Ambiguity bits — two loci

Ambiguity acts in two places.

### Parser table

Two ambiguity spellings are direct `pinyin_index` entries:

- `sua` → `shua` under `PINYIN_AMB_S_SH` (`pinyin_parser_table.h:505`);
- `zua` → `zhua` under `PINYIN_AMB_Z_ZH` (`pinyin_parser_table.h` adjacent
  entry).

These are parser-table bits exactly like the correction aliases.

### Table-search matrix

The remaining substitutions are added after the selected parse, in
`fuzzy_syllable_step` (`src/storage/phonetic_key_matrix.cpp:238-315`), which
mutates each matrix `ChewingKey`:

- initial substitutions `c↔ch`, `z↔zh`, `s↔sh`, `l↔r`, `l↔n`, `f↔h`,
  `g↔k` (`:262-275`); the initial substitution is appended only when the
  resulting key has a table index (`:255`);
- final substitutions `an↔ang`, `en↔eng`, `in↔ing` (`:301-306`).

`pinyin_parse_more_full_pinyins` applies the steps in this order: parse →
`resplit_step` → `inner_split_step` → `fuzzy_syllable_step`
(`src/pinyin.cpp:1512-1520`). The matrix is candidate collection, which is
W11 ground; W10 implements the minimal fuzzy-step addition and flags it.

## `DYNAMIC_ADJUST` — candidate-frequency adjustment, not a train gate

`DYNAMIC_ADJUST` is checked in the candidate-frequency path, not in the
training write path:

- `pinyin_guess_candidates` only looks up the previous token under the bit
  (`src/pinyin.cpp:2201-2203`), and loads/merges the bigram only under it
  (`:2208-2212`);
- `_compute_frequency_of_items` adds the bigram probability term only under
  the bit (`src/pinyin.cpp:1845-1851`);
- `pinyin_train` itself has **no** `DYNAMIC_ADJUST` check
  (`src/pinyin.cpp:2668-2688`); it writes whenever a user dir and an n-best
  result exist.

This was confirmed against the pin-built oracle: with
`PINYIN_INCOMPLETE|PINYIN_CORRECT_ALL|USE_DIVIDED_TABLE|USE_RESPLIT_TABLE`
and `DYNAMIC_ADJUST` deliberately clear, `pinyin_train` returned `true` and
created the user store (`user.conf`). The requested “bit-off ⇒ no training
writes” would therefore diverge from the pin; the matching implementation is
to leave `pinyin_train` ungated and record this finding.

### What the two sites gate

The two cited blocks together gate **the bigram contribution to candidate
frequency**, not the unigram sort input:

| Site | What it does under the bit | What it skips when clear |
|---|---|---|
| `pinyin_guess_candidates` `:2201-2212` | Looks up `prev_token` and loads/merges system+user **bigram** grams | That merged-gram input (both system and trained user bigram) |
| `_compute_frequency_of_items` `:1845-1851` | Sets `bigram_poss` from the merged gram | The `λ · bigram_poss · DISCOUNT` term of `m_freq` |

The unigram term of `m_freq` (`:1856-1865`) always reads
`FacadePhraseIndex` (system + user unigrams) with **no** `DYNAMIC_ADJUST`
check. Sentence decode has no check in `pinyin.cpp` either.

oxpinyin's equivalent: `RankKey.frequency` is the raw unigram count
(system + W6-T4 overlay). The bigram increment is omitted when the bit is
clear. When the bit is set, W6-T4's unigram merge stays; a non-zero
bigram increment on `RankKey` would be a ranking-model change outside
W10 (deferred: issue #99). `SharedLm::unigram_freq` stays ungated (the
phrase-index term). `SharedLm::score` stays ungated (decode, not the
cited sites).

The fork masks `DYNAMIC_ADJUST` out entirely (`PYPConfig.cc:145`), so
bit-clear is the fork's permanent state and this gate is
default-settings-critical. Verification is the populated-store
differential (`run-train-diff-dynamic-off.sh`): identical training, bit
CLEAR, compare candidate TEXT/ORDER at offset 0 and after choosing 你.

Measured 2026-08-17 against the pin-built oracle (`0x188`, one training
round, full tables): exports identical (3 rows); `nihao@0` and
`after-ni` top-10 identical (`你好` / `好` first). Trained user unigrams
still rank (the ungated phrase-index term); trained user bigrams do not
need to, because the bit is clear.

The populated phase grew a persistence leg (2026-08-27, `train-diff.c`
phase 4 / `TRAINDIFF_REOPEN=1`, always on in the runner's populated
leg): save → full context teardown → reopen of the SAME user dir under
the same word → the reopened window must equal the in-memory one on
each side → one more training round with the bit still clear → export
from the reopened context. Measured against the pin on datagen tables:
both sides export `你好|ni'hao|414` (persisted 138 doubled by the
subsequent-session round — a lost state would re-seed at 138) plus the
remembered `你好|5` / `世界|12` rows; `train-reopened:1` both. An
`initial@0` probe prints the un-populated baseline inside the same run,
and the runner fails unless the reopened probe/train/export lines exist
on both sides (the populated phase cannot silently degrade to the empty
case).

## Sweep TEXT/ORDER (ABI, top-10)

`run-option-sweep.sh` now diffs parse/aux **and** top-10 candidate
TEXT/ORDER through `pinyin_get_candidate_string`. Asserting through the
ABI is verification, not W11 ground.

Fork-default (`0x1fe00198`), 28 inputs:

- **20 identical** including the divided/resplit triggers `xian`
  (西安, n=756), `fanan` (翻案, n=175), `fangan` (方案/反感, n=179),
  `tian` (提案, n=263), and `nihao`.
- **2 tie-order-only** (`diou`, `ben`): same 10-set, every swapped
  position has equal `phrase_length` (RankKey 1). Span / frequency /
  collection-order are not on the ABI; this is the documented three-key
  tie class (`candidate-construction.md` §8.2).
- **6 TEXT-set tails**, all-off-controlled as W12 (see triage below).
  Excluded from W10's STOP gate.

`USE_DIVIDED_TABLE` / `USE_RESPLIT_TABLE` were never in W10's scope.
On the fork-default word (both bits set) the xian/fanan/fangan/tian
triggers are byte-identical across engines, including `n=`. No
divided/resplit machinery is added here.

Scan-matrix keep-rule (Fixes #103): `PhoneticTable::append` is an
unconditional bag push (`phonetic_key_matrix.h:92-99`); a column is
`(ChewingKey, ChewingKeyRest)` and Rest is the span (`chewing_key.h:97-104`).
Same key, different `m_raw_end`, coexist. oxpinyin key-only dedup is
**pre-fuzzy only** (selected + resplit + divided — the all-off / `0x18a`
pin). After `fuzzy_syllable_step`, keep is `(key, to)`. `amb-17`/`fangan`
is asserted identical (`方案|反感|翻案|访港|…`).

Initials-then-finals compose: `fuzzy_syllable_step` applies initials,
re-fetches the column, then finals (`phonetic_key_matrix.cpp:238-306`).
`can` under `AMB_C_CH|AMB_AN_ANG` yields `chan`, `cang`, and chained
`chang`. The sweep case `amb-chain` asserts that.

Bare `gn`/`mg`/`on` are not in `pinyin_index` (80/80/17 stemmed rows).
`checked_canonical` rejects an empty stem so those bits do not invent
`ng`/`ong`. `agn` remains gated by `PINYIN_CORRECT_GN_NG`.

## TEXT-set STOP triage (all-off control)

Control: same top-10 ABI assertion under ALL-BITS-OFF (`0x0`).
Each row is enumerated, reproduced on the fixture-based scoring path
(no live oracle), and classed in `docs/findings/all-off-tails.md`.

| Input | All-off verdict | Owner | Action |
|---|---|---|---|
| `cang` | TEXT-set DIFF prefix=8, `n=31` both | W12 | exclude from W10 gate |
| `sang` | TEXT-set DIFF prefix=6, `n=16` both | W12 | exclude from W10 gate |
| `lve` | TEXT-set DIFF prefix=4, `n=22` both | W12 | exclude from W10 gate |
| `lue` | IDENTICAL as `lu+e` (not a native `lve`). With `CORRECT_UE_VE`: same engine's list equals native `lve` (`n=22`). Cross-engine tail is `lve`'s all-off residual | W12 | exclude from W10 gate |
| `agn` | no complete parse (falls back to `a`, identical). With `CORRECT_GN_NG`: same engine's list equals native `ang` (`n=21`). Cross-engine tail is `ang`'s all-off residual | W12 | exclude from W10 gate |
| `amg` | same as `agn` via `CORRECT_MG_NG` → native `ang` | W12 | exclude from W10 gate |

Upstream draws a corrected key's inventory from the **same** `content_table` slot as the canonical spelling: `search_pinyin_index` (`pinyin_parser2.cpp:93-116`) sets `key = content_table[index->m_table_index].m_chewing_key`. `pinyin_parser_table.h` gives `agn`/`amg`/`ang` table index 4 and `lue`/`lve` table index 203. The corrected parse does **not** restrict the inventory; it is the native key. Capi is not admitting extra correction-only entries — `n=` matches, and the tail is the ~4k prefix-overlap residual the pins count rather than assert as text (W12). No W10 work.

**Closed 2026-08-24** (`docs/findings/all-off-tails.md` §"Closure"): all
six rows are class (iii), not divergences. The DIFF verdicts in the table
are pre-`e941090` measurements of the Class A comparator species;
post-port the lists are bit-identical to the pin at `0x0` (live-oracle
control, 2026-08-24), and the `run-option-sweep.sh` exclusion list never
fires.

`DYNAMIC_ADJUST` bit-SET (fold `λ · bigram_poss · DISCOUNT` into `m_freq`) is unreached and deferred: issue #99.

## GSettings → bit mapping in the fork

The mapping is in `src/PYPConfig.cc`.

Master switches:

- `correct-pinyin` gates every `PINYIN_CORRECT_*` bit through
  `m_option_mask` (`:612-616`, change handler `:727-731`);
- `fuzzy-pinyin` gates every `PINYIN_AMB_*` bit through `m_option_mask`
  (`:338-342`, change handler `:479-483`).

Individual keys:

- `incomplete-pinyin` → `PINYIN_INCOMPLETE|ZHUYIN_INCOMPLETE` (`:206`);
- the ten `fuzzy-pinyin-*` keys → the matching `PINYIN_AMB_*` (`:208-217`);
- `dynamic-adjust` → `DYNAMIC_ADJUST` (`:219`);
- the eight `correct-pinyin-*` keys → the matching `PINYIN_CORRECT_*`
  (`:530-537`).

`LibPinyinBackEnd::setPinyinOptions` passes
`config->option() | USE_RESPLIT_TABLE | USE_DIVIDED_TABLE`
(`src/PYLibPinyin.cc:196-198`).

The fork default option word is therefore:

```text
0x1fe00198
  = PINYIN_INCOMPLETE | ZHUYIN_INCOMPLETE
    | PINYIN_CORRECT_ALL
    | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE
```

Two details matter for the report:

1. `fuzzy-pinyin` defaults to `false`, so `m_option_mask` clears
   `PINYIN_AMB_ALL` even though several individual `fuzzy-pinyin-*` schema
   defaults are `true`.
2. `m_option_mask` is initialised to
   `PINYIN_INCOMPLETE|ZHUYIN_INCOMPLETE|PINYIN_CORRECT_ALL`
   (`PYPConfig.cc:145`) and never gains `DYNAMIC_ADJUST`; `Config::option()`
   returns `m_option & m_option_mask` (`src/PYConfig.h:55`). Consequently the
   fork default word has `DYNAMIC_ADJUST` clear even though the schema key
   defaults to `true`. The sweep must cover `0x1fe00198` as the fork default.

## Current oxpinyin inventory

`FULL_PINYIN_SYLLABLES` is 405 complete syllables and
`INCOMPLETE_PINYIN_KEYS` is 23 initial-only keys, matching the pinned
untuned inventory. It excludes:

- all 232 correction aliases;
- the two parser-table ambiguity aliases (`sua`, `zua`);
- the two canonical option-only targets `eng` and `nun`;
- tone forms, fuzzy matrix alternates, and Zhuyin.
