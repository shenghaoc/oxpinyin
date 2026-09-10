# The `interpolation2.text` grammar — one reader, and one recorded policy split

Date: 2026-09-10 · Status: **recorded / implemented**
(`crates/oxpinyin-data/src/interp_grammar.rs`) · Upstream tree read:
libpinyin at **`074a2219c90feaf962d0d24f034514033ece5f99`** (2.11.92, the
oracle pin from `tools/oracle/build-oracle.sh`), fetched by commit SHA
into a scratch checkout and verified with `git rev-parse`.

## What this is about

Two readers of the same `interpolation2.text` had drifted in four
observable ways, found during the shared-row-schema refactor
(`f3805133`), which left them alone because it was byte-identity
constrained:

- `crates/oxpinyin-data/src/interp.rs` — `parse_interpolation2`, feeding
  the decoder's `UnigramTable`. Reads only `\1-gram`.
- `crates/oxpinyin-datagen/src/system.rs` — `read_interpolation`, feeding
  the model20 compile. Reads both sections and validates token↔word.

A third reader turned up while measuring: `oxpinyin-counter`'s
`parse_interpolation_dump` (§6). This is an input-text grammar, not an
on-disk row layout, so `oxpinyin_data::row_format` does not cover it.

## Scope: this is not an ABI-compatibility question

`docs/findings/compatibility-policy.md` and its three exception classes
govern *"every exported symbol in the consumer union"* of the drop-in
`libpinyin.so.15`. `interpolation2.text` is on neither side of that
surface:

- Its upstream readers and writers — `utils/storage/import_interpolation`,
  `utils/storage/export_interpolation`,
  `utils/training/k_mixture_model_to_interpolation` — are **build-time
  utility binaries**, not exported library symbols.
- The shipping runtime never opens the file. It reads the *compiled*
  tables (`pinyin_index`, `phrase_index`, `bigram.db`, `punct`); only
  datagen, the emitter, the trainer, the eval harness and test/bench
  staging touch the text export.

So nothing here is a class (a)/(b)/(c) entry, nothing here belongs in
`upstream-divergences.md`, and the register is deliberately not amended.
What does govern it is AGENTS.md's source policy — *external interface
behaviour must be unchanged* — applied to the **format**: the grammar the
file is written in is upstream's, and reading it differently in two
places is a defect regardless of which surface it sits on.

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
pin's `configure.ac` defines neither and `build-oracle.sh` passes no
`CFLAGS` override, so **the pin-built tools abort on a refused line**.

## The measurement

`tag_utility.cpp` was compiled at the pin against GLib in
`debian:testing` and driven through exactly the tag registrations
`parse_unigram` uses. The probe is reproducible from this document: build
`src/storage/tag_utility.cpp` with a `config.h` defining only
`HAVE_MMAP`, stub the two `PhraseItem`/`SubPhraseIndex` symbols
`taglib_token_to_string` references, register `\data`/`\end`/`\1-gram`/
`\2-gram` then `\item` with 2 values and required `count`, and call
`taglib_read` per line. Measured, on the pin:

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

Two upstream crash paths, both null-pointer dereferences rather than
aborts: a blank line (`tag_utility.cpp:183,190` — measured, the probe
segfaults), and a quoted token ending in a backslash, where
`split_line`'s `g_return_val_if_fail` (`:137`) returns `NULL` for the
whole split.

Duplicate and zero counts are settled by the call site rather than the
probe: `import_interpolation.cpp:141` calls
`phrase_index->add_unigram_frequency(token, count)`, which is
`freq += delta; m_total_freq += delta` (`phrase_index.cpp:150-175`) with
the overflow guard written `delta > 0 && …`. So a repeated token **sums**
and a zero count is a **no-op, not an error**.

## The input the pin actually ships

`interpolation2.text` from the pinned `model20.text.tar.gz`
(`MODEL_SHA256=59c68e89…`), SHA-256
`e1578307719431e1b8ef1006ae265bbc9a447250bfde4c1b6e15be80afd3a118`:

| property | measured |
| --- | --- |
| lines | 1,913,520 |
| structural lines | exactly 4 — `\data model interpolation`, `\1-gram`, `\2-gram`, `\end` |
| `\1-gram` items | 63,907, **all exactly 5 fields** |
| `\2-gram` items | 1,849,609, **all exactly 7 fields** |
| blank lines | 0 |
| duplicate `\1-gram` tokens | 0 |
| duplicate `\2-gram` pairs | 0 |
| zero counts | 0 |

**None of the four divergent conditions occurs in the pinned input.** All
four are about inputs no conforming producer emits — and no upstream
producer can emit two of them: `export_interpolation`'s `gen_unigram`
walks tokens ascending once each and skips zero frequencies
(`export_interpolation.cpp:85-95`), and
`k_mixture_model_to_interpolation` skips them too (`:132`).
`oxpinyin-emitter` reproduces both filters (`emit.rs`).

## The four divergences, decided

| # | condition | pin | `data` (was) | `datagen` (was) | correct | disposition |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | duplicate `\1-gram` token | sums | `Err` | sums | `datagen` | **policy split, recorded** (§5) |
| 2 | zero count | accepts | `Err` | accepts | `datagen` | **policy split, recorded** (§5) |
| 3 | whitespace in phrase text | refuses both parities¹ | accepts | rejects | `datagen` | **converged** on the pin's parity rule |
| 4 | unrecognised line | refuses (assert → abort) | ignores² | `Err` | `datagen` | **converged** on refusal |

¹ Two different routes, same outcome from the tool: the odd shape is
refused by `taglib_read` itself; the even shape is *accepted* by the
grammar with the phrase text truncated to its first token, and then
refused by `taglib_validate_token_with_string`
(`import_interpolation.cpp:137-138`), whose `check_result` is the same
assert. `datagen` reproduces this exactly now: the grammar accepts, and
`validate_pair` rejects.

² `data` inspected only lines inside `\1-gram`, so a junk preamble was
silently skipped.

**`datagen` was right on all four and `data` was wrong on all four** —
but note that `datagen` was right on the four while still not being a
faithful port: its fixed 4-field (unigram) / 6-field (bigram) slice
pattern refuses `\item 10 甲 count 5 foo bar`, which the pin accepts. Both
readers now go through the shared module, so both are faithful.

## §5 The one recorded divergence: duplicate tokens and zero counts

Grammar is shared; **policy on a well-formed-but-contradictory record is
not**, and the split is deliberate:

- `oxpinyin-datagen` **is** the port of `import_interpolation`. It sums a
  repeated token and accepts a zero count, because that is what the tool
  it reproduces does.
- `oxpinyin_data::interp` is **not** that tool. It is the decoder's
  loader, and it **refuses both**.

Rationale: no conforming producer of this format can emit either record
(measured above — both upstream writers and oxpinyin's emitter filter
zeros and emit each token once). A file carrying one is corrupt, and
silently summing a corrupt model into the decoder destroys the evidence
that it was corrupt while producing a model no producer intended. The
guard costs nothing on every input that exists.

This is a divergence from a *build tool's* tolerance, not from the
drop-in surface, so it is recorded here rather than in
`upstream-divergences.md` (see the scope section).

## Where oxpinyin deliberately does not follow the pin

Both are constitution item 4 (nothing panics on any input; public APIs
return `Result`), which forbids reproducing an abort:

1. **Refused lines.** The pin-built tools `assert` and abort. Both
   readers return a typed error naming the line.
2. **The two null-pointer dereferences** — blank line, and a quoted token
   with a trailing backslash. `GrammarError::EmptyLine` and
   `GrammarError::UnterminatedEscape`. Rust structurally prevents the
   dereference; this is the shape class (b) describes, recorded here
   only because the surface is out of the register's scope.

One bounded fidelity residue, deliberate:

3. **`g_unichar_isgraph`.** GLib decides "printable and not a space" from
   its own Unicode tables, which also exclude format characters and
   unassigned code points; the port asks only whether the character is
   neither whitespace nor a control character. The two agree on every
   ASCII character and every assigned printable character — the whole of
   the pinned export. They differ only on format/unassigned code points,
   which no producer emits. Reproducing GLib's tables would mean carrying
   a Unicode property table for input that cannot occur.

The `for`-increment that makes a control byte *not* a token boundary
(`tag_utility.cpp:122`) **is** reproduced, and is tested: `a\x01b` is two
tokens `["a", "b"]`, while `a \x01b` is three `["a", "", "b"]`.

## §6 The third reader, reported not changed

`oxpinyin-counter::parse_interpolation_dump`
(`crates/oxpinyin-counter/src/parse.rs:72`) is a fourth divergence on the
same file, on an axis the original four did not name:

- It is **lenient by design** — any line it cannot parse is skipped
  silently (`continue`), including unknown lines and unparseable counts.
- On a duplicate `\1-gram` token it is **last-wins** (`unigrams.insert`),
  which is neither the pin's sum nor `data`'s refusal — a third answer.
- It takes the count from the *last* field and the tokens from fixed
  indices 1 and 3, so it inherits exactly the field-position assumption
  that made `data` wrong on divergence 3.

It is **not changed here**. It feeds the eval/λ path
(`oxpinyin-eval`, `oxpinyin-lambda`), whose inputs are emitter output,
and converging it would move a measured quantity (λ estimates) on a
surface this change has no measurement for. The task that surfaced the
drift named two parsers; this is the third, and closing it is owed work
with its own measurement, not a side effect of this one.

## Verification

Linux, `debian:testing` with the `ci.yml` test-job apt set, tkrzw
backend, toolchain `1.97.1` from `rust-toolchain.toml`:

```
PINYIN_MODEL_DIR=<model20 cache> cargo test -p oxpinyin-datagen -- --include-ignored
```

`fixtures_identity` (`mini_compile_reproduces_the_committed_fixture_directory`)
and `cross_backend` pass, before and after — the committed
`fixtures/w3/<backend>/` are reproduced unchanged, as the input analysis
predicts. `libpinyin_parity` fails identically before and after on the
unset `OXPINYIN_LIBPINYIN_DATA_DIR`, which is the documented
fail-don't-skip behaviour for a missing input
(`docs/testing/README.md`), not a regression.
