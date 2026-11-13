# Bopomofo/Zhuyin scheme SPEC

Status: **reference**

This SPEC states the pin's C API behaviour of the four functions in [Scope](#scope): the scheme ABI, the parser laws, the tables and indexes, the post-match tone-validity stop, the aux rendering, the index-fidelity rows and the exact-seam routing.
The oxpinyin types, files, key ids and tests named here describe its implementation.

## Scope

This SPEC states the pin's behaviour of these functions, which oxpinyin reproduces:

- `pinyin_parse_more_chewings`
- `pinyin_set_zhuyin_scheme`
- `pinyin_in_chewing_keyboard`
- `pinyin_get_chewing_auxiliary_text`

The source citations hold at the current pin, libpinyin `2.11.92`, commit `074a2219`.

## Scheme ABI

`ZhuyinScheme` values come from `src/storage/pinyin_custom2.h:122-133`:

- `ZHUYIN_STANDARD` = 1
- `ZHUYIN_HSU` = 2
- `ZHUYIN_IBM` = 3
- `ZHUYIN_GINYIEH` = 4
- `ZHUYIN_ETEN` = 5
- `ZHUYIN_ETEN26` = 6
- `ZHUYIN_STANDARD_DVORAK` = 7
- `ZHUYIN_HSU_DVORAK` = 8
- `ZHUYIN_DACHEN_CP26` = 9
- `ZHUYIN_DEFAULT` = `ZHUYIN_STANDARD`

`pinyin_set_zhuyin_scheme` selects the parser class by scheme (`src/pinyin.cpp:1161-1192`):

- `ZhuyinSimpleParser2` for STANDARD, IBM, GINYIEH, ETEN and STANDARD_DVORAK;
- `ZhuyinDiscreteParser2` for HSU, ETEN26 and HSU_DVORAK;
- `ZhuyinDaChenCP26Parser2` for DACHEN_CP26.

The setter deletes the old parser first (`src/pinyin.cpp:1163-1164`).
A value outside the enum reaches `default:` and aborts (`:1188-1189`).
STANDARD_DVORAK (7) also aborts: its `ZhuyinSimpleParser2::set_scheme` arm sets the tables, has no `return`, and falls through to `default: abort()` (`src/storage/zhuyin_parser2.cpp:291-295`).
oxpinyin aborts on the same values (`pinyin_set_zhuyin_scheme` in `crates/oxpinyin-capi/src/config.rs`; [Keyboard scope](#keyboard-scope)).
The parser is context-owned, so live instances observe the change on the next parse.

## Parser classes

Declarations: `src/storage/zhuyin_parser2.h:49-194`.
Implementations: `src/storage/zhuyin_parser2.cpp`.

- `ZhuyinSimpleParser2::parse_one_key` maps each keystroke through the scheme symbol table to a Zhuyin symbol string.
  It optionally removes a trailing tone key.
  It then looks up the concatenated Zhuyin string in the global `zhuyin_index` (`:162-213`).
- `ZhuyinDiscreteParser2::parse_one_key` consumes an explicit initial/middle/final/tone sequence from the scheme's separate tables (`:335-405`).
- `ZhuyinDaChenCP26Parser2::parse_one_key` is the repeat-count CP26 variant (`:573-716`).

All three `parse` methods probe the longest run of keys that `in_chewing_scheme` accepts.
They then greedy-longest-match each key up to `max_chewing_length = 4` (`zhuyin_parser2.cpp:37`), or `max_chewing_dachen26_length = 12` for CP26 (`:39`).
They stop when no longer key parses (`:216-268`, `:408-460`, `:718-795`).

## Scheme tables

All keyboards are source-embedded in the generated header `src/storage/zhuyin_table.h`.
They are copyable under the source policy and are not vendored model data.

Simple keyboards:

| Scheme | Symbols | Tones |
|---|---|---|
| STANDARD | `chewing_standard_symbols` (`:9`) | `chewing_standard_tones` (`:50`) |
| GINYIEH | `chewing_ginyieh_symbols` (`:59`) | `chewing_ginyieh_tones` (`:100`) |
| ETEN | `chewing_eten_symbols` (`:109`) | `chewing_eten_tones` (`:150`) |
| IBM | `chewing_ibm_symbols` (`:159`) | `chewing_ibm_tones` (`:200`) |
| STANDARD_DVORAK | `chewing_standard_dvorak_symbols` (`:325`) | `chewing_standard_dvorak_tones` (`:366`) |

Discrete keyboards (separate initial/middle/final/tone tables):

| Scheme | Initials | Middles | Finals | Tones |
|---|---|---|---|---|
| HSU | `chewing_hsu_initials` (`:209`) | `chewing_hsu_middles` (`:234`) | `chewing_hsu_finals` (`:241`) | `chewing_hsu_tones` (`:258`) |
| ETEN26 | `chewing_eten26_initials` (`:267`) | `chewing_eten26_middles` (`:292`) | `chewing_eten26_finals` (`:299`) | `chewing_eten26_tones` (`:316`) |
| HSU_DVORAK | `chewing_hsu_dvorak_initials` (`:375`) | `chewing_hsu_dvorak_middles` (`:400`) | `chewing_hsu_dvorak_finals` (`:407`) | `chewing_hsu_dvorak_tones` (`:424`) |

The CP26 tables start at `:433` (`initials`) and `:458` (`middles`), and continue through finals and tones in the same header.

`pinyin_in_chewing_keyboard` returns a `NULL`-terminated string vector of the Zhuyin symbol or symbols that one keystroke maps to.
Simple schemes return at most one symbol.
Discrete schemes return up to two for multi-purpose keys.
CP26 returns the same shape through its own lookup (`src/pinyin.cpp:1615-1625`; parser methods at `zhuyin_parser2.cpp:301-333`, `:496-545`, `:798+`).

## Tone and the SyllableKey vocabulary

Upstream `ChewingKey` carries tone in `m_tone`.
The parser assigns tone after it resolves the tone-less Zhuyin spelling (`zhuyin_parser2.cpp:162-213`).
The oxpinyin `SyllableKey` is a dense id over the tone-less full-pinyin inventory ([parser-spec.md](parser-spec.md)), and the pinned full-pinyin corpus is tone-less.

**Decision:** Zhuyin tone must not reshape `SyllableKey`.

- The bopomofo parser returns a tone-less `SyllableKey` plus a separate tone value.
- Tone is a scheme-level attribute that travels with the key through the scheme parser and the aux formatter; it is not a new key id.
- Candidate matching for tone-sensitive bopomofo lookup is outside the parser and outside this SPEC.
- If the pinned bopomofo candidate surface cannot be reproduced without tone in the decoder key, this decision is reopened ([Open](#open)).

## Downstream lattice

**Pin behaviour.**
The chewing parser outputs `ChewingKey`/`ChewingKeyRest` and feeds the same `PhoneticKeyMatrix` through `fill_matrix`, then `fuzzy_syllable_step` (`src/pinyin.cpp:1582-1609`).

**oxpinyin routing.**
The routing is the exact seam ([The exact seam](#the-exact-seam)).
The parser's keys reach the decoder as one `Exact` edge chain over the `'`-joined full-pinyin text (`SegmentGraph::build_exact`, `Session::replace_raw_exact`).
The pinyin inventory never re-segments them.
This is the same shape as upstream, which hands the scheme parser's `ChewingKey`s to `fill_matrix`.

The measured sentence residual is the trellis-side §12 class in [upstream-divergences.md](upstream-divergences.md).
Full pinyin shares it; it is not specific to a scheme.

## Auxiliary text

`pinyin_get_chewing_auxiliary_text` walks the matrix like full pinyin, but renders Zhuyin through `get_zhuyin_string()` (`src/pinyin.cpp:3518-3576`, `chewing_key.cpp:74-89`).

- Tones 2–5 append their tone *mark*.
- First and zero tones omit it.
- There is no apostrophe handling and no tone digit.

After a successful longest `parse_one_key`, upstream `ZhuyinSimpleParser2::parse` calls `_ChewingKey::is_valid_zhuyin` (`zhuyin_parser2.cpp:256-257`; Discrete `:448`, CP26 `:757`; `chewing_key.cpp:38-45`, over `valid_zhuyin_table` at `zhuyin_table.h:491`).
On failure it **breaks** the whole parse; it does not retry a shorter key.
So illegal tones (including first-tone ㄋㄧ / `"su "`) consume 0.
oxpinyin applies the same post-match stop.

`ZhuyinSimpleParser2::set_scheme` always ORs in `ZHUYIN_CORRECT_SHUFFLE` (`zhuyin_parser2.cpp:272`).
The oxpinyin port applies the same forced bit.
So shuffled spellings such as `1,u` (ㄅㄝㄧ → `bie`) parse through the 1062 `ZHUYIN_CORRECT_SHUFFLE` rows exactly as on the pin.

## Index fidelity

**The parser table.**
The parser table is the pinned `zhuyin_index`: 1493 rows (`pinyin_parser_table.h:1492`).
They are 417 plain + 1062 `ZHUYIN_CORRECT_SHUFFLE` + 14 `ZHUYIN_INCOMPLETE` rows.
They join to canonical spellings through `content_table`, and `check_chewing_options` gates them (`zhuyin_parser2.cpp:43-100`).
The oxpinyin port is in `oxpinyin-core/src/zhuyin_map.rs` and `scheme.rs`.
Unit tests pin these properties:

- the rows are sorted and unique (the binary-search invariant);
- the shuffle law: every multi-symbol plain row contributes all non-canonical permutations of its symbols, and each maps to its canonical row (227 two-symbol × 1 + 167 three-symbol × 5 = 1062);
- the option gating of incomplete and shuffle rows at the index level.

**The 12 recovered plain rows.**
405 of the 417 plain rows have pinyin that is a full-pinyin syllable.
The remaining 12 plain rows are of two kinds:

- 2 option-only syllables, `ㄥ`→`eng` and `ㄋㄨㄣ`→`nun`, mapped to the existing key ids 428/429;
- 10 zhuyin-only syllables, `chua, den, din, fe, kei, len, nia, rua, yai, zhei`.
  These are `content_table` rows with no `pinyin_index` spelling.
  They are a fourth inventory tier, `SyllableKey` ids 430..440 (`SYLLABLE_KEY_COUNT` 440).

`SyllableKey::from_canonical_text` resolves the fourth tier (it mirrors `content_table` membership).
`from_text` does not resolve it, on purpose (it mirrors `pinyin_index` membership), so the full-pinyin parse surface does not change.

Under the pinned `valid_zhuyin_table`:

- `eng, nun, den, zhei, nia, yai, chua` have non-empty tone masks (they are parse-live);
- `fe, din, kei, len, rua` and all 14 incomplete rows have all-zero masks (they are parse-dead: they consume 0 on both sides, by the upstream mechanism).
- The masks are `eng` 110000, `nun` 101010, `den`/`zhei` 100010, `nia`/`yai` 101000, `chua` 110100 and `ni` 101110.

`VALID_ZHUYIN_TONES` is id-indexed at `SYLLABLE_KEY_COUNT`.
It is the only fixed-size id-indexed table in the workspace; every other consumer derives from the count or is `.get()`-guarded.

**Canonical display.**
Upstream's aux text renders the canonical spelling of the matched key (`ChewingKey::get_zhuyin_string`).
So a shuffled input ㄅㄝㄧ displays as ㄅㄧㄝ.
The port carries the row's `content_table[].m_chewing` as a canonical column; spans stay input-based.

**Option seam.**
`pinyin_parse_more_chewings` strips `ZHUYIN_CORRECT_ALL` from the caller's option word and passes the rest to the parser (`pinyin.cpp:1582-1609`, the strip at `:1589`).
`pinyin_in_chewing_keyboard` has its own strip at `:1621`.
Three caller bits reach the parsers:

- `USE_TONE` (Simple `zhuyin_parser2.cpp:171`, Discrete `:379`, CP26 `:595`);
- `FORCE_TONE` (Simple `:178`, nested under `USE_TONE`; Discrete `:373` and `:387`; CP26 `:602`, nested);
- `ZHUYIN_INCOMPLETE` (bit 4, `pinyin_custom2.h:35`), which only `check_chewing_options` consults (`:49-51`).

`PINYIN_AMB_ALL` is masked off on entry (`:166`, `:342`, `:580`).
The pinyin facade's chewing batch seam forwards the whole word after the `ZHUYIN_CORRECT_ALL` strip ([compatibility policy](compatibility-policy.md#classification-table) rows 16 and 30, both CLOSED).
The libzhuyin facade forwards the whole word through `ZhuyinParser::parse_with_options`.

Keys that the shuffle and recovery rows produce join the documented non-gated full-candidate divergence class.
The string-keyed dictionary has no phrases for the new spellings, so the lookup is empty, with no panic.

## Keyboard scope

The parser is table-driven over every ported keyboard.
The scheme setter accepts 1/2/3/4/5/6/8/9.
Value 7 (`ZHUYIN_STANDARD_DVORAK`) and every out-of-enum value abort the process, as the pin does (`pinyin.cpp:1188-1189`, `zhuyin_parser2.cpp:291-295`; register rows 5c and 5d).

**Simple family: STANDARD (1), IBM (3), GINYIEH (4), ETEN (5).**

- Each has its pinned symbol and tone tables (`chewing_{standard,ibm,ginyieh,eten}_{symbols,tones}`).
- All share the 1493-row index, the forced `ZHUYIN_CORRECT_SHUFFLE` and the canonical aux rendering.
- The four keyboards use the same 37 symbols on different keys.
  GINYIEH and ETEN both bind the apostrophe; IBM's tone keys are `m , . /`.
- On none of them is a key both a symbol and a tone key.

**Discrete family: HSU (2), ETEN26 (6), HSU_DVORAK (8).**
The port is `ZhuyinDiscreteParser2` (`zhuyin_parser2.cpp:335-494`).

- These keyboards parse by **positional probe**:
  - position 0 in the keyboard's initial table, 1 in the middle table, 2 in the final table, and 3 (under `USE_TONE`) in the tone table;
  - a failed initial probe lets the middle probe read position 0;
  - the parse then requires full consumption and a row hit in the keyboard's own index.
- The port does not reuse the per-byte Simple concatenation, on purpose.
- A key may appear twice in one table: the dual-mapped keys `c` → ㄒ/ㄕ, `j` → ㄐ/ㄓ and `v` → ㄑ/ㄔ on the HSU keyboards.
  Parsing resolves the **first** row.
  `in_chewing_keyboard` reports both, through `search_chewing_symbols2`.
  It returns at most three strings in total, tone mark included (`assert(array->len <= 3)`, `:534`); HSU `j` = ㄐ, ㄓ + the tone-4 mark is the maximum.
- The keyboards' indexes carry the layout remaps as correction-gated rows:
  - `hsu_zhuyin_index` 500 = 417 plain + 78 `ZHUYIN_CORRECT_HSU` + 5 incomplete (`pinyin_parser_table.h:2988`);
  - `eten26_zhuyin_index` 482 = 417 + 59 `ZHUYIN_CORRECT_ETEN26` + 6 (`:3491`).
- Examples of the remaps: the same ㄍㄧ keystrokes are `ji` on the HSU keyboards and `qi` on ETEN26; ㄐㄨㄥ → zhong; ㄒ → shi.
  The vowel stand-ins are, for HSU: ㄇ→an, ㄋ→en, ㄌ→er, ㄍ→e, ㄎ→ang, ㄏ→o; for ETEN26: ㄆ→ou, ㄇ→an, ㄊ→ang, ㄋ→en, ㄌ→eng, ㄏ→er.
- HSU_DVORAK shares `hsu_zhuyin_index`, and its `chewing_hsu_dvorak_*` tables are byte-identical to `chewing_hsu_*` at this pin.

**DACHEN_CP26 (9).**
The port is `ZhuyinDaChenCP26Parser2` (`zhuyin_parser2.cpp:547-852`).

- The parser is constructor-configured: no `set_scheme`, no `m_options`, no correction bit.
  It searches the **global** index.
- Successful parsed keys resolve to plain rows, because the repeat-count probe always builds spellings in canonical initial+middle+final order.
  Incomplete rows can match only when the caller sets `ZHUYIN_INCOMPLETE`, and the parser's post-match tone-validity mask then rejects them.
- The probe's law: a run of the same key (counted by `count_same_chars`) cycles the rows that the key maps to, and the parser consumes the whole run at once.
  - Dual rows pick by `(count - 1) % 2`: 1 tap → first row, 2 taps → second, 3 → first, and so on.
  - Three middle keys have multi-way specials: `u` cycles `(count - 1) % 3` over middle ㄧ / final ㄚ / both; `m` cycles `(count - 1) % 2` over middle ㄩ / final ㄡ; `j` is always middle ㄨ.
- Under `USE_TONE`, the parser probes the **last** byte in the tone table first and strips it.
  So keys that are both initial and tone (`e` `r` `d` `y`) resolve by position.
  "eke" is ge+tone 2; a bare "ee" strips to the lone incomplete ㄍ and consumes 0.
- The maximum key length is 12 (`max_chewing_dachen26_length`).
- Every table row is parse-live (both rows of a dual cycle, by repeat count).
  The one display-only string is the `i` key's extra "ㄧㄚ" in `in_chewing_scheme`; parse produces ㄧㄚ only through the `u` triple-tap.
- The upstream `#if 0` partial-input block (`:768-792`) is dead code at the pin and is not ported.

**Not ported:** STANDARD_DVORAK (7), the abort slot.

## The exact seam

The scheme parsers hand the decoder their exact keys: one `Exact` edge chain (`SegmentGraph::build_exact`, `Session::replace_raw_exact`), with spans over the joined text and separators placed as in the parsed graph.

- The scan, sentence and composition-key walks all use it.
- `build_scan_matrix` generates divided and resplit alternates only in full-pinyin mode.
- Any other raw mutation (typing, commit, reset, a full-pinyin replace) clears the exact chain.
  So the full-pinyin paths are bit-unchanged.

**Rejected alternative: drive the session with the `'`-joined full-pinyin text.**
The session's graph then re-parses the text through the pinyin inventory, which gives three families of divergence from the pin:

- Ambiguous spellings gain extra segmentations.
  `bie` also walks `[bi][e]`, which adds the bi rows and the lone `bi'e` phrase: 41 vs 302 candidates on every ㄅㄧㄝ permutation.
- Zhuyin-only spellings (`den zhei nia yai nun eng chua`) are in no pinyin inventory.
  So the text re-parses as a shorter syllable plus leftovers and offers wrong candidates (`ㄉㄣˋ` → 的, `ㄋㄧㄚˊ` → 你啊).
  Upstream's whole-key lookup is legitimately empty there: the model carries zero rows for those spellings, measured against the canonical tables.
- The scan matrix's divided and resplit alternates are a full-pinyin-parse artifact upstream; applied to the scheme keys, they add candidates the pin does not offer.

**Regression cover.**
Core tests walk and reject the exact graph.
The capi tests use the mini tables' deliberate `xian`/`xi'an` pair: zhuyin ㄒㄧㄢ must offer the xian rows only, while bare full-pinyin `xian` still enumerates its `xi`+`an` segmentation.
They also cover the `den` no-truncation case.

## Open

- Tone stays outside the `SyllableKey` vocabulary unless the pinned bopomofo candidate surface needs tone in the decoder key ([Tone](#tone-and-the-syllablekey-vocabulary)).
- STANDARD_DVORAK (7) stays the abort slot until upstream fixes the fallthrough at `zhuyin_parser2.cpp:291-295`.
