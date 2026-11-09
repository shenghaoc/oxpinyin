# Bopomofo/Zhuyin scheme SPEC

Date: 2026-08-17 · Status: **frozen** (maintainer ruling, 2026-09-06; see the [Freeze record](#freeze-record))

What binds: the C API behaviour of the four functions in [Scope](#scope), as derived from the pin ([frozen-SPEC ruling](compatibility-policy.md#frozen-specs)).
The [Freeze record](#freeze-record) lists it under "What is frozen", with the items that stay provisional.
A change to that behaviour needs a maintainer ruling.
The description of oxpinyin's implementation (its types, files, key ids, tests and differential drivers) is current reference.

## Scope

This SPEC freezes the behaviour of these functions, from the pinned libpinyin oracle:

- `pinyin_parse_more_chewings`
- `pinyin_set_zhuyin_scheme`
- `pinyin_in_chewing_keyboard`
- `pinyin_get_chewing_auxiliary_text`

Source identity: libpinyin `2.11.91`, commit `0c5e80e1200f84fab185d1c5bde458b770a0636c`, the pin at freeze.
The current pin is libpinyin `2.11.92`, commit `074a2219`.
The source citations in this SPEC hold at `074a2219`.

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

Unknown values abort upstream; oxpinyin reports `false` and keeps the previous scheme.
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
They then greedy-longest-match each key up to `max_chewing_length = 4` (or `max_chewing_dachen26_length = 12` for CP26).
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

## Tone and the frozen SyllableKey vocabulary

Upstream `ChewingKey` carries tone in `m_tone`.
The parser assigns tone after it resolves the tone-less Zhuyin spelling (`zhuyin_parser2.cpp:162-213`).
The oxpinyin `SyllableKey` is a dense id over the tone-less full-pinyin inventory ([parser-spec.md](parser-spec.md)), and the pinned full-pinyin corpus is tone-less.

**Decision:** Zhuyin tone must not reshape `SyllableKey`.

- The bopomofo parser returns a tone-less `SyllableKey` plus a separate tone value.
- Tone is a scheme-level attribute that travels with the key through the scheme parser and the aux formatter; it is not a new key id.
- Candidate matching for tone-sensitive bopomofo lookup is outside the parser and outside this SPEC.
- Evidence may show that the pinned bopomofo candidate surface cannot be reproduced without tone in the decoder key.
  That is a settled-ground change: it requires maintainer sign-off and a new decision here.
  The parser work does not widen the frozen inventory on its own authority.

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

After a successful longest `parse_one_key`, upstream `ZhuyinSimpleParser2::parse` calls `_ChewingKey::is_valid_zhuyin` (`zhuyin_parser2.cpp:256-257`, `chewing_key.cpp:38-45`).
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

`VALID_ZHUYIN_TONES` is id-indexed at `SYLLABLE_KEY_COUNT`.
The audit confirmed that it is the only fixed-size id-indexed table in the workspace; every other consumer derives from the count or is `.get()`-guarded.

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
The string-keyed dictionary has no phrases for the new spellings, so the lookup is empty, with `UNKNOWN_COST` and no panic (verified by the id-table audit).

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
  This was verified at extraction, and the driver's table cross-check re-verifies it per run.

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
  - `hsu_zhuyin_index` 500 = 417 plain + 78 `ZHUYIN_CORRECT_HSU` + 5 incomplete;
  - `eten26_zhuyin_index` 482 = 417 + 59 `ZHUYIN_CORRECT_ETEN26` + 6.
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

**Recorded evidence.**
The per-scheme PARSE_AUX sweep (runs 1–6, 8) is IDENTICAL for each.
The Discrete corpus covers the dual-key ambiguity, tone-bearing and rejection inputs, the correction rows, one canonical control, and the incomplete rows.
Its keystrokes are positionally derived from the selected keyboard's tables (first row per key, as in the parser).

## Verification input set

The differential corpus is recorded evidence; the C driver is retired.
The corpus names zhuyin symbol sequences plus an optional tone.
The driver derived the keystroke strings at startup from the pinned STANDARD symbol and tone tables embedded in it (verbatim copies of `zhuyin_table.h:9-58`).
Before any input ran, it cross-checked the embedded tables against the library under test through `pinyin_in_chewing_keyboard`.
No keystroke is hand-authored.
Coverage, re-derived per run:

- the base set: `ㄋㄧ`/`ㄏㄠ`/`ㄨㄛ`/`ㄖㄣ`/`ㄓㄨㄥ`/`ㄕ`/`ㄧ`, `ㄋㄧ`+tone2, `ㄌ`, empty input;
- rejection class: illegal first tone on `ㄋㄧ`, tone after an invalid syllable, tone keys alone, each beside a valid control;
- incomplete keys `ㄅ`/`ㄌ` (consumed 0 under the all-zero masks);
- shuffle: all six permutations of `ㄅㄧㄝ`, a two-symbol swap beside its canonical, shuffle composed with tone 4, and a multi-key greedy boundary (`ㄋㄧ` + shuffled `ㄅㄝㄧ`);
- the twelve recovered rows: a mask-valid and a mask-invalid tone each, plus the five dead rows (`ㄈㄜ`, `ㄉㄧㄣ`, `ㄎㄟ`, `ㄌㄣ`, `ㄖㄨㄚ`) at consumed 0.
  This includes `ㄥ`, whose *first* tone is valid, pinned beside the illegal first tone of `ㄋㄧ`;
- `pinyin_in_chewing_keyboard` symbol vectors for symbol and tone keys.

Recorded evidence: the bopomofo scheme differential's parse/aux filter gives `PARSE_AUX_IDENTICAL` on this corpus, and double-pinyin PARSE_AUX does not change.
The full-log run is IDENTICAL ([The exact seam](#the-exact-seam)).

## The exact seam

The full-log bopomofo differential runs IDENTICAL: all 45 corpus inputs, the double and full-pinyin schemes, and the tone variant.
Two findings closed the full-log divergence.

**1. Part of the recorded candidate/sentence class was a harness artifact.**
The scheme drivers used `oxpinyin_init_for_fixtures` unconditionally.
So the capi side always ran the flat-export unigram tier, while the oracle used its real `interpolation2.text` counts: different linguistic datasets.
The drivers now use train-diff.c's rule: fixture init only when the system dir has no `interpolation2.text`.
The runner copies the file from `W13_CAPI_SYSTEM`.
With that change, double and full pinyin are IDENTICAL with no engine change.

**2. The real bopomofo defect: the joined text was re-segmented.**
The chewing (and double) parse paths drove the session with the `'`-joined full-pinyin *text*, and the session's graph re-parsed it through the pinyin inventory.
This one cause gave three families of divergence:

- Ambiguous spellings gained extra segmentations.
  `bie` also walked `[bi][e]`, which added the bi rows and the lone `bi'e` phrase: 41 vs 302 candidates on every ㄅㄧㄝ permutation.
- Zhuyin-only spellings (`den zhei nia yai nun eng chua`) are in no pinyin inventory.
  So the text re-parsed as a shorter syllable plus leftovers and offered wrong candidates (`ㄉㄣˋ` → 的, `ㄋㄧㄚˊ` → 你啊).
  Upstream's whole-key lookup is legitimately empty there: the model carries zero rows for those spellings, measured against the canonical tables.
- The scan matrix's divided and resplit alternates are a full-pinyin-parse artifact upstream, but oxpinyin applied them to the scheme keys too.

**The fix: `SegmentGraph::build_exact` + `Session::replace_raw_exact`.**

- The scheme parsers hand the decoder their exact keys: one `Exact` edge chain, with spans over the joined text and separators placed as in the parsed graph.
- The scan, sentence and composition-key walks all use it.
- `build_scan_matrix` generates divided and resplit alternates only in full-pinyin mode.
- Any other raw mutation (typing, commit, reset, a full-pinyin replace) clears the exact chain.
  So the full-pinyin paths are bit-unchanged: the replay pins, the decode differential and the live corpus differential all measured identical before and after.

**Regression cover.**
Core tests walk and reject the exact graph.
The capi tests use the mini tables' deliberate `xian`/`xi'an` pair: zhuyin ㄒㄧㄢ must offer the xian rows only, while bare full-pinyin `xian` still enumerates its `xi`+`an` segmentation.
They also cover the `den` no-truncation case.

## Freeze record

The record was drafted on 2026-09-03 after an audit of this SPEC against the pin, in the form of the [double-pinyin freeze](double-pinyin-spec.md#freeze-record-2026-09-02-maintainer) (2026-09-02, PR #287).
The maintainer's rulings on the audit (2026-09-03) set what is inside the freeze and what stays outside it.
The maintainer's ruling of 2026-09-06 (the last item below) set the status to **frozen**.

- **Every upstream citation was re-verified at the pin at freeze, `0c5e80e1`** (tag `2.11.91`), read from a fresh clone of libpinyin at that commit.
  The eight source files were also hashed against the pin's blobs.
  All of these held at that pin:
  - scheme enum `pinyin_custom2.h:122-133`;
  - setter `pinyin.cpp:1161-1192`, with the out-of-enum abort at `:1188`;
  - parse path `:1582-1609`, with the strip at `:1589`;
  - `in_chewing_keyboard` `:1615-1625`;
  - aux text `:3516-3574` and `chewing_key.cpp:74-89`;
  - the parser laws at `zhuyin_parser2.cpp:37/:39` (lengths 4 and 12), `:44-64` and `:71-101` (option gate and index search), `:162-213/:216-268/:272/:301-333` (Simple), `:335-405/:408-460/:462-494/:496-545` (Discrete), `:547-555/:557-571/:573-716/:718-795/:798-852` (CP26, dead block `:768-792`);
  - the post-match `is_valid_zhuyin` stop at `:256-257`, `:448`, `:757`, with the table at `chewing_key.cpp:38-45` and `zhuyin_table.h:491`;
  - all 26 keyboard tables at the lines in [Scheme tables](#scheme-tables);
  - the indexes at `pinyin_parser_table.h:1492` (1493 = 417 + 1062 + 14), `:2988` (500 = 417 + 78 + 5) and `:3491` (482 = 417 + 59 + 6);
  - the shuffle law (227 × 1 + 167 × 5, every row a non-identity permutation of its canonical spelling);
  - the twelve recovered spellings and their tone masks (`eng` 110000, `nun` 101010, `den`/`zhei` 100010, `nia`/`yai` 101000, `chua` 110100; `din`, `fe`, `kei`, `len`, `rua` and all 14 incomplete rows 000000; `ni` 101110);
  - the HSU/ETEN26 correction-row mappings;
  - the byte-identical `chewing_hsu_dvorak_*` tables;
  - the STANDARD_DVORAK fallthrough `:291-295`.
- **Five audit findings were corrected.**
  - The option-seam paragraph, and the `parse.rs` comment copied from it, said that `ZHUYIN_INCOMPLETE` was the only caller bit consulted and cited `pinyin.cpp:1621` for the strip.
    Both statements were false at the pin, and the claim was the stated reason that the pinyin facade forwarded one bit instead of the option word.
    That correction landed first, on its own (dea0880), and the register entry that it exposed followed (128076a).
  - The freeze included the other four: the dead-block cite (`:752-785` → `:768-792`); the Discrete `in_chewing_keyboard` overstatement (at most three strings in total, tone mark included); two drifted class ranges; and the Downstream lattice section.
- **Re-measured for the freeze (2026-09-03).**
  - Environment: the debian-testing gate container (`oxpinyin-validate`, pin-built tkrzw oracle, `pin_ref=libpinyin-2.11.91-0c5e80e1…+model20-59c68e89…+dbm-tkrzw`).
  - The chewing differential driver ran against `libpinyin_capi.so` built from this tree and against the oracle, both on full model20 tables compiled from the model text.
  - The full log is byte-identical for all eight keyboards: 1 STANDARD 888 lines over the 45-input corpus, 2 HSU 255, 3 IBM 206, 4 GINYIEH 206, 5 ETEN 206, 6 ETEN26 268, 8 HSU_DVORAK 255, 9 DACHEN_CP26 665.
  - The runner's `PARSE_AUX_ONLY` filter is identical on each; double scheme 2, re-run as a control, is identical.
  - The drivers ran directly with the runner's own arguments and filter.
  - The driver's embedded STANDARD tables are verbatim `zhuyin_table.h:9-58`.
    Its other keyboards' tables are set-identical to the pin, with the GINYIEH and ETEN apostrophe rows relocated; this is harmless, because the lookup is by key.
- **What is frozen.**
  The four functions in Scope, the scheme ABI, the parser laws, the tables and indexes, the post-match tone-validity stop, the aux rendering, the index-fidelity rows, and the exact-seam routing.
  This applies under the option word that the standing gate uses (`IS_ZHUYIN | ZHUYIN_INCOMPLETE | USE_TONE`) and under the one-key seam's FORCE_TONE profiles.
- **What remains provisional.**
  1. Tone stays outside the frozen `SyllableKey` vocabulary (the Tone section's decision and its sign-off condition are unchanged).
  2. STANDARD_DVORAK (7) stays the abort slot until upstream fixes the fallthrough.
  3. The pinyin facade's chewing batch seam and `FORCE_TONE`: **CLOSED** ([compatibility policy](compatibility-policy.md#classification-table) row 30; closure note below).
     The frozen law for that seam is the pin's: the whole word minus `ZHUYIN_CORRECT_ALL` crosses, with `FORCE_TONE` nested under `USE_TONE` for Simple and CP26 and unconditional for Discrete.
     The libzhuyin facade implements the same law.
- **Recorded evidence.**
  The bopomofo scheme differential for keyboards 1 2 3 4 5 6 8 9 (full log and the parse/aux filter), and the one-key seam differential (2,131 probe lines).
- **Re-opening condition.**
  Either of these re-opens the freeze:
  - a pin-observed behaviour of the four functions that a gate cannot reproduce under the frozen law;
  - evidence that the pinned bopomofo candidate surface needs tone in the decoder key (the Tone section's settled-ground clause).

  Either is a maintainer decision recorded here, not a silent amendment.
- **Maintainer ruling — 2026-09-06: frozen as drafted.**
  The 2026-09-03 draft is frozen without further amendment.
  The "What is frozen" list above is the contract, and the "What remains provisional" items keep their stated status.
  The standing gates and the re-opening condition apply from this date.
  The one implementation item, the pinyin facade's chewing batch seam not forwarding `FORCE_TONE`, was carried by its register entry ([compatibility policy](compatibility-policy.md#classification-table) row 30) and is closed (next item).
- **Closure note — 2026-09-14.**
  The closure follows the fix shape that the register entry prescribed.
  The seam forwards the whole option word after the `ZHUYIN_CORRECT_ALL` strip (`pinyin.cpp:1582-1609`).
  capi tests pin the register's measured shape.
  The chewing differential driver gained the FORCE_TONE profile pass (recorded evidence).
  The frozen law does not change: this is its implementation, not an amendment.
  The profile's live run against the pin (2026-09-16) is IDENTICAL on all eight implemented keyboards ([upstream-divergences.md](upstream-divergences.md)).
