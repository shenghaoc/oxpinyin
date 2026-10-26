# Tool record tokenizers (#926)

**Amendment 2026-10-26 UTC (#1297 Phase 2).** The #1297 scanner/tools lane supersedes the numeric-prefix and nonfinite-format reproduction conclusions in this dated audit. [Complete decimal fields](1297-tool-decimals.md) now produce line errors for malformed or out-of-range fields; [native special values](1297-tool-special-values.md) keep finite six-decimal output and Rust’s NaN spelling. All nonnumeric tokenization and domain validation rules in this record remain. The existing live counter/KMM/empty-input differentials keep their source-derived malformed fixtures and distinguish POLICY-ACCEPTED from valid-decimal MATCH controls.

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
and rejects `+7`, `-0`, `-1`, suffixes and larger values
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

## Continuation: segmented byte records and phrase validation

2026-10-25 UTC, based on main `27c785c0`. This section supersedes the
training parser's validation and classic NUL descriptions above for this change;
the earlier audit remains the source of the open numeric and classic cases.
The reference image is still the verified GLib 2.89.4-2/glibc 2.43-3 image.
The requested glibc 2.41 source work waits for the trixie reference-image
change; no further 2.43 fetch is attempted.

The implementation now reads physical LF-delimited byte records and borrows
the prefix before NUL, using ordinary Rust slice operations. Pin
`include/pinyin_utils.h:33-38`, `utils/utils_helper.h:51-69` and GLib tag
2.89.4 `glib/gstrfuncs.c:2507-2541` establish this boundary before decoding.
An empty visible prefix is token zero; a nonempty prefix needs one ASCII
space/TAB delimiter. Zero ignores all remaining bytes. The existing unsigned
decimal-prefix domain is retained, including stopping before an invalid
UTF-8 suffix; signed/overflow behavior is not claimed fixed.

For nonzero tokens, `src/storage/tag_utility.cpp:362-416` requires the exact
stored phrase (or `<start>` for token 1). The comparison also stops at an
embedded NUL in the stored text, as `strcmp` does at `:413`. A missing token
or mismatched phrase now returns a line-numbered `InvalidPhrase` instead of
silently increasing its count. This safe refusal avoids the pin's assertion
or NULL read; it does not emulate their mechanisms. Non-scalar stored phrase
loading remains with #1190 and is not modified here.

`counter::count_ngseg` and `lambda::count_deleted` both require the lexicon.
Counter, emitter and lambda now pass file bytes directly. Eval also reads
its held-out stream as bytes. Train reads held-out bytes, validates them once,
and shares the records between K-mixture scoring and deleted counts.
Classic parsing now also consumes bytes and truncates each physical line
before validating UTF-8, matching frontend `PYLibPinyin.cc:245-254`. Its
whole-file preflight and desired-count behavior are retained. A NUL after
the pinyin or count now imports the visible two/three-field record, including
when hidden bytes are invalid UTF-8; NUL at the start gives a blank record.
Unit and CLI tests compare exported counts to the ordinary input. Visible
invalid phrase bytes still produce a typed preflight error rather than the
frontend's silent skip. Invalid pinyin suffix bytes now retain the longest
valid prefix; numeric suffix handling awaits the glibc trace.

Accepted records still feed the same unigram/bigram algorithms and output
writers, with no new selection semantics or model coefficients. Work remains
linear in input bytes with borrowed fields, expected constant-time token
lookup, and the existing token-vector storage. No shipping library changes.

The existing BDB differential verifies ordinary values and accepted byte
variants, then verifies refusal for a known token with a wrong phrase,
repeated separator, CR tail, truncated phrase, or NUL before its delimiter.
Expectations come from the cited source before these executions. Native
checks cover arbitrary bytes in zero tails and after NUL, invalid bytes in
numeric suffixes, exact phrase validation, both count consumers, and CLI
output preservation on refusal. The existing driver also checks eval/train
reject a mismatched held-out phrase before producing output.

### Open partitions and held edits

* Signed numbers, long-range saturation/narrowing and numeric-domain changes:
  traced against glibc 2.41 source and reproduced in
  [Numeric conversion](#numeric-conversion-glibc-241) below. The old amd64
  disassembly findings are historical evidence only.
* **Resolved:** dictool keeps its own strict CLI specification
  (`dictool-format.md` §2), including preflight refusal and desired-floor
  counts. NUL and pinyin-prefix fixes remain. Frontend skipping is not the
  C ABI and does not replace dictool's error contract. Numeric investigation
  remains deferred as instructed.
* For a later bad record, exact durable pin DBM state after its assertion
  remains open. `gen_ngram.cpp:79-121` and `gen_deleted_ngram.cpp:78-118`
  establish incremental store calls but not libdb's dirty-page durability.
  These tools emit count text only on success; neither rollback nor prior
  durable writes are inferred from the refusal comparisons.
* The full #1190 overlap was checked before editing. Relevant prohibited
  files include `dictool/src/export.rs`, training `segment/lexicon.rs` and
  `eval/phrases.rs`, both shared ledgers (`glib-libc-verification.md` and
  `upstream-divergences.md`), and `contract-diff.py` / its test. The maintainer
  subsequently authorized own-family shared-ledger/driver edits; only G16
  is updated. All overlapping crate files remain untouched. No contract case
  is needed for these tool-only functions, and no replacement driver is
  introduced.

The continuation remains **Refs #926**, not completion of all partitions.
`interp_grammar.rs::is_graph` is unchanged.

### Continuation gates and reproduction

BDB Linux amd64: **136 native tests passed**, 13 external-input checks ignored
by the ordinary suite; fmt and clippy passed. All five counter integration
checks passed with external inputs enabled. Ordinary and classified byte
streams retained **138096 unigram / 199 bigram** values and the ordinary
checksum `1f7af24b04577d8b`; known-token failure cases were refused on both
sides. The existing import differential reported **IDENTICAL** for ordinary
and NUL-hidden-invalid-byte fixtures, including classic interchange in both
directions. No raw outputs are published.

Use the crate/fmt/clippy and BDB counter commands in the earlier reproduction
section. The existing import runner now accepts `IMPORT_FIXTURE` (an absolute
path) to use the same frontend harness with a classified input. After the
ordinary run, create the NUL variant and rerun:

```sh
uv run --python 3.15 python -c 'from pathlib import Path; p = Path("tools/bisection/classic-import.txt"); Path("/tmp/classic-nul.txt").write_bytes(b"".join(line + b"\0\xffhidden\n" for line in p.read_bytes().splitlines()))'
IMPORT_FIXTURE=/tmp/classic-nul.txt just heavy -- bash tools/bisection/run-import-diff.sh
```

When running inside the reference image, mount the input path as well as the
checkout and configured prefixes. The ordinary fixture is unchanged.

Each changed production function has a named mutation below. Every mutation
compiled and failed its selected test (exit 101); each was restored, then the
full native suite passed. Apply one replacement at a time and use
`just heavy -- cargo test --locked -p <crate> <test>`; CLI checks are in
`--test counter-differential` (training) or `--test cli` (dictool). The
eval/train test additionally needs the BDB export and `-- --include-ignored`.
The old `read_utf8` helper was removed; the importer mutation covers its
replacement at the file-reader boundary. Test/fixture setup functions and
removed match arms add no independent production behavior.

| Function | Named mutation | Selected test |
| --- | --- | --- |
| `counter::parse_ngseg` | `keep-record-lf`: retain terminal LF | `parse_ngseg_reproduces_segmented_lines` |
| `parse_segmented_line` | `skip-phrase-validation`: disable nonzero validation | `byte_records_validate_exact_phrase_and_nul_boundaries` |
| `atoi` | `decimal-radix-eleven`: multiply accumulated digits by 11 | `atoi_matches_c_for_token_fields` |
| `count_ngseg` | `counter-drop-boundary`: force pi-gram training off | `validated_streams_feed_both_model_consumers` |
| `count_deleted` | `held-out-drop-boundary`: force pi-gram training off | same |
| `CounterError::fmt` | `phrase-diagnostic-erased`: replace phrase diagnostic | `byte_records_validate_exact_phrase_and_nul_boundaries` |
| `CounterError::source` | `phrase-error-source-cycle`: return self as source | same |
| counter `run` | `counter-whole-file-utf8`: validate all bytes before counting | `counter_and_emitter_accept_ignored_bytes_and_refuse_wrong_phrases` |
| emitter `run` | `emitter-whole-file-utf8`: same replacement | same |
| lambda `run` | `lambda-whole-file-utf8`: validate whole system stream | `lambda_validates_both_byte_streams_before_output` |
| eval `run` | `eval-discard-held-out`: replace held-out bytes with zero record | `eval_and_train_refuse_mismatched_held_out_phrases` |
| train `run` | `train-discard-held-out`: same replacement | same |
| dictool `format::parse` | `classic-keep-nul-tail`: retain bytes after NUL | `nul_terminates_classic_records_before_utf8_validation` |
| dictool import `run` | `classic-import-whole-file-utf8`: decode whole input first | `classic_nul_tails_preserve_imported_counts_and_round_trip` |
| `ImportError::fmt` | `classic-error-diagnostic-erased`: remove parse detail | `malformed_file_reports_its_line_number` |
| `ImportError::source` | `classic-error-chain-erased`: remove parse cause | same |

`cells-for-diff.py origin/main` selects **bdb; no check fired**. The BDB
counter and import comparisons above are the applicable cells. No C-facing
function changed, so no contract case or shipping-library size build applies.

### Follow-through: K-mixture input and classic visible bytes

2026-10-25 UTC. The held-out K-mixture reader is within #926: it consumed
exactly the same input as deleted counting and decoded it too early. The
source is available, and no #1190 file is required, so leaving it open was
not justified. It is now fixed. Pin `gen_k_mixture_model.cpp:350-366` opens
one document, `:72-137` reads/validates each segmented record, `:371-384`
folds pair counts, increments the document count and updates unigrams.
The same macro/GLib split citations above govern NUL and invalid bytes.
`train::prepare_held_out` now parses once and feeds validated token/phrase
records to KMM's existing counting/folding algorithms and deleted counting.
There is no replacement-character decoding or serialize-and-reparse step.
KMM's existing text entry point shares the extracted aggregation/fold; its
unrelated standalone CLI contract is not changed.

The existing KMM gen/export differential now checks a source-derived variant
with hidden invalid bytes and an arbitrary-byte zero tail. Both the original
text path and the validated-record path match the pin's exported model
records. Native tests compare both held-out model outputs directly; the
existing eval/train CLI check confirms accepted bytes reach the subsequent
evaluation-input read, while a phrase mismatch is refused beforehand.

Classic pinyin is a byte field too. `pinyin.cpp:638` passes it to
`FullPinyinParser2`; `pinyin_parser2.cpp:164-213,217-333,346-379` matches
ASCII spellings and selects the longest prefix from byte zero. An invalid
UTF-8 byte cannot be crossed by a key, but does not invalidate an already
complete prefix. The parser now splits bytes before decoding fields and
passes the valid pinyin prefix to the unchanged `FewestKeys` selection.
The existing frontend driver verifies invalid pinyin suffixes with identical
exported rows in both directions. Invalid bytes before enough pinyin keys
still refuse the record; no new key-selection rule is introduced.

**Resolved by the maintainer ruling for #1226:** dictool keeps its strict
CLI contract (`dictool-format.md` §2). This is dictool's own specification,
not a pinned libpinyin import-tool contract: the pin has no import tool.
The frontend `importPinyinDictionary` at ibus-libpinyin
`42788824ef59ffc03c6f345e20f08ee860dd5c15`,
`src/PYLibPinyin.cc:249-275`, skips wrong-field-count lines and ignores
add failures before saving. That frontend handling is not the C ABI.
GLib tag 2.89.4 `glib/gutf8.c:940-996` and pin `pinyin.cpp:627-644`
still explain why an invalid phrase is rejected by an add; they do not
require dictool to copy the frontend's handling of that rejection.

Dictool retains full-file preflight: wrong field counts, empty phrase/pinyin,
unparseable pinyin, phrase/key-count mismatch, bad counts and duplicates
produce 1-based errors before any write. Invalid phrase bytes still refuse;
verified NUL truncation, valid pinyin prefixes, shared held-out records and
desired-floor counts remain unchanged. Skip-and-continue is not implemented.

After successful preflight, a failed add records its line while later rows
still run. The store is marked modified and saved, then the first failing
line is returned. On successful save, successful adds both before and after
that row are persisted. This corrects the earlier doc comment's misleading
claim about only earlier adds; runtime behavior is unchanged.

Numeric work remains deferred until #1229 merges and the glibc 2.41 trace
can begin. DBM durability after an assertion stays outside this scope.

Follow-through gates at `8fa83791` (preserved on wip/926-pre-squash): **179 native tests passed** across dictool, training
and KMM (18 external-input checks ignored); fmt and clippy passed. This
includes 138 dictool/training tests and 41 KMM tests. Five BDB counter checks
and the existing live KMM gen/export check passed. Classic invalid-pinyin
suffix interchange was **IDENTICAL** in both directions. Reproduce with the
same crate commands above, adding `-p oxpinyin-kmm`, and:

```sh
export PINYIN_GEN_KMM="$pin_src/utils/training/gen_k_mixture_model"
export PINYIN_EXPORT_KMM="$pin_src/utils/training/export_k_mixture_model"
just heavy -- cargo test --locked -p oxpinyin-kmm --test differential rust_kmm_matches_pin_gen_and_export -- --include-ignored --nocapture
uv run --python 3.15 python -c 'from pathlib import Path; rows = [line.split(b" ", 2) for line in Path("tools/bisection/classic-import.txt").read_bytes().splitlines()]; Path("/tmp/classic-pinyin-bytes.txt").write_bytes(b"".join(b" ".join([f[0], f[1] + b"\xffignored", *f[2:]]) + b"\n" for f in rows))'
IMPORT_FIXTURE=/tmp/classic-pinyin-bytes.txt just heavy -- bash tools/bisection/run-import-diff.sh
```

Review fix gate, 2026-10-25 UTC: the missed corpus caller now passes the
lexicon to `count_deleted`. **227 tests passed**, 21 external-input checks
ignored, across corpus, dictool, training and KMM; clippy and fmt passed.
This caller-only fix changes no parser behavior. Per the maintainer's rule-7
instruction, mutation and BDB comparisons above were not rerun. Reproduce:

```sh
just heavy -- cargo test --locked -p oxpinyin-corpus -p oxpinyin-dictool -p oxpinyin-training -p oxpinyin-kmm
just heavy -- cargo clippy --locked -p oxpinyin-corpus -p oxpinyin-dictool -p oxpinyin-training -p oxpinyin-kmm --all-targets -- -D warnings
just fmt --all --check
```

Nine additional named mutations compiled and failed their tests, then were
restored before the full suite passed. Each newly changed function is covered:

| Function | Mutation | Test |
| --- | --- | --- |
| `split_classic` | `classic-byte-space-only`: stop splitting TAB | `split_classic_matches_g_strsplit_set_max_three` |
| `format::parse` | `classic-reject-pinyin-suffix`: discard valid prefix | `invalid_pinyin_suffix_keeps_the_longest_ascii_prefix` |
| KMM `add_document` | `kmm-text-discard-document`: skip folding | `one_document_counts_pairs_and_documents` |
| KMM `add_token_document` | `kmm-token-drop-text`: discard phrase text | `held_out_byte_records_produce_identical_scoring_and_lambda_models` |
| KMM `fold_document` | `kmm-double-document`: increment document count twice | `one_document_counts_pairs_and_documents` |
| KMM `read_document` | `kmm-text-drop-boundary`: disable pi-grams | same |
| `Document::count` | `kmm-reset-every-token`: reset previous token each line | same |
| `prepare_held_out` | `held-out-drop-records`: parse empty input | `held_out_byte_records_produce_identical_scoring_and_lambda_models` |
| train `run` | `train-reject-hidden-bytes`: restore whole-file UTF-8 decoding | `eval_and_train_refuse_mismatched_held_out_phrases` |

## Numeric conversion (glibc 2.41)

2026-10-25 UTC, based on main `1e88675`. The reference image is Debian trixie,
glibc 2.41. Source read from `bminor/glibc` tag `glibc-2.41` (commit
`74f59e9271cbb4071671e5a474e7d4f1622b186f`), not 2.43; libpinyin at
`074a2219`. This replaces the 2.43 disassembly reading above for the
conversion rule. No pin tool was run to find a rule.

### The rule

The pin's tools convert record fields with `atoi` and `atol`
(`utils/utils_helper.h:23-35,57-69`). Both are `strtol(nptr, NULL, 10)`
(`stdlib/atoi.c:27`, `stdlib/atol.c:27`, inline forms
`stdlib/stdlib.h:481-489`), whose body is `__strtol_l_internal`
(`stdlib/strtol_l.c:235-546`; `strtol.c:108-117` passes `group` 0, so there
is no thousands grouping; the C23 entry `strtol.c:124-127` differs only in
the `0b` prefix, which is not read in base 10). On LP64:

* leading `isspace` bytes are skipped (`:304`); the tools call
  `setlocale(LC_ALL, "")`, so the table is the locale's. The C locale's is
  TAB, LF, VT, FF, CR and SPACE (`locale/C-ctype.c:366-369`), and a UTF-8
  locale adds no byte above 0x7f. Other single-byte locales are not traced;
* one `+` or `-` follows (`:311-317`). A field that is empty, white space
  only, a sign alone or has no digit gives 0 (`:306`, `:495`, `:526-545`);
  white space after the sign is not skipped;
* digits are consumed to the first other byte, without error (`:449-492`);
* a magnitude above `LONG_MAX` (below `LONG_MIN` when negative) saturates
  (`:503-521`); `-2^63` is exact (`:507-509`, `:524`);
* `atoi` casts to `int` (`atoi.c:27`) and the macros store a `guint32`: the
  low 32 bits. `atol` is kept as a `glong` and the K-mixture importer stores
  it in `guint32` fields (`utils/training/k_mixture_model.h:117-153`); the
  interpolation converter prints it whole with `%ld`
  (`k_mixture_model_to_interpolation.cpp:131-133,170-172`) and a token with
  `%d`.

Between 2.39 and 2.41 `strtol_l.c`, `strtol.c`, `atoi.c`, `atol.c` and
`C-ctype.c` differ only in copyright years and a `C2X`→`C23` macro rename.
The live pin tools below were built on the cloud host (Ubuntu 24.04, glibc
2.39), not in the reference image; that difference does not reach this rule.

### Sites

| Site | Class | Result |
| --- | --- | --- |
| `TAGLIB_PARSE_SEGMENTED_LINE` `atoi` (`utils_helper.h:61`): `gen_ngram`, `gen_deleted_ngram`, `gen_k_mixture_model`, `eval_correction_rate`, `mergeseq` | Reproduce | `oxpinyin_kmm::atoi` (`kmm/src/cnum.rs`) is used by `counter/parse.rs` (counter, emitter, lambda, train held-out), `eval/decode.rs`, `segment/mergeseq.rs` and `kmm/generate.rs`. A field with no digits is the null token, not an error, as at the pin. |
| `TAGLIB_GET_TOKEN` `atoi` and `TAGLIB_GET_TAGVALUE(glong, .., atol)` in `import_k_mixture_model` and `k_mixture_model_to_interpolation` | Reproduce | `kmm/src/text.rs`: tokens `atoi`, counts `atol` then the low 32 bits for the model fields; the interpolation converter prints the token as `i32` and the count as `i64`. The line grammar (fixed field positions) is the crate's own and is unchanged. `assert(count == T)` in the importer is not a conversion and is untouched. |
| Classic count `atoi` (ibus-libpinyin `PYLibPinyin.cc:262-267`; `pinyin.cpp:520-524`) | Frozen dictool contract | Not the pin library: the frontend is a separate package and dictool specifies its own grammar, `count := digit+`, with explicit errors and a *desired absolute count* meaning (`dictool-format.md` §2-3). The maintainer's #1226 ruling keeps that contract, so `atoi` is not reproduced. The leading `+` that `parse::<u64>` accepted contradicted `digit+`; it is now refused with the line-numbered count error. |
| `import_interpolation` `atoi`/`atol` (`utils/storage/import_interpolation.cpp:135-182`) | Strict, valid input identical | See "Datagen and `import_interpolation`" below. |
| `table_text_parser.h:54-69` `g_ascii_strtoull` (`.table` and `punct.table` rows) | Strict, valid input identical | See below. |
| `gen_zhuyin_table` `atoi` (`gen_zhuyin_table.cpp:153`), `gen_pinyin_table` `fscanf("%u")` (`gen_pinyin_table.cpp:145`) | No counterpart | Neither is in the data build (`data/Makefile.am:60-62`); oxpinyin has no port. |
| `parse_interpolation_dump`, `candidate.rs`, `status.rs` parses | No pin parser | They read oxpinyin's own counter dump, candidate index and status JSON; the pin consumes these only through Python scripts. Not a conversion of a pin input. |
| `printf("%f")` of lambda and correction rate | Reproduce (NaN only) | See "NaN at the `%f` print sites" below. |

No class (a)-(c) exception or language/runtime discrepancy is needed: every
reproduced result is plain checked integer arithmetic in safe Rust, linear in
the field length with no allocation.

### NaN at the `%f` print sites

The pin tools print floats at `estimate_interpolation.cpp:129,139`,
`estimate_k_mixture_model.cpp:141,152` and `eval_correction_rate.cpp:203`.

* Per-token `lambda:%f` is not NaN on a reachable input: a context exists only
  with at least one item whose held-out count is at least 1, so the divisor
  `table_num` is nonzero, and `numerator / sum` is guarded by `0 == sum`.
* `average lambda:%f` is `lambda_sum / lambda_count` (`double / int`,
  `estimate_interpolation.cpp:116-117,139`; `estimate_k_mixture_model.cpp`
  the same). With no held-out context it is `0.0 / 0`, a NaN. An empty held-out
  stream reaches it. x86-64's `divsd` yields the default NaN with the sign bit
  set, so glibc prints `-nan` (AArch64's default NaN prints `nan`). The tool
  exits 0.
* `correction rate:%f` is `passed_count / (parameter_t) tested_count`
  (`eval_correction_rate.cpp:201-203`). An empty or all-null evaluation
  stream tests nothing, `0.0 / 0.0`, and prints `-nan` on x86-64.

Reproduced in safe Rust at the print sites, with the sign taken from the
machine's own division as the C does: `oxpinyin_kmm::printf_f` (moved from the
KMM binary; glibc's `nan`/`-nan`/`inf`/`-inf`), `EvalReport::correction_rate_line`
(`passed as f64 / tested as f64`) and `oxpinyin-lambda` on `EmptyDeleted`
(`mean_of_no_contexts`, a `black_box` division so the compiler does not fold it
to a NaN of its own). The pipeline keeps its own contracts: `rate` stays 0 for
scoring and `estimate_lambda` still returns `EmptyDeleted`, because λ must lie
in `[0, 1]`. `oxpinyin-kmm` already printed these through `printf_f`.

The existing live lambda differential gains `empty_inputs_print_the_nan_the_pin_prints`:
the pin's `estimate_interpolation` on an empty held-out stream prints exactly
`average lambda:-nan` and its `eval_correction_rate` on an empty
`evals2.text` prints `correction rate:-nan`, equal to the Rust lines.

### Verification

Unit tests: `cnum.rs` (white space, signs, saturation, NUL, 32-bit
narrowing), `counter/parse.rs::segmented_tokens_read_as_glibc_atoi`,
`eval/decode.rs`, `mergeseq.rs`, `kmm/generate.rs`, and two `kmm/text.rs`
tests (import narrowing; `%d`/`%ld` conversion output).

The existing live counter test (`counter-differential`,
`rust_matches_live_gen_ngram`) gains a stream whose every token field is
respelled with a sign, leading CR/VT/FF, leading zeros, trailing bytes, a
value above 2^32, or a negative with the token's low 32 bits; zero lines
become `-4294967296`, `4294967296`, `-0`, `abc`, `+`, `-2^63` or `\r0`. The
pin's `gen_ngram` and `export_interpolation` agree with the Rust counter on
all 138096 unigrams and 199 bigrams. The existing live KMM tests gain the
same respelling for `gen_k_mixture_model`/`export_k_mixture_model`, and a
`k_mixture_model_to_interpolation` input carrying `+5`, `4294967301`, `-7`,
`abc`, `00012x`, a token above 2^31 (printed negative), a count above
2^32, a count that saturates, and a lone `-`.

Revert-and-check: with the old digits-only `atoi` restored in
`counter/parse.rs`, the respelled counter step fails (Rust bigrams 199 vs 0).

```sh
source ~/.oxpinyin-oracle.env
tools/oracle/build-oracle.sh --dbm bdb --work-dir "$pin_build" --prefix "$pin_prefix" --model-dir "$PINYIN_MODEL_DIR"
# the libpinyin stage builds the utils; the ibus stage needs autopoint and is not used here
pin_src="$pin_build/src/libpinyin-074a2219c90feaf962d0d24f034514033ece5f99"
export PINYIN_EXPORT_DIR="$OXPINYIN_LIBPINYIN_DATA_DIR" PINYIN_GEN_NGRAM_DATA="$pin_src/data"
export PINYIN_GEN_BINARY_FILES="$pin_prefix/bin/gen_binary_files" PINYIN_GEN_UNIGRAM="$pin_prefix/bin/gen_unigram"
export PINYIN_GEN_NGRAM="$pin_src/utils/training/gen_ngram" PINYIN_EXPORT_INTERPOLATION="$pin_src/utils/storage/export_interpolation"
just heavy -- cargo test --locked -p oxpinyin-training --test counter-differential -- --include-ignored
export PINYIN_GEN_KMM="$pin_src/utils/training/gen_k_mixture_model" PINYIN_EXPORT_KMM="$pin_src/utils/training/export_k_mixture_model"
export PINYIN_KMM_TO_INTERP="$pin_src/utils/training/k_mixture_model_to_interpolation"
just heavy -- cargo test --locked -p oxpinyin-kmm --test differential -- rust_kmm_matches_pin_gen_and_export rust_kmm_matches_pin_to_interpolation --include-ignored
```

Not run for that change: a live `import_k_mixture_model` differential (no
existing driver; its macros are the ones the converter run covers), the
import runner (`run-import-diff.sh`, which needs the ibus stage), and the kc
and tkrzw cells (no backend code changed).

## Datagen and `import_interpolation`

2026-10-26 UTC, based on main `e84f06f`; libpinyin `074a2219`. Source read;
no pin tool was run.

**Promise.** `oxpinyin-datagen` compiles the pinned model20 archive (and the
toned mini model under `fixtures/datagen-toned/`) into the system data a
pin-built libpinyin ships: chunk files byte for byte, DBM rows record for
record (`crates/oxpinyin-datagen/src/lib.rs`, `datagen-model20.md`). It is
a build-time tool that never ships, and its documented posture on malformed
input is refusal with a line-numbered error where the pin aborts or skips
(`datagen-model20.md`, the `import_interpolation` row;
`interpolation2-grammar.md` §5). Its readers are plain Rust parses with no
GLib emulation, so there is nothing to remove. The record below fixes which
inputs agree.

**Model tables** (`table.rs` `parse_table_line`, `read_table_file`). The pin
reads every `.table` with `table_text_read_pinyin_record`
(`src/storage/table_text_parser.h:34-97`, called from
`phrase_index.cpp:537`, `chewing_large_table2.cpp:118`,
`phrase_large_table3.cpp:131`), not with `fscanf`: `fgets` into a 4096-byte
buffer; fields separated by runs of space and TAB, each ending at space,
TAB, CR or LF; a line with fewer than four fields is skipped and bytes after
the fourth are ignored; pinyin and phrase must be under 256 bytes; token and
count must be whole decimal fields without a leading `-`, read by
`g_ascii_strtoull` (a leading `+` is accepted) and at most `guint32` and
`size_t` respectively, else the line is skipped.

| Input | Pin | Datagen |
| --- | --- | --- |
| Four fields, plain decimal token ≤ 2³²−1 and count ≤ 2⁶³−1 | row | the same row |
| Leading `+` on token or count | row | row (`str::parse` accepts `+`) |
| Fewer than four fields; negative or non-numeric token or count | skipped | skipped |
| A fifth field | ignored | error |
| `-0` count | skipped | row with count 0 |
| Count in 2⁶³..2⁶⁴−1 | row | skipped |
| A field of 256 bytes or more; a line over 4095 bytes | skipped; split into pieces | row; one line |
| Non-ASCII white space (U+3000, U+00A0) inside a field | part of the field | separator |
| A line starting with `#` | a row if its fields parse | skipped |
| Invalid UTF-8 | bytes kept | error for the file |

`punct.rs` `parse_punct_line` follows the same pattern against
`table_text_read_punct_record` (`table_text_parser.h:99-121`); it was read,
not changed.

**Interpolation model** (`system.rs` `read_interpolation`, `parse_count`,
`validate_pair`). The pin's `import_interpolation` converts the token with
`atoi` and the count with `atol` (`utils_helper.h:27-49`;
`import_interpolation.cpp:135-141,172-182`), then stores the count as
`guint32` (`add_unigram_frequency`, `phrase_index.h:628`;
`SingleGram::insert_freq` and the `guint32` total at `:205-209`). The rule
is the glibc one above. Datagen reads the same lines through the shared
grammar, then `str::parse::<u32>` for tokens and `str::parse::<i64>` with a
`u32` range check for counts. For a plain decimal token and a count of at
most 2³²−1 the stored values are the same; a repeated unigram token sums
and a zero count is a no-op, as at the pin. Any other spelling (trailing
bytes, a value out of range, a negative count, a sign on the token) is a
line-numbered error, where the pin stores `atoi`/`atol`'s low 32 bits. A
unigram sum above 2³²−1 is kept as `u64` where the pin wraps. No pinned or
toned row reaches any of these.

**Decoder loader** (`oxpinyin-data` `interp.rs`). Not a port of the tool:
it reads the same file for the runtime and refuses zero counts and
duplicate tokens on purpose (`interpolation2-grammar.md` §5). Its
`parse::<u32>`/`parse::<u64>` refuse every malformed spelling. `is_graph`
in `interp_grammar.rs` is unchanged; its bounded difference stays recorded
there.

**Valid-input evidence.** The existing differentials compare datagen with
the pin's `gen_binary_files` / `import_interpolation` / `gen_unigram` output
on the pinned model20 and the toned model
(`crates/oxpinyin-datagen/tests/libpinyin_parity.rs`, driven by
`tools/datagen/libpinyin-drop-in-differential.sh` on the kc and tkrzw
cells). Nothing they compare changes here: the only code edits are two
comments in `table.rs` and `punct.rs` that named the old `fscanf` reader.

**Decision (maintainer, 2026-10-26 UTC).** Datagen stays strict: it is a build-time tool, not a drop-in for the pin's `utils` programs, and the differences above meet #926's acceptance.
