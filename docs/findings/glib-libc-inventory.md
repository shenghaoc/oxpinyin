# GLib and libc at the pin: reachable calls and oxpinyin counterparts

Status: **current audit** (#917) · 2026-10-24 UTC · Parent: #915 ·
#1297 Phase 1 classification added 2026-10-26 UTC (see "Classification
of the remaining GLib- and libc-shaped code").
Pin: libpinyin 2.11.92 at `074a2219c90feaf962d0d24f034514033ece5f99`.
Oracle image: `debian:trixie` (Debian 13.7, the current stable codename)
from the default mirrors, linux/amd64. **GLib 2.84.4-3~deb13u5**,
**glibc 2.41-12+deb13u4**. The reference environment moved from the testing
digest/snapshot to trixie on 2026-10-25 UTC; see the G11 amendment below.
Scope: the 79 `pinyin_*` and 52 `zhuyin_*` exports in
`src/libpinyin.ver` and `src/libzhuyin.ver`, their reachable helpers,
and the oxpinyin `crates/` counterparts. Test, oracle and tool-only code
is listed separately.

## Abstract

The exported surface reaches **68 distinct GLib/libc functions at 1,224
call sites**: 43 GLib functions (883 sites) and 25 libc functions (341
sites). They collapse into fifteen reachable semantic families, plus
tool-only and dead groups. Most reachable boundaries have a dedicated
child; one family is new (#1144). The internal container, hash and
primitive rows are ordinary C-facing or internal parity with no
language/runtime quirk. No entry changes a valid-input answer on the
evidence read.

Three results govern the sweep:

1. Real GLib ownership is ABI, not emulation: `GArray` mutation in
   `pinyin_lookup_tokens` / `pinyin_token_get_nth_pronunciation` and
   their zhuyin twins stays on `glib_sys` (#924).
2. The UTF-8, `fscanf`, path, sorting, filesystem and diagnostic
   boundaries each have a scoped child; the only newly found semantic
   difference is the punctuation-table decoder (#1144).
3. `LC_NUMERIC` is a STOP (#923). It is not a language/runtime quirk to
   remove; row 39 is an explicit maintainer ruling.

The math surface is `log` only: seven reachable sites, all class (a)
under row 11. oxpinyin's fixed-point `-log2` is the transcendental
imitation that a `std` method could replace directly; the `poss`
conversion is its matching gfloat reconstruction. Both are recorded
here, with no replacement child.

## Reachability and negative coverage

A function is reachable when an exported `pinyin_*` or `zhuyin_*` entry
can call it without a test, tool or uninstantiated class in between.
The matrix below follows that definition. The complete call-site list is
[glib-libc-call-sites.tsv](glib-libc-call-sites.tsv), which records the
pin file and line, facade, backend, call, family, child and one of
three reach values: `exported`, `tool-only`, `dead`. Regenerate it with
`just audit-glib-call-sites <pin-checkout>`; the recipe runs
`tools/audit/glib_call_sites.py`, which verifies the checkout is at
`074a2219c90feaf962d0d24f034514033ece5f99` and joins the scan with the
hand-maintained `tools/audit/glib_call_sites.classification.tsv`.

- 1,224 reachable sites: 883 GLib, 341 libc.
- 115 tool-only sites: `src/storage/tag_utility.cpp`,
  `src/storage/table_text_parser.h` and the `load_text` methods, used by
  the training and `gen_binary_files` utilities, not by the shared
  objects.
- 173 dead sites: `PinyinLookup2` (never instantiated), the old
  `ChewingLargeTable` / `PhraseLargeTable2` and `FacadeChewingTable` /
  `FacadePhraseTable2` classes, the `PunctTableEntry` writer methods, and
  the `#if 0` debug dumps.

The negative list is in "Negative coverage" below. A closed candidate
for a `zhuyin_parse_chewing` over-read is also recorded there: GLib's
`g_utf8_strncpy` stops at the NUL and pads, so the candidate is not a
read.

## Matrix

Every row is at the pin and the image versions above. `Reach` is `both`
for libpinyin and libzhuyin, `pinyin` for the prediction-only surfaces;
`all` means bdb, kc and tkrzw. `Disposition` uses #1142's order:
`contract` (C-facing behaviour on valid input), `(a)`/`(b)`/`(c)`,
`quirk` (record-admitted language/runtime difference) and `ruling`
(STOP).

| ID | Family and pin call sites | Reach | oxpinyin counterpart | Valid / malformed | Disposition | Child |
| --- | --- | --- | --- | --- | --- | --- |
| G1 | UTF-8/UCS-4 conversion and category: `pinyin.cpp:628,715,1397,1450-1454,2658,3210,3678`; `lookup.cpp:54,58`; `zhuyin.cpp:918,969-973,1684,2165`; `zhuyin_parser2.cpp:871,879`; `punct_table.cpp:48` (dead); `tag_utility.cpp` (tool) | both (punct pinyin only); all | `capi-marshal/src/input.rs` `utf8_to_ucs4` / `parser_prefix` / `scalar_text` (#1279); `capi/src/phrase.rs` `pinyin_phrase_segment`; `data/src/phrase_library.rs` `phrase_chars`; `data/src/interp_grammar.rs` `is_graph`; `data/src/row_format.rs` `decode_puncts` | valid: identical strings. malformed/non-scalar: `g_utf8_to_ucs4` accepts surrogates and values above U+10FFFF; Rust `str` cannot. Embedded U+0000 stops the pin's conversion. | valid: contract. malformed: quirk (#894, #918); tool: quirk (#926); punct: quirk/class (b) (#1144) | #894, #918, #925, #926, #1144 |
| G2 | C strings, joins and vectors: `g_strdup` 39, `g_free` 155, `g_strdup_printf` 14, `g_strconcat` 16, `g_strjoinv` 3, `g_strndup` 7, `g_strfreev` 10, `g_strv_length`/`g_strv_contains` 2, `g_string_*` 4 | both (punct subset pinyin only); all | `capi/src/ffi.rs` `owned_cstr` / `owned_cstr_list`; `capi/src/aux_matrix.rs` `AuxMatrix::full` / `double` / `chewing`; `capi/src/predict.rs` `prepend_punctuations`; `capi/src/iterators.rs` export rendering; `data/src/pin_stderr.rs` `build_filename` | valid: identical bytes. malformed: raw path bytes, missing middle aux component, NUL in a diagnostic, duplicate punctuation. | valid: contract. malformed: quirk (#922) or #925; punctuation strings #1144 and punctuation order/duplicates #928, both contract (closed 2026-10-26 UTC) | #922, #924, #925, #928, #1144; export row 1 |
| G3 | Path joining: `g_build_filename`, 68 sites, e.g. `pinyin.cpp:164,252,268,286,308,395,400,952,968,985,1052`; `zhuyin.cpp:118,196,212,230,252,337,342,585,601,605,626,630,684` | both; all | `data/src/table_info.rs` `FileName::under` / `pin_join`; `data/src/pin_stderr.rs` `build_filename`; `capi-marshal/src/input.rs` `filename` (#1279) | valid: same resolved path. malformed: leading `/`, empty name, `..`, duplicate separators, non-UTF-8 bytes. | valid: contract. malformed: quirk (#922) | #922 |
| G4 | GArray/GPtrArray containers: `g_array_*` and `g_ptr_array_*`, 514 sites (`g_array_index` 101, `g_array_new` 64, `g_array_free` 68, `g_ptr_array_index` 58, `g_array_set_size` 57, `g_array_append_val` 54, ...) | both; all | `capi/src/dict.rs` and zhuyin twin keep real `GArray` for the ABI; internal steps are `Vec`/slices in `engine/src/nbest.rs`, `engine/src/session/*`, `data/src/phrase_library.rs` | valid: identical candidate/result arrays. malformed: out-of-range indices, a key with no whole word, a short chunk header. | valid: contract. malformed: class (b)/(c) rows; #924 for the ABI arrays | #924 |
| G5 | n-best tail sort: `phonetic_lookup.h:337` `g_ptr_array_sort(tails, trellis_value_compare)` | both; all | `engine/src/nbest.rs` `glib_merge_sort`, `tail_compare`, `c_float_to_gint` | valid: the `gint` truncation ties scores under one unit apart, so the comparator is not transitive on finite scores. malformed: NaN and ±∞ from λ outside `[0, 1]`. | contract; merge kept, [record](921-927-glib-sorts.md); superseded 2026-10-26 UTC: language artefact, #1297 classification | #921 |
| G6 | candidate and predicted list sorts: `pinyin.cpp:2286,2443` `g_array_sort_with_data(compare_item_with_sort_option)`; `zhuyin.cpp:1529,1610` `g_array_sort`; phrase-string dedup `pinyin.cpp:2067`, `zhuyin.cpp:1375` | both; all | `engine/src/nbest.rs` `glib_merge_sort` under `engine/src/session/mod.rs` `glib_rank_sort`, `RankKey::pin_compare` and `capi/src/predict.rs` `sort_predicted` | valid: a total order (frequencies below 2²⁴); stable order and dedup winner. malformed: λ outside `[0, 1]` wraps the `gint` frequency key into cycles. | contract; merge kept, [record](921-927-glib-sorts.md), superseded 2026-10-26 UTC (#1297 classification); dedup #913/#914 | #927, #928 |
| G7 | hash tables: `g_hash_table_new` 2, `insert` 4, `lookup_extended` 4, `destroy` 2; `g_direct_hash`/`equal` | both; all | `engine/src/nbest.rs` `Step`, `engine/src/session/*` vectors; no exposed iteration order | valid: lookup only; no order reaches the ABI. malformed: none. | contract (internal; no observable order) | — |
| G8 | diagnostics: `g_return_val_if_fail`, 6 sites (`pinyin.cpp:1454`, `zhuyin.cpp:973`, `phrase_index.cpp:354-356,361`); `g_warning` only in the tool-only tag utility | both; all | `capi/src/ffi.rs` `log_warning` / `log_critical`; `capi/src/phrase.rs` `g_return_if_fail_warning`; zhuyin twin | valid: class (c) sites keep one warning at warning level and the pin's domain. malformed: a GLib conversion failure logs the critical. | (c) where the pin aborts; #924 for the channel; #918 for the segment gate | #918, #924 |
| G9 | allocation/free and real GArray ABI: `g_free` 155, `g_strdup` 39, `g_strdup_printf` 14; C `malloc`/`free`/`realloc` 6; `g_array_set_size`/`g_array_append_vals` into caller arrays in `pinyin.cpp:2805,2817` and `zhuyin.cpp:1797,1809` | both; all | `capi/src/ffi.rs` `owned_cstr` / `owned_cstr_list` (C `malloc`/`g_free` contract); `capi/src/dict.rs` and zhuyin twin on `glib_sys`; `libpinyin.alloc` / `libzhuyin.alloc` | valid: caller may `g_free` / `g_strfreev`; the real `GArray` keeps GLib's private metadata. malformed: NULL or OOM paths. | contract; MUST RETAIN | #924 |
| G10 | rename and sync: `g_rename` 10 (`pinyin.cpp:1059,1078,1094,1108,1121`; `zhuyin.cpp:609,635,655,672,687`); `g_fsync` 1 (`memory_chunk.h:555`) | both; all | `user/src/persistence.rs` `save_with_bigram_reporting`, `pin_rename_order`; `data/src/phrase_library.rs` chunk save | valid: same file order and bytes. failure: per-file write/rename failures and the save's return. | contract on valid profiles; #929 for failure ordering | #929 |
| G11 | numeric parsing and locale: `fscanf` 10 (`table_info.cpp:208,214,220,228,232,255,338,344,352,357`); `atoi` 1 (`:160`); `setlocale` 6 (`:197,291,328,372,378,394`) | both; all | since external-I/O stage 1 (2026-10-28 UTC) the pin's own calls in `oxpinyin-input/native/config.c` (since 2026-10-31 UTC `oxpinyin-input/src/native.rs` through the `libc` crate, same formats), decoded by `data/src/table_info.rs` `TableConf::load` and `data/src/user_files.rs` `UserTableInfo::load`/`save` | valid: same table values, versions, λ bits and library mapping. malformed/host: glibc 2.43-3 `%d`/`%f`/`%255s` grammar, saturation, signed user.conf versions, hex/NaN/Inf, incomplete exponents, NUL/width and row grouping; class (b): missing source buffer and merged-row SIGSEGV. | valid: contract. malformed: contract except the class (b) refusals; host scanner (#919/#920); process side effect ruling (#923) | #919, #920, #923 |
| G12 | file I/O and `MemoryChunk`: `mmap` 10, `munmap` 2, `lseek` 4, `read` 5, `write` 3, `open` 3, `close` 12, `unlink` 22, `fopen` 3, `fclose` 8, `fprintf` 36 | both; all | `data/src/phrase_library.rs` `map_chunk`, `MappedFile`, chunk header; `user/src/persistence.rs`; `data/src/pin_stderr.rs` raw bytes | valid: same mapped extent, header check and file bytes. malformed: short/truncated/directory chunk; write failures. | contract on valid files. malformed: class (b) rows 76/96/109; #929 for writes | rows 76, 96, 109; #929 |
| G13 | C primitives: `memset` 124, `memcpy` 28, `strlen` 18, `strcmp` 18, `memcmp` 10, `memmove` 7, `strchr` 2 | both; all | Rust slices/`str`; `data/src/row_format.rs`, `core/src/single_gram.rs`, `capi/src/ffi.rs` | valid: same bytes. malformed: NULL/unreadable phrase strings and out-of-range walks are class (b) rows. | contract; class (b) at the registered NULL/over-read rows | rows 80, 106 |
| G14 | libm: `log` 7 (`phonetic_lookup.h:39,250,663,693`; `phrase_lookup.cpp:40,321,342`) | both; all | `core/src/cost.rs` `cost_of_probability` / `surprisal` (fixed-point `log2_fixed`); `engine/src/nbest.rs` `poss` | valid: class (a) last-bit differences are accepted. | class (a), row 11; record only | no child (math record) |
| G15 | punctuation decoding: `punct_table.cpp:56-72` `unescape`, `:74-94` `get_all_punctuations`, reached at `pinyin.cpp:2474` | pinyin; all | `data/src/row_format.rs` `decode_puncts` (stored words per punctuation; infallible); `data/src/punct.rs` `PunctTable::punctuations` (`Err` only for a failed read; `Ok(Vec::new())` for unopened/absent/empty); `runtime/src/lib.rs` `RuntimeDict::punctuations`; `capi/src/predict.rs` `prepend_punctuations` (`phrase_text_bytes` at the C boundary) | valid: identical punctuation strings. malformed: non-scalar, empty, unterminated and misaligned values give the pin's rows; partitions below. | contract; class (b) for the unterminated over-read only, registered (2026-10-26 UTC, #1144) | **#1144** |
| G16 | tool-only GLib: `tag_utility.cpp` (`g_strsplit_set`, `g_unichar_*`, `g_strv_length`, `g_utf8_get_char/next_char`, `g_ptr_array_foreach`, `g_hash_table_foreach_steal`, `g_warning`); `table_text_parser.h` (`g_ascii_strtoull`, `g_strlcpy`); the `load_text` methods | not C ABI | `data/src/interp.rs`, `data/src/interp_grammar.rs`; `training/src/counter/parse.rs`; `kmm/src/cnum.rs`; `dictool/src/format.rs`; `datagen/src/table.rs`, `punct.rs`, `system.rs` | valid: tool input, identical records. malformed: separator, escape, category and `atoi` quirks reproduced in the training tools; dictool and datagen refuse or skip by their own contracts. | tool scope, [record](926-tool-tokenizers.md); `is_graph` stays as [recorded](interpolation2-grammar.md) | #926 |
| G17 | dead code: `pinyin_lookup2.cpp`, `chewing_large_table.cpp`, `phrase_large_table2.cpp`, `facade_chewing_table.h`, `facade_phrase_table2.h`, `PunctTableEntry::escape/append/remove`, `#if 0` dumps | none | no shipped counterpart; the runtime uses `PhoneticLookup`, `PhraseLookup` and the large-table-3/chunk path | n/a | negative coverage | — |

## Family notes

### G1 — UTF-8 and UCS-4

The pin converts with GLib's `g_utf8_to_ucs4` and `g_ucs4_to_utf8` in
three candidate/sentence/export paths (`pinyin.cpp:1627-1645` and
`lookup.cpp:27-60`; the export is `pinyin.cpp:698-716`). GLib 2.89.4
encodes surrogates and values above U+10FFFF and stops at an embedded
U+0000. Rust `char`/`str` cannot represent the first two, and
`PhraseItemView::phrase_chars` treats them as unreadable. The valid-input
path is byte-identical; the malformed cases are #894 (phrase text) and
#1144 (punctuation), both now carried as stored words and encoded at the C
boundary. `phrase_segment` is
different: it deliberately keeps the direct GLib conversion because the
C entry's precondition and warning are observable (#918), and #924 keeps
the GLib-owned conversion buffer's `g_free`.

`g_utf8_strlen` at `pinyin.cpp:1450` / `zhuyin.cpp:969` is the same
gate. `g_utf8_strncpy` at `zhuyin_parser2.cpp:879` is cited by the
negative coverage, not as a read: GLib `g_utf8_strncpy` uses `strncpy`,
which stops at the NUL.

### G2 and G3 — strings and paths

`g_strdup_printf` builds `.tmp` names and diagnostic text; the formats
are `%s` and `%d` over bytes, not a locale conversion.
`g_strconcat` is the aux-text prefix/middle/postfix construction
(`pinyin.cpp:3421-3431,3482-3509,3560-3569`) and the export joins
(`pinyin.cpp:830,865`). `AuxMatrix`'s `join` preserves the
`g_strconcat` NULL-postfix law: a missing middle drops the postfix
(#925). `g_strjoinv`/`g_strfreev` at `pinyin.cpp:849,862` render the
bigram export; the pin's array is not NULL-terminated (row 1, class
(b)); oxpinyin renders owned strings before the iterator exists.

**Punctuation order and duplicates (#928, closed 2026-10-26 UTC).**
`pinyin_guess_predicted_candidates_with_punctuations` walks
`m_prefixes` as `pinyin_guess_predicted_candidates` left it
(`pinyin.cpp:2459-2463`), takes each token's `get_all_punctuations`
array, counts it with `g_strv_length` and skips a string that
`g_strv_contains` finds in the zero-terminated `punct_array`
(`:2468-2483`); GLib 2.84.4 `g_strv_contains` is a `g_str_equal` walk
(`gstrfuncs.c:3234-3247`), and `g_array_new(TRUE, TRUE, …)` allocates
and terminates the empty array, so the first test sees no entry
(`garray.c:356-361`). The reverse `g_array_prepend_val` (`:2488-2494`)
leaves the kept strings first, in first-occurrence order, ahead of the
predicted rows, which the test never sees. No entry is `NULL`
(`g_strdup` of the cache string), so `g_strv_length` is the entry count.
`prepend_punctuations` keeps the first of equal `phrase_text_bytes`
values in the same order and puts them ahead of the candidates; those
bytes are the pin's C string. The two agree on every stored value:
duplicates within one value, across prefix tokens (shortest suffix
first) and in empty strings from different stored words, non-scalar
strings, and a punctuation equal to a predicted phrase (both kept). This
is ordinary compatibility; `prepend_punctuations` now reads the prefixes
the prediction stored instead of computing them a second time. Cases
`punct-dedup/*` of `contract-diff.py` MATCH on bdb; the named mutation
`M928-no-dedup` (every punctuation kept) makes the five duplicate cases
differ while the `distinct` and `candidate` controls match.

`g_build_filename` is the only path constructor. `pin_join` keeps the
name beneath the directory even when it starts with `/`; `build_filename`
keeps raw bytes for stderr; `cstr_to_path` keeps the caller's bytes.
`g_rename` and `g_fsync` are G10.

**Close-out record, #922 (2026-10-26 UTC).** Pin read from a tree exported
with `git archive` from commit `074a2219c90feaf962d0d24f034514033ece5f99`;
GLib `g_build_path_va` is the 2026-10-25 reading in
[glib-libc-verification.md](glib-libc-verification.md) G3 (not re-read
here). Nothing was run to learn behaviour. The join law and its partitions
are in that G3 table; this record is the acceptance summary.

| Acceptance item | Result | Evidence already in the tree |
| --- | --- | --- |
| Valid configured paths | Unchanged: `pin_join` keeps a row name beneath the directory (`/x`, `//x`, `sub//x`), and an empty `dir` or `name` gives the other part. The pin's only path constructor is `g_build_filename(dir, name, NULL)` (`pinyin.cpp:395,400`, `zhuyin.cpp:337,342`, `table_info.cpp:201,332`). | unit `file_names_stay_beneath_their_directory` (`table_info.rs`); `glib-922-dir-*`, `glib-922-row-*` |
| User directory identity | Unchanged: `""` is the working directory, NULL is none, the bytes are the caller's (`g_strdup` at `pinyin.cpp:332`, `zhuyin.cpp:276`). `cstr_to_path` is a byte copy into `OsStr`; non-UTF-8 bytes survive. | `glib-922-dir-*`, `stderr-non-utf8-user-dir`, #619/#642/#643 cases |
| Consumer-visible diagnostics | The printed join is the pin's: one separator after the directory, trailing separators of the name kept, separator-only parts kept whole. `build_filename` and `user_file` both go through `join_bytes`. | unit `build_filename_joins_as_glib_does` (`pin_stderr.rs`), `user_conf_path_is_the_join_the_pin_writes` (`persistence.rs`); `glib-922-table-conf-*`, `glib-922-save-write-failed-dup-trailing-*`, `glib-922-fini-write-failed-dup-trailing-pinyin` |
| Remaining discrepancy | **None.** Every difference found was a defect fixed (the user directory's `Path::join`, `218b0a4c`). The Windows separator set follows GLib but has no pin run; Unix is complete. `..`, `.`, duplicate separators and `\` on Unix are ordinary bytes in both. | G3 table; `tools/bisection/run-contract-diff.sh ... -- --issue 922` |

### G4 and G9 — containers, allocation and the ABI

The C ABI's `GArray` is a real GLib object. `g_array_set_size` and
`g_array_append_vals` in `pinyin_lookup_tokens`,
`pinyin_token_get_nth_pronunciation` and their zhuyin twins mutate the
caller's `GArray` through `glib_sys`; a manual `Vec` write would corrupt
GLib's private `_GRealArray` metadata. This is MUST RETAIN (#924).
Internal `GArray`/`GPtrArray` uses are ordinary step, candidate and
lookup containers; their Rust counterparts are `Vec`, slices and fixed
arrays. The malformed cases are the registered out-of-range index and
empty-chunk sites, not a container-layout difference.

The allocation contract is the same: returned `gchar *` is
`malloc`-compatible and freed by `g_free`; `gchar **` is freed by
`g_strfreev`; the two `.alloc` files enumerate every ownership slot.
`g_free` on a GLib conversion in `phrase_segment` stays direct (#924).

**Close-out record, #924 (2026-10-26 UTC).** Pin read from the exported
`074a2219` tree. Disposition: **retain** the GLib ABI. The per-operation
MUST RETAIN / MAY DIVERGE matrix (every exported operation that touches a
GLib-owned object, allocation or diagnostic channel, with pin cites) is the
"#924 ABI ownership audit" in
[glib-libc-verification.md](glib-libc-verification.md) G9 and is not
repeated here. The allocation-failure follow-through and its limits are in
the same file's "#924 Allocation-failure injection follow-through". This
record adds what merged after those sections:

| Settlement | What it fixed | Where it stands |
| --- | --- | --- |
| #1224, PR #1247 | NULL `GArray` in `pinyin_lookup_tokens`, `pinyin_token_get_nth_pronunciation` and the zhuyin twins: GLib now diagnoses the NULL naturally (`g_array_set_size` / `g_array_append_vals` criticals in domain `GLib`, one append per prepared library for lookup), with the pin's return values. `*_token_get_phrase` keeps success with a NULL output after the nonfatal UTF-8 conversion allocation fails (`pinyin.cpp:1627-1645`, `zhuyin.cpp:1096-1113`). | Closed defects, not exceptions. `glib-924-null-array-{lookup,pronunciation}-{pinyin,zhuyin}` are in the `--issue 924` selection. |
| #1227, PR #1252 | Final C string and keyboard-vector allocation refusals: one facade-domain warning, false, untouched out-params, earlier buffers of the call freed; export iterators and `pinyin_get_pinyin_strings` defer their out-param writes. | Class (c), registered in `upstream-divergences.md` "Final C string/vector allocation refusal (#1227; class (c))". Gate `tools/abi/check-alloc-failure.sh` (runner `abi-alloc-failure`). |
| #1229 | Moves the reference environment to Debian trixie (GLib 2.84.4, glibc 2.41). | Reference-environment move only. It changes no ABI, ownership or diagnostic rule; it re-based the GLib and libc line citations. |

Guards that stay green (unchanged by this PR): `tools/abi/check-exports.sh`
(exported-symbol set equals the pin's version scripts);
`tools/abi/check-alloc-pairing.sh` with `alloc-pairing-pinyin.cc`,
`alloc-pairing-zhuyin.cc` and the `libpinyin.alloc` / `libzhuyin.alloc`
registers; caller mutation and `GArray` lifecycle in
`glib-924-array-lifecycle-{pinyin,zhuyin}`; `glib-924-oom-array-growth-*`
(real `GArray` growth stays fatal on both sides); the diagnostic channel in
`glib-924-segment-critical-*` and `glib-924-segment-fatal-critical-*`.

Stated limits (not closed by this record):

- Source-only caller rows: the final-allocation refusals other than the
  eleven injected witnesses (the remaining key renderers, the three
  auxiliary-text getters, both sentence writers, the zhuyin-side keyboard
  paths and the bigram iterator) rest on source reading and on the one
  helper they share; there is no per-row injection.
- Earlier Rust allocations (`String`, `CString`, `Vec`) can still terminate
  before the final foreign allocation; no identical allocation graph or
  whole-process OOM equivalence is claimed.
- The `*_iterator_get_next_phrase` phrase conversion stays non-warning and
  may return NULL with success, as the pin's unchecked `g_ucs4_to_utf8`
  does (`pinyin.cpp:715-716,737,745`).
- Diagnostic text with an embedded NUL has no producer in the C-facing call
  graph; the helper difference is not an admitted outward one.
- The glibc `vasprintf` allocation path of the formatted-tone renderers is
  not traced for 2.41.
- Per-trigger re-audit of the facade warning register belongs to G8; the
  entry-point inventory in the verification file is not a trigger proof.

No unregistered discrepancy remains for #924's operations: every difference
found is a fixed defect or a registered class (c) refusal. This PR changes no
ABI, ownership or allocator rule, and re-ran none of these gates for #924.

### G5 and G6 — sorts

`g_ptr_array_sort` at `phonetic_lookup.h:337` orders the n-best tails
with `trellis_value_compare`, which truncates the float difference to
`gint` (`cvttss2si` in the bdb binary). Scores less than one unit apart
compare equal, so the comparator is not transitive even on finite scores;
NaN and ±∞ come from a λ outside `[0, 1]`.

`g_array_sort_with_data` orders the pinyin and predicted candidate lists
under `compare_item_with_sort_option` (`pinyin.cpp:2286,2443`), and
`g_array_sort` the zhuyin lists (`zhuyin.cpp:1529,1610`). Their
`gint` frequency key is a total order for λ in `[0, 1]` and wraps into
cycles outside it. The phrase-string dedup sorts (`pinyin.cpp:2067`,
`zhuyin.cpp:1375`) are total; their winner rule is #913/#914, and the
dedup winner and punctuation order are #928.

**Superseded 2026-10-26 UTC** by the #1303 ruling and the #1297
classification below: the merge and the wrapping and truncating keys are
language artefacts. Historical text follows. All of these stay GLib's
merge (#921, #927): one implementation,
`oxpinyin_engine::glib_merge_sort`, used by the tails, `glib_rank_sort`
and `sort_predicted`. A standard-library sort gives other sentence orders
on NaN tails and may panic on a non-total comparator. The record is
[921-927-glib-sorts.md](921-927-glib-sorts.md).

### G7 — hash tables

`PhraseLookup` and the phonetic trellis use GLib hash tables for
prefix/step lookup, and no iteration order reaches an exported result.
`PinyinLookup2`'s copies are dead. Rust uses vectors keyed by step and
token; there is no semantic difference to record.

### G8 — diagnostics

`g_return_val_if_fail` in `phrase_index.cpp:354-356` logs a critical
when a loaded sub-index cannot be read; `pinyin_phrase_segment` and
`zhuyin_phrase_segment` log the pin's exact precondition at `pinyin.cpp:1454`
and `zhuyin.cpp:973`. Class (c) sites outside these already owe one
warning at level 16 in the facade's domain. The allocation and message
text stay with #924; the segment gate is #918. A warning is observable:
it is never a quirk by itself.

### G10 — rename and sync

**Amendment 2026-10-26 UTC (#1297 Phase 2).** The single-payload-write reproduction below is historical. [Complete payload writes](1297-chunk-payload.md) now use `write_all`; header-word diagnostics, ignored sync errors, file/rename order and tolerance remain.

`_rename_files` renames the library files by sub-index, then the user
pinyin index, the phrase index and the bigram; `zhuyin_save` renames
each file as it writes it. `MemoryChunk::save` calls `g_fsync` after the
payload. The Rust `save_with_bigram_reporting` keeps the per-file
tolerance, the rename order and the return value (#929), including the
pin's own answers after a failed write or rename.

#1221 closes the missing chunk sync in `oxpinyin_user::persistence::write_chunk_file`:
a successful payload write is followed by `File::sync_all`, whose result is
ignored before the file drops. This follows `074a2219`
`src/include/memory_chunk.h:536-557`, including the ignored sync failure.
The pinyin journal/chunk callers (`src/pinyin.cpp:971,988`) and zhuyin
callers (`src/zhuyin.cpp:607,633`) share that same `MemoryChunk::save`.
The older storage facade writers (`src/storage/facade_chewing_table.h:164`
and `src/storage/facade_phrase_table2.h:145`) also call it. The separate
`oxpinyin_datagen` build-time `write_chunk_file` uses `std::fs::write` and
is unchanged; `oxpinyin_data::phrase_library` reads/maps runtime libraries.

On macOS, retain std's `F_FULLFSYNC` implementation of `sync_all`, stronger
and potentially slower than the pin's Unix `g_fsync` macro (`fsync`, GLib
2.84.4 `glib/gstdio.h:60-82`). This is a durability improvement using safe
Rust, rather than adding a direct syscall to the unsafe-forbidden user
crate. The return remains ignored; file bytes, write/rename order and
save results remain unchanged. Linux uses `fsync` through std. This needs no register entry: the observable output, the files on disk and the ABI are unchanged, so it is an internal durability improvement.

**Payload, header and sync, step for step (2026-10-26 UTC).** The pin's
`MemoryChunk::save` (`memory_chunk.h:536-557`) writes the length word and the
checksum word with one `write` each (`assert`ed), the payload with one
`write(fd, begin(), size())`, and answers false with the fd closed and no
`g_fsync` when that returns -1 or a short count (`:549-553`); otherwise it
syncs, closes and answers true. The callers ignore the answer
(`pinyin.cpp:971,988`; `zhuyin.cpp:607,633`) and go on to the rename. Oxpinyin
now matches that step for step in `write_chunk` over a `ChunkSink` (write +
sync; `File` in production): two single header writes, one payload `write`, then
`sync`. A short payload count returns an `Io` error of kind `WriteZero`, and
`Interrupted` or any other error returns as it is: no retry, no `write_all`
loop, no sync. `PersistenceError::Io` is the soft path (`tolerate` in the
reporting save, `Staged::finish` for zhuyin), the same handling as the pin's
ignored false; the header-word `assert` remains register row 77
(`ChunkHeaderWrite`). A sync error is ignored. The recording-sink unit tests
are `chunk_writes_header_words_then_payload_then_syncs_once`,
`chunk_sync_error_is_ignored`, `chunk_short_payload_write_is_an_error_without_sync`
and `chunk_interrupted_payload_write_is_not_retried` in `persistence.rs`. File
bytes, write and rename order, tolerance and return values are unchanged; the
`write_all` it replaces differed only under `RLIMIT_FSIZE`, payloads over the
single-write cap and `EINTR`.

**Close-out record, #929 (2026-10-26 UTC).** Pin read from the exported
`074a2219` tree. The ordered per-operation table is in
[glib-libc-verification.md](glib-libc-verification.md) G10. Summary of what is
mandatory for a valid profile and what is a platform exception:

| Semantics | Status | Evidence |
| --- | --- | --- |
| File order (libraries in index order, then user pinyin index, phrase index, bigram) and the rename order; pinyin stages all then renames, zhuyin writes and renames each | Mandatory | `failed_renames_report_exact_order_and_preserve_destinations`, `interrupted_save_preserves_the_facade_specific_committed_prefix`, `interruption_after_user_chunks_keeps_indexes_and_bigram_at_previous_save`, `save-929-boundary-*` |
| Per-file tolerance: a failed write or rename does not stop later files; rename failures print the fixed `rename %s to %s failed.` pair; the marker and the dirty reset still follow | Mandatory | `save-929-rename-*`, the rename unit test above |
| Return values: true after attempted renames, false before staging when there is no user dir or nothing modified | Mandatory | G10 table rows 1 and 7; `--issue 929` |
| Same-backend bytes, no data loss on completed saves, reload round trips | Mandatory | `save_reload_keeps_bigrams_and_unsaved_changes_do_not_replace_them`, `save-929-reload-*`, `save-929-logical-cross-open-*` |
| Chunk write: header words, one payload write, `sync` (#1221) with its result ignored, short or failed payload soft | Mandatory, now tested | the four `chunk_*` unit tests; `sync_rejecting_chunk_target_does_not_stop_later_files`, `save-929-sync-rejecting-*` |
| Header-word `assert` abort | Register row 77 | `reporting_save_stops_at_a_chunk_header_write` |
| macOS `F_FULLFSYNC` through `File::sync_all` | Platform difference, no register entry | the macOS paragraph above |
| errno wording, Windows `MoveFileExW` mapping, power-loss durability, interleaving of backend diagnostics | Not claimed; no exception admitted | G10 limits in the verification file |


### G11 — numeric parsing and locale

**Amendment 2026-10-26 UTC (#1297 Phase 2).** The #1297 scanner lane replaces the scanner emulations described below with line headers, complete decimal fields, unlimited whole words, and exact marker tokens. The old G11 scanner conclusions are historical. See [decimal λ](1297-table-floats.md), [version fields](1297-table-decimals.md), [line boundaries](1297-table-lines.md), [addon indexes](1297-addon-index.md), [marker numbers](1297-user-decimals.md) and [marker tokens](1297-user-token.md). `LC_NUMERIC` remains unchanged. The source-history correction is glibc 2.42 for incomplete exponents and 2.43 for `0x.`.

`SystemTableInfo2::load` and `UserTableInfo::load` read their fields with
`fscanf`; `atoi` cuts an addon index to `guint8`; `setlocale(LC_NUMERIC,
"C")`'s return is the new name, and every early return skips the
restore. The valid file surface is #919 (table.conf) and #920
(user.conf). The process-wide locale is row 39 and #923: **STOP for a
ruling**, do not remove it.

The #919 source walk reads glibc **2.43-3**'s
`stdio-common/vfscanf-internal.c`, `stdlib/strtod_l.c` and
`stdlib/strtol_l.c`. The scanner reproduces its `%d` grammar (sign,
decimal digits, `strtol` saturation and the `int`/`guint8` cuts), its
`%f` lexer (decimal and hexadecimal spellings,
`inf`/`infinity`/`nan(payload)`, non-backtracking incomplete exponents,
overflow/subnormal rounding), and `%255s` (cross-line words, 255-byte
width, raw NUL/non-UTF-8 bytes). Confirmation only, not the basis of
these verdicts: a differential corpus over 2,648,804 mixed strings and
90,031 valid decimal/hex spellings found no value or consumed-byte
difference; only four NaN payload bits differed, and λ's NaN consumers
cannot observe the payload. The corpus is local test data and is not
committed.

Two differences were found and fixed; the user.conf writer is the only
shipping one. `UserTableInfo::save`
wrote the version fields as unsigned; the pin's `int` fields and `%d`
format write a `table.conf` value outside `int` back signed (for example
`-1`, not `4294967295`). `UserTableInfo::to_text` now formats the two
fields through `i32`, so the marker round-trips and the open-counter rules
are unchanged. The training segmenter's λ reader was a line-only
`parse_table_conf_lambda`; it now parses the full stream through
`TableConf::parse`, so hexadecimal λ, indented/one-line headers and the
incomplete-exponent rejection follow the pin instead of a local
approximation.

Two class (b) refusals remain: a missing `source table format:` line
reads the pin's uninitialised `char str[256]`, and a five-word row merged
with the next row's first word can make the pin read a NULL item and
SIGSEGV. The subject refuses the first deterministically and answers the
second with the declared result; neither reproduces the uninitialised or
invalid read. The four existing `host_scanf` cases remain HOST-DEPENDENT
on glibc 2.39 (`1e`, `0x1p`); the reference cell is the oracle image's
glibc 2.43-3.

**trixie reference (2026-10-25 UTC).** The pin-reference image now builds on
`debian:trixie` (Debian 13.7), whose glibc is **2.41-12+deb13u4**. Re-read
from that source: the 2.41 `%f` lexer sets `got_e = got_dot = 1` at an
exponent mark but does not clear `got_digit`
(`stdio-common/vfscanf-internal.c:2196-2201`), and the post-loop conversion
check tests only the empty buffer and the bare `0x` prefix, not `!got_digit`
(`:2545-2549`). 2.43-3 adds both (`vfscanf-internal.c:2218-2219`,
`:2571-2576`). Consequently 2.41 converts `1e`, `1e+`, `0x1p`, `0x1p+` and
`0x1p-` (the buffer is `1e`/`0x1p`, and `strtof` consumes `1`/`0x1`), while
`0x` and `infin` still fail. `stdlib/strtod_l.c` has no behavioural change
in the cited parse, rounding or exponent paths, so the value and consumed
bytes are 2.41's. The subject still emulates 2.43-3 and rejects these
spellings: this is a reference-environment difference to report, not to tune
away. Affected cases (both facades): `table-conf-lambda-exponent-without-digits`,
`table-conf-lambda-hex-exponent-without-digits`,
`table-conf-919-lambda-exponent-sign-without-digits`,
`table-conf-919-lambda-hex-exponent-sign-without-digits` and
`table-conf-919-lambda-hex-exponent-minus-without-digits`. On trixie the pin
side loads and the subject refuses, so the existing `host_scanf` disposition
reports them HOST-DEPENDENT rather than DIFFER. A second difference in the
same walk is `0x.`, which has the hexadecimal prefix and no digit. 2.41's
`%f` reaches `strtof("0x.")`, which treats it as the number 0 and returns
with the read pointer after `0` (`stdlib/strtod_l.c` at 2.41: hex prefix
`:690-700`, the `0x.` branch `:732-768`, the zero-digit return
`:1044-1048`), so the conversion succeeds. 2.43 rejects it because
`got_digit` is still zero (`vfscanf-internal.c:2571-2576`). The subject
follows 2.43, so on trixie the pin loads and the subject refuses
`table-conf-919-lambda-hex-dot-without-digits` and its zhuyin twin. At
`8ba13c53` (preserved on wip/1229-pre-squash), before `0x.` was registered,
those two cases reported DIFFER. Both facades now register
`host_scanf=('0x.',)` (maintainer ruling, 2026-10-25), so on a host whose
scanner accepts `0x.` the pin-loads/subject-refuses result reports
HOST-DEPENDENT; any other difference stays DIFFER. This is the same
library-version finding, not a subject defect to fix here. The `%d` lexer
is unchanged for the base-10 table fields (2.43's added
`(base == 0 || base == 16)` guard still leaves `0x` at `0` for `%d`), and
`%255s` is unchanged for the caller-provided 256-byte buffer (2.43's
buffer-size change is in the `%c` and `%m` allocation paths).

The #920 `user.conf` walk uses the same scanner: version and counter
`%d` values take LP64 `strtol` saturation followed by the low 32 bits of
the `int` store, so `4294967303` reads as 7 and `-9223372036854775808` as
0; the marker's conformance, counter raise/lower and wipe/write sequence
then decide user state. The one difference found was a NUL inside the
`database format:` token: `%255s` keeps the bytes, but the pin compares
with `strcmp`, so a known token followed by NUL+bytes still conforms. The
reader now truncates at the first NUL; contract cases retain trained and
imported state for wrapped conforming markers and compare the wipe for
non-conforming ones, with every fixture value read from the cell's
`table.conf` (no backend literal). The signed `%d` writer from #1178
round-trips the integer fields. The 24,687-string `%d` corpus from #919
is confirmation only here; it is not the basis.

**Close-out record, #923 (2026-10-26 UTC).** Pin read from the exported
`074a2219` tree. Disposition: **retain**. The maintainer ruling of
2026-10-25 UTC, quoted from the #923 comment:

> Maintainer ruling, 2026-10-25 UTC: **retain** the pinned `LC_NUMERIC` side effect (register row 39, #539/#618). The default build stays bug-for-bug, and no opt-in departure is planned. The maintainer intends to report the defect upstream; oxpinyin changes only if libpinyin does.

The default build stays bug-for-bug, there is no departure behind a
feature, and the row 39 entry in `upstream-divergences.md` and
`compatibility-policy.md` is unchanged. The mechanism is the pin's:
`SystemTableInfo2::load`, `UserTableInfo::load` and `UserTableInfo::save`
save `setlocale(LC_NUMERIC, "C")`'s return, which is the new name `"C"`
(`table_info.cpp:197,291`; `:328,372`; `:378,394`), so the closing call
re-installs `"C"` and every early return skips it. Oxpinyin's counterpart is
`pin_table_info_locale` in `capi/src/locale.rs` and its zhuyin twin, called
at the pin's points. Side-effect table (sequencing is before any parse or
return, so no legal return or error path leaves a different locale):

| Exported call | Pin site | Locale after | oxpinyin site (`context.rs`) |
| --- | --- | --- | --- |
| `pinyin_init`, any outcome | `pinyin.cpp:337` (`table.conf` load), then `check_format` `:178,:187` | `"C"` | `init_context` (called by `pinyin_init`), first step |
| `zhuyin_init`, any outcome | `zhuyin.cpp:281`, then `check_format` `:126-176` | `"C"` | `zhuyin_init`, first step |
| `pinyin_save`, past the user-dir and modified guards | `pinyin.cpp:1133-1137,1143` (`mark_version`) | `"C"` | `save_context`, under the same guards |
| `zhuyin_save`, past its guards | `zhuyin.cpp:547-552,695-698` | `"C"` | `zhuyin_save`, under the same guards |
| `pinyin_fini`, always | `pinyin.cpp:1200` | `"C"` | `pinyin_fini` |
| `zhuyin_fini` | `zhuyin.cpp:741-757` releases state and writes no marker | unchanged | none |

Guards: `run-locale-diff.sh` (registered in `tools/bisection/runners.json`)
diffs the success path and six failure forms of init byte for byte. The
"Locale after" results assume no other thread changes the locale
concurrently. `setlocale` is process-wide and not thread-safe, in the pin as
in oxpinyin. Retaining the side effect (the 2026-10-25 UTC ruling) accepts
that concurrency impact unchanged; no removal is proposed, so none is
assessed. Upstream's own fix would be to copy `setlocale(LC_NUMERIC, NULL)`
first and restore it on every path; oxpinyin changes only if libpinyin does.

### G12 — file I/O

`MemoryChunk` maps the file, checks the 8-byte header, the length word
and the checksum, and reads the payload. A short or unreadable chunk is
the pin's empty-chunk fault (rows 76 and 96); a sparse chunk is row 109.
`unlink` belongs to `_clean_user_files` and the user-file rewrite;
`fprintf` is the raw stderr channel of the same paths. The writes are
G10. The Rust mapping is `memmap2`-backed `MappedFile` and the same
header checks.

### G13 — C primitives

`memset`/`memcpy`/`memmove`/`memcmp`/`strlen`/`strcmp`/`strchr` are
ordinary byte and string operations in the pin's parsers, chunk and
candidate code. The Rust equivalents are slices and `str`; no observable
difference exists on valid data. The malformed NULL/unreadable cases are
the registered class (b) rows 80 and 106.

### G14 — libm

The only reachable libm call is `log`: the n-best trellis and the phrase
lookup accumulate natural logs in `gfloat`. The Rust engine uses
integer costs and a fixed-point `log2_fixed`; the class (a) register is
row 11. The math rule says Rust's standard maths is used at full speed
and a last-bit libm difference is recorded, not chased. There is no
replacement child for `std` calls.

### G15 — punctuation

Closed 2026-10-26 UTC (#1144). `decode_puncts` walks a stored value as
`get_all_punctuations` does (`punct_table.cpp:57-94`) and keeps each
punctuation's stored words (`PhraseText`); `prepend_punctuations` encodes
them with `phrase_text_bytes`, the encoder #894 and #1198 use, which is
the `g_string_append_unichar` form (GLib 2.84.4 `gstring.c:816-888`
copies `g_unichar_to_utf8`, `gutf8.c:546-596`). `PunctTable::punctuations`
fails only on a failed read; an unopened table, an absent entry and a
zero-length value answer `Ok(Vec::new())`, as the pin's
`get_all_punctuations` answers `false` on a zero-size chunk (`:78-79`).

| Partition | Pin (`074a2219`) | oxpinyin | Case |
| --- | --- | --- | --- |
| Scalar words, each punctuation zero-terminated | the strings | identical | `punct-decode/valid` |
| Zero-length stored value | no row (`size == 0`) | no row | `punct-decode/zero-length` |
| Surrogate or value above U+10FFFF | GLib's three- to six-byte form | identical bytes | `punct-decode/surrogate`, `punct-decode/above-unicode` |
| Zero first word | `unescape` appends it, so `g_strdup` cuts to an empty string; the run still takes the nonzero words after it | identical | `punct-decode/empty`, `punct-decode/empty-leading` |
| No final zero | the run ends at the last whole word after a one-word over-read whose result is not used | identical rows; no over-read | `punct-decode/unterminated`, `punct-decode/unterminated-zero` |
| Length not a multiple of 4 | `maxlen` is the `sar` of the byte difference and `begin < end` compares bytes: whole words, then one empty row after a terminated run | identical | `punct-decode/misaligned-terminated`, `punct-decode/misaligned-unterminated`, `punct-decode/misaligned-short` |
| Absent DBM entry, unopened table | no row | no row | — |

The unterminated read is class (b), registered as
[Unterminated punctuation value over-read](upstream-divergences.md#unterminated-punctuation-value-over-read):
the rows match whenever the pin returns, and only the read and its
possible fault are not reproduced. The other partitions are ordinary
compatibility. All eleven cases MATCH on bdb; against the parent build the
two controls match and the other nine differ.

### G16 and G17 — tools and dead code

`tag_utility.cpp` is included by `pinyin_internal.h` but no exported
entry calls `taglib_*`; it is used by `utils/training` and
`utils/storage`. `table_text_parser.h` and the `load_text` methods are
used only by `utils/storage/gen_binary_files.cpp`.
`PinyinLookup2` is never instantiated; `PhraseLookup` uses
`FacadePhraseTable3` and `FacadePhraseIndex`, not the old
`FacadePhraseTable2`/`ChewingLargeTable`. The `#if 0` dumps and the
`PunctTableEntry` writer methods have no exported caller.

The tool parsers are recorded per helper in
[926-tool-tokenizers.md](926-tool-tokenizers.md): the training tools
reproduce glibc `atoi`/`atol`; dictool keeps its frozen classic grammar;
datagen reads exactly the pin's valid `.table`, `punct.table` and
`interpolation2.text` records and refuses or skips malformed ones by its own
contract; `gen_pinyin_table` and `gen_zhuyin_table` have no counterpart.
## Exported C string inputs

The [#1279 per-parameter inventory](c-string-inputs-1279.md) covers all 25
exported functions with string inputs (29 parameters), their pin conversion,
invalid/non-scalar/NULL outcomes, prior state changes, and current Rust
helper. It records source-derived defects and the pending session-interface
coordination; it is not a completed verification claim.

## oxpinyin GLib emulations in `crates/`

The table covers semantic counterparts, not only names containing
`glib`. Each row is an internal re-expression of a C/GLib behaviour; the
child column is the audit that must decide it.

| Counterpart | Emulates | Pin cite | Child |
| --- | --- | --- | --- |
| `capi-marshal/src/input.rs` `utf8_to_ucs4`, `scalar_text`, `parser_prefix`, `direct_zhuyin_reading`, `filename` (since #1279, PR #1296; formerly `capi/src/ffi.rs` `cstr_to_*`) | `g_utf8_to_ucs4` / `g_utf8_strlen` gates; raw C path bytes | `pinyin.cpp:1450-1454`; `zhuyin.cpp:969-973`; `pinyin.cpp:1498-1509` | #918 |
| `capi/src/phrase.rs` `pinyin_phrase_segment` (formerly `segment_text`; `utf8_to_ucs4` plus two `g_return_if_fail_warning` calls since #1279) | `g_utf8_strlen` + `g_utf8_to_ucs4` gate and `g_return_val_if_fail` | `pinyin.cpp:1450-1454`; `zhuyin.cpp:969-973` | #918 |
| `data/src/phrase_library.rs` `phrase_chars` | `g_ucs4_to_utf8` on stored phrase text | `pinyin.cpp:1627-1645`; `lookup.cpp:27-60`; `pinyin.cpp:698-716` | #894 |
| `data/src/row_format.rs` `decode_puncts`; `capi-marshal/src/phrase_text.rs` `phrase_text_bytes` | `PunctTableEntry::unescape` / `get_all_punctuations`; `g_string_append_unichar` | `punct_table.cpp:56-94` | #1144 (closed: pin rows reproduced; over-read class (b)) |
| `capi/src/aux_matrix.rs` `AuxMatrix::chewing`, `split_zhuyin` (the former `join` is a `format!`) | `g_utf8_substring`, `g_strconcat` with a NULL middle | `pinyin.cpp:3518-3576` | #925 |
| `capi/src/predict.rs` `prepend_punctuations`, `seen` | `g_strv_length` + `g_strv_contains` + reverse prepend | `pinyin.cpp:2456-2498` | #928 (closed: byte-equality first occurrence, contract) |
| `data/src/pin_stderr.rs` `build_filename` | `g_build_filename` raw bytes for stderr | `table_info.cpp:201,332`; `pinyin.cpp:338`; `zhuyin.cpp:282` | #922 |
| `data/src/table_info.rs` `pin_join` | `g_build_filename(dir, name, NULL)` | `pinyin.cpp:395,400`; `zhuyin.cpp:337,342` | #922 |
| `data/src/table_info.rs` `Scan`, `strtol_digits`, `atoi_guint8`, `hex_float` | `fscanf` `%d`/`%f`/`%255s`, `atoi`/`strtol`, glibc `strtof` | `table_info.cpp:160,208-289` | #919 |
| `data/src/user_files.rs` `Scan::int` | `fscanf` `%d` and `strtol` truncation to `int` | `table_info.cpp:338-359` | #920 |
| `capi/src/locale.rs` `pin_table_info_locale` | `setlocale(LC_NUMERIC, "C")` | `table_info.cpp:197,291,328,372,378,394` | #923 (STOP) |
| `engine/src/nbest.rs` `glib_merge_sort`, `tail_compare`, `c_float_to_gint` | `g_sort_array` merge + `trellis_value_compare` | `phonetic_lookup.h:174-178,329-339`; GLib 2.84.4 `gqsort.c:69-281` | #921: kept, [record](921-927-glib-sorts.md) |
| `engine/src/session/mod.rs` `glib_rank_sort`, `RankKey::pin_compare` | `g_array_sort_with_data` / `g_array_sort` under the `gint` frequency key, through `glib_merge_sort` | `pinyin.cpp:1678-1709`; `zhuyin.cpp:1129-1144` | #927: kept, [record](921-927-glib-sorts.md) |
| `capi/src/predict.rs` `sort_predicted` | `gint` frequency key, through `glib_merge_sort` | `pinyin.cpp:1701-1705,2440-2445` | #927: kept, [record](921-927-glib-sorts.md) |
| `user/src/persistence.rs` `save_with_bigram_reporting`, `pin_rename_order` | `_write_files` / `_rename_files` and per-file tolerance | `pinyin.cpp:915-1130`; `zhuyin.cpp:585-693` | #929 |
| `data/src/phrase_library.rs` `map_chunk`, `MappedFile` | `MemoryChunk::mmap` / header / checksum | `memory_chunk.h:424-520` | rows 76, 96, 109 |
| `data/src/interp_grammar.rs` `is_graph`, `interp.rs` | `taglib_read` + `g_unichar_isgraph` | `tag_utility.cpp:171-264` | #926: kept as [recorded](interpolation2-grammar.md) |
| `dictool/src/format.rs` `split_classic`; `training/src/counter/parse.rs` `parse_segmented_line`, `kmm/src/cnum.rs` `atoi`; `datagen/src/table.rs`, `system.rs` numeric fields | `g_strsplit_set` / C `atoi`, `atol` / `g_ascii_strtoull` in tool input | `utils_helper.h`, `table_text_parser.h:34-121`; `import_interpolation.cpp:135-182` | #926: training reproduces `atoi`; dictool and datagen strict, [record](926-tool-tokenizers.md) |
| `capi/src/dict.rs`, `zhuyin-capi/src/dict.rs` direct `g_array_*` | real GLib `GArray` mutation | `pinyin.cpp:2805,2817`; `zhuyin.cpp:1797,1809` | #924 |
| `capi/src/ffi.rs` `owned_cstr`, `owned_cstr_list`, `log_warning`, `log_critical`; `phrase.rs` `g_return_if_fail_warning` (the `g_free` of a GLib buffer is gone since #1279) | `g_strdup` / `g_strfreev` / `g_log` / `g_return_val_if_fail` | `pinyin.cpp` diagnostics; `phrase_index.cpp:354-356` | #924 |

## Classification of the remaining GLib- and libc-shaped code (#1297, Phase 1)

Recorded 2026-10-26 UTC at `main` `ab0b1da` and classified under the
maintainer ruling of 2026-10-26 UTC (#1303, merged as PR #1304):
product behaviour is reproduced in idiomatic Rust; a language artefact
(C or C++ semantics such as an integer-returning comparator, unsigned
wrap-around, float-to-integer truncation or an integer width chosen for
storage; GLib, glibc or another libc, libstdc++, compiler or processor
behaviour) is never reproduced, on valid or malformed input, and its
difference is recorded without approval. A width or wrap that the C
ABI, an on-disk format or a domain algorithm prescribes is not an
artefact. The test in every reason below is the policy's: would a
language-neutral requirement for the behaviour mention it?

Pin read from a clone at `074a2219c90feaf962d0d24f034514033ece5f99`;
GLib read at tag `2.84.4` (the GitHub mirror of the GNOME tree); the
glibc citations are the #919, #920 and #926 readings in this file and in
[926-tool-tokenizers.md](926-tool-tokenizers.md), not re-read. Nothing was
run to learn a behaviour. This is Phase 1 of #1297, docs only. The
cleanup is Phase 2, below.

The search was by behaviour, not by the text `glib`: every crate under
`crates/` except the test-only `pinyin-oracle` was read for conversions,
comparisons, sorts, selections, scanners, joins, writers and arithmetic
shaped like a GLib, glibc, libstdc++ or C mechanism. The rows are one per
item of the emulations table above, split or merged where the code moved,
plus the items the behaviour search added (marked **new**).

Buckets:

- **Boundary** — the C ABI and its GLib-owned objects, the diagnostic
  channel, the on-disk system and user formats, path and stderr bytes
  (limb 1, and the earlier rulings limb 6 keeps). Kept as is.
- **Product behaviour** — what a user or a developer calling the shared
  object observes, including libpinyin's logic defects (limb 4). Kept, in
  idiomatic Rust, named for the behaviour.
- **Language artefact** — not reproduced (limb 3). Phase 2 removes it and
  records the difference.

A row can hold both a product rule and an artefact inside it; the bucket
then names each part.

| # | oxpinyin item (`crates/`) | Pin cite | Final bucket | Reason |
| --- | --- | --- | --- | --- |
| 1 | `oxpinyin-capi/src/dict.rs`, `oxpinyin-zhuyin-capi/src/dict.rs`: `glib_sys::g_array_set_size` / `g_array_append_vals` on the caller's array | `pinyin.cpp:2805,2817`; `zhuyin.cpp:1797,1809` | Boundary | The caller owns a real `GArray` (#924); the ABI prescribes it. |
| 2 | `ffi.rs` (both facades) `owned_cstr`, `owned_cstr_list`, `malloc_cstr_with`, `malloc_cstr_with_warning` | the `g_free` / `g_strfreev` ownership of returned strings (`pinyin.cpp:715-716,733`; the `.alloc` registers) | Boundary | Ownership and lifetime of returned C strings are ABI. |
| 3 | `ffi.rs` (both facades) `log_warning`, `log_critical` | `g_log` domains; `g_return_val_if_fail` (`phrase_index.cpp:354-356`) | Boundary | The diagnostic channel a caller's log handler observes, produced at the facade. |
| 4 | `oxpinyin-capi/src/phrase.rs` `pinyin_phrase_segment` and the zhuyin twin: two `g_return_if_fail_warning` calls | `gutf8.c:240`; `pinyin.cpp:1450-1454`; `zhuyin.cpp:969-973` | Boundary | The criticals reach the caller's log handler (#918, #924). |
| 5 | `locale.rs` (both facades) `pin_table_info_locale` | `table_info.cpp:197,291,328,372,378,394` | Boundary | Maintainer ruling 2026-10-25 UTC (row 39, #923); limb 6 keeps it until a new ruling. |
| 6 **new** | `oxpinyin-capi/src/types.rs` `GArray`, `GUint`, `GInt`, `GChar`; `oxpinyin-chewing/src/chewing_key.rs` packed key; the casts at exported parameters in `cursor.rs` (`guint16` rest length), `iterators.rs` (`guint` index into the `guint8` field), `user_data.rs` (`gint` count to `guint32`) | `pinyin.h`, `zhuyin.h`; `chewing_key.h:111-113`; `pinyin.cpp:126,520-524` | Boundary | The exported signatures prescribe these widths. |
| 7 | `oxpinyin-capi-marshal/src/input.rs` `utf8_to_ucs4`; `ucs4.rs` `decode`, `prefix_text` | `g_utf8_to_ucs4` (`gutf8.c:938-993`) at the C string entries | Boundary (excluded) | #1297 excludes the C string conversions (#1279, PR #1296); the words reach stored user data. Not re-classified here. |
| 8 | `input.rs` `scalar_text`, `parser_prefix`, `direct_zhuyin_reading`, `filename` | `pinyin_parser2.cpp:164-175,217-333`; `zhuyin_parser2.cpp:914-959`; `pinyin.cpp:332`; `zhuyin.cpp:276` | Boundary (excluded) | Same exclusion (#1279). |
| 9 | `oxpinyin-capi-marshal/src/phrase_text.rs` `phrase_text_bytes`, `stored_words_match` | `g_ucs4_to_utf8` / `g_unichar_to_utf8` (`gutf8.c:546-596,1022`); `pinyin.cpp:568-571` | Boundary | The C-string form of stored words; a C string cannot hold a zero (#1182). |
| 10 | `oxpinyin-core/src/ucs4.rs` `PhraseText`; `oxpinyin-data/src/phrase_library.rs` `PhraseItemView::phrase_text`, `phrase_codes`, `phrase_chars`; `row_format.rs` `punct_text` | `lookup.cpp:39-60`; `pinyin.cpp:1627-1645` | Product behaviour | Phrase text is the stored code points; the outward cut is row 9's boundary. |
| 11 | `oxpinyin-engine/src/nbest.rs` `glib_merge_sort` (re-exported from `lib.rs`) | `g_sort_array` → `msort_with_tmp` (GLib `gqsort.c:69-281,328-335`) behind `g_ptr_array_sort`, `g_array_sort`, `g_array_sort_with_data` | Language artefact | No requirement names GLib's merge; it was kept only because rows 12, 13 and 14 feed it artefact comparators. |
| 12 | `nbest.rs` `tail_compare`, `c_float_to_gint`, `tail_merge`, `poss` | `trellis_value_compare` (`phonetic_lookup.h:174-178`) at `:337` | Product: tails best score first. Artefact: the `float` to `gint` truncation that ties scores under one unit, the NaN and ±∞ `INT_MIN` branch (amd64 `cvttss2si`; arm64 `fcvtzs` differs), and the `gfloat` conversion in `poss` | A requirement says "the n-best sentences, best first"; it would not mention truncation, `gint` or an x86 instruction. |
| 13 | `oxpinyin-engine/src/session/mod.rs` `glib_rank_sort`, `RankKey::pin_compare`, `RankKey::for_sort_word` | `compare_item_with_sort_option` (`pinyin.cpp:1678-1709`) at `:2286`; `compare_item_with_length_and_frequency` (`zhuyin.cpp:1129-1144`) at `:1529,1610` | Product: descending phrase length, pinyin length, frequency over the enabled sort bits (`pinyin.h:55`), stable. Artefact: the wrapping `guint32` frequency difference returned as `gint`, and the `guint8` span key that wraps at 256 bytes (#1275) | A requirement names the keys and their order; the comparator's integer type and wrap are C semantics, and the 8-bit span is a local storage width. |
| 14 | `oxpinyin-capi/src/predict.rs` `sort_predicted` | `pinyin.cpp:2440-2445` (key `:1701-1705`) | Product: phrase length then frequency, descending, stable. Artefact: the wrapping `gint` frequency difference and the GLib merge | As row 13. |
| 15 **new** | `session/mod.rs` `dedup_ranked_by_text`, `dedup_by_text_keep_first` | `pinyin.cpp:2128-2138`; `zhuyin.cpp:1406-1416` | Product behaviour | One row per text, the higher frequency winning and the earlier on a tie (#913/#914); already a map, no C shape. |
| 16 | `predict.rs` `prepend_punctuations` | `pinyin.cpp:2468-2494`; `gstrfuncs.c:3234-3247` | Product behaviour | Distinct punctuation strings in prefix then table order, before the predictions (#928). |
| 17 | `session/mod.rs` `amplified_frequency`, `amplified_frequency_with_bigram` (the `predict.rs` copy and `guint32_of_score` were removed by #1416) | `pinyin.cpp:1811-1824,1862-1866`; `zhuyin.cpp:1264-1268`; `m_freq` is an internal `guint32` (`pinyin.cpp:105`), never exported | Product: the interpolated score `λ·bigram·discount + (1−λ)·unigram/total`. Artefact: scaling by 2²⁴ and truncating into a `guint32`, the near-tie collapse that truncation causes, the low-32-bit wrap of out-of-range scores and the `INT64_MIN` branch (amd64 `cvttss2si`; the arm64 pin saturates, `arm64-pin-portability-1137.md`) | A requirement ranks by the score; the integer it was stored in is internal storage, not ABI. `f32` against `f64` is class (a). |
| 18 | `oxpinyin-data/src/table_info.rs` `pin_join`, `join_bytes`, `FileName::under`; `oxpinyin-user/src/persistence.rs` `user_file`, `user_conf_path`; `pin_stderr.rs` `build_filename` | `g_build_filename` (`gfileutils.c:1904-1999,2202-2243`) at the G3 sites | Boundary | Which file a `table.conf` row names and the path bytes on stderr (#922; stderr ruling 2026-10-03 UTC, kept by limb 6). |
| 19 | `oxpinyin-data/src/pin_stderr.rs` `emit`, `path_bytes`, `report_*_table_conf`, `mmap_failed`; the `rename %s to %s failed.` lines in `persistence.rs` | the `fprintf(stderr, …)` sites (`table_info.cpp:201,332`; `pinyin.cpp:256,290,338,956,1059-1121,1265`; `zhuyin.cpp:200,282,589,609-687,800`) | Boundary | stderr bytes, ruling 2026-10-03 UTC (#545). |
| 20 | `persistence.rs` `save_session`, `save_reporting`, `pin_rename_order`, `tolerate`, `write_marker`, `clean_user_files`, `.tmp` staging | `_write_files` / `_rename_files` (`pinyin.cpp:922-1130`); `zhuyin_save` (`zhuyin.cpp:547-700`); `pinyin.cpp:149-170` | Boundary | Files on disk, their order and the save's answers (#929). |
| 21 | `persistence.rs` `write_chunk`, `write_header_word`, `ChunkSink` | `MemoryChunk::save` (`memory_chunk.h:536-557`) | Boundary: header words, payload, sync, a failure that is soft. Artefact: one `write(2)` with no retry, so a short count or `EINTR` fails the file | A requirement says "write the chunk"; it would not mention a single system call. |
| 22 | `oxpinyin-data/src/phrase_library.rs` `map_chunk`, `MappedFile`, `directory_chunk_fault`, `directory_end` | `memory_chunk.h:420-427,470-520` | Boundary | The chunk format; the directory and sparse cases are policy rows 76, 96 and 109. |
| 23 | `oxpinyin-data/src/row_format.rs` `decode_puncts` | `punct_table.cpp:56-94` at `pinyin.cpp:2468-2494` | Boundary: the `punct.bin` value layout (zero-terminated UCS-4 words). Artefact: the misaligned-length rows (`maxlen` as the `sar` of a byte difference, `begin < end` over bytes, one extra empty row) | A requirement reads whole words; pointer arithmetic over a length that is not a multiple of 4 is C. |
| 24 | `oxpinyin-data/src/table_info.rs` `Scan`, `strtol_digits`, `atoi_guint8`, `c_float`, `hex_float`, `round_to_f32`, `c_string`, `is_space`; `oxpinyin-training/src/segment/model.rs` through `TableConf::parse` | `table_info.cpp:159-161,208-289`; glibc 2.41 `vfscanf-internal.c`, `strtod_l.c`, `strtol_l.c` | Product: the `table.conf` grammar the pin's files use (five header lines in order, decimal integers, a decimal λ read to the pin's `f32`, rows of six words, the library names). Artefact: hexadecimal floats, `inf` / `infinity` / `nan(…)`, the incomplete-exponent rule, `strtol` saturation and the 32-bit store of `%d`, the `%255s` width cut and white-space directives that run across lines, and `atoi` wrapping an addon index through `guint8` | A requirement gives the file's grammar; fscanf and strtol details would not appear, and an out-of-range addon index is an error, not index 0. |
| 25 | `oxpinyin-data/src/user_files.rs` `Scan`, `upstream_db_format`, `UserTableInfo::parse` | `table_info.cpp:122-131,338-359` | Product: the `user.conf` grammar and the conformance test; a non-conforming marker resets the profile. Artefact: `strtol` saturation, the 32-bit store, and a NUL inside the format token ending `strcmp` | The pin writes `user.conf` itself with `%d` from an `int` (`table_info.cpp:378-394`), so an out-of-range number or a NUL in the token comes only from a hand-edited file; natural Rust (`str::parse`, exact token match) makes it non-conforming and the profile resets like any bad marker. Recorded. |
| 26 **new** | `user_files.rs` `UserTableInfo::to_text` | `table_info.cpp:378-394`; fields `int` (`table_info.h:99-102`) | Boundary | The `user.conf` text the pin reads back. |
| 27 | `oxpinyin-capi/src/aux_matrix.rs` `AuxMatrix::full`, `double`, `chewing`, `split_zhuyin` | `pinyin.cpp:3295-3576`; `g_utf8_substring` (`gutf8.c:292`) | Product behaviour | The byte delta used as a character offset is libpinyin's logic defect, reproduced as written (limb 4); idiomatic Rust already. |
| 28 | `oxpinyin-data/src/interp_grammar.rs` `read`, `Tokens`, `is_graph`; `interp.rs` `parse_interpolation2` (datagen and training only) | `taglib_read` (`tag_utility.cpp:127-264`); `g_unichar_isgraph` (`guniprop.c:185`) | Product: the `interpolation2.text` record grammar. Artefact, already not reproduced: GLib's Unicode tables (`is_graph` is Rust's predicate, recorded in `interpolation2-grammar.md`) | The model text format is the tools' product; GLib's category data is a library's. |
| 29 | `oxpinyin-kmm/src/cnum.rs` `atol`, `atoi`; callers `training/src/counter/parse.rs`, `eval/decode.rs`, `segment/mergeseq.rs`, `kmm/src/generate.rs`, `kmm/src/text.rs` | `utils_helper.h:23-35,57-69`; glibc `strtol_l.c` | Product: decimal token and count fields, the empty line as the null token (`0 == strlen(line)`). Artefact: `atoi`'s prefix parse (`12abc` is 12), a digit-less field as 0, `strtol` saturation and the 32-bit cut | A requirement says the field is a decimal token; C's conversion rules would not appear. |
| 30 **new** | `oxpinyin-kmm/src/cnum.rs` `printf_f`; `training/src/lambda/interpolation.rs` `mean_of_no_contexts`; `training/src/eval/decode.rs` `EvalReport::correction_rate_line` | `printf("%f")` (`estimate_interpolation.cpp:116-117,139`; `eval_correction_rate.cpp:201-203`) | Product: six decimals for a finite value. Artefact: glibc's `nan` / `-nan` / `inf` spellings and the sign of a hardware default NaN | A requirement fixes the number format; how one libc spells NaN on one processor is not part of it. |
| 31 | `oxpinyin-dictool/src/format.rs` `split_classic` | ibus-libpinyin `PYLibPinyin.cc:249` | Product behaviour | dictool's own frozen grammar (#1226); not the pin library. |
| 32 | `oxpinyin-datagen/src/table.rs`, `punct.rs`, `system.rs` parsers | `table_text_parser.h:34-121`; `import_interpolation.cpp:135-182` | Product behaviour | datagen's own strict contract (#926). |
| 33 **new** | `oxpinyin-engine/src/nbest.rs` `heap_push`, `heap_adjust`, `heap_pop`, `heap_make`, `heap_top` (a literal port of libstdc++ `stl_heap.h`), used by the node store, the beam and the tails | site A: `trellis_node::eval_item` with `std_lite::push_heap` / `pop_heap` (`phonetic_lookup_heap.h:56-79`); sites B and C: `get_top_results` with `std_lite::make_heap` / `pop_heap` (`phonetic_lookup.h:141-172`), called for the beam at `:771` and the tails at `:335`; `std_lite` is `std` (`include/stl_lite.h:38-44`) | Product: the selection policy as written, `trellis_value_less_than` with its `LONG_SENTENCE_PENALTY` = `log(1.2f)` clause (`phonetic_lookup.h:39,65-91`), `nstore` 2 and `nbest` 3 for pinyin (`pinyin.cpp:55`), 1 and 1 for zhuyin (`zhuyin.cpp:50`), a 32-entry beam. Artefact: which values libstdc++'s comparison sequence selects where the comparator is not a total order | A language-neutral requirement names the comparator and the counts, not the heap. Any other structure selects differently where the comparator contradicts itself (lengths one apart with a margin above `log(1.2)`, so each value is "less" than the other) and on exact ties (same length, equal cost), which libstdc++ and another structure break by different positional rules. `m_sentence_length` counts characters, not phrases (`unigram_gen_next_step` / `bigram_gen_next_step` add `phrase_length`, `phonetic_lookup.h:662,691`). Existing class (a), not from the swap: the pin applies the margin in `gfloat` natural log; oxpinyin applies the quantised `LONG_SENTENCE_PENALTY` = 263 on its log2×1000 `Cost` scale (`nbest.rs`), so pairs at the threshold can decide differently. |
| 34 **new** | in-memory wrapping sums: the facade unigram total in `oxpinyin-runtime/src/lib.rs` and `oxpinyin-data` (`phrase_libraries.rs`, `lm/mod.rs`), the user overlay totals in `oxpinyin-user` (`store.rs`, `registry.rs`, `seed.rs`), the `guint8` pronunciation count (`store_libpinyin.rs`) | `FacadePhraseIndex` `m_total_freq` (`phrase_index.h:615,633`); `add_unigram_frequency`; `phrase_index.cpp:28` | Boundary (no change) | The fields are on-disk `guint32` / `guint8` widths; `wrapping_add` gives the pin's values and files at no cost and is as natural as `saturating_add`. No issue, no change. |

### Checked and excluded

- The input caps `oxpinyin_engine::session::MAX_INPUT_BYTES` (the pin's
  32,767-byte parse) and `oxpinyin_core::graph::MAX_GRAPH_INPUT`: a limit
  is product behaviour (limb 4) and the second is oxpinyin's own refusal.
- Byte-order key comparison in `oxpinyin-store` and `PhraseItem` byte
  equality in `oxpinyin-user/src/replay.rs`: the backend's and the chunk
  format's own orders.
- `oxpinyin-core/src/cost.rs`: `f64::log2` directly. The "C-maths
  imitation" table below predates this; its `log2_fixed` row no longer
  exists in the tree and is historical.
- The candidate snapshot (`oxpinyin-facade/src/snapshot.rs`) and the
  probe-only trellis hash maps: no order reaches the ABI (G7).
- `pinyin-oracle`'s direct `glib_sys` use: the test harness, not shipped.

### Result

- **Boundary:** rows 1–9, 18–20, 22, 26 and 34, and the boundary parts
  of 21 and 23.
- **Product behaviour:** rows 10, 15, 16, 27, 31 and 32, and the product
  parts of 12–14, 17, 24, 25, 28–30 and 33.
- **Language artefact:** row 11 whole, and the artefact parts of 12–14,
  17, 21, 23–25, 28 (already native), 29, 30 and 33.
- Superseded: the earlier conclusion that the sorts stay a hand-written
  merge (the G5/G6 notes and
  [921-927-glib-sorts.md](921-927-glib-sorts.md)) and the
  "under #1303" column of the first version of this section. The
  comparators are total once their artefacts are gone, so a standard
  stable sort is well defined and cannot panic.

### Scanner/tools implementation (2026-10-26 UTC)

Rows 21, 23–25, 29 and 30 are implemented in native Rust in the scanner lane.
The pin’s chunk header, sync and rename rules remain; [payload writes](1297-chunk-payload.md)
retry short counts and interruptions. [Punctuation](1297-punct-words.md) reads whole words.
`ConfigLines` replaces both `Scan` types; `TableConf::parse` has representable
versions, decimal λ, line headers and complete unbounded words. Addon indexes
are checked against the sixteen slots. `UserTableInfo::parse` uses complete
fields and exact database tokens; malformed numbers and NUL-suffixed tokens
reset the profile. `numeric::decimal_field` replaces `atoi`/`atol`, and
`format_six_decimals` uses Rust formatting. The records above and in the register
name every admitted difference. The six generated function-order policies
accompany the removals; the PR reports their two-pass check, ordered validation,
crate gates, selected cells, mutations and stripped sizes. Ranking rows remain
owned by the separate ranking lane.

**Scanner-lane results (2026-10-26 UTC):** 600 touched-crate tests passed (20 external-input tests ignored); fmt and clippy passed. The existing #926 counter, finite lambda estimate, KMM gen/export, KMM transform and empty-input differentials passed. The contract selection `--issue 919 --issue 920 --issue 1144 --issue 929` passed on bdb, kc and tkrzw: each 135 cases, 93 MATCH and 42 POLICY-ACCEPTED. The seven named mutations in the discrepancy records were detected and restored. The harness suite passed 161 tests, including the non-glibc registration guard.

The shared generator’s two independent passes produced byte-identical policies
for all six backend/facade pairs. `function_order.py` verified the actual `.text`
policy prefixes of all twelve base/head objects, not only resolver output; no
policy path remained unresolved. Stripped shipping bytes at base `bab92bf0`
against this scanner implementation, one Ubuntu/glibc 2.44 host and the repository
release install recipe:

| Backend | Library | Base bytes | Head bytes | Difference |
| --- | --- | ---: | ---: | ---: |
| bdb | libpinyin | 2482928 | 2474736 | -8192 |
| bdb | libzhuyin | 2233072 | 2224880 | -8192 |
| kc | libpinyin | 2845928 | 2837736 | -8192 |
| kc | libzhuyin | 2591976 | 2583784 | -8192 |
| tkrzw | libpinyin | 2527976 | 2519784 | -8192 |
| tkrzw | libzhuyin | 2278120 | 2269928 | -8192 |

Main advanced to `c0c9c227` with an unrelated ABI-test repair; the lane rebased and its shared harness passed again. No shipping source, policy generator or profile format changed from measured base `bab92bf0`.

Reproduce the native gate with `just fmt --all --check`,
`just heavy -- cargo test -p oxpinyin-data -p oxpinyin-user -p oxpinyin-training -p oxpinyin-kmm`,
and the corresponding `cargo clippy --all-targets -- -D warnings` command.
Source the environment emitted by `tools/cloud/setup.sh`; run the existing
contract driver with the selection above for each selected cell. Tool-input
setup and live test commands remain in [the #926 audit](926-tool-tokenizers.md).
Use [packaging Regeneration](../packaging.md#regeneration), with
`repeat_output_dir` for every backend, compare both outputs with `cmp`, then
install both facades with ordering enabled. Validate linked objects using
`tools/packaging/function_order.py --library ARM/FACADE=OBJECT` (give the base
its historical `--function-order ARM/FACADE=POLICY`); run
`strip --strip-unneeded -o OUTPUT OBJECT` and compare file sizes. No timing,
RSS or instruction-count claim is made by this scanner lane.

### Phase 2: the cleanup, in two independent lanes

Each lane is one PR. Each records its differences with
`language-runtime-discrepancy-template.md` and a register entry in
`upstream-divergences.md`, and marks `POLICY-ACCEPTED` on the cases that
tell the two apart; no approval is needed for an artefact (limb 3). The
renames or removals and the regenerated function-order policies land in
the same commit: `glib_merge_sort`, `glib_rank_sort`, `atoi_guint8`,
`table_info::Scan` and `user_files::Scan` are named in all six policies
(`tools/packaging/function-order/{bdb,kc,tkrzw}/lib{pinyin,zhuyin}.order`),
regenerated with `tools/generate-function-order.py` as
`docs/packaging.md` "Regeneration" prescribes (two passes,
byte-identical, never hand-edited). Whichever lane lands second rebases
and regenerates again. Each lane reports stripped shipping-library sizes
base against head in one run.

**Ranking lane** (`oxpinyin-engine` `nbest` and `session`,
`oxpinyin-capi` `predict`; rows 11–14, 17, 33):

1. Tails sort by score: best cost first, `Ord` on the fixed-point
   `Cost`, stable; `tail_compare`, `c_float_to_gint`, `tail_merge` and
   the `poss` round trip go.
2. Candidate and predicted lists use `slice::sort_by` (stable) with a
   plain lexicographic order over the enabled keys: phrase length,
   pinyin length (the full span, no `u8` cut), frequency, each
   descending. `glib_merge_sort`, `glib_rank_sort`'s GLib name and
   `pin_compare`'s wrap go.
3. Frequency: rank by the interpolated score itself, compared with a
   total order; `guint32_of_score`, the 2²⁴ scale-and-truncate and the
   amd64 branches go, and the `predict.rs` copy folds into the session's
   (#1416 removed `guint32_of_score`, the amd64 branches and the copy; the
   2²⁴ scale-and-truncate stays as the ruled ranking key).
   The dedup (row 15) compares the same score.
4. Selection: replace the `stl_heap.h` port, keeping the product rule as
   written (`trellis_value_less_than`, its `log(1.2f)` clause, `nstore` 2
   for pinyin and 1 for zhuyin):
   - site A, `trellis_node::eval_item`: the pin's two-slot keep rule
     written out, at most two comparisons, in the pin's argument order and
     slot order, because backtracking reads the slot;
   - sites B and C, the 32-entry beam and the 3 tails: one winner
     (tournament) tree over the step's node storage, taking the comparator
     as a closure with a fixed, documented argument order (the lower leaf
     index on the left); O(m + k log m) time, O(m) space; no `Ord`, no
     `std` heap or sort on this comparator;
   - the final tails then sorted by score (step 1).
   The tree selects differently from libstdc++'s heap where the
   comparator contradicts itself and on exact ties (same length, equal
   cost), which the two break by different positional rules: a language
   artefact, recorded. Before the swap the lane measures on the corpus
   how often the beam selection, the first choice and the top N change,
   over ordinary steps as well as multi-length inputs, and registers the
   result. The record also states the existing class (a) margin
   difference (`gfloat` `log(1.2f)` against the quantised 263 `Cost`
   units), which the swap does not introduce.
5. Gates on bdb: `--issue 921 --issue 927 --issue 594` (sorts, keys,
   n-best), the `table-conf-lambda-*` selection, the predicted and
   punctuation cases; revert-and-check or a named mutation per change;
   kc and tkrzw only if `cells-for-diff.py` selects them.

**Scanners and tools lane** (`oxpinyin-data`, `oxpinyin-user`,
`oxpinyin-training`, `oxpinyin-kmm`; rows 21, 23–25, 29, 30):

1. `table.conf` and `user.conf`: plain Rust parsing of the product
   grammar (line headers, decimal integers in range, a decimal λ to
   `f32`, six-word rows); drop hexadecimal and special float spellings,
   saturation, the 32-bit store, the 255-byte cut, cross-line words and
   the `guint8` addon wrap (an out-of-range index is refused). For
   `user.conf`, `str::parse` and an exact token match: an out-of-range
   number or a NUL in the token makes the marker non-conforming and the
   profile resets as for any bad marker; recorded.
2. Tool fields: parse decimal tokens and counts; a field that is not a
   decimal integer is a line error; the empty line stays the null token.
   `printf_f` keeps six decimals and prints Rust's spelling for NaN and
   infinity.
3. `decode_puncts` reads whole words only; `write_chunk` writes the
   payload with `write_all`.
4. Gates on bdb: `--issue 919 --issue 920` (scanners, both facades),
   `--issue 1144` (punctuation), `--issue 929` (saves), the tool
   differentials of #926; all three cells for any file under
   `oxpinyin-store` or as `cells-for-diff.py` selects.

**Planned, outside the lanes** (row 33's comparator; it does not change
the cleanup):

- An upstream report entry in `upstream-report-drafts.md` on
  `trellis_value_less_than` (`phonetic_lookup.h:65-91`): the two-way
  contradiction (for adjacent lengths, the longer value is always "less"
  through the shorter-sentence clause, and the shorter is "less" when the
  longer is better by more than `log(1.2)`); the cycles for lengths
  1/1/2 and 1/2/3; the reverse adjacent-length rule (`:75-77`), which the
  shorter-sentence clause (`:87-88`) masks, so it never changes a result;
  the C++ standard's precondition that `make_heap`, `push_heap` and
  `pop_heap` take a strict weak ordering; and two independent fixes, the
  minimal one (exclude adjacent lengths from the shorter-sentence
  fallback, which still leaves the 1/2/3 cycles) and a unified score.

**Not in either lane:** row 34 needs no change; the excluded rows 7 and 8
stay with #1279.

### Status on main (2026-10-29 UTC)

Reconciled at `main` `9015a178` by reading the code and the records.
Nothing was built or run. The table below gives one line for each
language-artefact row or artefact part above.

**Rulings that bear on this table.**

- The frequency-comparator wrap (old rows 11–14 and 17) is now ruled.
  The maintainer ruling of 2026-10-29 UTC, "No ported external-library
  internals", is in the [source policy](../source-policy.md#internal-language-artefacts)
  and in [compatibility-policy.md](compatibility-policy.md) under the
  2026-10-28 amendment. Candidate-frequency sorting uses ordinary
  numerical comparison.

| Row | Status |
| --- | --- |
| 11 | **Tails overtaken; the rest open under the 2026-10-29 ruling.** The tails are overtaken by S3 (#1373, `8cf8e23`, [trellis-tail-sort-s3.md](trellis-tail-sort-s3.md)): they now use a stable `sort_by`. The candidate and predicted uses of `nbest::glib_merge_sort` are open; they are replaced as inventory item A below. |
| 12 | **Overtaken** by S3 (#1373, `8cf8e23`). `tail_compare`, `c_float_to_gint` and the `poss` round trip are gone. `tail_merge` now sorts by raw `Cost`, with the amendment's NaN-last order. The float scale itself belongs to #1378. |
| 13 | **Open under the 2026-10-29 ruling.** The wrapping difference in `RankKey::pin_compare` and `glib_rank_sort` are replaced as inventory item A. Separately, the `pinyin_span as u8` cut in `RankKey::for_sort_word` (#1275) is open; see item 4 below. |
| 14 | **Open under the 2026-10-29 ruling.** `capi/src/predict.rs` `sort_predicted` is replaced as inventory item A. |
| 17 | **Comparison ruled; value closed by #1416.** The comparison of the key is closed by the ruling (rows 13 and 14). The ruling keeps the scaled `guint32` value as a ranking key. `amplified_frequency_with_bigram` computes it in `f32` and stores it with Rust's `as u32`; the out-of-range conversion C leaves undefined is class (b), recorded in `upstream-divergences.md` (the "Pinyin candidate frequency" and "Zhuyin candidate frequency" entries). |
| 21 | **Done, then overtaken.** Done in `0147535`, [1297-chunk-payload](1297-chunk-payload.md): the payload uses `write_all`. Overtaken by the 2026-10-28 UTC external-I/O ruling: the record is a revert target, and stage 2 of [external-io-boundary.md](external-io-boundary.md) restores one host `write`. `wip/stage1-io` does not cover it. |
| 23 | **Done and standing.** Done in `0147535`, [1297-punct-words](1297-punct-words.md). The external-I/O ruling keeps it: whole-word decoding is the class (b)/(c) treatment of the pin's undefined read. |
| 24 | **Merged in #1404.** Done in `0147535`, recorded in [1297-table-lines](1297-table-lines.md), [1297-table-words](1297-table-words.md), [1297-table-decimals](1297-table-decimals.md), [1297-table-floats](1297-table-floats.md) and [1297-addon-index](1297-addon-index.md). Overtaken by the external-I/O ruling. #1404 (external-I/O stage 1, merged as `9015a178`) reverted all five divergences through `oxpinyin-input`, replacing the interim host-libc `read_lambda` from `f9663f1` as well. Its status lines and register entries are preserved. |
| 25 | **Merged in #1404.** Done in `0147535`, recorded in [1297-user-decimals](1297-user-decimals.md) and [1297-user-token](1297-user-token.md). Overtaken by the external-I/O ruling. #1404 (merged as `9015a178`) reverted both divergences through `oxpinyin-input`. Its status lines and register entries are preserved. |
| 28 | **Done before #1297, then overtaken.** `interp_grammar::is_graph` was already Rust's predicate ([interpolation2-grammar.md](interpolation2-grammar.md), 2026-09-10). Overtaken by the external-I/O ruling and the 2026-10-28 UTC tools drop-in ruling: it is a revert target to GLib's `g_unichar_isgraph` at stage 5. |
| 29 | **Done, then overtaken.** Done in `0147535`, [1297-tool-decimals](1297-tool-decimals.md): `kmm::numeric::decimal_field` replaced `cnum.rs`. Overtaken by the external-I/O and tools drop-in rulings: host `atoi`/`atol` at stage 5. |
| 30 | **Done, then overtaken.** Done in `0147535`, [1297-tool-special-values](1297-tool-special-values.md): `numeric::format_six_decimals` with Rust spellings. Overtaken by the external-I/O and tools drop-in rulings: the pin's `printf("%f")` at stage 5. |
| 33 | **Open under the 2026-10-29 ruling.** The pinyin comparator is overtaken by S1/S2 (#1352, `49fbc45`, [trellis-adjusted-cost-s1-s2.md](trellis-adjusted-cost-s1-s2.md)). The `stl_heap.h` port is still in place for both facades and is replaced as inventory item B, with zhuyin as ruled. |
| 34 | **Stored widths: no change.** The on-disk `guint32` and `guint8` widths stay. |

**Open Phase 2 items not owned elsewhere.** Items 1–4 were missing from
the 2026-10-26 table. Their pin cites are at `074a2219`, read from a
fetch of that commit with `HEAD` checked against the SHA. Items 1–3 and
5 are now settled by the 2026-10-29 ruling and are scheduled as
inventory items C, D and B below.

1. **`oxpinyin-core/src/single_gram.rs` `lower_bound`, `lower_bound_records`,
   `find`.** These reproduce libstdc++'s halving probe sequence for
   `std_lite::lower_bound` (`src/storage/ngram.cpp:161,185,215,239,259`;
   `src/include/stl_lite.h:29`). They run over gram values the pin never
   checks for order. This is inventory item C.
2. **`oxpinyin-user/src/pinyin_chain.rs` `lower_bound`, `upper_bound`,
   `equal_range`.** These are the same probe sequences for
   `std_lite::equal_range` (`src/storage/chewing_large_table2.h:149,202,216,241`;
   `stl_lite.h:35`), over stored KC and tkrzw entry chunks (#785). This
   is inventory item D.
3. **The zhuyin trellis comparator: `nbest.rs` `zhuyin_loses_to`, under
   the `heap_*` port.**
   - Pin cites: `trellis_value_less_than<1>` (`src/lookup/phonetic_lookup.h:65-91`),
     selected by `get_top_results<1>` (`:141-172`) at `:771` and `:335`,
     for `PhoneticLookup<1, 1>` (`src/zhuyin.cpp:50`).
   - Strict weak order: it is one only while no same-length value is NaN.
   - How NaN arises: with λ outside `[0, 1]`, a unigram step takes `log`
     of a negative product (`:663`, `unigram_lambda = 1 − λ`), while a
     bigram step can stay finite (`:692-693`).
   - The 2026-10-29 ruling sets the zhuyin key. This is inventory item B.
4. **The 8-bit span key.** `RankKey::for_sort_word` reproduces
   `guint8 len = m_end - m_begin` (`src/pinyin.cpp:1692-1697`). With
   `ni`, 251 apostrophes and `hao`, the span is 256 bytes and the key
   wraps to 0. Under `SORT_BY_PINYIN_LENGTH` the pin ranks 你好 after 你;
   the full span would rank it first. The existing cases are
   `pinyin-length-wrap/span-256`, `span-257` and `span-512`.
   Replacement and record remain.
5. **The pinyin `heap_*` port.** Inventory item B.

**Owned elsewhere:**

- The float scale belongs to #1378.
- Rows 24 and 25, with their seven config records, were reverted by #1404, merged as `9015a178`. Rows 21 and 28–30 belong to the later external-I/O stages.
- The C-string rows 7 and 8 belong to #1279.

### Executable copies of external-library algorithms (2026-10-29 UTC)

This inventory follows the maintainer ruling of 2026-10-29 UTC, "No
ported external-library internals". It was taken at `main` `0f1df85` and
covers `crates/` and `tools/`. Nothing was built or run.

- **Search.** The search looked for code that transcribes GLib,
  libstdc++, libc++ or glibc algorithms to keep their comparison or probe
  order: hand-written sorts, heaps and binary searches, and the comments
  that cite those libraries.
- **No other copies.** None exist outside the five items below. The
  matches under `tools/bisection/coverage_review_rules/` are review prose.
  `tools/audit/glib_call_sites.py` lists call sites; it does not port
  them.
- **Test-only references.** Two test-only transcriptions pin the old
  behaviour and go with their items: `old_lower_bound` in the
  `single_gram` tests, and the `msort_with_tmp` transcription in
  `glib_merge_sort_tests`.

**Compliant, not ports:**

- `oxpinyin-data/src/table_entries.rs` calls `Vec::binary_search` on a
  token list that its own `insert` keeps sorted. That is the standard
  API with its documented semantics.
- The `oxpinyin-capi-marshal` UTF-8 codecs are ABI-prescribed boundary
  code, not external-algorithm ports: `input.rs` `utf8_to_ucs4`,
  `ucs4.rs` `decode`, and `phrase_text.rs` `encode_words`/`encode_word`.
  They give the C strings the bytes GLib's `g_utf8_to_ucs4` and
  `g_ucs4_to_utf8` would.

**Schedule.**

- The prerequisite #1404 (external-I/O stage 1) has merged as `9015a178`. The
  `nbest.rs` items (A's merge, B) are coordinated with #1378 PR B, which
  moves the trellis to `f32`.
- Each replacement PR measures time, allocations, instruction counts and
  stripped size against base.
- This section changes no code and no test expectation.

| Item | Copy and callers | Replacement | Parity | Inputs that differ; cases that become `POLICY-ACCEPTED` | Tests that pin the old behaviour |
| --- | --- | --- | --- | --- | --- |
| A | `oxpinyin-engine/src/nbest.rs` `glib_merge_sort`, the GLib merge transcription, formerly called by candidate `glib_rank_sort` and prediction `sort_predicted` (`pinyin.cpp:1678-1709,2286,2440-2445`; `zhuyin.cpp:1129-1144,1529,1610` at the pin). | Stable std sorting with the existing enabled length/span fields and native `rhs.wrapping_sub(lhs) as i32` frequency comparison. Guard each leading-key tie group; use stable numeric descending fallback only on cyclic groups. | Exactly where every leading-key tie group's frequencies fit in a circular interval narrower than 2³¹; this includes wraparound across zero. Antipodal pairs are inconsistent and rejected by the guard. λ alone does not determine consistency. | Only cyclic groups may differ. Existing λ cases are attributed individually in [the item A finding](1297-candidate-numeric-sort.md); the four `row58-signed-{score,duplicate}-{off,on}` cases must MATCH, never POLICY-ACCEPTED. | Guard tests compare independent order laws at boundary triples and cover empty lists, ties, antipodal pairs, cycles and leading-field isolation. Stable ties, disabled keys, dedup alignment and eight-bit span tests remain. |
| B | `nbest.rs` `heap_push`, `heap_adjust`, `heap_pop`, `heap_make`, `heap_top`, a transcription of libstdc++ `stl_heap.h`. Callers: the two-slot node insert (pin `src/lookup/phonetic_lookup_heap.h:56-81`), beam selection (`heap_top` with `NBEAM`; pin `src/lookup/phonetic_lookup.h:770-771`) and final-tail selection (`heap_top` then `tail_merge`; pin `:331-337`). Pinyin compares with `node_loses_to`/`selection_loses_to`, zhuyin with `zhuyin_loses_to`. | Beam and tails: select-then-sort-the-prefix on a complete key (design note 2). Node: direct two-slot transitions (design note 3). | **Pinyin: everywhere.** The S1/S2 key (adjusted S, then batch position) is already total, and the node transitions are derived from the pin. **Zhuyin:** wherever the comparator is a strict weak order and no two values tie on both length and score. | **Zhuyin only.** (1) λ outside `[0, 1]`, where NaN and finite scores of one length meet in one selection and libstdc++'s sequence decides. (2) Exact ties of length and score, where the heap's positional order differs from batch order. Candidates: the `-zhuyin` `table-conf-lambda-*` cases, `nbest-user-table-order-zhuyin` and `glib-sort-tails-zhuyin`; the PR attributes each. | `the_zhuyin_shape_has_its_own_single_tail_heap_order` and `zhuyin_comparison_is_unchanged_for_generated_values`. The pinyin tests pin ruled behaviour and must still pass: `a_full_store_evicts_its_best_cost_and_preserves_slots`, `a_full_node_rejects_ties_without_a_batch_position`, `beam_and_tails_break_ties_by_the_complete_batch_position`, `selection_orders_special_costs_before_breaking_batch_ties`, `backtracking_uses_the_physical_slot_after_batch_selection`. |
| C | `oxpinyin-core/src/single_gram.rs` `lower_bound_records`, a transcription of libstdc++ `__lower_bound`, behind `lower_bound` and `find`. It serves `get_freq`, `set_freq`, `insert_freq`, `remove_freq` and the search walk (pin `src/storage/ngram.cpp:161,185,215,239,259`; `src/include/stl_lite.h:29`). | `records.partition_point(\|r\| record_token(r) < token)` on the existing `[[u8; ITEM_LEN]]` view. Unpartitioned stored data is deliberately not checked for order, sorted or cleaned because a per-lookup sortedness check would make the search linear; on such data, `partition_point` stays memory-safe and deterministic, but its result is unspecified and not the pin's; observable differences are recorded as `POLICY-ACCEPTED` ([#1405](https://github.com/shenghaoc/oxpinyin/pull/1405)), and the existing structural checks for truncated records, invalid lengths and other unsafe conditions still apply in full. | Wherever `item < probe` is partitioned over the stored items. That includes every gram the pin's writers produce, since `insert_freq` inserts at the lower bound (`ngram.cpp:185-200`). | Stored grams where an item below the probe follows one at or above it: foreign or hand-edited DBM values, or a user gram stored out of order and interleaved by `merge_single_gram` (`ngram.cpp:308-349`). The standard library leaves `partition_point` unspecified on an unpartitioned slice. Its answer is deterministic for one toolchain, and the record names the toolchain. Nothing is sorted or cleaned. Candidates: `user-gram-unsorted-phrase-segment`, whose layout is chosen to need the halving path. `user-gram-unsorted-guess`, `-dynamic-adjust`, `user-gram-unsorted-range-boundary` and `system-gram-unsorted-guess` are re-derived and attributed. | `record_views_preserve_the_byte_search` (against `old_lower_bound` on reversed arrays), `an_unsorted_pair_misses_and_inserts_a_duplicate`, `set_freq_can_miss_the_item_insert_freq_added`, and the "later smaller items" clause of `checked_search_asserts_short_and_faults_past_a_partial_tail`. |
| D | `oxpinyin-user/src/pinyin_chain.rs` `Entry::lower_bound`, `upper_bound`, `equal_range`, a transcription of libstdc++ `equal_range`. Callers: `Entry::add_index` and `Entry::remove_index` (pin `src/storage/chewing_large_table2.h:208-255`, `phrase_exact_less_than2`). | Two `partition_point`s over `as_chunks::<N>()` (design note 4). Unpartitioned stored data is deliberately not checked for order, sorted or cleaned because a per-lookup sortedness check would make the search linear; on such data, `partition_point` stays memory-safe and deterministic, but its result is unspecified and not the pin's; observable differences are recorded as `POLICY-ACCEPTED` ([#1405](https://github.com/shenghaoc/oxpinyin/pull/1405)), and the existing structural checks for truncated records, invalid lengths and other unsafe conditions still apply in full. | Wherever the entry's items are ordered by `phrase_exact_less_than2`, as the pin's `add_index` keeps them (`:216-229`). | KC and tkrzw entry chunks whose order a mask walk broke by leaving a previous record's tail (#785, module docs). Candidates: the #785 mask families in `tools/bisection/contract_user_index_phrase_mask.py` on the kc and tkrzw cells. | `pinyin_chain.rs` `entry_ops_keep_the_pins_order_and_report_presence` covers sorted entries and should hold. The #785 mask cases are re-derived. |
| E | `tools/bisection/contract_gram_lookup.py` `_pin_lower_bound`, a Python transcription of libstdc++ `__lower_bound`. Its one caller, `_interleave_miss_layout`, chooses the user gram for `user-gram-unsorted-phrase-segment`. | Remove the executable search. Store the layout it selects on the frozen system data as fixture data. | The case's captured pin observations and provenance stay: the docstrings citing `ngram.cpp:239,308-349` and `stl_lite.h:29`. | The case becomes `POLICY-ACCEPTED` when item C lands, if the subject's answer differs. | The `contract_gram_lookup` fixture: `_interleave_miss_layout` and the case registration in `contract_gram_lookup.test.py`. The fixture keeps its captured data and provenance. |

**Item A implementation.** Candidate lists now use stable `slice::sort_by`
over cached enabled keys and retain the dedup-only `m_freq`; predicted lists
sort their rows directly. `glib_merge_sort`, its test-only transcription and
the GLib-specific candidate-sort name are removed. The eight-bit pinyin span
and its defined unsigned wrap remain contract. The maintainer decision dated
2026-10-30 also retains the frequency comparison as
`rhs.wrapping_sub(lhs) as i32`. A linear guard on each equal length/span
group proves consistency before stable std sorting; only cyclic groups use
defined numeric-descending fallback. The λ-only parity premise in the
original row A is superseded: wrapped/failed-token total updates can produce
large scores at stock λ. All four `row58-signed-{score,duplicate}-{off,on}`
cases must MATCH and are never registered as differences. The proof and
correction's gate status are in
[1297-candidate-numeric-sort](1297-candidate-numeric-sort.md).

**Design notes.** All cites are at the pin.

**Item B: std selection and direct node slots (2026-10-30T05:29:46Z).**
The direct parent is #1414 at
`32a7c378280c68d73be47e4e78ea920d6a7d77e3`, on main
`575a65cb060377d984e64a19a9e4f5ad1b50feaa`. B is one commit above it;
the old A commit `c05a0fec` is not retained. B's implementation, tests and
named mutation are unchanged from published `7dd13491`. A's guarded
wrapping comparator and shared frequency-sort implementation are unchanged.

`nbest.rs` removes all five `heap_*` transcriptions. `Node::keep` writes
physical slots directly. Beam and final-tail selection call the same
fixed-row `select_top(Vec<BeamEntry>, count, nstore)` helper and comparator
closure; the mode is captured, rather than adding per-facade sort bodies.
A sorts `(RankKey, u32)` and `Predicted` by frequency. B sorts `BeamEntry`
by adjusted/raw f32 score and batch position; reusing A's instantiations
would change the row representation or ordering. S3's stable raw-score
`tail_merge` is unchanged, as is zhuyin's raw single-slot admission.

The pin was read from Git tree `074a2219c90feaf962d0d24f034514033ece5f99`:
`src/lookup/phonetic_lookup_heap.h` blob `299ddedfa55e` and
`src/lookup/phonetic_lookup.h` blob `c092e7610cb0`. Two-slot transitions
come from `phonetic_lookup_heap.h:56-81`; the single-slot rule is
`:108-122`. `number()` (`:49-53`) supplies physical slots during expansion
(`phonetic_lookup.h:282-293`); successors store them at `:665,695` and
advance to later steps at `:667,697`, after predecessor slots are final.

Closed #1314 and the retained `wip/1297-ranking*` checkpoints supplied
reusable node-slot analysis and test scenarios. Its loser tree retained a
cyclic comparator and established no speed gain, while leaving peak heap
unchanged and growing stripped libraries by 4–12 KiB. That implementation
and its measurements are not adopted.

**Measured choice (comparison at B `7dd13491`).** Keep selection followed
by sorting the retained prefix.
The maintained Criterion comparison uses the production `BeamEntry` layout
and complete keys, both facades' beam/tail limits, mixed IEEE-special and
tied batches of 8/32/128/512, 20 samples, one-second warmup and collection,
with cloning outside timing. Selection has the lower point estimate in
29/32 pairs; the unweighted geometric mean of select/heap estimates is
0.4905, a fixture summary rather than a corpus result. BinaryHeap wins
pinyin beam/512-mixed (select/heap 1.357), tail/32-tied (1.199) and
tail/512-mixed (1.396). All 16 zhuyin fixtures favour selection.
Selection costs O(m + k log k), in place over the existing batch without
a separate result allocation. Heapify-and-pop costs O(m + k log m) and
O(k) separate result storage. Direct node transitions cost O(1) time/space.
The test-only heap and Criterion code do not enter shipping libraries.

**Current restack gate.** Rule 7 on the touched engine crate passes:
238 tests, two ignored; all-target Clippy with warnings denied, fmt and
parent-relative whitespace checks pass. All six shipping inputs below are
rebuilt from this combined tree. The unchanged B implementation does not
repeat the earlier differential, mutation, algorithm or sentence/typing
campaigns under the maintainer's proportional re-gate instruction.

**Earlier functional gate, B `7dd13491` on A `7d6a9e85`.** Ubuntu BDB
debug full/default registry:
**3,309 cases: 3,287 MATCH, 20 existing POLICY-ACCEPTED, two backend-format
skips, zero DIFFER**. All four `row58-signed-{score,duplicate}-{off,on}`
witnesses MATCH; A's cyclic classifications and #1378 records remain valid.
No registration, digest or accepted scope changed. The path selector chooses
BDB only. Crate gates for engine, runtime, facade, both C adapters, dictool
and oracle pass: 588 tests, eight ignored; all-target Clippy denies warnings.
The three targeted contract harnesses pass all 26 tests; fmt passes.

The earlier named mutation `1297-reverse-selection-batch-position.patch`: both
`nbest-user-table-order-*` witnesses DIFFER, both `glib-sort-tails-*`
controls MATCH. Byte-for-byte source restoration, rebuild and the same
four cases give two existing POLICY-ACCEPTED and two MATCH.

**Shipping and policies against A `32a7c378`.** All six policies were regenerated unconditionally
from backend-specific shipping inputs and the unchanged pin-provisioned
profiles, in two independent byte-identical passes (396 collections,
nine admitted workers). Every output also matches A's policy byte for byte.
All six resolve and all twelve A/B linked objects are ordered: pinyin/zhuyin
path counts BDB 280/261, KC 362/344, sound-tkrzw 292/274. The tkrzw health
probe passes, and both pin and subject facades resolve the cache library.

| Backend / library | A bytes | B bytes | Increase |
| --- | ---: | ---: | ---: |
| bdb/pinyin | 2,474,792 | 2,495,272 | 20,480 |
| bdb/zhuyin | 2,204,456 | 2,229,032 | 24,576 |
| kc/pinyin | 2,833,672 | 2,854,152 | 20,480 |
| kc/zhuyin | 2,563,336 | 2,596,104 | 32,768 |
| tkrzw/pinyin | 2,515,744 | 2,536,224 | 20,480 |
| tkrzw/zhuyin | 2,249,504 | 2,274,080 | 24,576 |


`nm --size-sort --print-size --defined-only --demangle=rust`, counting each
physical `(address, size)` text symbol once, gives the same attribution in
all six libraries: `select_top` 3,360 B plus std selection/sort helpers
19,921 B; removed heap text −450 B; rewritten
`nbest_sentences_with_seeds_generation` −1,731 B and `expand_one` +25 B.
Net unique-text growth is 21,125 B. The largest new helpers are
`select::median_of_medians` (4,902 B), unstable quicksort (3,965 B) and
`small_sort_general` (2,219 B), all for the same `BeamEntry`/selector closure.
A's physical frequency-sort totals remain 21,950 B pinyin / 11,091 B zhuyin
on every backend, with no multiplied A pipelines. ELF `.text` grows
21,200 B (21,184 B on KC), plus unwind/relocation/constant data and alignment;
these section changes and stripped-file deltas are distinct from symbol sums.
The larger standard-library implementation is the binary-size cost of this
measured choice; the shared B helper avoids separate beam/tail/mode pipelines.

**Historical same-run campaigns — measured against A at `7d6a9e85`.**
B is the production tree of `7dd13491`; these results were not rerun or
relabeled as a comparison against `32a7c378`. Ubuntu 26.10 linux/amd64, Intel i7-9750H,
Rust 1.97.1, GNU ld 2.47, Valgrind 3.27.1; ext4 work/artifacts/TMPDIR,
shared heavy admission, CPU 0, default `nihaozhongguo`, options `0x18a`.
Plain shipping features are `bdb,shipped` for pinyin and `bdb` for zhuyin.
Profiled pinyin differs only by `RUSTFLAGS="-Cdebuginfo=1 -Cstrip=none"`;
DWARF, both code-neutrality checks, loader and linked-order guards pass.
The historical A shipping/profile objects have the identical production inputs
of its documentation-only `7d6a9e85` head. A, B, pin and both same-byte
controls run together in each campaign. All complete observations agree.

| Workload | A Ir | B Ir | Pin Ir | Qualification |
| --- | ---: | ---: | ---: | --- |
| Sentence | 2,470,913 | 2,418,714 | 3,825,927 | VALID; exact controls |
| Complete typing sequence | 65,215,125–65,217,439 | 63,983,718–63,986,972 | 77,108,897 | DIAGNOSTIC; RandomState scopes only |

Three rotating fresh-process rounds, one invocation per round. Typing ranges
include the corresponding same-byte control. Sentence
saves exactly 52,199 Ir. Typing's control spreads are 2,314 / 3,254 Ir;
its ranges are diagnostic and do not establish an exact instruction gain.
Native time uses plain shipping objects, 40 rotating rounds × 20 iterations,
with shared paired-bootstrap 95% intervals and valid controls for both:

| Workload | A median µs | B median µs | Pin median µs | Paired B/A, 95% interval |
| --- | ---: | ---: | ---: | --- |
| Sentence | 596.135 | 593.981 | 747.074 | 0.9878 [0.9673, 0.9967] |
| Complete typing sequence | 13,335.380 | 13,236.133 | 14,383.075 | 0.9804 [0.9668, 1.0085] |

Native control intervals (A-copy/A; B-copy/B) are sentence
[0.9821, 1.0203] / [0.9841, 1.0133], typing
[0.9822, 1.0027] / [0.9796, 1.0109], within the maintained [0.95, 1.05]
rule. Typing's timing interval includes one, so it establishes no timing
gain. These campaigns do not measure heap usage or establish a corpus-wide
performance improvement. C ABI, ownership and persistent formats are unchanged.

Reproduce the current rule-7 gate with `just heavy -- cargo test --locked
-p oxpinyin-engine` and `just heavy -- cargo clippy --locked -p
oxpinyin-engine --all-targets -- -D warnings`. Reproduce the historical full
gate/campaigns at B `7dd13491`, A `7d6a9e85`, with the seven-package
selection above through `just heavy`, `just heavy -- just cell-artifacts --artifacts
pinyin-capi,zhuyin-capi`, and
`just heavy -- just runner contract-diff bdb "$PINYIN_ORACLE_PREFIX" "$B_PY_SO" "$B_ZH_SO"`.
Apply/reverse the named mutation and select its four explicit cases.
The measured choice above came from the ignored Criterion test
`measure_selection_algorithms`, run at B `7dd13491` and since removed.
Use [policy regeneration](../packaging.md#regeneration) with
`repeat_output_dir` for every backend, then the maintained linked-layout
checker and `strip --strip-unneeded`. Run both
[sentence/typing campaigns](../runbooks/benches.md#sentence-and-typing-sequence-callgrind-campaign)
with exact A/B/pin objects, plain-shipping counterparts, A/B policy overrides,
`--native-rounds 40 --native-iterations 20` and new output directories.

1. **Candidate sort key.** The key is explicit, with three fields
   compared in order: phrase length, pinyin span, frequency. Each is
   length/span descending, frequency by the guarded wrapping comparison,
   and a field whose sort bit is clear is zero
   (`pinyin.cpp:1684-1706`; zhuyin uses phrase length and frequency,
   `zhuyin.cpp:1134-1143`). Do not use derived `RankKey::Ord`: it orders
   ascending, and it includes `m_freq`, which the dedup reads
   (`pinyin.cpp:2092,2130`) but the sort ignores when `SORT_BY_FREQUENCY`
   is clear. The dedup keeps reading `m_freq`.
2. **Selection keys.** Use complete keys.
   - Pinyin: adjusted S, compared by `ordered_cost_compare(adjusted_cost(..))`,
     then batch position.
   - Zhuyin, as ruled: shorter `m_sentence_length` first, as the pin's
     length clause has it (`phonetic_lookup.h:86-88`). Then raw score
     descending, with NaN last and signed zeros equal, then earlier batch
     position.
   - Beam (32, `:37`) and tails (`nbest` 3 or 1): replace `heap_top` with
     `select_nth_unstable_by` at `k − 1`, then sort the `k`-prefix. That
     is O(m + k log k) time and no extra space. The PR compares it
     against `std::collections::BinaryHeap` on identical keys:
     heapify-and-pop is O(m + k log m), and a bounded heap is
     O(m log k) time and O(k) space. It keeps the faster one.
   - Tails are then ordered by S3's `tail_merge`, unchanged.
3. **Two-slot node** (`phonetic_lookup_heap.h:56-81`, `nstore = 2`). The
   comparator is "loses to", so the front is the better value.
   - **Empty node:** store A in slot 0.
   - **One value A, newcomer X:** `push_heap` moves X to the front only
     when A loses to X (`:65`). The result is `[X, A]` if X is strictly
     better, otherwise `[A, X]`. A tie keeps A in front.
   - **Full node `[F, B]`, newcomer X:** X is admitted only when F loses
     to X, that is, when X is strictly better than the front (`:73`).
     Then:
     - `pop_heap` on two elements swaps them, giving `[B, F]` (`:74`);
     - slot 1 is overwritten with X, which evicts F, the best value
       (`:75`);
     - `push_heap` moves X in front of B (`:76`), giving `[X, B]`.
     A tying or worse X is rejected.
   - **Back-pointers.** `number()` assigns `m_current_index` as the
     physical slot (`:49-53`). This happens when `get_candidates` expands
     step `i` (`phonetic_lookup.h:282-293`, called at `:770`). Successors
     record it as `m_sub_index` (`:665,695`). Every successor lands at a
     later step (`save_next_step(end, …)` with `end > start`, `:667,697`),
     so a node's slots are final before any back-pointer is taken. The
     S1/S2 ruling's "rejects a tying newcomer" is this rule.
   - **Zhuyin.** The single-slot node (`:108-122`) is unchanged.
4. **`pinyin_chain` search.**
   - The stride is `item2_stride(len)`, one of 8, 12, …, 36 bytes, for
     lengths 1–16 (`row_format::pinyin_index::item2_stride`).
   - Dispatch with a `match` to a generic `search::<N>` over
     `bytes.as_chunks::<N>()` (stable since Rust 1.88; the toolchain is
     1.97.1). Then call `partition_point` twice, for the lower and the
     upper bound, with `exact_compare2` reading the packed key words in
     place from each `[u8; N]` chunk.
   - There is no per-search decoding: no `Vec<ChewingKey>` per probe, as
     `Entry::keys` builds today, and no decode of the record before the
     search.
   - The `as_chunks` remainder is dropped, as `count()` floors today.
5. **Float keys** (with #1378 PR B, when scores become `f32`).
   `f32::total_cmp` alone would order `-0.0` before `+0.0` and order NaNs
   by sign, so the comparator handles both explicitly:
   - NaN against NaN is equal, and NaN against a number is last;
   - otherwise compare `(b + 0.0).total_cmp(&(a + 0.0))` for descending
     order. Adding `+0.0` maps `-0.0` to `+0.0` under round-to-nearest.
   - Until PR B, the `Cost`-scale equivalent is `ordered_cost_compare`'s
     class order.

**Function-order policy paths the removals touch.** All six policies are
affected: `tools/packaging/function-order/{bdb,kc,tkrzw}/lib{pinyin,zhuyin}.order`.

| Removal | Policy lines |
| --- | --- |
| Item A | `oxpinyin_engine::nbest::glib_merge_sort::<(oxpinyin_engine::session::RankKey, u32), oxpinyin_engine::session::glib_rank_sort::{closure#0}>` and `oxpinyin_engine::session::glib_rank_sort` |
| Items B–D | No policy line names `heap_*`, `single_gram::lower_bound*` or the `pinyin_chain` searches. Their PRs still run `tools/packaging/function_order.py` against all six policies. |
| Item E | Tool code only; no policy. |

Item A therefore regenerates the bdb, kc and tkrzw policies in the same
PR, following [docs/packaging.md, "Regeneration"](../packaging.md#regeneration).

## C-maths imitation — record only

No production code is changed here and no child issue is opened for a
`std` call. The list is the places where oxpinyin currently expresses
C float/logarithm behaviour itself instead of calling a `std` method
directly:

| Site | What it imitates | Direct `std` expression | Disposition |
| --- | --- | --- | --- |
| `core/src/cost.rs` `log2_fixed`, `surprisal`, `cost_of_probability` | the pin's `-log2` over a `gfloat` probability; integer fixed-point repeated squaring | `f64::log2` (or `f64::ln` with the `LN_2` factor) | deliberate determinism; class (a) record row 11; no replacement child |
| `engine/src/nbest.rs:285-291` `poss` | reconstructs the pin's `gfloat` cost from the fixed-point integer via `-(cost as f64) * LN_2 / 1000.0` | `f64::exp2` / direct scale conversion | record only; comparator source is #921 |
| `engine/src/nbest.rs:294-310` `c_float_to_gint`, `tail_compare` | x86-64 `cvttss2si` narrowing and `-(lhs - rhs)` as `gint` | none: this is C conversion semantics, not a transcendental | #921 |
| `engine/src/session/mod.rs` `amplified_frequency_with_bigram` (before #1416: `guint32_of_score` and the `predict.rs:482` copy) | C `float` multiply/truncate and the amd64 low-32-bit store | `f32` arithmetic and `as u32` | done in #1416: the low-32-bit imitation is removed; the saturating `as u32` is class (b) on out-of-range scores |

The remaining float sites already call `std` directly:
`engine/src/phrase.rs:309` (`log2`), `training/src/eval/decode.rs:429,471`
and `training/src/segment/trellis.rs:174,209` (`ln`),
`word/src/newword.rs:83` (`ln`), and `kmm/src/model.rs:153` (`powf`).
They are not C-maths imitations.

## Negative coverage

Checked and excluded from the exported surface:

| Area | GLib/libc calls | Why not reachable |
| --- | --- | --- |
| `storage/tag_utility.cpp` | `g_strsplit_set`, `g_unichar_isspace`, `g_unichar_isgraph`, `g_strv_length`, `g_utf8_get_char`, `g_utf8_next_char`, `g_ptr_array_foreach`, `g_hash_table_foreach_steal`, `g_warning`, `g_strdup*`, `g_array_*` | `taglib_*` is called only by `utils/training` and `utils/storage`; no exported entry reaches it. |
| `storage/table_text_parser.h` | `g_ascii_strtoull`, `g_strlcpy` | included by the `load_text` methods; only `utils/storage/gen_binary_files.cpp` calls them. |
| `lookup/pinyin_lookup2.cpp` | `g_array_*`, `g_ptr_array_*`, `g_hash_table_*`, `g_free`, `g_strdup_printf`, `log` | `PinyinLookup2` is never instantiated; `PhraseLookup` uses `FacadePhraseTable3`. |
| `storage/chewing_large_table.cpp` | `g_array_*`, `g_utf8_strlen`, `fprintf` | `ChewingLargeTable` is used only by the unused `FacadeChewingTable`; the runtime uses `ChewingLargeTable2`. |
| `storage/phrase_large_table2.cpp` | `g_array_*`, `g_utf8_strlen`, `g_utf8_to_ucs4`, `g_free` | `PhraseLargeTable2` is used only by the unused `FacadePhraseTable2`; the runtime uses `PhraseLargeTable3`. |
| `storage/facade_chewing_table.h`, `facade_phrase_table2.h` | `mmap`, `fprintf`, `g_array_set_size` | no instance is constructed by an exported path. |
| `load_text` methods | `g_array_*`, `g_utf8_*`, `g_free`, `fprintf` | tool-only through `gen_binary_files`. |
| `PunctTableEntry::escape/append_punctuation/remove_punctuation` | `g_utf8_to_ucs4`, `g_array_*`, `g_strv_contains`, `g_strfreev`, `memcmp`, `fprintf` | no exported entry calls the writer methods; `get_all_punctuations` is the reachable reader. |
| `dump_max_value` / `dump_all_values` / `dump_matrix`; `#if 0` printf | `printf` | calls are commented out or compiled out. |
| `zhuyin_parse_chewing` truncated UTF-8 | `g_utf8_next_char`, `g_utf8_strncpy` | `g_utf8_strncpy` uses `strncpy`, which stops at the NUL and pads; no read past the string. Closed #1145 records the withdrawal. |
| oracle and tests | `glib_sys` use in `pinyin-oracle` and test fixtures | test/oracle-only, not shipped. |

## Conclusions and child order

- No valid-input answer changes in this inventory. The valid-input side
  of every family is C-facing contract; the malformed side is the named
  class or quirk.
- The new child is **#1144**: the punctuation reader is the one
  unregistered semantic difference this audit found.
- Existing children cover the rest: #894, #918, #919, #920, #921, #922,
  #924, #925, #926, #927, #928, #929. #923 remains the STOP for a new
  `LC_NUMERIC` ruling.
- The math record is row 11: `log` is class (a); oxpinyin's fixed-point
  cost is the only direct C-maths imitation, recorded for the record,
  not a replacement child.
- No production code, C ABI, allocation rule, on-disk format or data
  ownership changes in #917.
