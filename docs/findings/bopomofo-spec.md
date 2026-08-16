# Bopomofo/Zhuyin scheme SPEC

Date: 2026-08-17 · Status: W13 Phase 0 draft (human freeze pending)

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
(`zhuyin_parser2.cpp:272`). This first STANDARD pass does not: shuffled
spellings such as `1,u` (ㄅㄝㄧ → `bie`) parse as consumed 0. Recorded
here rather than treated as accidental.

## Keyboard scope for the first PR

The first bopomofo PR scopes **STANDARD only**, and leaves HSU, IBM,
GINYIEH, ETEN, ETEN26, STANDARD_DVORAK, HSU_DVORAK and DACHEN_CP26 deferred.
STANDARD is the fork's default (`ZHUYIN_DEFAULT = ZHUYIN_STANDARD`) and the
pinned differential surface. Scheme setters accept the full ABI values but
unimplemented schemes report `false` rather than aborting.

## Verification input set

The first differential uses STANDARD keystrokes covering:

- multi-key syllables: `ru4`/`ru ` style tone digits, `1`, `q`, `a`, `u`;
- tone keys `' '` (first tone), `3`, `4`, `6`, `7`;
- zero-initial syllables;
- invalid keys and trailing non-keyboard bytes to pin consumed length;
- rejection class: `"su "` (illegal first tone), `"sux6"` (tone after an
  invalid syllable), `"6"` / `" "` (tone with no syllable), each next to
  a valid control (`"su6"`, `"sucl"`, `"su"`);
- `pinyin_in_chewing_keyboard` symbol vectors for symbol and tone keys.
