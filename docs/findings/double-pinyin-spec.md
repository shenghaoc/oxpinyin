# Double-pinyin scheme SPEC

Date: 2026-08-17 · Status: **frozen** (maintainer, 2026-09-02; see the [Freeze record](#freeze-record-2026-09-02-maintainer))

What binds: the C API behaviour of the three double-pinyin functions in [Scope](#scope), as derived from the pin — the scheme ABI, the parser laws, the tone law on both seams, the scheme tables, the downstream key shape and the auxiliary text ([frozen-SPEC ruling](compatibility-policy.md#frozen-specs)).
A change to that behaviour needs a maintainer ruling.
The description of how oxpinyin routes the keys (the oxpinyin function and file names under [Downstream lattice](#downstream-lattice)) is current reference.

## Scope

This SPEC freezes the behaviour that oxpinyin must reproduce from the pinned libpinyin oracle for these functions:

- `pinyin_parse_more_double_pinyins`
- `pinyin_set_double_pinyin_scheme`
- `pinyin_get_double_pinyin_auxiliary_text`

The full-pinyin corpus never reaches this parser.
A moved corpus pin is a leak and must STOP the workstream.

Source identity: libpinyin `2.11.91`, commit `0c5e80e1200f84fab185d1c5bde458b770a0636c`.
This is the pin at the freeze; the current pin is in `tools/oracle/oracle-pin.txt`.

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

`pinyin_set_double_pinyin_scheme` calls `context->m_double_pinyin_parser->set_scheme(scheme)` (`src/pinyin.cpp:1155-1159`).
The parser object is on the context, and instances dereference the context parser on every parse (`src/pinyin.cpp:1540-1552`).
So instances that already exist observe a setter call on their next parse.

`DOUBLE_PINYIN_CUSTOMIZED` is not a compiled table, and upstream `set_scheme` aborts on it (`src/storage/pinyin_parser2.cpp:610-612`).
oxpinyin must not abort: it reports `false` for that value and keeps the previous scheme.

## Parser class

`DoublePinyinParser2` is declared at `src/storage/pinyin_parser2.h:180-205` and implemented at `src/storage/pinyin_parser2.cpp:403-615`.

The parser holds three source-embedded tables:

- a 27-entry shengmu (initial) table, indexed by `a`..`z` then `;` (`charid = ch == ';' ? 26 : ch - 'a'`);
- a 27-entry yunmu (final) table, with two candidate finals per key;
- an optional fallback table for a small number of two-key spellings.

Input characters are `a`..`z` and `;` only.
Tone digits `1`..`5` are part of the raw probe window.
`parse_one_key` accepts a tone digit as a third byte only when `USE_TONE` is set.

### One-key incomplete form

With `PINYIN_INCOMPLETE`, a single `a`..`z`/`;` key maps through the shengmu table.
The parser returns it only when the mapped initial is non-null and is not the zero-initial marker `"'"` (`src/storage/pinyin_parser2.cpp:415-432`).

### Two-key form

1. `parse_one_key` clears the correction and ambiguity bits and forces `PINYIN_CORRECT_UE_VE | PINYIN_CORRECT_V_U` (`:409, :434-436`).
2. It reads the first key as shengmu; the `"'"` entry means zero initial.
3. It tries the first yunmu, then the second yunmu (`:452-499`).
4. If neither matches, it searches the scheme's fallback table by exact two-byte input (`:501-524`).

### Tone

For double pinyin, tone is a third input byte.
The parser accepts it only when `USE_TONE` is set and the third byte is `1`..`5`.
`FORCE_TONE` requires a three-byte key (`:411-450`).
The inner `FORCE_TONE` zero-tone check at `:448` is dead code: the digit parse above it already refuses every non-tone byte.
So the live law is exactly the length-3 gate plus the `USE_TONE` tone acceptance.

**Frozen law, both seams.**
The batch seam passes the context option word into the parser (`src/pinyin.cpp:1543`).
So `pinyin_parse_more_double_pinyins` must follow the same law as the one-key seam:

- under `FORCE_TONE`, a key that is not exactly three bytes refuses;
- a three-byte key carries its `1`..`5` digit as the tone only under `USE_TONE`.

Implementation status:

- The one-key seam (`pinyin_parse_double_pinyin`) implements this law in full ([compatibility policy](compatibility-policy.md#classification-table) row 16).
  Recorded evidence: IDENTICAL against the pin over 2,131 probe lines (double schemes 1–6).
- The batch seam also implements it (5ec782ea).
  The caller's option word crosses the seam, and the greedy walk follows the length-3 gate and the tone carriage.
  Row 16 of the compatibility policy is CLOSED.

The pinned full-pinyin corpus is tone-less, and the fork's double-pinyin context does not set `USE_TONE`.

### Maximum forward match

`parse` probes the longest possible input of valid key and tone bytes.
From the current position, it then tries lengths `3`, `2`, `1` in that order (`max_double_pinyin_length = 3`) and stops at the first success (`src/storage/pinyin_parser2.cpp:531-574`).
This is a greedy longest match per key, not a global DP.
It is not the full-pinyin exhaustive path set.

## Scheme tables

The six compiled schemes are in the generated header `src/storage/double_pinyin_table.h`.
The header is source-generated and copyable under the source policy; it is not vendored model data.
The table names and locations:

| Scheme | Shengmu | Yunmu | Fallback |
|---|---|---|---|
| MS | `double_pinyin_mspy_sheng` (`:9`) | `double_pinyin_mspy_yun` (`:39`) | none |
| ZRM | `double_pinyin_zrm_sheng` (`:69`) | `double_pinyin_zrm_yun` (`:114`) | `double_pinyin_zrm_fallback` (`:99`) |
| ABC | `double_pinyin_abc_sheng` (`:144`) | `double_pinyin_abc_yun` (`:174`) | none |
| Ziguang | `double_pinyin_zgpy_sheng` (`:204`) | `double_pinyin_zgpy_yun` (`:234`) | none |
| PYJJ | `double_pinyin_pyjj_sheng` (`:264`) | `double_pinyin_pyjj_yun` (`:294`) | `double_pinyin_pyjj_fallback` (`:324`) |
| Xiaohe | `double_pinyin_xhe_sheng` (`:339`) | `double_pinyin_xhe_yun` (`:369`) | `double_pinyin_xhe_fallback` (`:399`) |

`set_scheme` switches exactly these pointers (`src/storage/pinyin_parser2.cpp:578-614`).

## Downstream lattice

**Pin behaviour.**
Upstream double-pinyin parsing feeds the same `PhoneticKeyMatrix` as full pinyin, through `fill_matrix` followed by `fuzzy_syllable_step` only (`src/pinyin.cpp:1540-1563`).
Unlike full pinyin, it does **not** run `resplit_step` or `inner_split_step` (`src/pinyin.cpp:1516-1518`).
So a two-key double-pinyin spelling is one atomic decoder key, and nothing re-segments it into shorter syllables.

**oxpinyin routing.**
The oxpinyin analogue is the `SegmentGraph`, k-best and scoring code.
Full-pinyin `SegmentGraph::build` emits segmentation edges inside a spelling on purpose, so the scheme path uses the exact seam:

- `exact_input` joins the parser-produced keys as `'`-separated full-pinyin text, with one `ExactSegment` per key (`crates/oxpinyin-capi/src/parse.rs`).
- `Session::replace_raw_exact` drives the session with that text and those segments.
- The graph is then one `EdgeKind::Exact` chain.
  The pinyin inventory never re-segments the joined spelling.
  This is exactly upstream, whose decoder receives the scheme parser's `ChewingKey`s the same way (no `resplit_step` or `inner_split_step` analogue).
- The original scheme bytes stay on the capi instance for aux text and offset mapping.

The sentence surface uses this routing.
`get_sentence` decodes on every input, and NBEST rows are present on the double-pinyin and bopomofo surfaces ([sentence-surface.md](sentence-surface.md) §6–§8).
The measured sentence residual is the trellis-side §12 entry, which full pinyin shares.
The [compatibility policy](compatibility-policy.md#classification-table) classifies it (row 11).
It is not specific to a scheme.

## Auxiliary text

`pinyin_get_double_pinyin_auxiliary_text` is the same prefix/middle/postfix walk as full pinyin, with two differences (`src/pinyin.cpp:3438-3514`):

- there is no apostrophe handling;
- the middle key is split into shengmu + `|` + yunmu, not raw bytes.

The shengmu and yunmu strings are the `ChewingKey` components, not the original two keystrokes.
With no tone, the cursor-at-boundary form is `|`.
Inside a two-byte key, cursor 1 renders `shengmu|yunmu ` and cursor 2 renders `shengmuyunmu| `.

## Verification input set

The scheme differential uses, at minimum:

- cross-scheme collisions: `ni`, `wo`, `sh`, `zh`, `a`, `;`, `jv`, `lv`, `nv`, `er`;
- two-key pairs whose first key is zero-initial (`o`-key in MS/ZRM/XHE);
- fallback rows for ZRM/PYJJ/XHE;
- incomplete one-key forms under `PINYIN_INCOMPLETE`;
- invalid bytes and trailing digits, to pin the consumed-length boundary.

## Freeze record (2026-09-02, maintainer)

The maintainer froze this SPEC after reviewing the draft against the pinned source and the implementation (20e6b3a).

- Every source citation was re-verified against the pin `0c5e80e1`, and all hold at the pin:
  - scheme enum `pinyin_custom2.h:108-117`;
  - setter and parse path `pinyin.cpp:1155-1159/:1540-1563`;
  - `parse_one_key` laws at `pinyin_parser2.cpp:409/:412/:415-432/:434-436/:448/:452-499/:501-524`;
  - greedy `parse` `:531-574`, with `max_double_pinyin_length = 3` at `:84`;
  - `set_scheme` `:578-614`, with the CUSTOMIZED abort at `:610-612`;
  - the six scheme tables at the line numbers above;
  - aux text `pinyin.cpp:3438-3514`.
- The freeze included three amendments:
  - the Tone section states the frozen law for both seams;
  - the Downstream lattice section records the exact-segment routing (`replace_raw_exact`);
  - the Xiaohe yunmu and fallback table rows carry their exact line numbers (`:369`/`:399`).
- The recorded evidence for this surface is the scheme differential (parse, aux and full-candidate comparisons over the six schemes) and the one-key seam differential (2,131 probe lines).
- The one open implementation item at the freeze, the batch seam's `FORCE_TONE` length-3 gate, is closed by 5ec782ea ([Tone](#tone); compatibility policy row 16).
