# Findings: option bits

Status: **reference**

This finding records where each `pinyin_option_t` bit class acts in the
libpinyin source, how the ibus-libpinyin fork maps GSettings keys to
those bits, and how oxpinyin's syllable inventory handles them. Upstream
line numbers are at these trees (`tools/oracle/oracle-pin.txt`):

- libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`;
- ibus-libpinyin `1.16.5` / `2d2cdac0187101aa0cd7ac06694a8340721ddfbb`.

`DYNAMIC_ADJUST` has its own record: [`dynamic-adjust.md`](dynamic-adjust.md).

## Bit values

From `src/storage/pinyin_custom2.h:31-92` (mirrored in
`crates/oxpinyin-capi/pinyin.h`):

| Class | Bits |
|---|---|
| `PINYIN_INCOMPLETE` | `1 << 3` (`:34`) |
| `DYNAMIC_ADJUST` | `1 << 9` (`:40`) |
| `PINYIN_AMB_*` | `1 << 10` .. `1 << 19`, `PINYIN_AMB_ALL = 0x3ff << 10` (`:50-60`) |
| `PINYIN_CORRECT_*` | `1 << 21` .. `1 << 28`, `PINYIN_CORRECT_ALL = 0xff << 21` (`:79`) |

## Correction bits: parser-table entry flags

`FullPinyinParser2` filters each `pinyin_index` entry against the context
options in `check_pinyin_options` (`src/storage/pinyin_parser2.cpp:38-58`):

```cpp
flags &= PINYIN_CORRECT_ALL | PINYIN_AMB_ALL;
options &= PINYIN_CORRECT_ALL | PINYIN_AMB_ALL;
if (flags) {
    if ((flags & options) != flags)
        return false;
}
```

The generated `pinyin_index` marks each correction spelling with exactly
one correction bit. For example `src/storage/pinyin_parser_table.h:11` is
`{"agn", IS_PINYIN|PINYIN_CORRECT_GN_NG, 4, 1}` and `:306` is
`{"lue", IS_PINYIN|PINYIN_CORRECT_UE_VE, 203, 1}`. Each such entry reuses
the canonical `content_table` id: `search_pinyin_index`
(`pinyin_parser2.cpp:93-120`) sets
`key = content_table[index->m_table_index].m_chewing_key`. Thus `lue`
parses to the same `ChewingKey` as `lve`, `agn` and `amg` to the same key
as `ang`, and `pinyin_get_pinyin_string` returns the canonical spelling.
A corrected parse does not restrict the candidate inventory; it is the
native key.

The flagged entries of `pinyin_index` give **232 correction aliases**
across the eight bits:

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

Two correction targets are **not** in oxpinyin's 405-syllable inventory:
`eng` (target of `egn`/`emg` under `GN_NG`/`MG_NG`) and `nun` (target of
`nuen` under `UEN_UN`). The pin produces them only through those
correction spellings: `pinyin_index` (`pinyin_parser_table.h:9-673`) has
no `eng` or `nun` entry. oxpinyin therefore carries `eng` and `nun` as
option-only canonical keys after the frozen id space
(`OPTION_ONLY_COMPLETE_SYLLABLES`), and `FULL_PINYIN_SYLLABLES` is
unchanged.

Bare `gn`, `mg` and `on` are not in `pinyin_index`; their rows are 80, 80
and 17 stemmed aliases. `checked_canonical`
(`crates/oxpinyin-core/src/syllables.rs`) rejects an empty stem, so those
bits do not invent `ng` or `ong`. `agn` stays gated by
`PINYIN_CORRECT_GN_NG`.

## Ambiguity bits: two loci

### Parser table

Two ambiguity spellings are direct `pinyin_index` entries, parser-table
bits like the correction aliases:

- `sua` → `shua` under `PINYIN_AMB_S_SH` (`pinyin_parser_table.h:505`);
- `zua` → `zhua` under `PINYIN_AMB_Z_ZH` (`pinyin_parser_table.h:665`).

### Table-search matrix

The other substitutions are added after the selected parse, in
`fuzzy_syllable_step` (`src/storage/phonetic_key_matrix.cpp:226-316`),
which returns at once unless an `PINYIN_AMB_*` bit is set (`:228-229`) and
appends to each matrix column:

- initial substitutions `c↔ch`, `z↔zh`, `s↔sh`, `l↔r`, `l↔n`, `f↔h`,
  `g↔k` (`:262-275`); an initial substitution is appended only when the
  resulting key has a table index (`:255`);
- final substitutions `an↔ang`, `en↔eng`, `in↔ing` (`:301-306`).

The step applies the initials, fetches the column again (`:282`), then the
finals, so the two compose: `can` under `AMB_C_CH|AMB_AN_ANG` yields `chan`,
`cang` and the chained `chang`.

`pinyin_parse_more_full_pinyins` applies the steps in this order: parse,
`resplit_step`, `inner_split_step`, `fuzzy_syllable_step`
(`src/pinyin.cpp:1507-1520`).

`PhoneticTable::append` is an unconditional push
(`phonetic_key_matrix.h:92-100`). A column holds `(ChewingKey,
ChewingKeyRest)` items, and the rest is the span (`chewing_key.h:97-104`),
so identical entries and entries with different `m_raw_end` values coexist.
Both oxpinyin matrix builders keep all appends in order. Per column, all
initial swaps precede all final swaps of the grown column. A final swap is
kept even when no syllable-table row names its packed key; the internal
key representation carries that packed value without inventing a spelling.
Singleton zero-key columns and readers of the first slot are unchanged.

## GSettings → bit mapping in the fork

The mapping is in `src/PYPConfig.cc`.

Master switches:

- `correct-pinyin` gates every `PINYIN_CORRECT_*` bit through
  `m_option_mask` (`:614-618`; change handler `:728-733`);
- `fuzzy-pinyin` gates every `PINYIN_AMB_*` bit through `m_option_mask`
  (`:340-344`; change handler `:480-485`).

Individual keys:

- `incomplete-pinyin` → `PINYIN_INCOMPLETE|ZHUYIN_INCOMPLETE` (`:206`);
- the ten `fuzzy-pinyin-*` keys → the matching `PINYIN_AMB_*` (`:208-217`);
- `dynamic-adjust` → `DYNAMIC_ADJUST` (`:219`);
- the eight `correct-pinyin-*` keys → the matching `PINYIN_CORRECT_*`
  (`:531-538`).

`LibPinyinBackEnd::setPinyinOptions` passes
`config->option() | USE_RESPLIT_TABLE | USE_DIVIDED_TABLE`
(`src/PYLibPinyin.cc:195-197`).

The fork's default option word is therefore:

```text
0x1fe00198
  = PINYIN_INCOMPLETE | ZHUYIN_INCOMPLETE
    | PINYIN_CORRECT_ALL
    | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE
```

Two details set that word:

1. `fuzzy-pinyin` defaults to `false`, so `m_option_mask` clears
   `PINYIN_AMB_ALL`, although several individual `fuzzy-pinyin-*` schema
   defaults are `true`.
2. `m_option_mask` starts as
   `PINYIN_INCOMPLETE|ZHUYIN_INCOMPLETE|PINYIN_CORRECT_ALL`
   (`PYPConfig.cc:145`), and only the two master switches change it, so it
   never gains `DYNAMIC_ADJUST`. `Config::option()` returns
   `m_option & m_option_mask` (`src/PYConfig.h:56`). The fork's word
   therefore has `DYNAMIC_ADJUST` clear, although the schema key defaults
   to `true`
   (`data/com.github.libpinyin.ibus-libpinyin.gschema.xml:68-71`).

On the fork's default word, `USE_DIVIDED_TABLE` and `USE_RESPLIT_TABLE`
are set; the divided and resplit steps are the
[matrix split tables](matrix-split-tables.md).

## oxpinyin inventory

`FULL_PINYIN_SYLLABLES` is 405 complete syllables and
`INCOMPLETE_PINYIN_KEYS` is 23 initial-only keys, matching the pinned
untuned inventory (`crates/oxpinyin-core/src/table_data/`). It excludes:

- all 232 correction aliases;
- the two parser-table ambiguity aliases (`sua`, `zua`);
- the two option-only canonical targets `eng` and `nun`;
- tone forms, fuzzy matrix alternates, and Zhuyin.
