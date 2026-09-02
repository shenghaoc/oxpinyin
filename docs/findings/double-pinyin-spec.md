# Double-pinyin scheme SPEC

Date: 2026-08-17 · Status: **frozen** (2026-09-02; the W13 Phase 0 draft
with freeze amendments — see the Freeze record at the bottom)

## Scope

This SPEC freezes the behaviour oxpinyin must reproduce for
`pinyin_parse_more_double_pinyins`, `pinyin_set_double_pinyin_scheme`, and
`pinyin_get_double_pinyin_auxiliary_text` from the pinned libpinyin oracle.
The full-pinyin corpus never reaches this parser; a moved corpus pin is a
leak and must STOP the workstream.

Source identity: libpinyin `2.11.91`, commit
`0c5e80e1200f84fab185d1c5bde458b770a0636c` (the same pin as
`docs/testing/oracle-environment.md`).

## Scheme ABI

`DoublePinyinScheme` values come from `src/storage/pinyin_custom2.h:108-117`:

- `DOUBLE_PINYIN_ZRM` = 1
- `DOUBLE_PINYIN_MS` = 2
- `DOUBLE_PINYIN_ZIGUANG` = 3
- `DOUBLE_PINYIN_ABC` = 4
- `DOUBLE_PINYIN_PYJJ` = 5
- `DOUBLE_PINYIN_XHE` = 6
- `DOUBLE_PINYIN_CUSTOMIZED` = 30
- `DOUBLE_PINYIN_DEFAULT` = `DOUBLE_PINYIN_MS`

`pinyin_set_double_pinyin_scheme` calls
`context->m_double_pinyin_parser->set_scheme(scheme)`
(`src/pinyin.cpp:1155-1159`). The parser object lives on the context, and
instances dereference the context parser on every parse
(`src/pinyin.cpp:1540-1552`), so a setter call is observed by already
allocated instances on their next parse. `DOUBLE_PINYIN_CUSTOMIZED` is not a
compiled table; upstream `set_scheme` aborts on it
(`src/storage/pinyin_parser2.cpp:610-612`). oxpinyin must not abort: it
reports `false` for that value and keeps the previous scheme.

## Parser class

`DoublePinyinParser2` is declared at `src/storage/pinyin_parser2.h:180-205`
and implemented at `src/storage/pinyin_parser2.cpp:403-615`.

The parser holds three source-embedded tables:

- a 27-entry shengmu (initial) table indexed by `a`..`z` then `;`
  (`charid = ch == ';' ? 26 : ch - 'a'`);
- a 27-entry yunmu (final) table with two candidate finals per key;
- an optional fallback table for a handful of two-key spellings.

Input characters are `a`..`z` and `;` only. Tone digits `1`..`5` are part of
the raw probe window but are only accepted by `parse_one_key` as a third
byte when `USE_TONE` is set.

### One-key incomplete form

With `PINYIN_INCOMPLETE`, a single `a`..`z`/`;` key maps through the shengmu
table and is returned only when the mapped initial is non-null and not the
zero-initial marker `"'"` (`src/storage/pinyin_parser2.cpp:415-432`).

### Two-key form

`parse_one_key` first clears correction/ambiguity bits and forces
`PINYIN_CORRECT_UE_VE | PINYIN_CORRECT_V_U` (`:409, :434-436`). It reads the
first key as shengmu (the `"'"` entry means zero initial), then tries the
first and second yunmu in that order (`:452-499`). If neither matches, the
scheme's fallback table is searched by exact two-byte input (`:501-524`).

### Tone

For double pinyin, tone is a third input byte. It is accepted only when
`USE_TONE` is set and the third byte is `1`..`5`; `FORCE_TONE` requires a
three-byte key (`:411-450`). The inner `FORCE_TONE` zero-tone check at
`:448` is dead: the digit parse above it already refuses every non-tone
byte, so the live law is exactly the length-3 gate plus the `USE_TONE`
tone acceptance.

**Frozen law, both seams.** The batch seam passes the context option word
into the parser (`src/pinyin.cpp:1543`), so
`pinyin_parse_more_double_pinyins` must honour the same law as the one-key
seam: under `FORCE_TONE` a key that is not exactly three bytes refuses,
and a three-byte key carries its `1`..`5` digit as the tone only under
`USE_TONE`. Status at freeze:

- the one-key seam (`pinyin_parse_double_pinyin`) implements this law in
  full — the Tier-A amendment in the `upstream-divergences.md` FORCE_TONE
  entry, measured IDENTICAL against the pin over 2,131 probe lines
  (`tools/bisection/run-key-surface-diff.sh`, double schemes 1–6);
- the batch seam does not yet: the landed W13 batch parse runs the
  tone-less profile (a three-byte key refuses and the greedy walk retries
  length 2 — matching the pin only when `USE_TONE` is unset), so with
  `FORCE_TONE` set it is observably less restrictive than the pin. This is
  the open implementation item the freeze creates; the divergence register
  carries it (FORCE_TONE entry, double-pinyin batch seam).

The pinned full-pinyin corpus is tone-less and the fork's double-pinyin
context does not set `USE_TONE`, which is why the landed W13 batch
implementation could ship the tone-less profile without perturbing the
frozen scheme sweeps. The freeze does not change that scope boundary — it
fixes the law the batch seam must grow into.

**Amendment (2026-09-02, 5ec782ea):** the batch seam implements the frozen
law — the caller's option word crosses the seam and the greedy walk honours
the length-3 gate and the tone carriage; the measured closure and the one
pre-existing scheme-3 trellis residual are recorded in the divergence
register's FORCE_TONE entry (Double-pinyin batch closure amendment). The
"Status at freeze" bullets above stay as the freeze-time snapshot.

### Maximum forward match

`parse` probes the longest possible input of valid key/tone bytes, then
repeatedly tries lengths `3`, `2`, `1` from the current position
(`max_double_pinyin_length = 3`) and stops at the first success
(`src/storage/pinyin_parser2.cpp:531-574`). This is a greedy longest-match
per key, not a global DP; it is not the full-pinyin exhaustive path set.

## Scheme tables

The six compiled schemes live in the generated header
`src/storage/double_pinyin_table.h` (source-generated, copyable under the
source policy; not vendored model data). The table names and locations:

| Scheme | Shengmu | Yunmu | Fallback |
|---|---|---|---|
| MS | `double_pinyin_mspy_sheng` (`:9`) | `double_pinyin_mspy_yun` (`:39`) | none |
| ZRM | `double_pinyin_zrm_sheng` (`:69`) | `double_pinyin_zrm_yun` (`:114`) | `double_pinyin_zrm_fallback` (`:99`) |
| ABC | `double_pinyin_abc_sheng` (`:144`) | `double_pinyin_abc_yun` (`:174`) | none |
| Ziguang | `double_pinyin_zgpy_sheng` (`:204`) | `double_pinyin_zgpy_yun` (`:234`) | none |
| PYJJ | `double_pinyin_pyjj_sheng` (`:264`) | `double_pinyin_pyjj_yun` (`:294`) | `double_pinyin_pyjj_fallback` (`:324`) |
| Xiaohe | `double_pinyin_xhe_sheng` (`:339`) | `double_pinyin_xhe_yun` (`:369`) | `double_pinyin_xhe_fallback` (`:399`) |

`set_scheme` switches exactly these pointers
(`src/storage/pinyin_parser2.cpp:578-614`).

## Downstream lattice

Upstream double-pinyin parsing feeds the same `PhoneticKeyMatrix` as full
pinyin through `fill_matrix`, followed by `fuzzy_syllable_step` only
(`src/pinyin.cpp:1540-1563`). Unlike full pinyin, it does **not** run
`resplit_step` or `inner_split_step` (`src/pinyin.cpp:1516-1518`). A
two-key double-pinyin spelling is therefore one atomic decoder key; it is
not re-segmented into shorter syllables.

The oxpinyin analogue is the existing `SegmentGraph`/k-best/scoring
machinery, but full-pinyin `SegmentGraph::build` intentionally emits
segmentation edges inside a spelling. The landed routing therefore takes
the exact-seam route: `exact_input` joins the parser-produced keys as
`'`-separated full-pinyin text with one `ExactSegment` per key
(`crates/oxpinyin-capi/src/parse.rs`), and `Session::replace_raw_exact`
drives the session with that text and those segments. The graph is then
one `EdgeKind::Exact` chain — the pinyin inventory never re-segments the
joined spelling, exactly upstream, whose decoder receives the scheme
parser's `ChewingKey`s the same way (no `resplit_step` /
`inner_split_step` analogue). The original scheme bytes stay on the capi
instance for aux text and offset mapping.

The W14 sentence surface rides this routing: the C ABI wiring gap this
draft's first version recorded (`SharedLm` not overriding
`nbest_step_costs`, NBEST rows absent on every scheme encoding) was
closed by W14 (`docs/findings/sentence-surface.md` §6–§8, PR #113 and
follow-ups) — `get_sentence` decodes on every input and NBEST rows are
present on the double-pinyin and bopomofo surfaces alike. The measured
sentence residual is the trellis-side §12 entry, shared with full pinyin
and registered in `upstream-divergences.md` (n-best gfloat accumulation);
it is not scheme-specific.

## Auxiliary text

`pinyin_get_double_pinyin_auxiliary_text` is the same prefix/middle/postfix
walk as full pinyin, but with no apostrophe handling and with the middle key
split into shengmu + `|` + yunmu rather than raw bytes
(`src/pinyin.cpp:3438-3514`). The shengmu and yunmu strings are the
`ChewingKey` components, not the original two keystrokes. With no tone the
cursor-at-boundary form is `|`; inside a two-byte key cursor 1 renders
`shengmu|yunmu ` and cursor 2 renders `shengmuyunmu| `.

## Verification input set

The scheme differential uses, at minimum:

- cross-scheme collisions: `ni`, `wo`, `sh`, `zh`, `a`, `;`, `jv`, `lv`,
  `nv`, `er`;
- two-key pairs whose first key is zero-initial (`o`-key in MS/ZRM/XHE);
- fallback rows for ZRM/PYJJ/XHE;
- incomplete one-key forms under `PINYIN_INCOMPLETE`;
- invalid bytes and trailing digits to pin the consumed-length boundary.

## Freeze record (2026-09-02, maintainer)

The maintainer froze this SPEC after W13 landed (20e6b3a), reviewing the
Phase 0 draft against the pinned source and the landed implementation:

- Every source citation was re-verified against the pin `0c5e80e1`
  (scheme enum `pinyin_custom2.h:108-117`; setter and parse path
  `pinyin.cpp:1155-1159/:1540-1563`; `parse_one_key` laws at
  `pinyin_parser2.cpp:409/:412/:415-432/:434-436/:448/:452-499/:501-524`;
  greedy `parse` `:531-574` with `max_double_pinyin_length = 3` at `:84`;
  `set_scheme` `:578-614` with the CUSTOMIZED abort at `:610-612`; the
  six scheme tables at the line numbers above; aux text
  `pinyin.cpp:3438-3514`). All hold at the pin.
- Three amendments landed with the freeze: the Tone section now states the
  frozen law for both seams (the batch seam's missing `FORCE_TONE`
  length-3 gate is the one open implementation item, carried in the
  divergence register); the Downstream lattice section records the landed
  exact-segment routing (`replace_raw_exact`) and the closed W14 wiring
  gap, replacing the draft's `type_pinyin` shortcut and pre-W14
  measurements; and the Xiaohe yunmu/fallback table rows carry their exact
  line numbers (`:369`/`:399`).
- The standing gates for this surface are `run-scheme-diff.sh` (parse,
  aux, and full-candidate differentials over the six schemes) and
  `run-key-surface-diff.sh` (the one-key seam, 2,131 probe lines). Any
  future change to the batch parse — including the open `FORCE_TONE` gate
  — must keep both green against the pinned oracle.

The freeze's one open implementation item — the batch seam's `FORCE_TONE`
length-3 gate — was closed by 5ec782ea on the same day, stacked on the
freeze PR, keeping both standing gates green (see the Tone section
amendment and the divergence register's FORCE_TONE entry).
