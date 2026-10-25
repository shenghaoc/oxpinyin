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
| Numeric overflow, embedded NUL, invalid UTF-8 | Source-classified below, including the amd64 reference's numeric narrowing. Existing compatibility gaps remain; ordinary producer output does not exempt these inputs. |

The safe refactor does not fix the existing phrase-validation/numeric-domain
holes, nor does it classify them as necessary differences. They remain
follow-ups, including the held-out caller that has no lexicon parameter.
Following explicit authorization to use GitHub mirrors, the bminor/glibc
clone succeeded, but fetching `glibc-2.43` failed: that remote tag is absent
(confirmed with `git ls-remote --tags origin refs/tags/glibc-2.43`). The
provisioned image has only a `deb` snapshot entry, no `deb-src`, and no local
implementation source. Exact upstream/debian-patchset source inspection
remains open; no other version is substituted. Follow-through below uses
the already-installed reference header and compiled objects.
No input was executed against the pin to discover these expectations.

## Remaining input partitions: source follow-through

2026-10-25 UTC; this extends the bounded source classification, **not**
the implementation or verification of the known differences. The PR remains
partial work on #926. No parser implementation or `is_graph` changes follow
from this section.

The GLib GitHub checkout is tag **2.89.4**, with splitter source at
`glib/gstrfuncs.c:2489-2551`. For the Debian **2.89.4-2** revision, reading
installed `g_strsplit_set` (address `0x8dfa0`) confirms the relevant empty
input branch (`+0x54..+0x59`, `+0x1c8`), byte/NUL loop and token limit
(`+0xc0..+0x10e`), and retained final field (`+0x110..+0x121`). These agree
with the tagged source for this tokenizer; this is not a claim that the
entire Debian patchset was inspected. The libc header and machine-code
checks below likewise use the actual Debian **2.43-3** revision.

### Nonzero token and phrase validation

At `utils/utils_helper.h:61-67`, zero bypasses validation. Every other token
goes through `src/storage/tag_utility.cpp:384-416`: library-zero token 1 maps
to `<start>` (`:362-393`); an ordinary resident token maps to its full UTF-8
phrase (`:395-404`). `strcmp` at `:413` compares that exact byte string with
the entire second field. It does not trim separators/CR, normalize Unicode,
or interpret escapes. Thus repeated separators, an empty/wrong phrase and
a CRLF tail fail for a resident token unless those bytes actually belong to
its stored phrase. These are context-dependent validation failures, not
uniformly malformed separator syntax. A mismatch reaches `assert` through
`utils_helper.h:64-65` and `include/pinyin_utils.h:27-30`, before that line's
count updates. An unknown token instead returns NULL (`tag_utility.cpp:380-381`
or `:395-398`) and passes it to `strcmp`: an unsafe read, not a defined
validation-false result. The bounded conclusions assume sound scalar phrase
items; corrupt stored items belong to their existing data-path audits.

Rust `parse_segmented_line` discards the second field. `count_ngseg` has a
lexicon but uses it only to seed counts; the held-out `count_deleted` caller
has no lexicon parameter. Therefore a resident token with the wrong phrase
still increases its Rust count. Counter's dump exposes the numeric increase;
the emitter resolves the *canonical lexicon phrase*, hiding the bad supplied
phrase while retaining the increase (`emitter/emit.rs::phrase_text` /
`emit_interpolation2`). Lambda uses those counts in estimation. Unknown-token
counts remain in the counter dump; the emitter skips unresolved phrase rows,
which does not make the reader compatible.

Classification: mismatch-to-typed-error is the safe refusal corresponding to
the pin assertion; NULL lookup-to-typed-error avoids the unsafe read. Silently
counting either case is an existing defect, not a necessary Rust difference.
Normal Rust lookup plus comparison can implement validation; the held-out
caller would need its lexicon passed explicitly. No such implementation is
included here.

### Signed prefixes and overflow: installed reference evidence

No `atoi.c` or `strtol_l.c` was found in the checked local source/cache paths
or the reference image's `/usr/src` and `/usr/share`. However, its installed
libc6-dev **2.43-3** `/usr/include/stdlib.h:483-488` defines `atoi` as
`(int) strtol(nptr, NULL, 10)`. The compiled pin `gen_ngram` confirms the
actual call, rather than assuming the header was used: `main+0x1c9`
(`0x29679`) calls `__isoc23_strtol`; `main+0x1d1` tests `%eax` for zero and
`main+0x1e2` passes `%eax` as the unsigned token to validation. Only the low
32 bits of the returned long reach the token/count loop.

The installed amd64 libc6 **2.43-3** disassembly settles the remaining
conversion at `__isoc23_strtol` (symbol address `0x504a0`): locale ctype-space
skipping at `+0x78..+0xb1`, minus/plus recognition at `+0xca..+0xe4`, base-10
digit accumulation/cutoff at `+0x148..+0x1af`, no-digit zero at `+0x3c0`
branching to `+0x61`, and signed-long overflow saturation at
`+0x1e6..+0x1fe`, `+0x320..+0x351`, `+0x358..+0x382`. The latter returns
`LONG_MAX` or `LONG_MIN` and sets ERANGE; the macro never reads errno.
This is a binary reading, not an overflow probe. Conclusions are bounded to
the provisioned amd64 reference; no arm64 conversion claim is made.

For C-locale ASCII fields, trim the leading C whitespace, consume one
optional sign and decimal digits, saturate to signed 64-bit, then retain
the low 32 bits. SP/TAB at the start of the *line* have already created an
empty token field, so they yield zero instead; CR/VT/FF can survive inside
the first field and be consumed by the numeric parser. Examples derived
before verification:

| First field | Pin token | Existing Rust token / consequence |
| --- | --- | --- |
| `+16817937`, `\r16817937` | 16817937; exact phrase still required | 0, loses this token's count and resets the boundary |
| `+0`, `-0`, `+`, `-` | 0 | 0; no-digit and signed-zero controls |
| `-1` | 4294967295 | 0; pin lookup outcome depends on that token, not on its sign |
| `4311785233` (2^32 + 16817937) | 16817937 | 0; an ordinary resident-token count can differ |
| `4294967296` | 0 | 0; wrapping to zero is not proof of general overflow parity |
| Beyond positive signed-long range | 4294967295 | 0 |
| Beyond negative signed-long range | 0 | 0 |

Classification: the reference's deterministic result can be represented
with checked integer parsing and explicit conversion in ordinary safe Rust.
No overflow or sign difference is admitted merely because C's abstract
`atoi` contract has a narrower defined domain. The remaining limitation is
implementation/verification, not an unavailable reference conversion rule.

Classic counts use the same frontend `atoi` (`PYLibPinyin.cc:262-267`),
but then reach signed `pinyin_iterator_add_phrase`. Pin
`pinyin.cpp:520-524` maps -1 to default 5; other counts become unsigned
pronunciation deltas at `:578-579,602`. A new item's unigram delta is three
times the count (`:604-605`); the reference `_add_phrase+0x37d`
(`0x8f79d`) computes that with a 32-bit `lea`, so its low 32 bits are used.
`PhraseItem::add_pronunciation` adds an existing reading with its overflow
guard (`phrase_index.cpp:56-92`); the caller ignores its bool before restoring
the item. Dictool instead accepts complete unsigned counts through i32::MAX,
including Rust's `+7`, and rejects `-0`, `-1`, suffixes and larger values
before any writes. This is the existing explicitly frozen tool-validation
contract, not a newly justified runtime exception; changing that contract
is not part of this equivalent refactor. Its numeric conversion is fully
representable in safe Rust.

### NUL and invalid UTF-8, through model output

`LineString` converts to `std::string::c_str()`
(`include/pinyin_utils.h:33-38`), so the segmented macro sees only bytes
before the first NUL even though `getline` consumed the whole physical line.
GLib's splitter is byte-based (`gstrfuncs.c:2507-2541`) and does not validate
UTF-8. For a sound scalar phrase index:

| Location | Pin result | Rust result and classification |
| --- | --- | --- |
| NUL is first byte | Empty C string → zero | No truncation; may return an error or parse a later separator. Existing gap; safe byte slicing suffices. |
| NUL before the only delimiter, after digits | One field → abort before this line's update | May split after NUL and consume the preceding digits. Existing gap; safe refusal required. |
| NUL after a complete matching nonzero phrase | Hidden suffix ignored; token counted | Token counted because phrase is discarded. Value-equivalent control, not full phrase-validation parity. |
| NUL truncates a nonzero phrase before it matches | Exact comparison fails → assert | Token counted. Existing validation defect. |
| NUL or arbitrary bytes in zero-token tail | Tail ignored; boundary zero | Scalar NUL tails count as zero. Invalid UTF-8 instead fails the whole Rust file before counting. Safe byte processing could reproduce the pin; not a necessary difference. |
| Invalid UTF-8 in nonzero phrase before NUL | Cannot match the sound scalar phrase's UTF-8 → assert | Reader rejects before parsing; safe refusal, with different streaming side effects below. |
| Invalid byte after a valid decimal prefix in token field, followed by a correct phrase | Conversion stops at that byte; exact phrase matches and token counts | Whole-file UTF-8 rejection. Existing byte-reader gap, not inherently malformed token syntax. |
| Invalid bytes after first NUL | Never seen by macro | Whole-file UTF-8 rejection. Existing gap, safely reproducible with byte input. |

The counter and emitter validate the entire file with `str::from_utf8`
before counting and call `write_output` only after successful counting.
Lambda validates both its system file and held-out file before printing
estimates. An invalid byte therefore leaves an existing output file untouched,
even after preceding valid lines. Pin `gen_ngram.cpp:79-117` processes records
incrementally, calls `bigram.store` after each accepted nonzero token, and
saves phrase frequencies only on normal completion (`:120-121`);
`gen_deleted_ngram.cpp:78-118` likewise stores bigrams incrementally.
There is no rollback call on a later assertion. Exact durable DBM bytes after
abnormal termination are **not settled** by this tokenizer trace: libdb's
dirty-page flush/abort behavior would need backend source evidence. This is
the remaining semantic boundary for multi-line failing files; neither a
clean rollback nor preservation of all previous writes is claimed.

For classic input, frontend `getline` followed by `strlen` and
`g_strsplit_set` (`PYLibPinyin.cc:245-267`) similarly truncates each line at
NUL. A NUL before enough fields skips the line; one after a valid two-field
prefix can import with default 5, and one after a count imports that count.
There is no whole-file UTF-8 check. For malformed/incomplete UTF-8 in the
phrase, GLib `gutf8.c:940-996` returns NULL without setting `items_written`;
the pin initialized `phrase_length` to zero and refuses the row at
`pinyin.cpp:627-644`. It still saves accepted preceding/later rows at frontend
`:272-275`. Invalid suffix bytes in pinyin can leave a valid longest parsed
prefix (`pinyin_parser2.cpp:164-213,217-333,346-379`); invalid count suffix
bytes can leave a valid decimal prefix. Neither is generically rejected by
the frontend. GLib-accepted surrogate/extended encodings are distinct from
malformed UTF-8 (`gutf8.c:665-746`); their stored-item consequences overlap
the existing non-scalar audit, not a new #926 exception.

Dictool's `read_utf8` instead rejects an invalid byte anywhere, including
comments or after NUL, before opening the store. Its parser rejects NUL in
record fields (comments can contain scalar NUL). The frozen validation and
full-file preflight contract explains these existing choices; it does not
make them impossible to reproduce differently in Rust. Tests below verify
that contract's no-write boundary, not universal frontend byte-input parity.

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

BDB, Linux amd64, 2026-10-25 UTC: 132 native tests passed (12 external-input
checks ignored by the normal suite); fmt and clippy passed. The existing
counter differential's four tests, including both external-input checks,
then passed against the pin: **138096 unigrams / 199 bigrams**, checksum `1f7af24b04577d8b` for the
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

Named mutation **segmented-space-only**: replace the parser's
`split_once([' ', '\t'])` with `split_once(' ')`, rebuild the existing
`counter-differential` test and run `rust_matches_live_gen_ngram` with
`--include-ignored --exact`. The classified TAB stream failed (exit 101).
Restoring the expression and rerunning all four integration tests passed,
including signed-zero and NUL-tail controls. Invalid-byte CLI tests cover
dictool preflight, counter/emitter output preservation, and both lambda
streams; they characterize current rejection boundaries, not pin parity
for the open byte-reader and phrase-validation differences.

`tools/bisection/cells-for-diff.py origin/main` reports
**Cells: bdb (the reference cell; no check fired)**; all three backend checks
are not fired. `interp_grammar.rs`, all #1190 paths and other ledger families
are unchanged. No new necessary divergence was found, so
`upstream-divergences.md` is unchanged.
