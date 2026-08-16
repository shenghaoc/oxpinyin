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
