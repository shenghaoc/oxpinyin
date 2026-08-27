# Bopomofo/Zhuyin scheme SPEC

Date: 2026-08-17 · Status: W13 Phase 0 draft (human freeze pending)
Amended 2026-08-20 by `zhuyin-index-fidelity` (PR 1 of the #109 stack):
the recorded no-shuffle decision below is superseded — see "Index
fidelity". Amended again by `zhuyin-simple-keyboards` (PR 2): the
Simple keyboard family is table-driven — see "Keyboard scope" — and by
`zhuyin-discrete` (PR 3): the Discrete family joins it there — and by
`zhuyin-dachen-cp26` (PR 4): CP26 completes the ported keyboard set.

## Scope

This SPEC freezes the behaviour of `pinyin_parse_more_chewings`,
`pinyin_set_zhuyin_scheme`, `pinyin_in_chewing_keyboard`, and
`pinyin_get_chewing_auxiliary_text` from the pinned libpinyin oracle.

Source identity: libpinyin `2.11.91`, commit
`0c5e80e1200f84fab185d1c5bde458b770a0636c`.

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

`pinyin_set_zhuyin_scheme` selects the parser class by scheme
(`src/pinyin.cpp:1161-1192`): `ZhuyinSimpleParser2` for STANDARD, IBM,
GINYIEH, ETEN and STANDARD_DVORAK; `ZhuyinDiscreteParser2` for HSU, ETEN26
and HSU_DVORAK; and `ZhuyinDaChenCP26Parser2` for DACHEN_CP26. Unknown
values abort upstream; oxpinyin reports `false` and keeps the previous
scheme. The parser is context-owned, so live instances observe the next
parse.

## Parser classes

Declarations: `src/storage/zhuyin_parser2.h:49-194`.
Implementations: `src/storage/zhuyin_parser2.cpp`.

- `ZhuyinSimpleParser2::parse_one_key` maps each keystroke through the
  scheme symbol table to a Zhuyin symbol string, optionally removes a
  trailing tone key, then looks the concatenated Zhuyin string up in the
  global `zhuyin_index` (`:162-213`).
- `ZhuyinDiscreteParser2::parse_one_key` consumes an explicit
  initial/middle/final/tone sequence from the scheme's separate tables
  (`:335-405`).
- `ZhuyinDaChenCP26Parser2::parse_one_key` is the repeat-count CP26 variant
  (`:573-716`).

All three `parse` methods probe the longest run of keys accepted by
`in_chewing_scheme`, then greedy longest-match each key up to
`max_chewing_length = 4` (or `max_chewing_dachen26_length = 12` for CP26)
and stop when no longer key parses (`:216-268`, `:408-460`, `:718-795`).

## Scheme tables

All keyboards are source-embedded in the generated header
`src/storage/zhuyin_table.h` and are copyable under the source policy, not
vendored model data.

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

CP26 tables start at `:433` (`initials`), `:458` (`middles`), and continue
through finals/tones in the same header.

`pinyin_in_chewing_keyboard` returns a `NULL`-terminated string vector of the
Zhuyin symbol(s) mapped by one keystroke. Simple schemes return at most one
symbol; discrete schemes return up to two for multi-purpose keys, and CP26
returns the same shape via its own lookup (`src/pinyin.cpp:1615-1625`;
parser methods at `zhuyin_parser2.cpp:301-333`, `:496-545`, `:798+`).

## Tone and the frozen SyllableKey vocabulary

Upstream `ChewingKey` carries tone in `m_tone`, and tone is assigned after
the tone-less Zhuyin spelling is resolved (`zhuyin_parser2.cpp:162-213`).
The existing oxpinyin `SyllableKey` is a dense id over the tone-less
full-pinyin inventory (`docs/findings/parser-spec.md`), and the pinned
full-pinyin corpus is tone-less.

**Decision for this workstream:** Zhuyin tone must not reshape `SyllableKey`.
The bopomofo parser returns a tone-less `SyllableKey` plus a separate tone
value; tone is a scheme-level attribute that rides alongside the key through
the scheme parser and aux formatter, not a new key id. Candidate matching
for tone-sensitive bopomofo lookup is outside W13's parser ground and is
deferred to the data/engine workstreams. If later evidence shows the pinned
bopomofo candidate surface cannot be reproduced without adding tone to the
decoder key, that is a settled-ground change requiring maintainer sign-off
and a new decision here; W13 does not unilaterally widen the frozen
inventory.

## Downstream lattice

The chewing parser outputs `ChewingKey`/`ChewingKeyRest` and feeds the same
`PhoneticKeyMatrix` through `fill_matrix`, then `fuzzy_syllable_step`
(`src/pinyin.cpp:1582-1608`). This first W13 pass uses the same
transformed-spelling shortcut as double pinyin (`'`-joined tone-less full
pinyin into `Session::type_pinyin`). On real unigrams,
`CandidateKind::Sentence` is absent for `sucl` and `sucl5j/eji` exactly
as it is for full-pinyin `nihao` / `nihaozhongguo` on main. That gap is
full-pinyin ground, not W11. The scheme-edge construction in
`double-pinyin-spec.md` remains a later segmentation-fidelity pass.

## Auxiliary text

`pinyin_get_chewing_auxiliary_text` walks the matrix like full pinyin but
renders Zhuyin via `get_zhuyin_string()` (`src/pinyin.cpp:3516-3574`,
`chewing_key.cpp:74-89`): tones 2–5 append their tone *mark*; first and
zero tones omit it. There is no apostrophe handling and no tone digit.

After a successful longest `parse_one_key`, upstream
`ZhuyinSimpleParser2::parse` calls `_ChewingKey::is_valid_zhuyin`
(`zhuyin_parser2.cpp:256-257`, `chewing_key.cpp:38-45`) and **breaks**
the whole parse — it does not retry a shorter key. Illegal tones
(including first-tone ㄋㄧ / `"su "`) therefore consume 0. oxpinyin
applies the same post-match stop.

`ZhuyinSimpleParser2::set_scheme` always ors `ZHUYIN_CORRECT_SHUFFLE`
(`zhuyin_parser2.cpp:272`). ~~This first STANDARD pass does not~~ The
`zhuyin-index-fidelity` port applies the same forced bit, so shuffled
spellings such as `1,u` (ㄅㄝㄧ → `bie`) parse through the 1062
`ZHUYIN_CORRECT_SHUFFLE` rows exactly like the pin.

## Index fidelity (`zhuyin-index-fidelity`, 2026-08-20)

The parser table is now the pinned `zhuyin_index` itself — 1493 rows
(`pinyin_parser_table.h:1492`): 417 plain + 1062 `ZHUYIN_CORRECT_SHUFFLE`
+ 14 `ZHUYIN_INCOMPLETE`, joined to canonical spellings through
`content_table` and gated by `check_chewing_options`
(`zhuyin_parser2.cpp:43-100`) in `oxpinyin-core/src/zhuyin_map.rs` /
`scheme.rs`. Properties pinned by unit tests: rows sorted/unique
(binary-search invariant), the shuffle law (every multi-symbol plain row
contributes all non-canonical symbol permutations: 227 two-symbol × 1 +
167 three-symbol × 5 = 1062, each mapping to its canonical row), and the
option gating of incomplete/shuffle rows at the index level.

**The 12-row recovery.** The first W13 table carried only the 405 plain
rows whose pinyin is a full-pinyin syllable. The remaining 12 plain rows
split into: 2 option-only syllables (`ㄥ`→`eng`, `ㄋㄨㄣ`→`nun`, mapped
to existing key ids 428/429) and 10 zhuyin-only syllables (`chua, den,
din, fe, kei, len, nia, rua, yai, zhei` — `content_table` rows with no
`pinyin_index` spelling). The latter become a fourth inventory tier,
`SyllableKey` ids 430..440 (`SYLLABLE_KEY_COUNT` 430→440):
`SyllableKey::from_canonical_text` resolves them (mirroring
`content_table` membership) while `from_text` deliberately does not
(mirroring `pinyin_index` membership), so the full-pinyin parse surface
is unchanged. Live/dead split under the pinned `valid_zhuyin_table`:
`eng, nun, den, zhei, nia, yai, chua` have non-empty tone masks (live
parse divergences before this port); `fe, din, kei, len, rua` and all 14
incomplete rows are all-zero (parse-dead; consumed 0 both sides, now by
the upstream mechanism). `VALID_ZHUYIN_TONES` is id-indexed at
`SYLLABLE_KEY_COUNT` (the audit confirmed it was the only fixed-size
id-indexed table in the workspace; every other consumer derives from the
count or is `.get()`-guarded).

**Canonical display.** Upstream's aux text renders the matched key's
canonical spelling (`ChewingKey::get_zhuyin_string`), so a shuffled
input ㄅㄝㄧ displays as ㄅㄧㄝ. The port carries the row's
`content_table[].m_chewing` as a canonical column; spans stay
input-based. The differential surfaced exactly this class before the
column was added (aux-only divergence on shuffled inputs; parse surfaces
already matched).

**Option seam.** `pinyin_parse_more_chewings` strips
`ZHUYIN_CORRECT_ALL` from caller options and passes the word through
(`pinyin.cpp:1621`); the only caller bit the Simple parser consults is
`ZHUYIN_INCOMPLETE` (bit 4), which `parse_chewing_more` now masks from
the whole-word options atomic into `ZhuyinParser::parse`'s
`allow_incomplete`. Keys recovered by the shuffle/recovery rows join the
already-documented non-gated full-candidate divergence class (no
phrases for the new spellings in the string-keyed dictionary — empty
lookup, `UNKNOWN_COST`, no panic; verified by the id-table audit).

## Keyboard scope

The `zhuyin-simple-keyboards` PR (2026-08-20) made the parser
table-driven over the four Simple keyboards: **STANDARD (1), IBM (3),
GINYIEH (4), ETEN (5)**, each with its pinned symbol/tone tables
(`chewing_{standard,ibm,ginyieh,eten}_{symbols,tones}`) and all sharing
the 1493-row index, the forced `ZHUYIN_CORRECT_SHUFFLE`, and the
canonical aux rendering. The three keyboards use the same 37 symbols on
different keys (GINYIEH and ETEN both bind the apostrophe; IBM's tone
keys are `m , . /`); no key is both a symbol and a tone key on any of
them, verified at extraction and re-verified per run by the driver's
table cross-check.

The `zhuyin-discrete` PR (2026-08-20) adds the Discrete family:
**HSU (2), ETEN26 (6), HSU_DVORAK (8)**, ported as
`ZhuyinDiscreteParser2` (`zhuyin_parser2.cpp:335-490`). These parse by
**positional probe** — position 0 in the keyboard's initial table, 1 in
the middle table, 2 in the final table, 3 (under `USE_TONE`) in the
tone table, a failed initial probe letting the middle probe read
position 0 — then require full consumption and a row hit in the
keyboard's own index; the per-byte Simple concatenation is deliberately
not reused. A key may appear twice in one table (the dual-mapped keys:
`c` → ㄒ/ㄕ, `j` → ㄐ/ㄓ, `v` → ㄑ/ㄔ on the HSU keyboards): parsing
resolves the **first** row, `in_chewing_keyboard` reports both — up to
three symbols plus the tone mark, `search_chewing_symbols2`. The
keyboards' indexes (`hsu_zhuyin_index` 500 = 417 plain + 78
`ZHUYIN_CORRECT_HSU` + 5 incomplete; `eten26_zhuyin_index` 482 = 417 +
59 `ZHUYIN_CORRECT_ETEN26` + 6) carry the layout remaps as
correction-gated rows — the same ㄍㄧ keystrokes are `ji` on the HSU
keyboards and `qi` on ETEN26, ㄐㄨㄥ → zhong, ㄒ → shi, and the vowel
stand-ins (HSU: ㄇ→an, ㄋ→en, ㄌ→er, ㄍ→e, ㄎ→ang, ㄏ→o; ETEN26:
ㄆ→ou, ㄇ→an, ㄊ→ang, ㄋ→en, ㄌ→eng, ㄏ→er). HSU_DVORAK shares
`hsu_zhuyin_index` and its `chewing_hsu_dvorak_*` tables are
byte-identical to `chewing_hsu_*` at this pin. At that stage, scheme
setters accepted ABI values 1/2/3/4/5/6/8; the CP26 port below adds
value 9. Unsupported values report `false` and keep the previous scheme
rather than aborting.

The `zhuyin-dachen-cp26` PR (2026-08-20) adds **DACHEN_CP26 (9)**,
ported as `ZhuyinDaChenCP26Parser2` (`zhuyin_parser2.cpp:541-844`).
The parser is constructor-configured — no `set_scheme`, no `m_options`,
no correction bit — and searches the **global** index. Successful
parsed keys resolve to plain rows because the repeat-count probe always
builds spellings in canonical initial+middle+final order; incomplete
rows can match only when the caller sets `ZHUYIN_INCOMPLETE`, and are
then rejected by the parser's post-match tone-validity mask. The probe's law: a
run of the same key (counted by `count_same_chars`) cycles the rows
that key maps to — dual rows pick by `(count - 1) % 2` (1 tap → first
row, 2 taps → second, 3 → first…), the whole run consumed at once.
Three middle keys have multi-way specials: `u` cycles
`(count - 1) % 3` over middle ㄧ / final ㄚ / both, `m` cycles
`(count - 1) % 2` over middle ㄩ / final ㄡ, and `j` is always middle
ㄨ. Under `USE_TONE` the **last** byte is probed in the tone table
first and stripped, so keys that are both initial and tone (`e` `r`
`d` `y`) resolve by position ("eke" is ge+tone 2; a bare "ee" strips
to the lone incomplete ㄍ and consumes 0). Max key length is 12
(`max_chewing_dachen26_length`). Every table row is parse-live (both
rows of a dual cycle by repeat count); the one display-only string is
the `i` key's extra "ㄧㄚ" in `in_chewing_scheme` — parse produces
ㄧㄚ only through the `u` triple-tap. The upstream `#if 0`
partial-input block (`:752-785`) is dead code at the pin and is not
ported. Deferred: STANDARD_DVORAK (the abort slot). The scheme setter
accepts 1/2/3/4/5/6/8/9. The per-scheme
PARSE_AUX sweep (runs 1–6, 8) is IDENTICAL for each; the Discrete
corpus covers the dual-key ambiguity, tone-bearing/rejection, the
correction rows, one canonical control, and the incomplete rows, with
keystrokes positionally derived from the selected keyboard's tables
(first row per key, like the parser itself).

## Verification input set

The differential corpus (`tools/bisection/chewing-diff.c`) names zhuyin
symbol sequences plus an optional tone; the keystroke strings are
**derived at startup** from the pinned STANDARD symbol/tone tables
embedded in the driver (verbatim copies of `zhuyin_table.h:9-58`), and
the embedded tables are cross-checked against the library under test
through `pinyin_in_chewing_keyboard` before any input runs. No
keystroke is hand-authored. Coverage, re-derived per run:

- the legacy W13 set: `ㄋㄧ`/`ㄏㄠ`/`ㄨㄛ`/`ㄖㄣ`/`ㄓㄨㄥ`/`ㄕ`/`ㄧ`,
  `ㄋㄧ`+tone2, `ㄌ`, empty input;
- rejection class: illegal first tone on `ㄋㄧ`, tone after an invalid
  syllable, tone keys alone, each beside a valid control;
- incomplete keys `ㄅ`/`ㄌ` (consumed 0 under the all-zero masks);
- shuffle: all six permutations of `ㄅㄧㄝ`, a two-symbol swap beside its
  canonical, shuffle composed with tone 4, and a multi-key greedy
  boundary (`ㄋㄧ` + shuffled `ㄅㄝㄧ`);
- the twelve recovered rows: a mask-valid and a mask-invalid tone each —
  including `ㄥ` whose *first* tone is valid, pinned beside `ㄋㄧ`'s
  illegal first tone — plus the five dead rows (`ㄈㄜ`, `ㄉㄧㄣ`, `ㄎㄟ`,
  `ㄌㄣ`, `ㄖㄨㄚ`) at consumed 0;
- `pinyin_in_chewing_keyboard` symbol vectors for symbol and tone keys.

Gate: `SCHEME_DIFF_PARSE_AUX_ONLY=1 ./run-scheme-diff.sh bopomofo` →
`PARSE_AUX_IDENTICAL` (re-measured with this corpus; double-pinyin
PARSE_AUX unchanged). ~~The full-log run still diverges in the
documented candidate/sentence class only.~~ Superseded by the exact
seam below: the full-log run is IDENTICAL.

## The exact seam (2026-08-27) — the full-log divergence closed

Two findings landed together; the full-log bopomofo differential now
runs IDENTICAL (all 45 corpus inputs, double and full-pinyin schemes
likewise, tone variant included).

**1. The recorded "documented candidate/sentence class" was partly a
harness artifact.** The scheme drivers preferred
`oxpinyin_init_for_fixtures` unconditionally, so the capi side always
ran the flat-export unigram tier against the oracle's real
`interpolation2.text` counts — different linguistic datasets. With the
drivers porting train-diff.c's rule (fixture init only when the system
dir lacks `interpolation2.text`) and the runner copying the file from
`W13_CAPI_SYSTEM`, double and full-pinyin went IDENTICAL with no engine
change at all.

**2. The real bopomofo bug: the joined text was re-segmented.** The
chewing (and double) parse paths drove the session with the
`'`-joined full-pinyin *text*, and the session's graph re-parsed it
through the pinyin inventory. Three families, one cause:

- ambiguous spellings gained extra segmentations (`bie` also walked
  `[bi][e]`, adding the bi rows and the lone `bi'e` phrase — 41 vs 302
  candidates on every ㄅㄧㄝ permutation);
- zhuyin-only spellings (`den zhei nia yai nun eng chua`) exist in no
  pinyin inventory, so the text re-parsed as a shorter syllable plus
  leftovers and offered wrong candidates (`ㄉㄣˋ` → 的, `ㄋㄧㄚˊ` → 你啊),
  where upstream's whole-key lookup is legitimately empty (the model
  carries zero rows for those spellings — measured against the
  canonical tables);
- the scan matrix's divided/resplit alternates are a full-pinyin-parse
  artifact upstream, and were applied to the scheme keys too.

Fix: `SegmentGraph::build_exact` + `Session::replace_raw_exact` — the
scheme parsers hand the decoder their exact keys (one `Exact` edge
chain, spans over the joined text, separators riding as in the parsed
graph), the scan/sentence/composition-key walks all use it, and
`build_scan_matrix` generates divided/resplit alternates only for the
full-pinyin mode. Any other raw mutation (typing, commit, reset, a
full-pinyin replace) clears the exact chain, so the full-pinyin paths
are bit-unchanged: the replay pins, the decode differential, and the
live corpus differential all measured identical before and after.

Regression cover: core tests walk/reject the exact graph; the capi
tests use the mini tables' deliberate `xian`/`xi'an` pair — zhuyin
ㄒㄧㄢ must offer the xian rows only, while bare full-pinyin
`xian` still enumerates its `xi`+`an` segmentation — plus the `den`
no-truncation case.
