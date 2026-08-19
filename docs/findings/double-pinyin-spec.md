# Double-pinyin scheme SPEC

Date: 2026-08-17 · Status: W13 Phase 0 draft (human freeze pending)

## Scope

This SPEC freezes the behaviour oxpinyin must reproduce for
`pinyin_parse_more_double_pinyins`, `pinyin_set_double_pinyin_scheme`, and
`pinyin_get_double_pinyin_auxiliary_text` from the pinned libpinyin oracle.
The full-pinyin corpus never reaches this parser; a moved corpus pin is a
leak and must STOP the workstream.

Source identity: libpinyin `2.11.91`, commit
`0c5e80e1200f84fab185d1c5bde458b770a0636c` (the same pin as
`docs/findings/oracle-environment.md`).

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
three-byte key (`:411-450`). The pinned full-pinyin corpus is tone-less, and
the fork's double-pinyin context does not set `USE_TONE`, so the first W13
implementation can reject three-byte keys exactly as upstream does without
`USE_TONE` while still recording the rule for a later tone-aware pass.

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
| Xiaohe | `double_pinyin_xhe_sheng` (`:339`) | `double_pinyin_xhe_yun` (continues below `:339`) | `double_pinyin_xhe_fallback` |

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
segmentation edges inside a spelling. This first W13 pass routes each
parser-produced key through the existing session by joining the keys as
tone-less full pinyin (`ni'hao`) and calling `Session::type_pinyin`. That
is the same path full pinyin uses.

W14 (`docs/findings/sentence-surface.md`) landed the n-best trellis in the
engine — the direct-Session parity is 488/385/370 on the pinned model —
but the C ABI path still shows `CandidateKind::Sentence` / ABI
`NBEST_MATCH` absent for full pinyin **and** for every scheme encoding.
The re-measure in §6 of the sentence-surface note tracks the reason to a
single seam: `SharedLm`
(`crates/oxpinyin-capi/src/state.rs:301-363`) does not override
`nbest_step_costs`, so the trait's default returns
`Ok(NbestStepCosts::default())` and the trellis yields no rows for any C
ABI caller. `pinyin_guess_sentence` still returns `true`;
`pinyin_get_sentence` returns `false` / `(null)` (the W14 decoded-or-
nothing gate over an empty row set). This is a C ABI wiring gap, not a
scheme problem, and closing it belongs upstream of the scheme paths.
Oracle `pinyin_guess_sentence` prepends n-best rows (`你好` on `nihao`;
`你好中国` on `nihaozhongguo`).

A later pass can replace the transformed-spelling shortcut with a
scheme-edge construction that emits each parser key as a single `Exact`
edge over the original double-pinyin byte span (no `resplit_step` /
`inner_split_step`). That is segmentation fidelity, not sentence
routing.

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
