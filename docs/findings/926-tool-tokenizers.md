# Tool record tokenizers (#926)

Date: 2026-10-25 UTC. Source baseline: oxpinyin `6be364d9`;
libpinyin checkout `074a2219c90feaf962d0d24f034514033ece5f99`;
ibus-libpinyin checkout `2d2cdac0187101aa0cd7ac06694a8340721ddfbb`;
GLib 2.89.4 checkout `fa41d356ee4936264c45cf11fa6c2640a89fbdda`
(the upstream version of the reference image's Debian 2.89.4-2).
All pin citations below refer to these trees, read before verification.

## Interface promise and complete paths

These are format/value compatibility promises, not a promise that every
command-line option or diagnostic matches, and not Rust API stability.

* **Classic dictionary:** `dictool-format.md` freezes frontend interchange,
  explicit errors, comments/blank lines, CRLF support, and idempotent desired
  counts. It does not promise the frontend's silent skipping or additive
  re-import behavior. The current implementation is safe Rust:
  `dictool/src/import.rs::read_utf8` → `format::parse` → `split_classic` →
  `Record` → `import::run` (full-file preflight, snapshot, count delta) →
  `UserStore::add_phrase_in` → save. Export uses `export::write_rows` /
  `write_row`, phrase rows followed by bigram rows, with zero as two fields.
  The old format document's claim that dictool itself calls C is historical;
  `main.rs` and `import.rs` now use the safe store/facade machinery. The
  changed helper has no C-ABI caller.
* **Segmented stream:** `oxpinyin-counter::run` reads bytes, validates UTF-8,
  loads `PhraseLexicon`, calls `count_ngseg` → `parse_ngseg` →
  `parse_segmented_line`, then `Counter::count` → `Counts::dump` → output.
  Counter, emitter, lambda and their tests use this parser; held-out lambda
  parsing also calls `parse_ngseg` directly. The promise is integer model
  values on the segmenter's output (`counter-port.md`), not identical output
  file representation: the pin writes phrase-index/DBM files and Rust emits
  a count dump. The pin reads LF-delimited records at
  `utils/training/gen_ngram.cpp:77-90`, validates with
  `utils/utils_helper.h:51-69`, updates counts at `:89-117`, and saves the
  phrase index at `:120-121`. `export_interpolation.cpp:75-104,107-142`
  exposes the values compared by the existing counter differential.

No changed function is reachable from an exported C symbol. No new contract
case or driver is needed for this equivalent tool-only refactor. The existing
classic import differential and counter differential remain the comparisons.
The training crate explicitly never ships; neither changed source is in a
shipping cdylib, so release-stage library-size builds do not apply.

## Classic three-field split

The actual frontend source is `src/PYLibPinyin.cc:245-275`, with the split
at **254** (the older format record's line 249 predates this tree).
GLib `glib/gstrfuncs.c:2507-2512,2524-2541` returns no fields for empty
input, otherwise preserves empty fields and the unsplit tail after its
limit. Rust `splitn(3, [' ', '\t'])` gives precisely the old helper's
borrowed fields. Empty input is removed before calling it; Rust's one empty
field there is consequently unobservable. No GLib table/list/allocation
mechanism is needed. At most three borrowed fields use constant auxiliary
space; scanning is O(n), with no per-field string copies (GLib copies O(n)
bytes). The replacement preserves even the old helper's unreachable empty
input result.

| Input | Classification and downstream result |
| --- | --- |
| Two/three fields, SP or TAB, LF/EOF | Intended interchange. Two fields floor the count at 5; explicit counts remain unchanged. Export/re-import is a fixed point under the documented zero-count asymmetry. |
| Repeated or leading SP/TAB | Empty phrase/pinyin; deterministic preflight error, no writes. Never collapse separators with `split_whitespace`. |
| Extra separators after count | Retained in count text, rejected by the existing integer parser. The frontend's `atoi` may accept a numeric prefix: this is the already-frozen dictool validation contract, not a new runtime exception. |
| CRLF; whitespace-only or comment lines | Existing dictool extensions, stripped/skipped before record parsing. |
| NBSP, EM SPACE, line/paragraph separators | Ordinary field content, not separators. A file lacking ASCII separators is rejected; a scalar in the phrase is not rejected just for its category. |
| Backslash/quote, including a final backslash | No escape/quote syntax. Literal phrase characters; a trailing pinyin backslash is ignored by the existing longest-prefix parser. Not intrinsically malformed. |
| Format/unassigned/combining scalars | Accepted phrase characters when scalar length matches parsed keys. No `isgraph` call on this path; do not import interpolation's category restriction. |
| Invalid UTF-8, NUL, CR inside phrase, too many scalars, duplicate reading | Existing explicit preflight errors; no partially imported file. |

## Segmented first separator and numeric prefix

`split_once([' ', '\t'])` expresses the two-part split directly, retaining
an empty first field or empty/arbitrarily long phrase tail. Empty lines are
handled separately as zero. The decimal-prefix helper now borrows the ASCII
prefix for `str::parse`, eliminating its temporary `String`, without changing
any result. Both implementations are O(n); the prefix now needs O(1) extra
space instead of O(n). The name `atoi` does not establish full C equivalence.

| Input | Source-derived classification |
| --- | --- |
| Ordinary unsigned token plus its exact phrase | Intended format. Nonzero token increments its unigram and previous/current bigram; zero resets the sentence boundary (`gen_ngram.cpp:82-117`). |
| Empty file / empty LF record / final LF | Respectively no tokens / zero / no extra record, matching `std::getline`. A CR-only record is not empty. |
| Missing SP/TAB, including a Unicode-space-only separator | Pin aborts at `utils_helper.h:58-59`; Rust returns `MalformedLine`. Safe error handling is necessary, not a reason to widen the separator set. |
| `0 `, `0  text`, `0\t\ttext`, arbitrary Unicode/quotes/backslashes after zero | Valid: zero bypasses phrase validation (`:63`). The whole tail is retained; there is no escape grammar. |
| Leading SP/TAB or a nondigit prefix | Empty/nonnumeric first field becomes zero. Phrase validation is bypassed. Do not classify every repeated separator as malformed. |
| Unsigned in-range numeric prefix followed by nondigits | Decimal prefix is consumed; the remaining first-field bytes do not affect the token. A matching phrase remains required at the pin. |
| Nonzero token with extra leading phrase separator, wrong/empty phrase, or CRLF tail | Pin compares exact phrase bytes at `src/storage/tag_utility.cpp:407-416` and asserts on mismatch. Rust currently discards the phrase column. Existing defect/follow-up, **not** an accepted quirk or verified equivalence. |
| Signed or ASCII-whitespace-prefixed first field | Pin calls `atoi`; Rust's digits-only helper yields zero. The nonzero signed-token case is an existing defect/follow-up; a normal Rust parser can preserve it. No exception is admitted. |
| Numeric overflow, embedded NUL, invalid UTF-8 | Outside ordinary producer output; not certified by this refactor. Rust over-u32 prefixes currently become zero and invalid UTF-8 is refused at the reader. Pin C-string truncation and integer narrowing need their own bounded caller/output check before any change. |

The safe refactor does not fix the existing phrase-validation/numeric-domain
holes, nor does it classify them as necessary differences. They remain
follow-ups, including the held-out caller that has no lexicon parameter.
Reading glibc 2.43-3 from sources.debian.org was blocked by HTTP 403; no
alternative route was attempted after that refusal. No overflow result was
inferred by executing the pin. Existing ordinary value tests cannot certify
these partitions.

## Other split/category sites feeding tool formats

The inventory of `g_strsplit*` and `g_unichar_*` in the pin's `src/` and
`utils/` has six active calls, all distinct from the frontend classic reader:

* `src/storage/tag_utility.cpp:94-95`: split required/ignored **registration**
  strings on comma/colon. These are tag schema lists, not input field splits.
* `:127,152`: `g_unichar_isspace` and `g_unichar_isgraph` in `split_line`,
  feeding `taglib_read` at `:171-264`. Registered interpolation/k-mixture
  readers consume positional fields plus paired tag/value tails; unknown
  keys skip their pair, missing required values refuse the line.
* `utils/utils_helper.h:57`: the segmented macro above, also consumed by
  `gen_deleted_ngram.cpp:81`, `gen_k_mixture_model.cpp:75`,
  `eval_correction_rate.cpp:178`, and `segment/mergeseq.cpp:157`.
* `utils/storage/gen_zhuyin_table.cpp:148-162`: tsi.src splits on **space
  only**, max three; phrase/frequency/pinyin feed `feed_line`. Its code
  indexes/atoi-converts before checking length. This is not the classic
  format or the four-field model table grammar.

`storage/table_text_parser.h:34-120` instead uses `strspn`/`strcspn`, bounded
copies and complete `g_ascii_strtoull` conversions for four-field pinyin or
punctuation records; repeated ASCII separators are consumed there. Its
whole datagen audit is separate from these two helpers.

GLib 2.89.4 `glib/guniprop.c:185-193` excludes Cc/Cf/Cn/Cs/Zs from graph;
`:278-296` recognizes TAB/LF/CR/FF plus Zs/Zl/Zp as space. In particular VT
and NEL differ from Rust `char::is_whitespace`, and Zl/Zp are graph when
encountered inside an already-started unquoted run, despite being skipped
at its start. These are classification facts, not permission to alter the
existing approved grammar. `interp_grammar.rs::is_graph` remains untouched.

Quoted escapes at pin `tag_utility.cpp:129-147` retain the backslash bytes;
a dangling quoted backslash returns NULL at `:137`, then the caller uses
`tokens[0]` at `:183`. Rust returns `UnterminatedEscape`. Unquoted final
backslashes are literal graph characters. The prior
[interpolation record](interpolation2-grammar.md) owns this safe refusal and
the bounded format/unassigned difference. Neither is transferred to the
classic or segmented formats, and no new divergence is registered here.

## Verification and reproduction

BDB, Linux amd64, 2026-10-25 UTC: 128 native tests passed (12 external-input
checks ignored by the normal suite); fmt and clippy passed. The existing
counter differential's two explicitly selected tests then passed against the
pin: **138096 unigrams / 199 bigrams**, checksum `1f7af24b04577d8b` for the
ordinary fixture, with all values also identical for the classified variant.
`run-import-diff.sh` reported **IDENTICAL**, including classic frontend
interchange in both directions. The reference image supplied GLib 2.89.4-2
and glibc 2.43-3. No logs or captures are deliverables.

After [cloud setup](../runbooks/cloud.md), use the repository toolchain:

```sh
just heavy -- cargo test --locked -p oxpinyin-dictool -p oxpinyin-training
just fmt --all --check
just heavy -- cargo clippy --locked -p oxpinyin-dictool -p oxpinyin-training --all-targets -- -D warnings
```

For the existing live counter test, build the pin with
`tools/oracle/build-oracle.sh --dbm bdb --work-dir "$pin_build" --prefix "$pin_prefix" --model-dir "$PINYIN_MODEL_DIR"`
in the reference environment with the documented frontend prerequisites
(including `libibus-1.0-dev` and the `sqlite3` executable). Both build/prefix
paths must be task-owned and the initial prefix empty. The provisioned BDB
prefix remains the separately hash-verified reference. Then set:

```sh
pin_src="$pin_build/src/libpinyin-074a2219c90feaf962d0d24f034514033ece5f99"
export PINYIN_EXPORT_DIR="$OXPINYIN_LIBPINYIN_DATA_DIR"
export PINYIN_GEN_NGRAM_DATA="$pin_src/data"
export PINYIN_GEN_BINARY_FILES="$pin_prefix/bin/gen_binary_files"
export PINYIN_GEN_UNIGRAM="$pin_prefix/bin/gen_unigram"
export PINYIN_GEN_NGRAM="$pin_src/utils/training/gen_ngram"
export PINYIN_EXPORT_INTERPOLATION="$pin_src/utils/storage/export_interpolation"
just heavy -- cargo test --locked -p oxpinyin-training --test counter-differential -- --include-ignored --nocapture
export PINYIN_IBUS_BUILD_DIR="$pin_build" PINYIN_IBUS_PREFIX="$pin_prefix"
export IMPORT_SYSTEM="$OXPINYIN_LIBPINYIN_DATA_DIR"
just heavy -- bash tools/bisection/run-import-diff.sh
```

The native Rust test binary can also run inside the reference image, with
these same paths mounted and environment variables, as it did in this run;
the pin-built tools must run with the reference runtime libraries. Explicit
`OXPINYIN_CAPI_SO` and `OXPINYIN_DICTOOL` select prebuilt debug artifacts for
the import runner. These are existing drivers, not session-only test code.

Named mutation **collapse-classic-separators**: replace
`line.splitn(3, [' ', '\t']).collect()` with
`line.split_whitespace().take(3).collect()` and run
`just heavy -- cargo test --locked -p oxpinyin-dictool split_classic_matches_g_strsplit_set_max_three`.
It failed on the repeated-separator case (exit 101); restoring the expression
made the same test pass. No mutation remains in the branch.

`tools/bisection/cells-for-diff.py origin/main` reports
**Cells: bdb (the reference cell; no check fired)**; all three backend checks
are not fired. `interp_grammar.rs`, all #1190 paths and other ledger families
are unchanged. No new necessary divergence was found, so
`upstream-divergences.md` is unchanged.
