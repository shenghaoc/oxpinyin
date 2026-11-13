# The `interpolation2.text` grammar: one taglib port

Status: **reference**

Pin lines are at libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`
(2.11.92; `tools/oracle/oracle-pin.txt`).

## What this is about

`interpolation2.text` is the model's tagged text form. Upstream reads it
with `taglib_read` (`src/storage/tag_utility.cpp`). oxpinyin has one port
of that tagger, `oxpinyin_utils::taglib`
(`crates/oxpinyin-utils/src/utils_helper/taglib.rs`). These readers use it:

- `oxpinyin-utils` `import_interpolation`
  (`crates/oxpinyin-utils/src/storage/import_interpolation.rs`);
- `oxpinyin-utils` `k_mixture_model_to_interpolation`;
- the k-mixture-model tools in `oxpinyin-utils` `k_mixture_model`.

The tagger's character steps and warnings are GLib's, through the tool
boundary (`oxpinyin_input::tool`), including `g_unichar_isgraph`
([external I/O boundary](external-io-boundary.md)). This is an input-text
grammar, not an on-disk row layout, so `oxpinyin_data::row_format` does
not cover it.

## Scope: build-time tools, not the runtime

The shipping runtime reads compiled tables, never `interpolation2.text`.
The tagged text belongs to the pin's build-time import, export and KMM utilities.

oxpinyin's utilities reproduce the pin's tools under their own names, so
they follow the pin's grammar and its failure behaviour
([failure behaviour](compatibility-policy.md#failure-behaviour)).

## The authority: one function, not a field split

Upstream does not parse this file as a whitespace-separated table. Every
line goes through `pinyin::taglib_read`
(`src/storage/tag_utility.cpp:171-264`), against five tags that
`import_interpolation` registers:

| tag | `m_num_of_values` | required | registered at |
| --- | --- | --- | --- |
| `\data` | 0 | `model` | `import_interpolation.cpp:73` |
| `\end` | 0 | — | `:96` |
| `\1-gram` | 0 | — | `:97` |
| `\2-gram` | 0 | — | `:98` |
| `\item` (in `\1-gram`) | 2 | `count` | `:128` |
| `\item` (in `\2-gram`) | 4 | `count` | `:163` |

A line is `<tag> <value>… <key> <value> <key> <value>…`. After the
positional values, the tail is walked **strictly in pairs**, and a key
that is not a registered tag is warned over and *its pair skipped anyway*
(`tag_utility.cpp:238-242`). The walk therefore only ever inspects every
second token, and acceptance turns on **parity**, not on field count.

`taglib_read` reports refusal by returning `false`, and every caller
wraps it in `check_result` (`src/include/pinyin_utils.h:26-31`), which is
`assert(expr)` unless `NDEBUG` or `G_DISABLE_ASSERT` is defined. The
pin's `configure.ac` defines neither and `tools/oracle/build.py` passes no
`CFLAGS` override, so **the pin-built tools abort on a refused line**.

## The pin's answers

`taglib_read` at the pin, under the tag registrations that
`parse_unigram` uses (`\data`, `\end`, `\1-gram`, `\2-gram`, then
`\item` with 2 values and the required `count`), answers:

| line | `taglib_read` | result |
| --- | --- | --- |
| `\item 10 甲 count 5` | `true` | token 10, word 甲, count 5 |
| `\item 10 甲 count 0` | `true` | count **0 is accepted** |
| `\item 10 甲 乙 count 5` | **`false`** | 1 token between → odd |
| `\item 10 a b c count 5` | `true` | word is **`a`**; `b`/`c` dropped |
| `\item 10 x y z w count 5` | **`false`** | 3 between → odd |
| `\item 10 甲 count 5 foo bar` | `true` | count 5; junk dropped |
| `\item 10 甲 count 5 foo` | `true` | count 5; junk dropped |
| `\item 10 甲 count` | **`false`** | required key with no value |
| `\item 10 甲` | **`false`** | `count` never seen |
| `\item 10` | **`false`** | positional value 1 missing |
| `\frobnicate 1 2` / `hello world` / `item 10 x count 5` | **`false`** | no tag matches |
| `\end trailing junk` | `true` | zero required tags |
| `\data   model    interpolation` | `true` | model = `interpolation` |
| `\data` | **`false`** | required `model` missing |
| `\item 10 "a b" count 5` | `true` | quoting makes one token |
| *(blank line)* | **SIGSEGV** | `tokens[0]` is `NULL`, handed to `strcmp` |

Two upstream crash paths are null-pointer dereferences, not aborts: a
blank line (`tag_utility.cpp:183,190`), and a quoted token ending in a
backslash, where
`split_line`'s `g_return_val_if_fail` (`:137`) returns `NULL` for the
whole split.

Duplicate and zero counts are settled by the call site:
`import_interpolation.cpp:141` calls
`phrase_index->add_unigram_frequency(token, count)`, which is
`freq += delta; m_total_freq += delta` (`phrase_index.cpp:150-175`) with
the overflow guard written `delta > 0 && …`. So a repeated token **sums**
and a zero count is a **no-op, not an error**.

## The input the pin ships

The pinned model's `interpolation2.text` (SHA-256
`e1578307719431e1b8ef1006ae265bbc9a447250bfde4c1b6e15be80afd3a118`) has
four structural lines, `\1-gram` items of exactly 5 fields and `\2-gram`
items of exactly 7 fields. It has no blank line, no duplicate token or
pair and no zero count, so none of the divergent conditions above occurs
in it. No upstream producer emits a duplicate or a zero count:
`export_interpolation` walks tokens ascending once each and skips zero
frequencies (`export_interpolation.cpp:85-95`), and
`k_mixture_model_to_interpolation` skips them too (`:132`).
`oxpinyin-utils`' ports of both tools reproduce these filters.

## How oxpinyin's readers answer

- **Accepted lines**: the port accepts exactly what `taglib_read`
  accepts, by the same parity walk. A duplicate `\1-gram` token sums
  and a zero count is a no-op in `import_interpolation`, as
  `add_unigram_frequency` does.
- **Refused lines**: `check_result` is the pin's `assert`, and the
  utilities abort as the pin-built tools do. A phrase-text mismatch that
  `taglib_validate_token_with_string` refuses aborts the same way.
- **The two null-pointer dereferences** (a blank line, and a quoted
  token with a trailing backslash) and a truncated UTF-8 sequence at the
  line's end are undefined in the pin. The tagger answers them as
  `taglib::Undefined` (`NoToken`, `UnterminatedEscape`, `PastLineEnd`),
  and the utilities refuse them (class (b)).
- **A missing or wrong `\data` header**: `import_interpolation` exits
  `ENODATA`, as the pin's `parse_headline` failure does.

The `split_line` tests in `taglib.rs` cover quotes, escapes, empty tokens,
a character that is neither space nor graph (U+200B), and the undefined
reads.

## The evaluation dump reader

`parse_interpolation_dump`
(`crates/oxpinyin-utils/src/training/eval/counts.rs`) is a separate, lenient
reader for `export_interpolation` dumps that feed the correction-rate
evaluator. It skips any line it cannot parse, keeps the last count on a
duplicate token, and reads the tokens from fixed field positions. Its
inputs are `export_interpolation` output, which never carries those
shapes. It does not go through the tagger.
