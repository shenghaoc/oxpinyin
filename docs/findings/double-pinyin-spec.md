# Double-pinyin scheme SPEC

Status: **reference**

This SPEC states the pin's C API behaviour of the three double-pinyin functions in [Scope](#scope): the scheme ABI, the parser laws, the tone law on both seams, the scheme tables, the downstream key shape and the auxiliary text.
The oxpinyin function and file names under [Downstream lattice](#downstream-lattice) describe how oxpinyin routes the keys.

## Scope

This SPEC states the pin's behaviour of these functions, which oxpinyin reproduces:

- `pinyin_parse_more_double_pinyins`
- `pinyin_set_double_pinyin_scheme`
- `pinyin_get_double_pinyin_auxiliary_text`

The full-pinyin corpus never reaches this parser.
A moved corpus pin is a leak: the workstream halts, and continuing needs the maintainer's approval ([approval conditions](../../CONTRIBUTING.md#approval-conditions)).

The source citations hold at the current pin, libpinyin `2.11.92`, commit `074a2219` (`tools/oracle/oracle-pin.txt`).

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

`pinyin_set_double_pinyin_scheme` calls `context->m_double_pinyin_parser->set_scheme(scheme)`, ignores its result and returns `true` (`src/pinyin.cpp:1155-1159`).
The parser object is on the context, and instances dereference the context parser on every parse (`src/pinyin.cpp:1540-1552`).
So instances that already exist observe a setter call on their next parse.

`set_scheme` first clears the fallback table (`src/storage/pinyin_parser2.cpp:580`), then switches on the scheme:

- `DOUBLE_PINYIN_CUSTOMIZED` is not a compiled table, and `set_scheme` aborts on it (`:610-611`).
- Any other value outside the enum returns `false` (`:614`) with the shengmu and yunmu tables unchanged and the fallback table cleared.
  The wrapper ignores that `false`, so `pinyin_set_double_pinyin_scheme` returns `true`.

oxpinyin does the same: `pinyin_set_double_pinyin_scheme` (`crates/oxpinyin-capi/src/config.rs`) aborts on 30, and for an out-of-enum value it clears the live scheme's fallback table and returns `true`.

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
The parser accepts it only when `USE_TONE` is set and the third byte is `1`..`5` (`:439-445`).
`FORCE_TONE` requires a three-byte key (`:412`).
The inner `FORCE_TONE` zero-tone check at `:448` is dead code: the digit parse above it already refuses every non-tone byte.
So the live law is exactly the length-3 gate plus the `USE_TONE` tone acceptance.

**One law, both seams.**
The batch seam passes the context option word into the parser (`src/pinyin.cpp:1543`).
So `pinyin_parse_more_double_pinyins` must follow the same law as the one-key seam:

- under `FORCE_TONE`, a key that is not exactly three bytes refuses (`pinyin_parser2.cpp:412`);
- a three-byte key carries its `1`..`5` digit as the tone only under `USE_TONE`.

Both seams implement this law ([compatibility policy](compatibility-policy.md#classification-table) row 16).
The one-key seam is `pinyin_parse_double_pinyin`.
On the batch seam, the caller's option word crosses the seam, and the greedy walk follows the length-3 gate and the tone carriage.

The pinned full-pinyin corpus is tone-less, and the fork's double-pinyin context does not set `USE_TONE`.

### Maximum forward match

`parse` probes the longest possible input of valid key and tone bytes.
From the current position, it then tries lengths `3`, `2`, `1` in that order (`max_double_pinyin_length = 3`, `pinyin_parser2.cpp:84`) and stops at the first success (`src/storage/pinyin_parser2.cpp:531-574`).
This is a greedy longest match per key, not a global DP.

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

`pinyin_get_double_pinyin_auxiliary_text` is the same prefix/middle/postfix walk as full pinyin, with two differences (`src/pinyin.cpp:3440-3516`):

- there is no apostrophe handling;
- the middle key is split into shengmu + `|` + yunmu, not raw bytes.

The shengmu and yunmu strings are the `ChewingKey` components, not the original two keystrokes.
With no tone, the cursor-at-boundary form is `|`.
Inside a two-byte key, cursor 1 renders `shengmu|yunmu ` and cursor 2 renders `shengmuyunmu| `.
