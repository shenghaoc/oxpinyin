# GLib and libc at the pin: reachable calls and oxpinyin counterparts

Status: **current reference** (#917, parent #915). The current
classification of oxpinyin's GLib- and libc-shaped code is
[Classification on main](#classification-on-main-1297-2026-10-31-utc).
Pin: libpinyin 2.11.92 at `074a2219c90feaf962d0d24f034514033ece5f99`.
Reference environment: unpinned Debian testing (`tools/oracle/version-manifest.json`), linux/amd64.
Recorded evidence: this inventory read **GLib 2.84.4-3~deb13u5** and **glibc 2.41-12+deb13u4**.
Scope: the 79 `pinyin_*` and 52 `zhuyin_*` exports in
`src/libpinyin.ver` and `src/libzhuyin.ver`, their reachable helpers,
and the oxpinyin `crates/` counterparts. Test, oracle and tool-only code
is listed separately.

## Abstract

The exported surface reaches **70 distinct GLib/libc functions at 1,179
call sites**: 43 GLib functions (874 sites) and 27 libc functions (305
sites). They form fifteen reachable semantic families, plus tool-only and
dead groups. Most reachable boundaries have a dedicated child issue. One
family is new (#1144). The internal container, hash and primitive rows are
ordinary C-facing or internal parity, with no language/runtime quirk. On
the evidence read, no entry changes a valid-input answer.

Three results govern the sweep:

1. Real GLib ownership is ABI, not emulation: `GArray` mutation in
   `pinyin_lookup_tokens` / `pinyin_token_get_nth_pronunciation` and
   their zhuyin twins stays on `glib_sys` (#924).
2. The UTF-8, `fscanf`, path, sorting, filesystem and diagnostic
   boundaries each have a scoped child; the only newly found semantic
   difference is the punctuation-table decoder (#1144).
3. `LC_NUMERIC` is ruled (#923, 2026-10-25 UTC): oxpinyin retains the pin's
   behaviour (row 39). Only a reversal needs a new ruling.

The math surface is `log` only: seven reachable sites, all class (a)
under row 11. oxpinyin's costs use `f64::log2` directly
(`core/src/cost.rs`).

## Reachability and negative coverage

A function is reachable when an exported `pinyin_*` or `zhuyin_*` entry
can call it without a test, tool or uninstantiated class in between.
The matrix below follows that definition. The complete call-site list is
[glib-libc-call-sites.tsv](glib-libc-call-sites.tsv), which records the
family, library, facade, backend, pin file and line, function and call.
Regenerate it with `just audit-glib-call-sites <pin-checkout>`. The
[runbook](../runbooks/glib-call-sites.md) names the tools. The recipe runs
`tools/audit/glib_call_sites.py`, which:

- verifies that the checkout is at
  `074a2219c90feaf962d0d24f034514033ece5f99`;
- parses the pin with libclang;
- reads `facade` from the linker's maps.

The one hand-kept input is `tools/audit/glib_call_sites.families.tsv`.

- 1,179 reachable sites (`pinyin`, `zhuyin` or `both`): 874 GLib, 305 libc.
- 190 `tools` sites, kept only by `utils/` programs: `tag_utility.cpp`,
  `table_text_parser.h`, the `load_text` methods, the `FlexibleBigram` and
  `FlexibleSingleGram` classes behind the training utilities, and the
  punctuation and parser helpers that only `utils/` calls.
- 188 `none` sites, kept by no link: `PinyinLookup2` (never instantiated), the
  old `ChewingLargeTable` / `PhraseLargeTable2` and `FacadeChewingTable` /
  `FacadePhraseTable2` classes, the `PunctTableEntry` writer methods, and
  the debug dumps. Code under `#if 0` is not compiled and has no row.
- Six of the `none` sites are in functions that no link sees at all,
  because nothing calls them: `memory_chunk.h:404`, `facade_chewing_table.h:115,185`,
  `facade_phrase_table2.h:96,165` and `flexible_single_gram.h:136`.

The negative list is in "Negative coverage" below. It also records a
closed candidate for a `zhuyin_parse_chewing` over-read. GLib's
`g_utf8_strncpy` stops at the NUL and pads, so the candidate is not an
over-read.

## Matrix

Every row is at the pin and the image versions above. The columns use
these values:

- `Reach`: `both` for libpinyin and libzhuyin, `pinyin` for the
  prediction-only surfaces. `all` means bdb, kc and tkrzw.
- `Disposition`, in #1142's order: `contract` (C-facing behaviour on valid
  input), `(a)`/`(b)`/`(c)`, `quirk` (record-admitted language/runtime
  difference) and `ruling` (STOP).

| ID | Family and pin call sites | Reach | oxpinyin counterpart | Valid / malformed | Disposition | Child |
| --- | --- | --- | --- | --- | --- | --- |
| G1 | UTF-8/UCS-4 conversion and category: `pinyin.cpp:628,715,1397,1450-1454,2658,3210,3678`; `lookup.cpp:54,58`; `zhuyin.cpp:918,969-973,1684,2165`; `zhuyin_parser2.cpp:871,879`; `punct_table.cpp:48` (dead); `tag_utility.cpp` (tool) | both (punct pinyin only); all | `capi-marshal/src/input.rs` `utf8_to_ucs4` / `parser_prefix` / `scalar_text` (#1279); `capi/src/phrase.rs` `pinyin_phrase_segment`; `data/src/phrase_library.rs` `phrase_text`; `input/src/tool.rs` `unichar_isgraph` (tools); `data/src/row_format.rs` `decode_puncts` | valid: identical strings. malformed/non-scalar: `g_utf8_to_ucs4` accepts surrogates and values above U+10FFFF; Rust `str` cannot. Embedded U+0000 stops the pin's conversion. | valid: contract. malformed: quirk (#894, #918); tool: quirk (#926); punct: quirk/class (b) (#1144) | #894, #918, #925, #926, #1144 |
| G2 | C strings, joins and vectors: `g_strdup` 44, `g_free` 152, `g_strdup_printf` 14, `g_strconcat` 16, `g_strjoinv` 3, `g_strndup` 6, `g_strfreev` 10, `g_strv_length`/`g_strv_contains` 2, `g_string_*` 4 | both (punct subset pinyin only); all | `capi/src/ffi.rs` `strdup` / `key_text` (`g_strdup_printf`) / `strconcat_aux` / `strconcat_pair` / `strjoinv` / `symbol_vector`, the pin's own calls (#1227); `capi/src/aux_matrix.rs` `AuxText`; `capi/src/predict.rs` `prepend_punctuations`; `capi/src/iterators.rs` export rendering; `input/src/path.rs` `build_filename` | valid: identical bytes. malformed: raw path bytes, missing middle aux component, NUL in a diagnostic, duplicate punctuation. | valid: contract. malformed: quirk (#922) or #925; punctuation strings #1144 and punctuation order/duplicates #928, both contract (closed 2026-10-26 UTC) | #922, #924, #925, #928, #1144; export row 1 |
| G3 | Path joining: `g_build_filename`, 68 sites, e.g. `pinyin.cpp:164,252,268,286,308,395,400,952,968,985,1052`; `zhuyin.cpp:118,196,212,230,252,337,342,585,601,605,626,630,684` | both; all | `input/src/path.rs` `build_filename` / `build_path` / `build_bytes` (the pin's `g_build_filename` through `glib-sys`); `capi-marshal/src/input.rs` `filename` (#1279) | valid: same resolved path. malformed: leading `/`, empty name, `..`, duplicate separators, non-UTF-8 bytes. | valid: contract. malformed: quirk (#922) | #922 |
| G4 | GArray/GPtrArray containers: `g_array_*` and `g_ptr_array_*`, 502 sites (`g_array_index` 105, `g_array_new` 64, `g_array_free` 68, `g_ptr_array_index` 59, `g_array_set_size` 49, `g_array_append_val` 45, ...) | both; all | `capi/src/dict.rs` and zhuyin twin keep real `GArray` for the ABI; internal steps are `Vec`/slices in `engine/src/nbest.rs`, `engine/src/session/*`, `data/src/phrase_library.rs` | valid: identical candidate/result arrays. malformed: out-of-range indices, a key with no whole word, a short chunk header. | valid: contract. malformed: class (b) rows; #924 for the ABI arrays | #924 |
| G5 | n-best tail sort: `phonetic_lookup.h:337` `g_ptr_array_sort(tails, trellis_value_compare)` | both; all | `engine/src/nbest.rs` `tail_merge` (stable `sort_by` on raw possibility, NaN last) | valid: the pin's `gint` truncation ties scores under one unit apart, so its comparator is not transitive on finite scores. malformed: NaN and ±∞ from λ outside `[0, 1]`. | language artefact (#1297): S3 in [compatibility-policy.md](compatibility-policy.md#registered-sites) | #921 |
| G6 | candidate and predicted list sorts: `pinyin.cpp:2286,2443` `g_array_sort_with_data(compare_item_with_sort_option)`; `zhuyin.cpp:1529,1610` `g_array_sort`; phrase-string dedup `pinyin.cpp:2067`, `zhuyin.cpp:1375` | both; all | `engine/src/candidate.rs` `rank_by_frequency`, called by `engine/src/session/mod.rs` `rank_candidates` and `capi/src/predict.rs` `sort_predicted` | valid: stable order over the enabled keys and the dedup winner. Frequency groups can be cyclic under the wrapping comparison. | contract: guarded wrapping frequency, [1297-candidate-numeric-sort](1297-candidate-numeric-sort.md); dedup #913/#914 | #927, #928 |
| G7 | hash tables: `g_hash_table_new` 2, `insert` 4, `lookup_extended` 4, `destroy` 2; `g_direct_hash`/`equal` | both; all | `engine/src/nbest.rs` `Step`, `engine/src/session/*` vectors; no exposed iteration order | valid: lookup only; no order reaches the ABI. malformed: none. | contract (internal; no observable order) | — |
| G8 | diagnostics: `g_return_val_if_fail`, 6 sites (`pinyin.cpp:1454`, `zhuyin.cpp:973`, `phrase_index.cpp:354-356,361`); `abort` 45, nearly all after a failed internal check; `g_warning` only in the tool-only tag utility | both; all | `capi/src/ffi.rs` `log_warning`; zhuyin twin; `capi/src/phrase.rs` calls `glib_sys::g_utf8_strlen` | valid: the pin's asserts abort in the C adapters (`std::process::abort()`). malformed: the `g_utf8_strlen` critical is GLib's own; the other five sites are early returns of the pin's value with no critical ([1297-segment-critical](1297-segment-critical.md), [1297-layout-criticals](1297-layout-criticals.md)). | abort where the pin aborts; #924 for the channel; #918 for the segment gate | #918, #924 |
| G9 | allocation/free and real GArray ABI: `g_free` 152, `g_strdup` 44, `g_strdup_printf` 14; C `malloc`/`free`/`realloc` 5; `g_array_set_size`/`g_array_append_vals` into caller arrays in `pinyin.cpp:2805,2817` and `zhuyin.cpp:1797,1809` | both; all | `capi/src/ffi.rs` (both facades): returned strings and the keyboard vector from the pin's GLib calls, a failed allocation aborting as the pin's (#1227); `malloc_cstr_with`, the fallible libc-`malloc` encoder for the `g_ucs4_to_utf8` sites (NULL, no warning); `capi/src/dict.rs` and zhuyin twin on `glib_sys`; `libpinyin.alloc` / `libzhuyin.alloc` | valid: caller may `g_free` / `g_strfreev`; the real `GArray` keeps GLib's private metadata. malformed: NULL or OOM paths. | contract; MUST RETAIN | #924 |
| G10 | rename and sync: `g_rename` 10 (`pinyin.cpp:1059,1078,1094,1108,1121`; `zhuyin.cpp:609,635,655,672,687`); `g_fsync` 1 (`memory_chunk.h:555`) | both; all | `user/src/persistence.rs` `save_reporting`, `pin_rename_order`; `input/src/path.rs` `rename`; `input/src/chunk.rs` `save` | valid: same file order and bytes. failure: per-file write/rename failures and the save's return. | contract on valid profiles; #929 for failure ordering | #929 |
| G11 | numeric parsing and locale: `fscanf` 10 (`table_info.cpp:208,214,220,228,232,255,338,344,352,357`); `atoi` 1 (`:160`); `setlocale` 6 (`:197,291,328,372,378,394`) | both; all | the pin's own calls in `oxpinyin-input` `config.rs` / `native.rs` through the `libc` crate (external-I/O stage 1), decoded by `data/src/table_info.rs` `TableConf::load` and `data/src/user_files.rs` `UserTableInfo::load`/`save` | valid: same table values, versions, λ bits and library mapping. malformed/host: the host libc's `%d`/`%f`/`%255s` scanner, the pin's own call; class (b): missing source buffer and merged-row SIGSEGV. | valid: contract. malformed: the pin's call except the class (b) refusals; host scanner (policy row 78); process side effect ruling (#923) | #919, #920, #923 |
| G12 | file I/O and `MemoryChunk`: `mmap` 1, `munmap` 2, `lseek` 4, `read` 5, `write` 3, `open` 3, `close` 12, `unlink` 20, `fopen` 3, `fclose` 8, `feof` 1, `fprintf` 27 | both; all | `input/src/chunk.rs` `load` / `save` (the pin's `open`/`read`/`mmap`/`write`/`fsync`/`close`); `data/src/phrase_library.rs` `map_chunk`; `user/src/persistence.rs`; `data/src/c_stderr.rs` raw bytes | valid: same mapped extent, header check and file bytes. malformed: short/truncated/directory chunk; write failures. | contract on valid files. malformed: class (b) rows 76/96/109; #929 for writes | rows 76, 96, 109; #929 |
| G13 | C primitives: `memset` 86, `memcpy` 11, `strlen` 16, `strcmp` 18, `memcmp` 3, `memmove` 7, `strchr` 1 | both; all | Rust slices/`str`; `data/src/row_format.rs`, `core/src/single_gram.rs`, `capi/src/ffi.rs` | valid: same bytes. malformed: NULL/unreadable phrase strings and out-of-range walks are class (b) rows. | contract; class (b) at the registered NULL/over-read rows | rows 80, 106 |
| G14 | libm: `log` 7 (`phonetic_lookup.h:39,250,663,693`; `phrase_lookup.cpp:40,321,342`) | both; all | `core/src/cost.rs` `cost_of_probability` / `surprisal` (`f64::log2`); `engine/src/phrase.rs` (`ln`) | valid: class (a) last-bit differences are accepted. | class (a), row 11; record only | no child (math record) |
| G15 | punctuation decoding: `punct_table.cpp:56-72` `unescape`, `:74-94` `get_all_punctuations`, reached at `pinyin.cpp:2474` | pinyin; all | `data/src/row_format.rs` `decode_puncts` (whole stored words per punctuation; infallible); `data/src/punct.rs` `PunctTable::punctuations` (`Err` only for a failed read; `Ok(Vec::new())` for unopened/absent/empty); `runtime/src/lib.rs` `RuntimeDict::punctuations`; `capi/src/predict.rs` `prepend_punctuations` (`phrase_text_bytes` at the C boundary) | valid: identical punctuation strings. malformed: partitions below; misaligned lengths read whole words ([1297-punct-words](1297-punct-words.md)). | contract; class (b) for the unterminated over-read, registered (#1144) | **#1144** |
| G16 | tool-only GLib: `tag_utility.cpp` (`g_strsplit_set`, `g_unichar_*`, `g_strv_length`, `g_utf8_get_char/next_char`, `g_ptr_array_foreach`, `g_hash_table_foreach_steal`, `g_warning`); `table_text_parser.h` (`g_ascii_strtoull`, `g_strlcpy`); the `load_text` methods | not C ABI | `kmm/src/taglib.rs` (the pin's taglib); `input/src/tool.rs` (`g_strsplit_set`, `g_unichar_isgraph`, `atoi`, `atol`, `fprintf` through `glib-sys` and `libc`) | valid: tool input, identical records. malformed: the pin's calls, so its results. | tool scope: the pin's calls ([external I/O](external-io-boundary.md)) | #926 |
| G17 | dead code: `pinyin_lookup2.cpp`, `chewing_large_table.cpp`, `phrase_large_table2.cpp`, `facade_chewing_table.h`, `facade_phrase_table2.h`, `PunctTableEntry::escape/append/remove`, `#if 0` dumps | none | no shipped counterpart; the runtime uses `PhoneticLookup`, `PhraseLookup` and the large-table-3/chunk path | n/a | negative coverage | — |

## Family notes

### G1 — UTF-8 and UCS-4

The pin converts with GLib's `g_utf8_to_ucs4` and `g_ucs4_to_utf8` in
three candidate/sentence/export paths (`pinyin.cpp:1627-1645` and
`lookup.cpp:27-60`; the export is `pinyin.cpp:698-716`). GLib encodes
surrogates and values above U+10FFFF, and stops at an embedded U+0000.
Rust `char`/`str` cannot represent surrogates or values above U+10FFFF.
oxpinyin therefore carries phrase text as stored words (`PhraseText`) and
encodes it at the C boundary (`phrase_text_bytes`). The valid-input path
is byte-identical. The malformed cases are #894 (phrase text) and #1144
(punctuation).

`phrase_segment` calls the pin's `g_utf8_strlen` through `glib-sys`, so
GLib writes its own NULL-input critical. The invalid-UTF-8 critical is not
written ([1297-segment-critical](1297-segment-critical.md)).
`g_utf8_strlen` at `pinyin.cpp:1450` / `zhuyin.cpp:969` is the same gate.
The negative coverage cites `g_utf8_strncpy` at `zhuyin_parser2.cpp:879`,
not as an over-read: GLib `g_utf8_strncpy` uses `strncpy`, which stops at
the NUL.

### G2 and G3 — strings and paths

- `g_strdup_printf` builds `.tmp` names and diagnostic text. The formats
  are `%s` and `%d` over bytes, not a locale conversion.
- `g_strconcat` builds the aux-text prefix/middle/postfix
  (`pinyin.cpp:3421-3431,3482-3509,3560-3569`) and the export joins
  (`pinyin.cpp:830,865`). `AuxMatrix`'s `join` keeps the `g_strconcat`
  NULL-postfix law: a missing middle drops the postfix (#925).
- `g_strjoinv`/`g_strfreev` at `pinyin.cpp:849,862` render the bigram
  export. The pin's array is not NULL-terminated (row 1, class (b)).
  oxpinyin renders owned strings before the iterator exists.

**Punctuation order and duplicates (#928, closed 2026-10-26 UTC).** This
is ordinary compatibility. The pin's steps:

1. `pinyin_guess_predicted_candidates_with_punctuations` walks
   `m_prefixes` as `pinyin_guess_predicted_candidates` left it
   (`pinyin.cpp:2459-2463`).
2. It takes each token's `get_all_punctuations` array and counts it with
   `g_strv_length`.
3. It skips a string that `g_strv_contains` finds in the zero-terminated
   `punct_array` (`:2468-2483`). GLib 2.84.4 `g_strv_contains` is a
   `g_str_equal` walk (`gstrfuncs.c:3234-3247`).
   `g_array_new(TRUE, TRUE, …)` allocates and terminates the empty array,
   so the first test sees no entry (`garray.c:356-361`).
4. The reverse `g_array_prepend_val` (`:2488-2494`) puts the kept strings
   first, in first-occurrence order, ahead of the predicted rows. The test
   never sees the predicted rows.

No entry is `NULL` (`g_strdup` of the cache string), so `g_strv_length`
is the entry count.

`prepend_punctuations` reads the prefixes that the prediction stored. It
keeps the first of equal `phrase_text_bytes` values in the same order and
puts them ahead of the candidates. Those bytes are the pin's C string. The
two agree on every stored value:

- duplicates within one value;
- duplicates across prefix tokens (shortest suffix first);
- empty strings from different stored words;
- non-scalar strings;
- a punctuation equal to a predicted phrase (both kept).

Recorded evidence: the `punct-dedup/*` cases MATCHed on bdb. The named
mutation `M928-no-dedup` (every punctuation kept) made the five duplicate
cases differ, while the `distinct` and `candidate` controls matched.

`g_build_filename` is the only path constructor. oxpinyin calls it through
`glib-sys` in `oxpinyin-input` `path.rs` (`build_filename`, `build_path`,
`build_bytes`). So a name that starts with `/` stays beneath the
directory, and stderr gets the pin's bytes. The caller's directory bytes
stay as given. `g_rename` and `g_fsync` are G10.

**#922 result.** Every difference found was a defect and is fixed. The
join is the pin's call. The Windows separator set follows GLib but has no
pin run; Unix is complete. `..`, `.`, duplicate separators and `\` on Unix
are ordinary bytes in both. The partition table is in
[glib-libc-verification.md](glib-libc-verification.md) G3.

### G4 and G9 — containers, allocation and the ABI

The C ABI's `GArray` is a real GLib object. `g_array_set_size` and
`g_array_append_vals` in `pinyin_lookup_tokens`,
`pinyin_token_get_nth_pronunciation` and their zhuyin twins mutate the
caller's `GArray` through `glib_sys`. A manual `Vec` write would corrupt
GLib's private `_GRealArray` metadata. This is MUST RETAIN (#924).
Internal `GArray`/`GPtrArray` uses are ordinary step, candidate and
lookup containers. Their Rust counterparts are `Vec`, slices and fixed
arrays. The malformed cases are the registered out-of-range index and
empty-chunk sites, not a container-layout difference.

The allocation contract is the same as the pin's:

- a returned `gchar *` is `malloc`-compatible and freed by `g_free`;
- a `gchar **` is freed by `g_strfreev`;
- the two `.alloc` files list every ownership slot.

**#924 result.** Disposition: **retain** the GLib ABI. The per-operation
MUST RETAIN / MAY DIVERGE matrix and the allocation-failure partitions are
in [glib-libc-verification.md](glib-libc-verification.md) G9.

| Settlement | Current state |
| --- | --- |
| #1224 | A NULL `GArray` in `pinyin_lookup_tokens`, `pinyin_token_get_nth_pronunciation` and the zhuyin twins reaches GLib, which writes its own `g_array_set_size` / `g_array_append_vals` criticals in domain `GLib` (one append per prepared library for lookup), with the pin's return values. `*_token_get_phrase` keeps success with a NULL output after the nonfatal UTF-8 conversion allocation fails (`pinyin.cpp:1627-1645`, `zhuyin.cpp:1096-1113`). Closed defects, not exceptions. |
| #1227 | Returned C strings and the keyboard vector come from the pin's GLib calls, so allocation failure aborts as the pin's ([record](returned-string-allocation.md)). `just abi alloc-failure` (`tools/abi/alloc-failure.cc`) expects GLib's fatal failure on both sides. |

Gates: `just abi exports` (the exported-symbol set equals the pin's
version scripts) and `just abi alloc-pairing` (`alloc-pairing-pinyin.cc`,
`alloc-pairing-zhuyin.cc` and the `libpinyin.alloc` / `libzhuyin.alloc`
registers). The `GArray` lifecycle is held by `oxpinyin-capi`
`boundary_tests::lookup_tokens_resets_the_callers_array` and
`oxpinyin-zhuyin-capi`
`boundary_tests::caller_arrays_keep_their_metadata_and_clear_through_the_callback`.

Limits:

- Earlier Rust allocations (`String`, `CString`, `Vec`) can terminate
  before the final foreign allocation. This record claims no identical
  allocation graph and no whole-process OOM equivalence.
- The `*_iterator_get_next_phrase` phrase conversion writes no warning. It
  may return NULL with success, as the pin's unchecked `g_ucs4_to_utf8`
  does (`pinyin.cpp:715-716,737,745`).
- Nothing in the C-facing call graph produces diagnostic text with an
  embedded NUL. The helper difference is therefore not an admitted outward
  difference.
- Per-trigger re-audit of the facade warning register belongs to G8.

No unregistered discrepancy remains for #924's operations. Every
difference found is a fixed defect or a registered difference.

### G5 and G6 — sorts

`g_ptr_array_sort` at `phonetic_lookup.h:337` orders the n-best tails
with `trellis_value_compare`. That comparator truncates the float
difference to `gint` (`cvttss2si` in the bdb binary). Scores less than one
unit apart compare equal, so the comparator is not transitive even on
finite scores. NaN and ±∞ come from a λ outside `[0, 1]`.

`g_array_sort_with_data` orders the pinyin and predicted candidate lists
under `compare_item_with_sort_option` (`pinyin.cpp:2286,2443`).
`g_array_sort` orders the zhuyin lists (`zhuyin.cpp:1529,1610`). Their
`gint` frequency key is a total order for λ in `[0, 1]`, and wraps into
cycles outside it. The phrase-string dedup sorts (`pinyin.cpp:2067`,
`zhuyin.cpp:1375`) are total. Their winner rule is #913/#914. The dedup
winner and punctuation order are #928.

These comparators are language artefacts under the #1303 ruling.
oxpinyin does not port GLib's merge. The tails use a stable `sort_by` on
raw possibility (S3, `nbest.rs` `tail_merge`). The candidate and predicted
lists use `candidate::rank_by_frequency`: stable std sorting with the
pin's guarded wrapping frequency comparison
([1297-candidate-numeric-sort](1297-candidate-numeric-sort.md)).

### G7 — hash tables

`PhraseLookup` and the phonetic trellis use GLib hash tables for
prefix/step lookup. No iteration order reaches an exported result.
`PinyinLookup2`'s copies are dead. Rust uses vectors keyed by step and
token. There is no semantic difference to record.

### G8 — diagnostics

`g_return_val_if_fail` in `phrase_index.cpp:354-356` logs a critical
when a loaded sub-index cannot be read. `pinyin_phrase_segment` and
`zhuyin_phrase_segment` log the pin's exact precondition at
`pinyin.cpp:1454` and `zhuyin.cpp:973`. Registered pin aborts outside
these sites abort in the C adapters. The allocation and message text stay
with #924. The segment gate is #918. A warning is observable, so it is
never a quirk by itself.

### G10 — rename and sync

`_rename_files` renames the library files by sub-index, then the user
pinyin index, the phrase index and the bigram. `zhuyin_save` renames each
file as it writes it. `MemoryChunk::save` calls `g_fsync` after the
payload. The Rust `save_reporting` keeps the per-file tolerance, the
rename order and the return value (#929). This includes the pin's own
answers after a failed write or rename.

The chunk write is the pin's call sequence (external-I/O stages 2 and 3).
`oxpinyin-user`'s `write_chunk_file` calls `oxpinyin-input` `chunk::save`,
which (`memory_chunk.h:536-557`):

1. opens the file;
2. writes the length word and the checksum word, with one `write` each;
3. writes the payload with one `write`;
4. calls `fsync` and `close`, and ignores both results.

A failed header write is the pin's `assert`, reproduced as an abort
(register row 77, `ChunkHeaderWrite`). A short or failed payload write is
the pin's soft `false` with no sync. It is `PersistenceError::Io`, and the
reporting save tolerates it, as the pin's callers ignore the answer
(`pinyin.cpp:971,988`; `zhuyin.cpp:607,633`). There is no retry. File
bytes, write and rename order, tolerance and return values are the pin's.

**#929 result.** The ordered per-operation table is in
[glib-libc-verification.md](glib-libc-verification.md) G10. This table
summarises what is mandatory for a valid profile:

| Semantics | Status | Evidence |
| --- | --- | --- |
| File order (libraries in index order, then user pinyin index, phrase index, bigram) and the rename order; pinyin stages all then renames, zhuyin writes and renames each | Mandatory | `failed_renames_report_exact_order_and_preserve_destinations`, `interrupted_save_preserves_the_facade_specific_committed_prefix`, `interruption_after_user_chunks_keeps_indexes_and_bigram_at_previous_save`, `save-929-boundary-*` |
| Per-file tolerance: a failed write or rename does not stop later files; rename failures print the fixed `rename %s to %s failed.` pair; the marker and the dirty reset still follow | Mandatory | `save-929-rename-*`, the rename unit test above |
| Return values: true after attempted renames, false before staging when there is no user dir or nothing modified | Mandatory | G10 table rows 1 and 7; `--issue 929` |
| Same-backend bytes, no data loss on completed saves, reload round trips | Mandatory | `save_reload_keeps_bigrams_and_unsaved_changes_do_not_replace_them`, `save-929-reload-*`, `save-929-logical-cross-open-*` |
| Chunk write: header words, one payload write, `fsync` with its result ignored, short or failed payload soft | Mandatory | `oxpinyin-input` `chunk.rs` tests (`a_chunk_is_the_written_bytes`, `an_unopenable_path_is_the_soft_failure`, `a_header_write_failure_is_the_abort_site`); `sync_rejecting_chunk_target_does_not_stop_later_files`, `save-929-sync-rejecting-*` |
| Header-word `assert` abort | Register row 77 | `reporting_save_stops_at_a_chunk_header_write` |
| errno wording, Windows `MoveFileExW` mapping, power-loss durability, interleaving of backend diagnostics | Not claimed; no exception admitted | G10 limits in the verification file |


### G11 — numeric parsing and locale

`SystemTableInfo2::load` and `UserTableInfo::load` read their fields with
`fscanf`. `atoi` cuts an addon index to `guint8`. The return of
`setlocale(LC_NUMERIC, "C")` is the new name, and every early return skips
the restore. The valid file surface is #919 (table.conf) and #920
(user.conf). The process-wide locale is row 39 and #923. The
2026-10-25 UTC ruling retains it. Only a reversal needs a new ruling.

From external-I/O stage 1, oxpinyin makes the pin's own calls.
`oxpinyin-input` `config.rs` runs `setlocale`, `fopen`, `fscanf`, `feof`,
`atoi`, `fprintf` and `fclose` through the `libc` crate, with the pin's
format strings and call order. `data/src/table_info.rs` (`TableConf`) and
`data/src/user_files.rs` (`UserTableInfo`) keep what the pin does with
each answer. The host libc's scanner therefore decides malformed
spellings, as it does for the pin. That is not a divergence
([source policy, external I/O](../source-policy.md#external-io)).
`UserTableInfo::to_text` formats the two version fields through `i32`. A
value outside `int` is therefore written back signed, as the pin's `%d`
does.

Two class (b) refusals remain:

- A missing `source table format:` line reads the pin's uninitialised
  `char str[256]` (`TableConfError::UninitialisedSourceFormat`). oxpinyin
  refuses it deterministically.
- A five-word row merged with the next row's first word can make the pin
  read a NULL item and SIGSEGV. oxpinyin answers it with the declared
  result.

Neither refusal reproduces the uninitialised or invalid read.

**Host scanner versions.** The glibc that this inventory read is 2.41 (recorded evidence). Its `%f`
lexer sets `got_e = got_dot = 1` at an exponent mark but does not clear
`got_digit` (`stdio-common/vfscanf-internal.c:2196-2201`). The post-loop
conversion check tests only the empty buffer and the bare `0x` prefix
(`:2545-2549`). glibc 2.42 rejects incomplete exponents, and 2.43 also
rejects `0x.` (`vfscanf-internal.c:2218-2219`, `:2571-2576` at 2.43-3).
2.41 therefore converts `1e`, `1e+`, `0x1p`, `0x1p+`, `0x1p-` and `0x.`,
and fails on `0x` and `infin`. These spellings are the `host_scanf` cases
of policy row 78, reported HOST-DEPENDENT on both facades:
`table-conf-lambda-exponent-without-digits`,
`table-conf-lambda-hex-exponent-without-digits`,
`table-conf-919-lambda-exponent-sign-without-digits`,
`table-conf-919-lambda-hex-exponent-sign-without-digits`,
`table-conf-919-lambda-hex-exponent-minus-without-digits` and
`table-conf-919-lambda-hex-dot-without-digits` (maintainer ruling
2026-10-25 UTC for `0x.`). The `%d` lexer is unchanged for the base-10
table fields, and `%255s` is unchanged for the caller-provided 256-byte
buffer.

The #920 `user.conf` walk uses the same scanner. Version and counter `%d`
values take LP64 `strtol` saturation, then the low 32 bits of the `int`
store. So `4294967303` reads as 7 and `-9223372036854775808` as 0. The
marker's conformance, the counter raise/lower and the wipe/write sequence
then decide user state. A NUL inside the `database format:` token ends the
C string, as the pin's `strcmp` does. `oxpinyin-capi`'s
`pinned_data_tests::user_conf_versions_past_int_take_the_scanners_int`
retains trained and imported state for a wrapped conforming marker and
compares the wipe for a non-conforming one, with every fixture value read
from the cell's `table.conf`.

**#923 result.** Disposition: **retain**. The maintainer ruling of
2026-10-25 UTC, quoted from the #923 comment:

> Maintainer ruling, 2026-10-25 UTC: **retain** the pinned `LC_NUMERIC` side effect (register row 39, #539/#618). The default build stays bug-for-bug, and no opt-in departure is planned. The maintainer intends to report the defect upstream; oxpinyin changes only if libpinyin does.

The default build stays bug-for-bug. There is no departure behind a
feature. The row 39 entry in `upstream-divergences.md` and
`compatibility-policy.md` is unchanged.

The mechanism is the pin's. `SystemTableInfo2::load`,
`UserTableInfo::load` and `UserTableInfo::save` save the return of
`setlocale(LC_NUMERIC, "C")`, which is the new name `"C"`
(`table_info.cpp:197,291`; `:328,372`; `:378,394`). So the closing call
re-installs `"C"`, and every early return skips it. oxpinyin makes the
same calls at the same points through `oxpinyin-input` `config.rs` (the
readers and the writer).

The side-effect table follows. The `setlocale` call comes before any parse
or return, so no legal return or error path leaves a different locale:

| Exported call | Pin site | Locale after | oxpinyin site (`context.rs`) |
| --- | --- | --- | --- |
| `pinyin_init`, any outcome | `pinyin.cpp:337` (`table.conf` load), then `check_format` `:178,:187` | `"C"` | `init_context` (called by `pinyin_init`), first step |
| `zhuyin_init`, any outcome | `zhuyin.cpp:281`, then `check_format` `:126-176` | `"C"` | `zhuyin_init`, first step |
| `pinyin_save`, past the user-dir and modified guards | `pinyin.cpp:1133-1137,1143` (`mark_version`) | `"C"` | `pinyin_save`, under the same guards |
| `zhuyin_save`, past its guards | `zhuyin.cpp:547-552,695-698` | `"C"` | `zhuyin_save`, under the same guards |
| `pinyin_fini`, always | `pinyin.cpp:1200` | `"C"` | `pinyin_fini` |
| `zhuyin_fini` | `zhuyin.cpp:741-757` releases state and writes no marker | unchanged | none |

Guards: `oxpinyin-capi` `tests/locale.rs`
(`lc_numeric_follows_the_pins_table_info_sites`) and `oxpinyin-zhuyin-capi`
`tests/locale.rs` (`a_failed_init_leaves_lc_numeric_at_c`).

The "Locale after" results assume that no other thread changes the locale
at the same time. `setlocale` is process-wide and not thread-safe, in the
pin and in oxpinyin. The 2026-10-25 UTC ruling retains the side effect and
so accepts that concurrency impact unchanged. No removal is proposed, so
none is assessed. An upstream fix would copy `setlocale(LC_NUMERIC, NULL)`
first and restore it on every path. oxpinyin changes only if libpinyin
does.

### G12 — file I/O

`MemoryChunk` maps the file, checks the 8-byte header, the length word
and the checksum, and reads the payload. A short or unreadable chunk is
the pin's empty-chunk fault (rows 76 and 96). A sparse chunk is row 109.
`unlink` belongs to `_clean_user_files` and the user-file rewrite.
`fprintf` is the raw stderr channel of the same paths. The writes are
G10. oxpinyin's mapping is `oxpinyin-input` `chunk::load` (the pin's
`open`, header reads and `mmap`), with the same header checks.

### G13 — C primitives

`memset`/`memcpy`/`memmove`/`memcmp`/`strlen`/`strcmp`/`strchr` are
ordinary byte and string operations in the pin's parsers, chunk and
candidate code. The Rust equivalents are slices and `str`. There is no
observable difference on valid data. The malformed NULL/unreadable cases are
the registered class (b) rows 80 and 106.

### G14 — libm

The only reachable libm call is `log`. The n-best trellis and the phrase
lookup accumulate natural logs in `gfloat`. Three of the seven sites
(`phonetic_lookup.h:39,250`, `phrase_lookup.cpp:40`) pass a `float`, so
they resolve to `std::log(float)` in `<cmath>`. oxpinyin uses Rust's
standard maths (`f64::log2` in `core/src/cost.rs`, `ln` in
`engine/src/phrase.rs`). The class (a) register is row 11. A last-bit libm
difference is recorded and not investigated further.

### G15 — punctuation

Closed (#1144).

- `decode_puncts` walks a stored value in whole words and keeps each
  punctuation's stored words (`PhraseText`). The pin's
  `punct_table.cpp:57-94` walks bytes; see
  [1297-punct-words](1297-punct-words.md).
- `prepend_punctuations` encodes them with `phrase_text_bytes`, the
  encoder that #894 and #1198 use. That is the `g_string_append_unichar`
  form (GLib 2.84.4 `gstring.c:816-888` copies `g_unichar_to_utf8`,
  `gutf8.c:546-596`).
- `PunctTable::punctuations` fails only on a failed read. An unopened
  table, an absent entry and a zero-length value answer `Ok(Vec::new())`,
  as the pin's `get_all_punctuations` answers `false` on a zero-size chunk
  (`:78-79`).

| Partition | Pin (`074a2219`) | oxpinyin | Case |
| --- | --- | --- | --- |
| Scalar words, each punctuation zero-terminated | the strings | identical | `punct-decode/valid` |
| Zero-length stored value | no row (`size == 0`) | no row | `punct-decode/zero-length` |
| Surrogate or value above U+10FFFF | GLib's three- to six-byte form | identical bytes | `punct-decode/surrogate`, `punct-decode/above-unicode` |
| Zero first word | `unescape` appends it, so `g_strdup` cuts to an empty string; the run still takes the nonzero words after it | identical | `punct-decode/empty`, `punct-decode/empty-leading` |
| No final zero | the run ends at the last whole word after a one-word over-read whose result is not used | identical rows; no over-read | `punct-decode/unterminated`, `punct-decode/unterminated-zero` |
| Length not a multiple of 4 | `maxlen` is the `sar` of the byte difference and `begin < end` compares bytes: whole words, then one empty row after a terminated run | whole words only: no extra empty row. `misaligned-terminated` and `misaligned-short` are POLICY-ACCEPTED ([1297-punct-words](1297-punct-words.md)) | `punct-decode/misaligned-terminated`, `punct-decode/misaligned-unterminated`, `punct-decode/misaligned-short` |
| Absent DBM entry, unopened table | no row | no row | — |

The unterminated read is class (b), registered as
[Unterminated punctuation value over-read](upstream-divergences.md#unterminated-punctuation-value-over-read).
The rows match whenever the pin returns. Only the read and its possible
fault are not reproduced. The misaligned partition is the
language-artefact record above. The other partitions are ordinary
compatibility.

### G16 and G17 — tools and dead code

`pinyin_internal.h` includes `tag_utility.cpp`, but no exported entry
calls `taglib_*`. `utils/training` and `utils/storage` use it. `table_text_parser.h` and the `load_text` methods are
used only by `utils/storage/gen_binary_files.cpp`.
`PinyinLookup2` is never instantiated; `PhraseLookup` uses
`FacadePhraseTable3` and `FacadePhraseIndex`, not the old
`FacadePhraseTable2`/`ChewingLargeTable`. The `#if 0` dumps and the
`PunctTableEntry` writer methods have no exported caller.

The tools make the pin's calls through `oxpinyin-input` `tool.rs`
(external-I/O stage 5): `taglib` is `oxpinyin-utils/src/utils_helper/taglib.rs`, and
`atoi`, `atol`, `g_strsplit_set` and `g_unichar_isgraph` are the host's.
`oxpinyin-utils` builds `gen_pinyin_table`, `gen_zhuyin_table` and the
other utilities under the pin's names.

## Exported C string inputs

The #1279 per-parameter inventory covers all 25 exported functions with
string inputs (29 parameters). For each, it records the pin conversion,
the invalid/non-scalar/NULL outcomes, prior state changes and the current
Rust helper. It records source-derived defects and the pending
session-interface coordination. It is not a completed verification
claim.

## Executable copies of external-library algorithms (2026-10-29 UTC)

This inventory follows the maintainer ruling of 2026-10-29 UTC, "No
ported external-library internals" ([source policy](../source-policy.md#internal-language-artefacts)).
It covers `crates/` and `tools/`. All five items are replaced; the
[classification on main](#classification-on-main-1297-2026-10-31-utc)
gives the current code.

- **Search.** The search looked for code that transcribes GLib,
  libstdc++, libc++ or glibc algorithms to keep their comparison or probe
  order: hand-written sorts, heaps and binary searches, and the comments
  that cite those libraries.
- **No other copies.** None exist outside the five items below.
  `tools/audit/glib_call_sites.py` lists call sites; it does not port
  them.

**Compliant, not ports:**

- `oxpinyin-data/src/table_entries.rs` calls `Vec::binary_search` on a
  token list that its own `insert` keeps sorted. That is the standard
  API with its documented semantics.
- The `oxpinyin-capi-marshal` UTF-8 codecs are ABI-prescribed boundary
  code, not external-algorithm ports: `input.rs` `utf8_to_ucs4`,
  `ucs4.rs` `decode`, and `phrase_text.rs` `encode_words`/`encode_word`.
  They give the C strings the bytes GLib's `g_utf8_to_ucs4` and
  `g_ucs4_to_utf8` would.

| Item | Former copy and callers | Replacement | Parity | Inputs that differ; cases that become `POLICY-ACCEPTED` | Tests |
| --- | --- | --- | --- | --- | --- |
| A | `oxpinyin-engine/src/nbest.rs` `glib_merge_sort`, a GLib merge transcription, called by the candidate and prediction sorts (`pinyin.cpp:1678-1709,2286,2440-2445`; `zhuyin.cpp:1129-1144,1529,1610` at the pin). | `candidate::rank_by_frequency`: stable std sorting with the enabled length/span fields and native `rhs.wrapping_sub(lhs) as i32` frequency comparison. Each leading-key tie group is guarded; only cyclic groups use stable numeric descending fallback. | Exactly where every leading-key tie group's frequencies fit in a circular interval narrower than 2³¹; this includes wraparound across zero. Antipodal pairs are inconsistent and rejected by the guard. λ alone does not determine consistency. | Only cyclic groups may differ. Existing λ cases are attributed individually in [the item A finding](1297-candidate-numeric-sort.md); the four `row58-signed-{score,duplicate}-{off,on}` cases must MATCH, never POLICY-ACCEPTED. | `candidate.rs` guard tests (independent order laws at boundary triples, empty lists, ties, antipodal pairs, cycles, leading-field isolation); stable ties, disabled keys, dedup alignment and the eight-bit span test `session::tests::the_pinyin_length_key_wraps_at_eight_bits`. |
| B | `nbest.rs` `heap_push`, `heap_adjust`, `heap_pop`, `heap_make`, `heap_top`, a transcription of libstdc++ `stl_heap.h`. Callers: the two-slot node insert (pin `src/lookup/phonetic_lookup_heap.h:56-81`), beam selection (pin `src/lookup/phonetic_lookup.h:770-771`) and final-tail selection (pin `:331-337`). | `select_top`: `select_nth_unstable_by` at `k − 1`, then a sort of the `k`-prefix, on a complete key (`selection_compare`). `Node::keep`: direct two-slot transitions. | **Pinyin: everywhere.** The S1/S2 key (adjusted S, then batch position) is total, and the node transitions are derived from the pin. **Zhuyin:** wherever the comparator is a strict weak order and no two values tie on both length and score. | **Zhuyin only.** (1) λ outside `[0, 1]`, where NaN and finite scores of one length meet in one selection and libstdc++'s sequence decides. (2) Exact ties of length and score, where the heap's positional order differs from batch order. The accepted cases are in [1378-zhuyin-selection](1378-zhuyin-selection.md). | `nbest.rs` tests, including `selection_keeps_the_complete_key_at_each_limit` and `backtracking_uses_the_physical_slot_after_batch_selection`; `binary_heap_top` is a test-only comparison. |
| C | `oxpinyin-core/src/single_gram.rs` `lower_bound_records`, formerly a transcription of libstdc++ `__lower_bound`, behind `lower_bound` and `find`. It serves `get_freq`, `set_freq`, `insert_freq`, `remove_freq` and the search walk (pin `src/storage/ngram.cpp:161,185,215,239,259`; `src/include/stl_lite.h:29`). | `records.partition_point(\|r\| record_token(r) < token)` on the existing `[[u8; ITEM_LEN]]` view. Unpartitioned stored data is deliberately not checked for order, sorted or cleaned because a per-lookup sortedness check would make the search linear; on such data, `partition_point` stays memory-safe and deterministic, but its result is unspecified and not the pin's; observable differences are recorded as `POLICY-ACCEPTED` ([#1405](https://github.com/shenghaoc/oxpinyin/pull/1405)), and the existing structural checks for truncated records, invalid lengths and other unsafe conditions still apply in full. | Wherever `item < probe` is partitioned over the stored items. That includes every gram the pin's writers produce, since `insert_freq` inserts at the lower bound (`ngram.cpp:185-200`). | Stored grams where an item below the probe follows one at or above it: foreign or hand-edited DBM values, or a user gram stored out of order and interleaved by `merge_single_gram` (`ngram.cpp:308-349`). The standard library leaves `partition_point` unspecified on an unpartitioned slice. Its answer is deterministic for one toolchain, and the record names the toolchain. Nothing is sorted or cleaned. Accepted cases: [1297-gram-partition-segmentation](1297-gram-partition-segmentation.md) and [1297-gram-partition-training](1297-gram-partition-training.md). | `single_gram.rs` tests, including `checked_search_asserts_short_and_faults_past_a_partial_tail`. |
| D | `oxpinyin-user/src/user_index.rs` `Entry::equal_range`, formerly a transcription of libstdc++ `equal_range`. Callers: `Entry::add_index` and `Entry::remove_index` (pin `src/storage/chewing_large_table2.h:208-255`, `phrase_exact_less_than2`). | `search_records::<N>`: two `partition_point`s over `as_chunks::<N>()` (design note 4). Unpartitioned stored data is deliberately not checked for order, sorted or cleaned because a per-lookup sortedness check would make the search linear; on such data, `partition_point` stays memory-safe and deterministic, but its result is unspecified and not the pin's; observable differences are recorded as `POLICY-ACCEPTED` ([#1405](https://github.com/shenghaoc/oxpinyin/pull/1405)), and the existing structural checks for truncated records, invalid lengths and other unsafe conditions still apply in full. | Wherever the entry's items are ordered by `phrase_exact_less_than2`, as the pin's `add_index` keeps them (`:216-229`). | KC and tkrzw entry chunks whose order a mask walk broke by leaving a previous record's tail (#785, module docs). The #785 mask cases on the kc and tkrzw cells. | `user_index.rs` `entry_ops_keep_the_pins_order_and_report_presence` covers sorted entries. |
| E | A Python transcription of libstdc++ `__lower_bound` in the contract-case tooling chose the user gram for `user-gram-unsorted-phrase-segment`. | Removed with that tooling. The case's recorded pin observations and provenance stay in [1297-gram-partition-segmentation.md](1297-gram-partition-segmentation.md). | The case's captured pin observations stay. | `user-gram-unsorted-phrase-segment` is POLICY-ACCEPTED. | — |

**Item A.** Candidate lists use stable `slice::sort_by` over cached enabled
keys and keep the dedup-only `m_freq`. Predicted lists sort their rows
directly. The eight-bit pinyin span and its defined unsigned wrap remain
contract. The maintainer decision dated 2026-10-30 retains the frequency
comparison as `rhs.wrapping_sub(lhs) as i32`. A linear guard on each equal
length/span group proves consistency before stable std sorting. Only
cyclic groups use the defined numeric-descending fallback. λ alone does
not decide consistency: wrapped/failed-token total updates can produce
large scores at stock λ. All four `row58-signed-{score,duplicate}-{off,on}` cases must
MATCH and are never registered as differences. The proof is in
[1297-candidate-numeric-sort](1297-candidate-numeric-sort.md).

**Design notes.** All cites are at the pin.

**Item B: std selection and direct node slots.** `nbest.rs` has no heap
transcription. `Node::keep` writes physical slots directly. Beam and
final-tail selection call the same fixed-row
`select_top(Vec<BeamEntry>, count, nstore)` helper and comparator. The
comparator captures the mode, so there are no per-facade sort bodies.
S3's stable raw-score `tail_merge` orders the final tails. Zhuyin keeps the
pin's raw single-slot admission.

The pin was read from Git tree `074a2219c90feaf962d0d24f034514033ece5f99`:
`src/lookup/phonetic_lookup_heap.h` blob `299ddedfa55e` and
`src/lookup/phonetic_lookup.h` blob `c092e7610cb0`. Two-slot transitions
come from `phonetic_lookup_heap.h:56-81`; the single-slot rule is
`:108-122`. `number()` (`:49-53`) supplies physical slots during expansion
(`phonetic_lookup.h:282-293`); successors store them at `:665,695` and
advance to later steps at `:667,697`, after predecessor slots are final.

Selection, then a sort of the retained prefix, costs O(m + k log k), in
place over the existing batch. Heapify-and-pop costs O(m + k log m) and
O(k) separate result storage. A measured comparison on the production
`BeamEntry` layout chose selection. Direct node transitions cost O(1) time
and space.

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
   - Pinyin: adjusted S (`adjusted_poss`, compared by
     `ordered_poss_compare`), then batch position.
   - Zhuyin, as ruled: shorter `m_sentence_length` first, as the pin's
     length clause has it (`phonetic_lookup.h:86-88`). Then raw score
     descending, with NaN last and signed zeros equal, then earlier batch
     position.
   - Beam (32, `:37`) and tails (`nbest` 3 or 1):
     `select_nth_unstable_by` at `k − 1`, then a sort of the `k`-prefix.
   - Tails are then ordered by S3's `tail_merge`.
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
4. **`user_index` entry search.**
   - The stride is `item2_stride(len)`, one of 8, 12, …, 36 bytes, for
     lengths 1–16 (`row_format::pinyin_index::item2_stride`).
   - A `match` dispatches to a generic `search_records::<N>` over
     `bytes.as_chunks::<N>()`. It calls `partition_point` twice, for the
     lower and the upper bound, reading the packed key words in place from
     each `[u8; N]` chunk.
   - There is no per-search decoding of the record.
   - The `as_chunks` remainder is dropped, as `count()` floors.
5. **Float keys.** `ordered_poss_compare` orders `f32` scores best first:
   +∞, finite descending, −∞, NaN last. Signed zeros tie, and NaN payload
   and sign never affect ranking. `f32::total_cmp` alone would order
   `-0.0` before `+0.0` and order NaNs by sign.

## Classification on main (#1297, 2026-10-31 UTC)

This is the current classification. It follows two maintainer rulings,
both in [source-policy.md](../source-policy.md):

- 2026-10-29 UTC: no ported external-library internals.
- 2026-10-31 UTC: no hand-written C or C++ shims; macros such as
  `g_return_val_if_fail` and `G_STRFUNC` are not reproduced.

The pin was read from a `git archive` of
`074a2219c90feaf962d0d24f034514033ece5f99` (`src/pinyin.cpp` blob
`f27f7cf7`, `src/zhuyin.cpp` blob `d1520c18`). The search covered every
crate under `crates/`, for GLib, glibc and libstdc++ names and for sorts,
searches, joins, scanners and diagnostics shaped like them.

Buckets:

- **Boundary**: a real GLib or libc call the pin makes, through
  `glib-sys`, `libc` or an `extern "C"` declaration, or a C ABI shape,
  ownership rule, on-disk width or ABI-prescribed encoding.
- **Behaviour**: product behaviour, kept in plain Rust and named for it.
- **Imitation**: Rust that reproduces a GLib, libc or libstdc++
  implementation detail, or a macro the 2026-10-31 ruling names.
- **Owned elsewhere**: imitation whose replacement is assigned to a named
  later change.

| # | oxpinyin item (`crates/`) | Pin cite (`074a2219`) | Bucket | Reason and action |
| --- | --- | --- | --- | --- |
| 1 | `oxpinyin-capi/src/dict.rs`, `oxpinyin-zhuyin-capi/src/dict.rs`: `glib_sys::g_array_set_size` / `g_array_append_vals` on the caller's array | `pinyin.cpp:2805,2817`; `zhuyin.cpp:1797,1809` | Boundary | The caller owns a real `GArray` (#924). |
| 2 | `ffi.rs` (both facades) `strdup`, `key_text`, `strconcat_aux`, `strjoinv`, `symbol_vector`, `sentence_text` (the pin's GLib calls, #1227), `malloc_cstr_with`, `take_owned_cstr`; the hand-declared `malloc` / `free` | `g_free` / `g_strfreev` ownership of returned strings (`pinyin.cpp:715-716,733`) | Boundary | Ownership and lifetime of returned C strings; `extern "C"` declarations are what the 2026-10-31 ruling permits. |
| 3 | `ffi.rs` (both facades) `log_warning` (`g_log` at warning level in the facade's domain) and the lines it carries for class (b) refusals and notices with no pin counterpart | none: the pin's `assert` / `abort()` sites, for example `pinyin.cpp:2507` (`pinyin_choose_candidate`) and `:3738` (`pinyin_remove_user_candidate`), abort in the C adapters and write no such line | Behaviour | The warning text is oxpinyin's own and is not compared with the pin. Kept. |
| 4 | `phrase.rs` (both facades), NULL `sentence`: GLib's `g_utf8_strlen` critical | `pinyin.cpp:1450`; `zhuyin.cpp:969`; GLib 2.84.4 `gutf8.c:240` | Imitation → Boundary | It hand-spelled GLib's internal `g_return_val_if_fail` (domain `GLib`, `g_utf8_strlen`, `p != NULL \|\| max == 0`). **Replaced** (#1466) by the pin's call, `glib_sys::g_utf8_strlen(sentence, -1)`, so GLib logs its own critical and applies the caller's fatal mask itself. The ruled no-input outcome after it is unchanged. Same bytes, level and domain as before, so no case tells the two forms apart and no mutation applies. |
| 5 | `phrase.rs` (both facades), invalid UTF-8: `g_return_if_fail_warning(NULL, "bool pinyin_phrase_segment(pinyin_instance_t*, const char*)", "num_of_chars == ucs4_len")` and the zhuyin twin | `pinyin.cpp:1454`; `zhuyin.cpp:973` | Imitation → removed | The pin's own `g_return_val_if_fail` expansion with a copied `G_STRFUNC`. **Removed** (#1466) under decision D1: the call answers `false` and keeps the earlier result, with no log call. [1297-segment-critical](1297-segment-critical.md). |
| 6 | `SubPhraseIndex::load`'s four `g_return_val_if_fail` checks | `phrase_index.cpp:354-356,361` | Imitation → removed | The same macro expansion, formerly carried from the load to the facades only to be logged. **Removed** (#1466) under D1, with its queue, hooks, copied expression text and signature string. `LayoutFault` keeps only `total_freq`, which the load, reload and save still use for the pin's empty sub-index and totals. [1297-layout-criticals](1297-layout-criticals.md). |
| 7 | `oxpinyin-input` `config.rs`, `native.rs` (`setlocale`, `fopen`, `fscanf`, `feof`, `atoi`, `fprintf`, `fclose` through `libc`); the facades' locale calls | `table_info.cpp:197,208-291,328,338-359,372-394` | Boundary | External-I/O stage 1; `LC_NUMERIC` row 39 stands. |
| 8 | `oxpinyin-capi/src/types.rs` `GArray`, `GUint`, `GInt`, `GChar`; `oxpinyin-core` packed `ChewingKey`; the casts at exported parameters (`cursor.rs`, `iterators.rs`, `user_data.rs`) | `pinyin.h`, `zhuyin.h`; `chewing_key.h:111-113` | Boundary | The exported signatures prescribe the widths. |
| 9 | `oxpinyin-capi-marshal` `input.rs` `utf8_to_ucs4`, `scalar_text`, `parser_prefix`, `direct_zhuyin_reading`, `filename`; `ucs4.rs` `decode`, `prefix_text`; `phrase_text.rs` `phrase_text_bytes`, `encode_words`, `encode_word`, `stored_words_match` | `g_utf8_to_ucs4` / `g_ucs4_to_utf8` at the C string entries (`pinyin.cpp:568-571,1450-1454`) | Boundary | ABI-prescribed encodings, which the 2026-10-29 ruling's scope keeps as boundary code; #1279 settled them. |
| 10 | `oxpinyin-core` `PhraseText`; `oxpinyin-data` `phrase_library` `phrase_text`, `phrase_codes`, `phrase_chars`; `row_format` `punct_text` | `lookup.cpp:39-60`; `pinyin.cpp:1627-1645` | Behaviour | Stored code points; the outward cut is row 9. |
| 11 | `oxpinyin-engine` `nbest.rs` `select_top`, `tail_merge`, `Node::keep` | `phonetic_lookup_heap.h:56-81,108-122`; `phonetic_lookup.h:141-172,331-337,770-771` | Behaviour | The selection rule and slot semantics, now std selection and sort (item B, `f4d1a980`; S3). No libstdc++ heap remains; `binary_heap_top` is a test-only comparison. |
| 12 | `oxpinyin-engine` `candidate.rs` guarded frequency order; `session/mod.rs` `RankKey`, `compare_descending`, `for_sort_word`, `rank_candidates`; `oxpinyin-capi` `predict.rs` row sort | `pinyin.cpp:1678-1709,2286,2440-2445`; `zhuyin.cpp:1129-1144` | Behaviour | Stable std sort over the enabled keys; the `rhs.wrapping_sub(lhs) as i32` frequency order and the eight-bit span are retained by the maintainer decision of 2026-10-30 (item A, `000b7ec4`). Names are already the behaviour's. |
| 13 | `session/mod.rs` `amplified_frequency`, `amplified_frequency_with_bigram` | `pinyin.cpp:1811-1824,1862-1866`; `zhuyin.cpp:1264-1268` | Behaviour | The ruled scaled `guint32` ranking key (#1416, `9799db38`). |
| 14 | `session/mod.rs` `dedup_ranked_by_text`, `dedup_by_text_keep_first` | `pinyin.cpp:2128-2138`; `zhuyin.cpp:1406-1416` | Behaviour | One row per text (#913, #914). |
| 15 | `oxpinyin-capi` `predict.rs` `prepend_punctuations` | `pinyin.cpp:2468-2494` | Behaviour | Distinct punctuation strings before the predictions (#928). |
| 16 | `oxpinyin-core` `single_gram.rs` `lower_bound`, `find` | `ngram.cpp:161,185,215,239,259` | Behaviour | `partition_point` (item C, `67323c83`); unpartitioned grams are recorded under #1405. |
| 17 | `oxpinyin-user` `user_index.rs` `Entry::equal_range` | `chewing_large_table2.h:208-255` | Behaviour | Two `partition_point`s (item D, `f0906b22`). |
| 18 | `oxpinyin-input` `path.rs` `build_filename`, `build_path`, `build_bytes` | `g_build_filename` at `pinyin.cpp:150-165,250-259,331-338,395-400,952-1121`; `zhuyin.cpp:104-119,196-203,276-282,337-342,585-687` | Boundary | The pin's call through `glib-sys`; [external-I/O stage 3](external-io-boundary.md#proposed-crate-and-staged-implementation) implemented it. |
| 19 | `oxpinyin-data` `c_stderr.rs` `path_bytes`, `table_conf_load_failed`, `mmap_failed`, `rename_failed` through `oxpinyin_input::diagnostic` (`libc::fprintf` on the C library's `stderr`) | `table_info.cpp:201,332`; `pinyin.cpp:256,290,338,956,1059-1121,1265`; `zhuyin.cpp:200,282,589,609-687,800` | Boundary | External-I/O stage 4 (#1437). |
| 20 | `oxpinyin-user` `persistence.rs` `save_reporting`, the rename order, `tolerate`, `write_marker`, `clean_user_files`, `.tmp` staging | `pinyin.cpp:149-170,922-1130`; `zhuyin.cpp:547-700`; `memory_chunk.h:536-549` | Boundary | Files on disk, their order and the save's answers (#929). The rename and unlink calls are the pin's, through `oxpinyin-input` `path.rs` (external-I/O stage 3). |
| 21 | `oxpinyin-input` `chunk.rs` `save`, called by `persistence.rs` `write_chunk_file` | `memory_chunk.h:536-557` | Boundary | The pin's `open`, two header `write`s, one payload `write`, `fsync` and `close` (external-I/O stages 2 and 3). |
| 22 | `oxpinyin-input` `chunk.rs` `load`; `oxpinyin-data` `phrase_library.rs` `map_chunk`, `directory_end` | `memory_chunk.h:420-427,470-520` | Boundary | The chunk format and the pin's mapping call (external-I/O stage 2, row 109). |
| 23 | `oxpinyin-data` `row_format.rs` `decode_puncts` | `punct_table.cpp:56-94` | Behaviour | Whole-word reading, the language-artefact record [1297-punct-words](1297-punct-words.md). |
| 24 | `oxpinyin-data` `table_info.rs` `TableConf::parse`; `user_files.rs` `UserTableInfo::parse`, `to_text` | `table_info.cpp:122-131,194-294,338-394` | Boundary | The `table.conf` / `user.conf` reads and write are the pin's `fscanf` / `fprintf` calls through `oxpinyin-input` (#1404); the domain checks stay here. |
| 25 | `oxpinyin-capi` `aux_matrix.rs` `full`, `double`, `chewing`, `split_zhuyin` | `pinyin.cpp:3295-3576` | Behaviour | The byte delta used as a character offset is libpinyin's defect, reproduced (limb 4). |
| 26 | The tools' numeric fields, `printf("%f")` output and Unicode categories: `oxpinyin-input` `tool.rs` `atoi`, `atol`, `unichar_isgraph`, `fprintf` | `tag_utility.cpp:127-264`; `utils_helper.h:23-69`; `printf("%f")` at `estimate_interpolation.cpp:116-139`, `eval_correction_rate.cpp:201-203` | Boundary | The pin's calls through `libc` and `glib-sys` (external-I/O stage 5). |
| 27 | `oxpinyin-utils/src/utils_helper/taglib.rs`; `oxpinyin-input` `tool.rs`, `native.rs` | `tag_utility.cpp:70-299`; the tools' GLib and libc calls | Boundary | libpinyin's own taglib in Rust; its GLib and libc calls are real (`glib-sys`, `libc`). `Required` is a `HashMap`, no hash order reaches output. |
| 29 | Wrapping in-memory totals: `oxpinyin-runtime` unigram total; `oxpinyin-data` `phrase_libraries.rs`, `lm/mod.rs`; `oxpinyin-user` `store.rs`, `registry.rs`, `seed.rs`; the `guint8` pronunciation count in `store_libpinyin.rs` | `phrase_index.h:615,633`; `phrase_index.cpp:28` | Boundary | On-disk `guint32` / `guint8` widths; defined unsigned wrap. |
| 30 | `oxpinyin-engine` `nbest.rs` node index (`HashMap` beside the node `Vec`) | `phonetic_lookup.h:314-315` | Behaviour | Creation order lives in the `Vec`; the map is a lookup only. |

Out of scope: `oxpinyin-store`'s DBM calls and test-only code.
The table has no row 28. That row covered `oxpinyin-dictool` and `oxpinyin-datagen`, which no longer exist. The other rows keep their numbers.

### Maintainer decisions (2026-10-31 UTC)

**D1 — the pin's own `g_return_val_if_fail` criticals (rows 5 and 6).**
Approved as a narrow exception to the library stderr ruling (#545). For
both facades' `phrase_segment` and for `SubPhraseIndex::load`, oxpinyin
keeps the pin's return values, state changes and continuation exactly. It
emits no critical and substitutes no other log call. Two observable
differences are POLICY-ACCEPTED:

- the missing critical (to the caller's log handler, and to stderr under
  GLib's default handler);
- the missing abort under GLib fatal-critical settings
  (`G_DEBUG=fatal-criticals`, `g_log_set_always_fatal`), where the pin
  aborts and oxpinyin continues. This is an intentional observable
  difference, not exact compatibility.

This is not a general exception for copying GLib macros. It changes no
other native GLib call and no class (c) ruling.

**D2 — `g_build_filename` (row 18).** External-I/O stage 3 implements it
as the pin's call. It keeps the pin's path behaviour. This includes a file
name that begins with a separator, which stays beneath its directory
(policy row 78). `std::path::Path::join` is not a drop-in replacement,
because there an absolute name replaces the directory.

### Result

- Boundary: rows 1, 2, 7–9, 18–22, 24, 26, 27 and 29.
- Behaviour: rows 3, 10–17, 23, 25 and 30. Each is named for its
  behaviour.
- Imitation replaced: row 4 (the pin's call). Imitation removed: rows 5
  and 6 (D1).

No row of #1297 remains open.

## C-maths record

oxpinyin's floating-point sites call `std` directly: `core/src/cost.rs`
(`f64::log2`), `engine/src/nbest.rs` and `engine/src/phrase.rs` (`ln`),
and `training/src/eval/decode.rs` (`ln`). The scaled candidate frequency in
`session/lookup.rs` `amplified_frequency_with_bigram` is `f32` arithmetic
stored with `as u32`. Its saturation on out-of-range scores is class (b)
(#1416). None of these is a C-maths imitation. A last-bit difference from the pin's
`gfloat` `log` is class (a), row 11.

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
| tests | `glib_sys` use in test fixtures | test-only, not shipped. |

## Conclusions

- The valid-input side of every family is C-facing contract; the malformed
  side is the named class, record or host dependence.
- Children: #894, #918, #919, #920, #921, #922, #924, #925, #926, #927,
  #928, #929 and #1144. #923 is ruled: retain `LC_NUMERIC` (2026-10-25 UTC).
  Only a reversal needs a new ruling.
- The math record is row 11: `log` is class (a).
