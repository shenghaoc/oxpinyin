# GLib and libc at the pin: reachable calls and oxpinyin counterparts

Status: **reference**. The current classification of oxpinyin's GLib- and
libc-shaped code is [Classification on main](#classification-on-main).
Pin: libpinyin 2.11.92 at `074a2219c90feaf962d0d24f034514033ece5f99`.
GLib and glibc citations are to **GLib 2.84.4-3~deb13u5** and **glibc 2.41-12+deb13u4**
(Debian testing, linux/amd64).
Scope: the 79 `pinyin_*` and 52 `zhuyin_*` exports in
`src/libpinyin.ver` and `src/libzhuyin.ver`, their reachable helpers,
and the oxpinyin `crates/` counterparts. Test, oracle and tool-only code
is listed separately.

## Abstract

The exported surface reaches **70 distinct GLib/libc functions at 1,179
call sites**: 43 GLib functions (874 sites) and 27 libc functions (305
sites). They form fifteen reachable semantic families (G1–G15), plus
tool-only (G16) and dead (G17) groups. The internal container, hash and
primitive rows are ordinary C-facing or internal parity, with no
language/runtime quirk. No entry changes a valid-input answer.

Three results govern the inventory:

1. Real GLib ownership is ABI, not emulation: `GArray` mutation in
   `pinyin_lookup_tokens` / `pinyin_token_get_nth_pronunciation` and
   their zhuyin twins stays on `glib_sys` (MUST RETAIN).
2. The UTF-8, `fscanf`, path, sorting, filesystem and diagnostic
   boundaries are the pin's calls or recorded differences. The only
   semantic difference in a decoder is the punctuation table (G15).
3. `LC_NUMERIC` is retained (row 39,
   [ruling](compatibility-policy.md#lc_numeric-after-init-row-39)).

The math surface is `log` only: seven reachable sites, all class (a)
under row 11. oxpinyin's costs use `f64::log2` directly
(`core/src/cost.rs`).

## Reachability and negative coverage

A function is reachable when an exported `pinyin_*` or `zhuyin_*` entry
can call it without a test, tool or uninstantiated class in between.
The matrix below follows that definition. The complete call-site list is
[glib-libc-call-sites.tsv](glib-libc-call-sites.tsv), which records the
family, library, facade, backend, pin file and line, function and call.
`just audit-glib-call-sites <pin-checkout>` regenerates it; the
[runbook](../runbooks/glib-call-sites.md) names the tools. The `facade`
column comes from the linker's maps. The one hand-kept input is `tools/audit/glib_call_sites.families.tsv`.

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

The negative list is in [Negative coverage](#negative-coverage). Inventory
membership does not prove coverage of every input.

## Verdict terms and citations

Pin citations are `file:line` at `074a2219`. Relative pin paths start at
`src/` unless prefixed `utils/`. Rust paths start at `crates/oxpinyin-` and
cite items, not line numbers. "Both" means pinyin and zhuyin. A
backend-independent source argument covers bdb, kc and tkrzw only to the
stated extent.

- **Identical**: a source-derived equivalence for the named scope, not a
  test result.
- **Open**: not settled, not a policy question.

A family label alone does not admit a quirk. Each difference follows the
[decision order](compatibility-policy.md#decision-order) and links its
record. `POLICY-ACCEPTED` never means `MATCH`. Where the source leaves a
question open, the compiled pin decides, read under the
[Stage 2 reading procedure](../source-policy.md#stage-2); a cite then names
the symbol and instruction offset. amd64 is the compatibility reference.

## Matrix

`Reach`: `both` for libpinyin and libzhuyin, `pinyin` for the
prediction-only surfaces; `all` means bdb, kc and tkrzw. `Disposition`:
`contract` (C-facing behaviour on valid input), `(a)`/`(b)`/`(c)`,
`quirk` (record-admitted language/runtime difference) and `ruling`.

| ID | Family and pin call sites | Reach | oxpinyin counterpart | Valid / malformed | Disposition |
| --- | --- | --- | --- | --- | --- |
| G1 | UTF-8/UCS-4 conversion and category: `pinyin.cpp:628,715,1397,1450-1454,2658,3210,3678`; `lookup.cpp:54,58`; `zhuyin.cpp:918,969-973,1684,2165`; `zhuyin_parser2.cpp:871,879`; `punct_table.cpp:48` (dead); `tag_utility.cpp` (tool) | both (punct pinyin only); all | `capi-marshal/src/input.rs` `utf8_to_ucs4` / `parser_prefix` / `scalar_text`; `capi/src/phrase.rs` `pinyin_phrase_segment`; `data/src/phrase_library.rs` `phrase_text`; `input/src/tool.rs` `unichar_isgraph` (tools); `data/src/row_format.rs` `decode_puncts` | valid: identical strings. malformed/non-scalar: `g_utf8_to_ucs4` accepts surrogates and values above U+10FFFF; Rust `str` cannot. Embedded U+0000 stops the pin's conversion. | valid: contract. malformed: identical through `PhraseText`; tool: the pin's calls; punct: class (b) (G15) |
| G2 | C strings, joins and vectors: `g_strdup` 44, `g_free` 152, `g_strdup_printf` 14, `g_strconcat` 16, `g_strjoinv` 3, `g_strndup` 6, `g_strfreev` 10, `g_strv_length`/`g_strv_contains` 2, `g_string_*` 4 | both (punct subset pinyin only); all | `capi/src/ffi.rs` `strdup` / `key_text` (`g_strdup_printf`) / `strconcat_aux` / `strconcat_pair` / `strjoinv` / `symbol_vector`, the pin's own calls; `capi/src/aux_matrix.rs` `AuxText`; `capi/src/predict.rs` `prepend_punctuations`; `capi/src/iterators.rs` export rendering; `input/src/path.rs` `build_filename` | valid: identical bytes. malformed: raw path bytes, missing middle aux component, NUL in a diagnostic, duplicate punctuation. | valid: contract. malformed: G3 paths; aux over-read class (b); punctuation order and duplicates contract |
| G3 | Path joining: `g_build_filename`, 68 sites, e.g. `pinyin.cpp:164,252,268,286,308,395,400,952,968,985,1052`; `zhuyin.cpp:118,196,212,230,252,337,342,585,601,605,626,630,684` | both; all | `input/src/path.rs` `build_filename` / `build_path` / `build_bytes` (the pin's `g_build_filename` through `glib-sys`); `capi-marshal/src/input.rs` `filename` | valid: same resolved path. malformed: leading `/`, empty name, `..`, duplicate separators, non-UTF-8 bytes. | contract: the pin's call |
| G4 | GArray/GPtrArray containers: `g_array_*` and `g_ptr_array_*`, 502 sites (`g_array_index` 105, `g_array_new` 64, `g_array_free` 68, `g_ptr_array_index` 59, `g_array_set_size` 49, `g_array_append_val` 45, ...) | both; all | `capi/src/dict.rs` and zhuyin twin keep real `GArray` for the ABI; internal steps are `Vec`/slices in `engine/src/nbest.rs`, `engine/src/session/*`, `data/src/phrase_library.rs` | valid: identical candidate/result arrays. malformed: out-of-range indices, a key with no whole word, a short chunk header. | valid: contract. malformed: class (b) rows; ABI arrays MUST RETAIN |
| G5 | n-best tail sort: `phonetic_lookup.h:337` `g_ptr_array_sort(tails, trellis_value_compare)` | both; all | `engine/src/nbest.rs` `tail_merge` (stable `sort_by` on raw possibility, NaN last) | valid: the pin's `gint` truncation ties scores under one unit apart, so its comparator is not transitive on finite scores. malformed: NaN and ±∞ from λ outside `[0, 1]`. | language artefact: S3 in [compatibility-policy.md](compatibility-policy.md#registered-sites) |
| G6 | candidate and predicted list sorts: `pinyin.cpp:2286,2443` `g_array_sort_with_data(compare_item_with_sort_option)`; `zhuyin.cpp:1529,1610` `g_array_sort`; phrase-string dedup `pinyin.cpp:2067`, `zhuyin.cpp:1375` | both; all | `engine/src/candidate.rs` `rank_by_frequency`, called by `engine/src/session/mod.rs` `rank_candidates` and `capi/src/predict.rs` `sort_predicted` | valid: stable order over the enabled keys and the dedup winner. Frequency groups can be cyclic under the wrapping comparison. | contract: guarded wrapping frequency, [Candidate sorting guards the wrapping frequency order](upstream-divergences.md#candidate-sorting-guards-the-wrapping-frequency-order) |
| G7 | hash tables: `g_hash_table_new` 2, `insert` 4, `lookup_extended` 4, `destroy` 2; `g_direct_hash`/`equal` | both; all | `engine/src/nbest.rs` `Step`, `engine/src/session/*` vectors; no exposed iteration order | valid: lookup only; no order reaches the ABI. malformed: none. | contract (internal; no observable order) |
| G8 | diagnostics: `g_return_val_if_fail`, 6 sites (`pinyin.cpp:1454`, `zhuyin.cpp:973`, `phrase_index.cpp:354-356,361`); `abort` 45, nearly all after a failed internal check; `g_warning` only in the tool-only tag utility | both; all | `capi/src/ffi.rs` `log_warning`; zhuyin twin; `capi/src/phrase.rs` calls `glib_sys::g_utf8_strlen` | valid: the pin's asserts abort in the C adapters (`std::process::abort()`). malformed: the `g_utf8_strlen` critical is GLib's own; the other five sites are early returns of the pin's value with no critical ([Phrase segmentation writes no invalid-UTF-8 critical](upstream-divergences.md#phrase-segmentation-writes-no-invalid-utf-8-critical), [Phrase-library layout faults write no critical](upstream-divergences.md#phrase-library-layout-faults-write-no-critical)). | abort where the pin aborts; D1 for the six macro sites |
| G9 | allocation/free and real GArray ABI: `g_free` 152, `g_strdup` 44, `g_strdup_printf` 14; C `malloc`/`free`/`realloc` 5; `g_array_set_size`/`g_array_append_vals` into caller arrays in `pinyin.cpp:2805,2817` and `zhuyin.cpp:1797,1809` | both; all | `capi/src/ffi.rs` (both facades): returned strings and the keyboard vector from the pin's GLib calls; `malloc_cstr_with`, the fallible libc-`malloc` encoder for the `g_ucs4_to_utf8` sites (NULL, no warning); `capi/src/dict.rs` and zhuyin twin on `glib_sys`; `libpinyin.alloc` / `libzhuyin.alloc` | valid: caller may `g_free` / `g_strfreev`; the real `GArray` keeps GLib's private metadata. malformed: NULL or OOM paths. | contract; MUST RETAIN |
| G10 | rename and sync: `g_rename` 10 (`pinyin.cpp:1059,1078,1094,1108,1121`; `zhuyin.cpp:609,635,655,672,687`); `g_fsync` 1 (`memory_chunk.h:555`) | both; all | `user/src/persistence.rs` `save_reporting`, `pin_rename_order`; `input/src/path.rs` `rename`; `input/src/chunk.rs` `save` | valid: same file order and bytes. failure: per-file write/rename failures and the save's return. | contract |
| G11 | numeric parsing and locale: `fscanf` 10 (`table_info.cpp:208,214,220,228,232,255,338,344,352,357`); `atoi` 1 (`:160`); `setlocale` 6 (`:197,291,328,372,378,394`) | both; all | the pin's own calls in `oxpinyin-input` `config.rs` / `native.rs` through the `libc` crate, decoded by `data/src/table_info.rs` `TableConf::load` and `data/src/user_files.rs` `UserTableInfo::load`/`save` | valid: same table values, versions, λ bits and library mapping. malformed/host: the host libc's `%d`/`%f`/`%255s` scanner, the pin's own call; class (b): missing source buffer and merged-row SIGSEGV. | valid: contract. malformed: the pin's call except the class (b) refusals; host scanner host-dependent (policy row 78); `LC_NUMERIC` row 39 |
| G12 | file I/O and `MemoryChunk`: `mmap` 1, `munmap` 2, `lseek` 4, `read` 5, `write` 3, `open` 3, `close` 12, `unlink` 20, `fopen` 3, `fclose` 8, `feof` 1, `fprintf` 27 | both; all | `input/src/chunk.rs` `load` / `save` (the pin's `open`/`read`/`mmap`/`write`/`fsync`/`close`); `data/src/phrase_library.rs` `map_chunk`; `user/src/persistence.rs`; `data/src/c_stderr.rs` raw bytes | valid: same mapped extent, header check and file bytes. malformed: short/truncated/directory chunk; write failures. | contract on valid files. malformed: class (b) row 76, abort row 96, row 109 |
| G13 | C primitives: `memset` 86, `memcpy` 11, `strlen` 16, `strcmp` 18, `memcmp` 3, `memmove` 7, `strchr` 1 | both; all | Rust slices/`str`; `data/src/row_format.rs`, `core/src/single_gram.rs`, `capi/src/ffi.rs` | valid: same bytes. malformed: NULL/unreadable phrase strings and out-of-range walks are class (b) rows. | contract; class (b) at rows 80, 106 |
| G14 | libm: `log` 7 (`phonetic_lookup.h:39,250,663,693`; `phrase_lookup.cpp:40,321,342`) | both; all | `core/src/cost.rs` `cost_of_probability` / `surprisal` (`f64::log2`); `engine/src/phrase.rs` (`ln`) | valid: class (a) last-bit differences are accepted. | class (a), row 11 |
| G15 | punctuation decoding: `punct_table.cpp:56-72` `unescape`, `:74-94` `get_all_punctuations`, reached at `pinyin.cpp:2474` | pinyin; all | `data/src/row_format.rs` `decode_puncts` (whole stored words per punctuation; infallible); `data/src/punct.rs` `PunctTable::punctuations` (`Err` only for a failed read; `Ok(Vec::new())` for unopened/absent/empty); `runtime/src/lib.rs` `RuntimeDict::punctuations`; `capi/src/predict.rs` `prepend_punctuations` (`phrase_text_bytes` at the C boundary) | valid: identical punctuation strings. malformed: partitions below; misaligned lengths read whole words ([Punctuation values contain whole words](upstream-divergences.md#punctuation-values-contain-whole-words)). | contract; class (b) for the unterminated over-read |
| G16 | tool-only GLib: `tag_utility.cpp` (`g_strsplit_set`, `g_unichar_*`, `g_strv_length`, `g_utf8_get_char/next_char`, `g_ptr_array_foreach`, `g_hash_table_foreach_steal`, `g_warning`); `table_text_parser.h` (`g_ascii_strtoull`, `g_strlcpy`); the `load_text` methods | not C ABI | `utils/src/utils_helper/taglib.rs` (the pin's taglib); `input/src/tool.rs` (`g_strsplit_set`, `g_unichar_isgraph`, `atoi`, `atol`, `fprintf` through `glib-sys` and `libc`) | valid: tool input, identical records. malformed: the pin's calls, so its results. | tool scope: the pin's calls ([external I/O](external-io-boundary.md)) |
| G17 | dead code: `pinyin_lookup2.cpp`, `chewing_large_table.cpp`, `phrase_large_table2.cpp`, `facade_chewing_table.h`, `facade_phrase_table2.h`, `PunctTableEntry::escape/append/remove`, `#if 0` dumps | none | no shipped counterpart; the runtime uses `PhoneticLookup`, `PhraseLookup` and the large-table-3/chunk path | n/a | negative coverage |

## Family notes

### G1 — UTF-8 and UCS-4

The pin converts with GLib's `g_utf8_to_ucs4` and `g_ucs4_to_utf8` in
three candidate/sentence/export paths (`pinyin.cpp:1627-1645` and
`lookup.cpp:27-60`; the export is `pinyin.cpp:698-716`). GLib encodes
surrogates and values above U+10FFFF, and stops at an embedded U+0000.
Rust `char`/`str` cannot represent surrogates or values above U+10FFFF.
oxpinyin therefore carries phrase text as stored words (`PhraseText`,
`core/src/ucs4.rs`) and encodes it at the C boundary
(`phrase_text_bytes`).

`phrase_segment` calls the pin's `g_utf8_strlen` through `glib-sys`, so
GLib writes its own NULL-input critical. The invalid-UTF-8 critical is not
written ([Phrase segmentation writes no invalid-UTF-8 critical](upstream-divergences.md#phrase-segmentation-writes-no-invalid-utf-8-critical)).
`g_utf8_strlen` at `pinyin.cpp:1450` / `zhuyin.cpp:969` is the same gate.
`g_utf8_strncpy` at `zhuyin_parser2.cpp:879` is not an over-read: GLib
`g_utf8_strncpy` uses `strncpy`, which stops at the NUL.

Verdicts (pin `pinyin.cpp:614-652,698-768,1450-1454,1627-1645,1953-2138,3669-3710`;
`zhuyin.cpp:500-537,969-973,1096-1113,1294-1416`; `lookup/lookup.cpp:27-69`;
`storage/phrase_index.cpp:103-111,522-555`):

- Complete bounded phrase items, matching readings, nonzero scalar text:
  identical. GLib encoding and Rust scalar encoding agree.
- Stored surrogates and values above Unicode below `0x80000000`:
  identical. GLib emits extended UTF-8; `PhraseText` carries the words and
  emits the same bytes. Candidate, sentence and pinyin-export cases MATCH.
- Embedded U+0000 in a declared phrase body: identical. GLib stops per
  item; oxpinyin ends the outward text at the first zero word and keeps the
  stored length.
- Stored values `>= 0x80000000`: `phrase_segment` with a matching raw key
  is identical. The text getters differ: the pin's `_token_get_phrase`
  returns true with the declared length and the NULL from
  `g_ucs4_to_utf8` (`pinyin.cpp:1627-1645`, `zhuyin.cpp:1096-1113`);
  oxpinyin returns false and leaves the outputs. This difference is not
  admitted.
- Truncated, empty, overlong or inconsistent items: out-of-bounds reads are
  class (b) (rows 101 and 107). An in-range inconsistent length can make a
  path or token that the validating Rust reader refuses. That difference is
  not admitted and not normalized.
- Auxiliary UTF-8 substrings, tool Unicode categories and stored
  punctuation are G2, G16 and G15.

#### G1 phrase-segment closure

Pin `pinyin.cpp:1446-1460`, `zhuyin.cpp:965-979`,
`lookup/phrase_lookup.cpp:120-157,255-428`; GLib
`glib/gutf8.c:117-128,237-273,665-747,940-997`.

- **Bounded non-scalar input.** GLib accepts canonical historical encodings
  through `0x7fffffff` (`glib/gutf8.c:665-747`). The Rust decoder routes
  them to the same raw `u32` path; no `char` or `str` is built from a
  non-scalar. Sound data gives a length-sized zero-token result, and a
  matching raw key gives the pin's true/token.
- **Lead-byte walk past the allocation.** Class (b); no deterministic
  return is assigned.
- **Short user gram with a system gram.** Both sides abort at
  `get_total_freq` (row 85). Partial gram items and sub-total rows without
  a system gram are row 98; the raw span walk routes a reached unsafe
  predecessor through its one-warning refusal.
- **Inconsistent item lengths and token bounds.** `PhraseTableEntry::search`
  walks four-byte tokens to the raw value end
  (`storage/phrase_large_table3.cpp:29-51`); `get_phrase_item` trusts the
  declared length (`phrase_index.cpp:180-208`); `save_next_step` indexes by
  it (`phrase_lookup.cpp:348-377`). Out-of-range shapes are class (b) (rows
  101 and 107).
- **Invalid readings** past the last one are the uninitialised-key class
  (b) of rows 51 and 70; the segment path never renders a reading.
- **NULL, zero-length token-lookup key.** A GLib-rejected `*_lookup_tokens`
  call passes `search(0, NULL, tokens)`
  (`storage/facade_phrase_table3.h:111-138`). No backend dereferences the
  key: db5.3's get accepts a zero-sized DBT and `__bam_defcmp` bounds its
  loop by the smaller size (`src/btree/bt_compare.c:155-181`); kc's
  `ProtoDB::accept` tests the pointer at amd64 `+0xe5` and the size at
  `+0x310`; tkrzw copies the key with a zero-length `memcpy`. The pin
  returns `SEARCH_NONE`, leaves the caller's `GArray` empty and answers
  false, as oxpinyin does.
- **Faulting prefix stores.** A returned read failure is no gram on both
  sides; the search continues. oxpinyin reproduces the pin's kc
  second-read assertion as an abort.
- **Extreme lengths.** Both entries pass a `glong` count to
  `PhraseLookup::get_best_match(int)`; above `INT_MAX` the narrowing and
  `m_sentence_length + 1` overflow make the later step access class (b)
  (`lookup/phrase_lookup.cpp:55-73,120-127,382-428`).
- **Allocation failure.** The pin's conversion uses `g_try_malloc_n` and
  refuses with the ordinary critical (`glib/gutf8.c:905-912,977`), while its
  later container allocations abort. Rust's allocations abort. This is
  host-dependent resource failure with no single shared behaviour.
- **Neighbouring string-helper callers.** The parsers, sentence-prefix
  guesses, prediction, character offsets, token lookups, imports and
  remember pass raw bytes or convert at their own boundary. Rows 19 and 21
  hold the assert cases
  `c-string-input/pinyin-character-offset-invalid-past-matrix` and
  `c-string-input/pinyin-full-key-apostrophe-invalid`, where oxpinyin
  aborts as the pin does.

### G2 and G3 — strings and paths

- `g_strdup_printf` builds `.tmp` names and diagnostic text. The formats
  are `%s` and `%d` over bytes, not a locale conversion.
- `g_strconcat` builds the aux-text prefix/middle/postfix
  (`pinyin.cpp:3421-3431,3482-3509,3560-3569`) and the export joins
  (`pinyin.cpp:830,865`).
- `g_strjoinv`/`g_strfreev` at `pinyin.cpp:849,862` render the bigram
  export. The pin's array is not NULL-terminated (row 1, class (b),
  [record](upstream-divergences.md#bigram-export-iterators-pinyin-buffer)).
  oxpinyin renders owned strings before the iterator exists.
- Valid scalar phrase strings and their C marshalling are identical within
  G1's scope. Returned-string allocation is G9.

**Punctuation order and duplicates.** This is ordinary compatibility. The
pin's steps:

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

The `punct-dedup/*` cases hold this.

#### G2 auxiliary-text closure

The three exports read one matrix. `pinyin_parse_more_full_pinyins`
(`pinyin.cpp:1497-1517`), `pinyin_parse_more_double_pinyins`
(`:1540-1560`) and `pinyin_parse_more_chewings` (`:1582-1605`) fill it
through `fill_matrix` (`storage/phonetic_key_matrix.cpp:30-80`): one column
per consumed byte, each key at its `m_raw_begin`, zero keys in the gaps
(apostrophes), and the reserved zero key at `parsed_len`, so
`matrix.size() == parsed_len + 1`. Each export clamps the cursor with
`std_lite::min(cursor, instance->m_parsed_len)` (`:3390`, `:3449`, `:3527`),
leaving `cursor < matrix.size()`.

`_get_aux_text_prefix` and `_get_aux_text_postfix` (`:3295-3379`) walk the
same matrix, stop at the reserved last column, and build their result with
`g_strconcat` over non-NULL pieces (`:3325`, `:3369`); the only
NULL-argument stop is the final `g_strconcat(prefix, middle, postfix, NULL)`
(`:3431`, `:3509`, `:3569`).

**The NULL stop is unreachable.** For `full`, let `offset` be the loop
variable (`:3400-3429`); it starts at zero and `cursor >= 0`. Each step
computes `newoffset = _compute_pinyin_start(matrix, offset) >= offset`
(`:2903-2919`). If `offset <= cursor <= newoffset`, the boundary branch sets
`middle`. Otherwise `newoffset < cursor`, the walk assigns
`offset = newoffset`, and `key.begin == offset <= cursor`. If
`begin < cursor < end`, the split branch sets `middle`; otherwise
`cursor >= end`, and `offset = key.end <= cursor`. The invariant
`offset <= cursor` therefore holds until the walk reaches the reserved last
column `size - 1 == parsed_len`, where `newoffset == offset == parsed_len`
and the boundary branch fires because `cursor <= parsed_len`.

`double` and `chewing` (`:3460-3507`, `:3538-3567`) keep the same
invariant: if `cursor != offset`, then `offset < cursor`; the span at
`offset` starts there, so either it contains the cursor and the split branch
sets `middle`, or it ends at or before the cursor and `offset = end <= cursor`
with `end > offset`. The walk cannot step over `cursor`.

A reached empty column is the pin's `assert(matrix.get_column_size(offset) >= 1)`
(`:3311`, `:3353`, `:3410`, `:3467`, `:3545`) and oxpinyin's `None`; it never
reaches the concat. oxpinyin has no `middle == NULL` arm.

**`g_utf8_substring`'s absolute end position.** The chewing split calls
`g_utf8_substring(zhuyin, 0, len)` and `g_utf8_substring(zhuyin, len, end)`
(`:3557-3558`), where `len = cursor - begin` is a raw-input byte delta and
`end = m_raw_end` is an **absolute** raw-input byte offset. GLib
`g_utf8_substring` (`glib/gutf8.c:298-322`, read at tag `2.89.4`) resolves
`start = g_utf8_offset_to_pointer(str, len)` and
`end = g_utf8_offset_to_pointer(start, end_pos - len)`, then copies
`end - start` bytes and appends a terminator. `g_utf8_offset_to_pointer`
(`glib/gutf8.c:376-386`) steps `g_utf8_next_char` `end_pos - start_pos` times
and does not check for the terminator. The second call therefore asks for
`raw_end - len` scalars from scalar `len`, not for the `C - len` scalars
that remain. When `raw_end > C`, the call is outside GLib's documented
domain, and the walk passes the NUL. The copy reads beyond the allocation
once the end pointer is at least one byte past the NUL
(`raw_end >= C + 2`).

`raw_end` is the absolute key end. For a first key it is the raw span `B`;
for every later key it is larger. `B >= C` for every splittable
`content_table` row (all 441 rows at the pin: only the one-byte incomplete
`w`/`y` placeholders have `B < C`, and neither has an interior cursor), so
`raw_end > C` for every interior cursor except a first key whose raw span
equals its scalar count. For example, `nihao@3` cuts `hao` (`begin 2`,
`raw_end 5`, `C = 2`) at `len = 1`: the second call starts at `ㄠ` and asks
for `5 - 1 = 4` scalars, so it walks three scalars past the NUL. The zhuyin
parser's spans are also absolute, so its later keys reach the same
condition.

`g_strconcat(left, "|", right, " ", NULL)` (`:3560`) measures `right` with
`strlen`. For `len <= C` the first NUL in the copy is the spelling's end, so
the visible right half is the spelling from `len` to `C` (empty at
`len == C`). This matches `split_zhuyin`, an ordinary `str` scalar cut. For
`len > C` the copy begins after the NUL, so the pin's visible right half is
heap bytes; `split_zhuyin` returns an empty right half.

Disposition: class (b), registered as
[Chewing auxiliary substring over-read](upstream-divergences.md#chewing-auxiliary-substring-over-read):

- the absent out-of-allocation read, and possible fault, where the copied
  end passes the terminator (`raw_end >= C + 2`);
- the out-of-domain walk for `raw_end == C + 1`;
- the differing bytes for `len > C`.

The visible bytes for `len <= C` are equal only when the pin returns.

**Cases.** `aux-text-chewing-cursors` visits every byte cursor on four
full-pinyin inputs plus the maximum `size_t`: `nihao@3` reads past the NUL;
`nihao@1` is the first key's `B == C` in-bounds cut; `ni1@1` and `ni1@2` are
the `raw_end == C + 1` walk; `ni3hao4@6` reads past it.
`aux-text-chewing-zhuyin-cursors` does the same over the zhuyin parser
(`su3cl3`): its first key stays in bounds and its second key's absolute end
is `C + 3`. The Standard keyboard spends one keystroke per zhuyin scalar
plus the tone key, so an interior cursor's `len` stays below `C`. Longer
keyboards such as CP26 can spend two keys on one scalar, so their interior
`len` can exceed `C`; `split_zhuyin` applies the same class (b) clamp.
`aux-text-chewing-overrun-cursor` uses one past, seven past and the maximum
`size_t` on both parse paths; all clamp to `parsed_len` at `:3527`. The
cases compare visible bytes. The missing-middle and `len > C` partitions
have no case.

The matrix-empty branch writes an allocated `""` and returns `false`
(`:3385-3388`, `:3444-3447`, `:3522-3525`). Every other answer is `true`
plus one caller-owned allocation from the pin's `g_strdup`/`g_strconcat`.
Only `pinyin_get_chewing_auxiliary_text` reaches `split_zhuyin`, through
`SharedMatrix::chewing` and `chewing_aux_text`.

#### G3 path joining

`g_build_filename` is the only path constructor. oxpinyin calls it through
`glib-sys` in `oxpinyin-input` `path.rs` (`build_filename`, `build_path`,
`build_bytes`). GLib `glib/gfileutils.c:1925-2025` (`g_build_path_va` with
`"/"` on Unix). The pin joins exactly two elements, `dir` and `name`: an
empty `name` gives `dir`, an empty `dir` gives `name`; otherwise `dir` keeps
its leading separators and loses its trailing ones, `name` loses its
leading ones and keeps its trailing ones, and one separator stands between
the bodies. A part made only of separators has no body (`dir` is then kept
whole, `name` is appended whole). Nothing inside a part is touched.

| Partition | Verdict | Held by |
| --- | --- | --- |
| Ordinary pairs, empty `dir`, empty `name` | Identical bytes | `glib-922-dir-*`, `glib-922-row-*` |
| Leading `/` in a `table.conf` name (`/x`, `//x`, `sub//x`) | Stays beneath the directory; `Path::join` would make it absolute (rejected) | `glib-922-row-*`, `glib-922-row-missing-*` (`mmap … failed!` bytes) |
| Repeated or trailing separators on the directory (`/d///`) | Same file; printed path has one separator (`/d/x`) | `path::tests::names_stay_beneath_their_directory` |
| A part made only of separators (`/`, `//`) | Kept whole (`//` + `x` is `//x`; `/a` + `//` is `/a//`) | same test |
| `..`, `.`, leading `//` on the directory | Untouched | same test |
| `""` user directory (cwd) versus NULL (none) | Different arguments; `g_strdup` keeps the bytes (`pinyin.cpp:331-332`, `zhuyin.cpp:275-276`) | both capi crates' `tests/userdir_arg.rs` `null_and_empty_user_dirs_are_different_arguments` |
| Non-UTF-8 directory or name bytes | Raw bytes everywhere | `e2e_tests::directories_with_non_utf8_names_open`, `c_stderr::tests::paths_are_written_as_their_bytes` |
| `\` as a separator | An ordinary byte on Unix. Windows: GLib accepts both | no case; open on Windows |
| User-directory file names and `.tmp` siblings with `/u///` | Same file; the pin prints `/u/user.conf` (`table_info.cpp:332`, `:382`) and the `rename %s to %s failed.` pair | `user_conf_path_is_the_join_the_pin_writes`, `glib-922-dir-user-dup-trailing`, `glib-922-save-write-failed-dup-trailing` |

The system DBM opens (`data/src/dict.rs`, `runtime/src/lib.rs`) join a
constant name with `Path::join`. That is the same file, and the pin prints
no path of its own there. No discrepancy is registered for G3.

### G4 and G9 — containers, allocation and the ABI

The C ABI's `GArray` is a real GLib object. `g_array_set_size` and
`g_array_append_vals` in `pinyin_lookup_tokens`,
`pinyin_token_get_nth_pronunciation` and their zhuyin twins mutate the
caller's `GArray` through `glib_sys`. A manual `Vec` write would corrupt
GLib's private `_GRealArray` metadata. This is MUST RETAIN.
Internal `GArray`/`GPtrArray` uses are ordinary step, candidate and
lookup containers. Their Rust counterparts are `Vec`, slices and fixed
arrays. The malformed cases are the registered out-of-range index and
empty-chunk sites, not a container-layout difference.

GLib's implementation is why the adapter stays: `glib/garray.c:63-74` has
private element size, capacity, clear/terminator flags, reference count and
callback beyond public data/len; `:605-625,773-795,890-916` update those and
invoke caller clear callbacks; `:524-574` implements `g_array_free(FALSE)`'s
caller-owned detached payload. `glib/gmem.c:95-111,206-210` pairs
malloc/free; `glib/gstrfuncs.c:2571-2582` frees every vector element and then
the vector.

The allocation contract is the pin's:

- a returned `gchar *` is `malloc`-compatible and freed by `g_free`;
- a `gchar **` is freed by `g_strfreev`;
- the two `.alloc` files list every ownership slot (26 pinyin, 12 zhuyin).

The per-site GLib calls for returned strings and the allocation-failure
gate are in [abi-allocator-pairing §5](abi-allocator-pairing.md#5-caller-owned-strings).

Gates: `just abi exports` (the exported-symbol set equals the pin's
version scripts, 79 + 52 symbols) and `just abi alloc-pairing`
(`alloc-pairing-pinyin.cc`, `alloc-pairing-zhuyin.cc` and the
`libpinyin.alloc` / `libzhuyin.alloc` registers, with LeakSanitizer
negative controls). The `GArray` lifecycle is held by the
`glib-924-array-lifecycle-*` cases (a seeded clear callback, 257 appended
caller elements, shrink, a refusing lookup, clearing and zero termination,
zero-fill growth, `g_array_free(FALSE)` and `g_free`), by `oxpinyin-capi`
`boundary_tests::lookup_tokens_resets_the_callers_array` and by
`oxpinyin-zhuyin-capi`
`boundary_tests::caller_arrays_keep_their_metadata_and_clear_through_the_callback`.

The version scripts `src/libpinyin.ver` and `src/libzhuyin.ver` are the ABI. At the pin,
the macOS export list `src/libpinyin.exp` omits five exports that `libpinyin.ver` has:
`pinyin_begin_get_bigram_phrases`, `pinyin_bigram_iterator_get_next_phrase`,
`pinyin_bigram_iterator_has_next_phrase`, `pinyin_end_get_bigram_phrases` and
`pinyin_guess_predicted_candidates_with_punctuations`. The
[call-site audit](../runbooks/glib-call-sites.md#what-it-does) therefore rewrites both `.exp`
files from the version scripts on macOS.

#### Exported allocation and lifetime matrix

Decision: **retain the GLib ABI**; no runtime exception is admitted.
`MUST RETAIN` applies to the outward contract. `MAY DIVERGE` means
internal implementation freedom with identical outward results, **not**
permission to change a message, return, out-parameter, lifetime or
allocator pairing. Opaque Rust handles may use `Box`; Rust strings and
vectors may build payloads; only the C adapter transfers them to the
matching foreign allocation. A Rust `CString` from `into_raw` is not a
substitute for a promised `g_free` buffer.

`P` = `pinyin_`; `Z` = `zhuyin_`. The four caller-supplied `GArray *`
operations are additional to the `.alloc` registers (they have no pointer
return or `T **` slot).

| Exported operation | Allocation → library write → caller mutation/free: MUST RETAIN | Pin source; MAY DIVERGE internally |
| --- | --- | --- |
| P/Z `init`, `fini` | Caller lends directory strings for the call; library owns copies and context. Returns an opaque context, released only with the matching `fini`; the handle is never `g_free`d. | `pinyin.cpp:326-441,1194-1224`; `zhuyin.cpp:270-357,741-765`. Internal `g_strdup`/`g_free` may be Rust ownership, preserving path bytes and side effects. |
| P/Z `alloc_instance`, `free_instance` | Context must outlive instance; only matching destructor frees the handle and its internal storage. | `pinyin.cpp:1310-1356`; `zhuyin.cpp:845-883`. Internal arrays may be Rust vectors. |
| P/Z `begin_add_phrases`, `end_add_phrases` | Opaque iterator borrows context; matching end releases it and performs the pin's compact/modified effects. | `pinyin.cpp:506-520,655-660`; `zhuyin.cpp:392-406,540-545`. |
| P `begin_get_phrases`, `end_get_phrases` | Owned iterator even when it has no rows; released with end, not `g_free`. | `pinyin.cpp:662-689,771-773`. |
| P `begin_get_bigram_phrases`, `end_get_bigram_phrases` | Owned iterator; matching end releases its buffers, arrays and handle. | `pinyin.cpp:775-790,913-919`. Row-1 over-read exception stays bounded. |
| P `get_context` | Borrowed originating context pointer; caller must not free it. | `pinyin.cpp:1358-1360`. No zhuyin twin. |
| P/Z `get_candidate` | Borrowed candidate, NULL on out-of-range false; invalidated with candidate storage, never caller-freed. | `pinyin.cpp:2851-2863`; `zhuyin.cpp:1843-1855`. |
| P/Z `get_candidate_string` | Borrowed candidate-owned string; success allocates nothing and returns true. | `pinyin.cpp:2873-2877`; `zhuyin.cpp:1865-1869`. |
| P `get_pinyin_key`, Z `get_zhuyin_key` | Borrowed key, false-nulls; the pin uses a function-static key overwritten by later calls. | `pinyin.cpp:2921-2941`; `zhuyin.cpp:1886-1905`. Representation may differ only without changing the borrowed-slot behaviour. |
| P `get_pinyin_key_rest`, Z `get_zhuyin_key_rest` | Borrowed rest slot, false-nulls; same static-slot constraint. | `pinyin.cpp:2944-2964`; `zhuyin.cpp:1908-1927`. |
| P/Z `get_sentence` | Fresh caller-owned `g_free` bytes when a sentence is produced; no guessed results means false and untouched out-pointer. | `pinyin.cpp:1463-1481`; `zhuyin.cpp:982-999`; `lookup/lookup.cpp:27-69`. `ffi::sentence_text` makes the pin's `g_strdup`/`g_strconcat` chain over Rust-built phrases. |
| P `get_full_pinyin_auxiliary_text`, `get_double_pinyin_auxiliary_text`, `get_chewing_auxiliary_text` | Owned string, including an allocated empty string on false with an empty matrix; freed regardless of success. | `pinyin.cpp:3381-3575`. The pin's `g_strdup`/`g_strconcat`; G2 owns substring behaviour. |
| P `get_zhuyin_string`, `get_pinyin_string`, `get_luoma_pinyin_string`, `get_secondary_zhuyin_string`; Z `get_zhuyin_string`, `get_pinyin_string` | Fresh `g_free` buffer; zero-table-index key gives false and NULL. Z `get_pinyin_string` selects the Hanyu, Luoma or secondary renderer from the context. | `pinyin.cpp:2707-2748`; `zhuyin.cpp:1732-1765`; `storage/chewing_key.cpp:47-114`. Rust rendering is free. |
| P `get_pinyin_strings` | Two independently owned `g_free` buffers; NULL output addresses allowed independently; zero key leaves both outputs untouched and returns false. | `pinyin.cpp:2751-2762`; `storage/chewing_key.cpp:60-71`. No shared allocation for two independently freed slots. |
| P/Z `token_get_phrase` | Optional length/text outputs; missing item returns false without writing either. Successful text is an independent `g_free` allocation. | `pinyin.cpp:1627-1645,2776-2783`; `zhuyin.cpp:1096-1113,1768-1775`. |
| P `iterator_get_next_phrase` | Two independent `g_free` buffers per row. The pin initialises outputs NULL/count -1, then writes the row and returns true, including the last row. A call without a row is the row-62 abort/refusal exception. | `pinyin.cpp:698-768`. The phrase uses the fallible `malloc_cstr_with`; the reading is `ffi::strjoinv`, the pin's `g_strjoinv`. |
| P `bigram_iterator_get_next_phrase` | Two independent `g_free` copies. The last row returns false **after** writing both strings; the caller must free them. | `pinyin.cpp:896-910`. |
| P/Z `in_chewing_keyboard` | Caller owns the NULL-terminated vector and each string; may replace strings or resize the vector, finally `g_strfreev`. Unmapped key returns false with a NULL vector. | `pinyin.cpp:1615-1624`; `zhuyin.cpp:1087-1093`; `storage/zhuyin_parser2.cpp:301-332,496-544,798-851`. `ffi::symbol_vector` makes the pin's `g_ptr_array_*` calls. |
| P/Z `lookup_tokens` | Caller supplies a **real** `GArray` of `phrase_token_t`. The library clears previous contents, appends in library order, and preserves clear callback, element size, zero termination, refcount and capacity semantics. No-match false still clears. | `pinyin.cpp:2652-2667`; `zhuyin.cpp:1678-1693`; `storage/phrase_large_table3.h:77-97`. Rust may build token payloads; GLib must mutate the caller object. |
| P/Z `token_get_nth_pronunciation` | Real caller `GArray` of two-byte `ChewingKey`; cleared **before** item lookup, including missing-token false. A valid reading appends phrase-length keys. | `pinyin.cpp:2801-2818`; `zhuyin.cpp:1793-1810`; `storage/phrase_index.cpp:33-43`. Out-of-range reading uses the existing safety classification. |
| P/Z `phrase_segment` | No allocation transferred to the caller. The pin's temporary `g_utf8_to_ucs4` buffer is freed after lookup. | `pinyin.cpp:1446-1460`; `zhuyin.cpp:965-979`. `phrase.rs` decodes in Rust; there is no GLib conversion allocation there. |

Other temporary GLib arrays (parser matrices, per-library `PhraseTokens`,
candidate vectors, iterator scratch) do not cross the ABI.

#### Diagnostic channels

| Exported operation / trigger | Pin outward result and channel | oxpinyin |
| --- | --- | --- |
| P/Z `phrase_segment`, well-bounded `ff fe` after a successful segmentation | One NULL-domain critical (level 8), `bool <facade>_phrase_segment(<facade>_instance_t*, const char*): assertion 'num_of_chars == ucs4_len' failed`; false; previous result readable and unchanged (`pinyin.cpp:1450-1459`, `zhuyin.cpp:969-978`; `gutf8.c:950-990`). | D1: no critical; return and old result kept. `glib-924-segment-critical-*` and `glib-924-segment-fatal-critical-*` are POLICY-ACCEPTED ([Phrase segmentation writes no invalid-UTF-8 critical](upstream-divergences.md#phrase-segmentation-writes-no-invalid-utf-8-critical)). |
| P/Z `init`, `load_phrase_library`, `save`, `mask_out`; P `load_addon_phrase_library`, reached malformed sub-index separators or end | `phrase_index.cpp:354-356,361`: the first failed check emits one NULL-domain critical and `SubPhraseIndex::load` returns false. Facade outcomes are caller-specific: the load caller may ignore false; `save` may report true after a critical. | D1: early return of the pin's value with no critical ([Phrase-library layout faults write no critical](upstream-divergences.md#phrase-library-layout-faults-write-no-critical)); caller outcomes are the pin's. |
| P/Z `token_get_nth_pronunciation`, NULL `keys`, known token and in-range reading | GLib `g_array_set_size: assertion 'array' failed` then `g_array_append_vals: assertion 'array' failed`, domain `GLib`; facade returns true. Unknown token: only the first critical, false (`garray.c:611,778`). | The NULL array reaches GLib, which writes its own criticals with the pin's returns. `glib-924-null-array-pronunciation-*`. |
| P/Z `lookup_tokens`, NULL output array | `reduce_tokens` calls set_size, then append for each non-NULL prepared library array: one plus the number of prepared arrays `GLib` criticals; facade returns `SEARCH_OK & retval`. | Same; the fixture prepares seven library arrays, so eight criticals. `glib-924-null-array-lookup-*`. |
| Facade refusals mapped from unsafe reads | No pin counterpart. | Each class (b) record keeps its trigger, false/NULL/out-state and exactly one facade-domain warning (level 16) through `ffi::log_warning` (`libpinyin`/`libzhuyin`). |
| `ffi::log_warning` with an embedded NUL | — | `CString::new` drops the whole message; GLib `%s` would truncate. No reachable producer: literal messages, operation names and assertion expressions are NUL-free, `TableConfError` and `ChunkHeaderWrite` texts are static, input paths come through `CStr`, configured filenames stop at the first NUL, and backend messages end at `CStr`. Not an admitted outward difference. |

GLib `glib/gmessages.h:663-674` calls `g_return_if_fail_warning` then
returns the given value; `glib/gmessages.c:3193-3201` chooses critical and
formats the assertion; `glib/gmessages.h:323-325` defaults the domain to
NULL; `glib/gmacros.h:932-939` selects `__PRETTY_FUNCTION__` for this C++
build. GLib's own array checks use its `GLib` domain. GLib invokes the
handler before it enforces the fatal mask (`glib/gmessages.c:1274-1313`), so
under fatal criticals the pin dies of SIGTRAP at the segment critical;
oxpinyin does not (D1, POLICY-ACCEPTED).

`SubPhraseIndex::load` first deletes the old owning chunk, reads the total
and offsets, tests three separators, installs views, then tests the end
(`phrase_index.cpp:335-361`). A first-three-check refusal on a fresh
sub-index keeps the newly read local total but installs no views; a reload
of a populated object leaves stale views, whose later reads are unsafe and
not an output contract. The exported load/init callers ignore the inner
false (`pinyin.cpp:232-323,370-394,477-495`; `zhuyin.cpp:176-267`); save's
diff uses a fresh baseline and ignores its load false
(`phrase_index.cpp:270-283`); mask reload also ignores it
(`pinyin.cpp:1224-1295`; `zhuyin.cpp:759-830`). The fourth check
`index_three <= end` cannot fail on these callers' bounds without the
preceding separator read already leaving the payload.

The warning channel is MUST RETAIN at these adapters (no per-trigger
closure implied):

| Adapter path | Exported operations reaching the warning channel |
| --- | --- |
| Both `context.rs` | P/Z `init`, `save`. |
| Both `config.rs`; pinyin `state.rs::load_addon`/`unload_addon` | P `set_full_pinyin_scheme`, `set_double_pinyin_scheme`, `set_zhuyin_scheme`; Z `set_full_pinyin_scheme`, `set_chewing_scheme`; P/Z `load_phrase_library`, `unload_phrase_library`, `mask_out`; P `load_addon_phrase_library`, `unload_addon_phrase_library`. |
| Both `cursor.rs` | P `get_pinyin_key`, `get_pinyin_key_rest`, `get_left_pinyin_offset`, `get_right_pinyin_offset`; Z `get_zhuyin_key`, `get_zhuyin_key_rest`, `get_left_zhuyin_offset`, `get_right_zhuyin_offset`. |
| Both `keys.rs` | P/Z `parse_full_pinyin`; P `get_pinyin_is_incomplete`. |
| Both `sentence.rs`, `phrase.rs` | P/Z `guess_sentence`, `guess_sentence_with_prefix`, `get_character_offset`, `phrase_segment`; P `get_sentence`, `guess_candidates`; Z `guess_candidates_before_cursor`, `guess_candidates_after_cursor`. |
| Both `candidates.rs` | P/Z `train`; P `get_candidate_nbest_index`, `remove_user_candidate`, `choose_candidate`, `choose_predicted_candidate`. |
| Both `iterators.rs` | P/Z `iterator_add_phrase`; P `begin_get_phrases`, `iterator_get_next_phrase`, `begin_get_bigram_phrases`, `bigram_iterator_get_next_phrase`. |
| P `predict.rs` | P `guess_predicted_candidates`, `guess_predicted_candidates_with_punctuations`. |
| P `text.rs::render_shared` | P `get_full_pinyin_auxiliary_text`, `get_double_pinyin_auxiliary_text`, `get_chewing_auxiliary_text`. |

#### Allocation-failure partitions

GLib fatal allocation and fallible conversion are different contracts.
`g_malloc`/`g_realloc` call fatal `g_error` (`glib/gmem.c:95-106,164-177`);
`glib/garray.c:605-625,1071-1100,1890-1916` grows arrays through that
realloc; `glib/gstrfuncs.c:315-329` duplicates through `g_malloc`. The
conversion's `try_malloc_n` returns NULL
(`glib/gutf8.c:905-912,977-990,1034-1068`); the pin passes NULL `GError`
pointers, so a failed conversion logs nothing and leaves `items_written`
untouched. A conversion failure is never classified as a `g_malloc` abort. The
reference GLib defines `GLIB_USING_SYSTEM_PRINTF`; its `g_vasprintf` aborts
on ENOMEM with minimal raw stderr (`glib/gprintf.c:339-357`), not `g_log`.
Structured `g_error` (`glib/gmessages.h:328-334`) invokes its writer before
fatal termination (`glib/gmessages.c:1874-1910`); its level is 4, unlike the
precondition critical's 8.

Only the final foreign allocation is compared. Earlier Rust `String`,
`CString` and `Vec` allocations can terminate first; no identical
allocation graph and no whole-process OOM equivalence is claimed.

| Allocation boundary / exported operations | Pin result | oxpinyin result |
| --- | --- | --- |
| Real caller arrays, all four operations | GLib growth failure terminates through fatal allocation. | The same GLib growth; same fatal boundary. |
| P/Z key renderers, P `get_pinyin_strings`, the auxiliary text | Duplication and concatenation OOM is fatal; the formatted renderer aborts with raw stderr; `get_pinyin_strings` can terminate after writing the first output. | The pin's `g_strdup`/`g_strdup_printf`/`g_strconcat` through `ffi::key_text`, `ffi::strdup`, `ffi::strconcat_aux`: same fatal boundary. |
| P/Z `in_chewing_keyboard` | Vector and string allocation OOM is fatal; no partial vector. | `ffi::symbol_vector`, the pin's calls: same. |
| P/Z `token_get_phrase` | Conversion failure writes the declared length and NULL text, returns true, no warning (`pinyin.cpp:1627-1645`; `zhuyin.cpp:1096-1113`). | Both `dict.rs` getters do the same. |
| P/Z `get_sentence` | Each token conversion is fallible; a NULL conversion leaves NULL before a prefix exists or keeps the prefix, omitting that token; later dup/concat OOM is fatal; true (`lookup/lookup.cpp:27-69`). | `ffi::sentence_text`: the fallible encoder per phrase, then the pin's chain, true even when NULL. |
| P unigram iterator | Phrase conversion can yield NULL; the row still writes reading and count and returns true; reading allocation OOM terminates (`pinyin.cpp:698-768`). | Fallible `malloc_cstr_with` for the phrase, the pin's `g_strjoinv` for the reading. |
| P bigram iterator | Pair conversion can yield a NULL first phrase or omit a NULL second phrase; the returned `g_strdup` copies are fatal (`pinyin.cpp:822-836,896-910`; `glib/gstrfuncs.c:576-578`). | `BigramHandle::convert_loaded_pair` with the fallible encoder and `ffi::strconcat_pair`; the returned copies are the pin's `g_strdup`. |
| P/Z `phrase_segment` temporary conversion | Failed conversion of nonempty input leaves length zero: critical, false, old result kept. Empty input proceeds to an empty successful result (`lookup/phrase_lookup.cpp:36-52,129-157,382-433`). | Rust conversion allocation terminates; D1 for the critical. |
| P/Z `iterator_add_phrase`, `remember_user_input`; P `get_character_offset` | Failed conversion leaves length zero; return false before adding, remembering or writing an offset (P `:628-646,3678-3681,3210-3213`; Z `:513-532,2165-2168`). | Rust allocations terminate; ordinary returns kept. |
| P/Z `lookup_tokens` temporary conversion | Zero length on conversion failure searches zero-length input, clears the output and returns false (P `:2652-2667`, Z `:1678-1693`). | Caller-array ownership is independent of this partition. |
| P/Z `guess_sentence_with_prefix` | Failed conversion skips the prefix tokens and continues; P frees the prior prefix, stores NULL and keeps the prior length; Z uses a local zero length (P `:1389-1443`, Z `:912-962`). | Rust allocation can terminate first; C allocation staging is not imitated. |
| Context, instance and iterator handles | C++ `new` failure transfers no handle. | Rust `Box`; thresholds need not match. |

`malloc_cstr_with` frees and refuses a failed fill or an embedded NUL. A
checked-size overflow returns NULL before allocation. `just abi
alloc-failure` (`tools/abi/alloc-failure.cc`, [harness](../../tools/abi/alloc-failure.md))
expects GLib's `g_error` then SIGTRAP or SIGABRT on both sides for every
injected GLib allocation, and the `gprintf.c` abort for `vasprintf`. A
failed `GArray` growth has no contract case: both facades grow a caller's
array only through GLib, so the failure happens inside GLib's `g_realloc`
on both sides.

Open:

- the earlier Rust allocations, and individual runtime coverage of the
  source-only allocation-failure rows;
- the per-trigger re-audit of the facade warning register (G8).

### G5 and G6 — sorts

`g_ptr_array_sort` at `phonetic_lookup.h:337` orders the n-best tails
with `trellis_value_compare` (`:174-178`). That comparator truncates the
float difference to `gint` (`cvttss2si` in the bdb binary, symbol
`0x96010`). Scores less than one unit apart compare equal, so the
comparator is not transitive even on finite scores. NaN and ±∞ come from a
λ outside `[0, 1]`; oxpinyin sorts NaN last with signed zeros tied. The
pin's NaN conversion is not observable. Witness record:
[Pinyin trellis adjusted cost — S1/S2 repair](upstream-divergences.md#pinyin-trellis-adjusted-cost--s1s2-repair).

`g_array_sort_with_data` orders the pinyin and predicted candidate lists
under `compare_item_with_sort_option` (`pinyin.cpp:2286,2443`).
`g_array_sort` orders the zhuyin lists (`zhuyin.cpp:1529,1610`). Their
`gint` frequency key is a total order for λ in `[0, 1]`, and wraps into
cycles outside it. The phrase-string dedup sorts (`pinyin.cpp:2067`,
`zhuyin.cpp:1375`) are total; the dedup keeps one row per text.

These comparators are language artefacts. oxpinyin does not port GLib's
merge. The tails use a stable `sort_by` on raw possibility (S3,
`nbest.rs` `tail_merge`). The candidate and predicted lists use
`candidate::rank_by_frequency`: stable std sorting with the pin's guarded
wrapping frequency comparison
([Candidate sorting guards the wrapping frequency order](upstream-divergences.md#candidate-sorting-guards-the-wrapping-frequency-order)).
Out-of-range λ cases are attributed in that record and in
[Zhuyin selection uses length, raw score and input order](upstream-divergences.md#zhuyin-selection-uses-length-raw-score-and-input-order). Accumulation near-ties
are class (a) only within row 11.

The arm64 pin stores out-of-range unsigned frequencies differently from
amd64; amd64 is the reference, so this is a pin portability defect with no
oxpinyin exception ([binary finding](upstream-divergences.md#outstanding-upstream-defects-report-back)). The
sites are `_compute_frequency_of_items` in the bdb `libpinyin.so.15.0.0`
(amd64 symbol `0x8fa20`, conversion `+0x2ae`, store `+0x14e`; arm64 symbol
`0x72408`, conversion `+0x23c`, store `+0x128`) and `libzhuyin.so.15.0.0`
(amd64 `0x8dee0`, `+0x18d`, `+0x192`; arm64 `0x70804`, `+0x134`, `+0x138`).

### G7 — hash tables

`PhraseLookup` and the phonetic trellis use GLib hash tables for
prefix/step lookup (`phonetic_lookup.h:212,235,263,307-315,350`;
`phrase_lookup.cpp:49,65,80,358-363,421`). No iteration order reaches an
exported result: hashes index stored nodes, and result order comes from
content arrays. `PinyinLookup2`'s copies are dead. Rust uses vectors keyed
by step and token (`nbest.rs` `Step::node`, `phrase.rs` `Step::relax`).
Verdict: identical; no record needed.

### G8 — diagnostics

`g_return_val_if_fail` in `phrase_index.cpp:354-356` logs a critical
when a loaded sub-index cannot be read. `pinyin_phrase_segment` and
`zhuyin_phrase_segment` check the pin's precondition at `pinyin.cpp:1454`
and `zhuyin.cpp:973`. Under D1 none of the six sites writes a critical.
The pin's registered asserts and aborts are reproduced with
`std::process::abort()`; class (b) refusals keep exactly one warning with
the correct domain and level. A warning is observable, so it is never a
quirk by itself. The channels are in
[Diagnostic channels](#diagnostic-channels).

### G10 — rename and sync

The C-facing path is `pinyin_save` / `zhuyin_save` →
`facade::ContextCore::save_user` → `UserStore::save_reporting_with_baseline`
→ `persistence::save_reporting`. The reporting save is the only save.
`UserConfLaw` selects staged pinyin versus interleaved zhuyin writes.

| Path / side effect | Pin citation | Mandatory result |
| --- | --- | --- |
| No user directory or clean context | `pinyin.cpp:1132-1137`; `zhuyin.cpp:547-552` | Returns false before compaction, staging or marker write. A clean retry after a soft save failure does not repair the failed file. |
| Loaded default libraries, ascending index 1..15, skipping absent, NOT_USED and NULL user names | `pinyin.cpp:922-994,1025-1086`; `zhuyin.cpp:559-644` | Exact library order. SYSTEM_FILE/DICTIONARY writes a diff against the mapped system chunk; USER_FILE writes a chunk. |
| Pinyin `.tmp` staging and rename pass | `pinyin.cpp:971,988,996-1022,1059,1078,1094,1108,1121` | **All** selected libraries, the user pinyin index, user phrase index and user bigram are staged, then renamed in that order. A failure never reorders later files. |
| Zhuyin staging and immediate renames | `zhuyin.cpp:607-609,633-635,646-690` | Each library, then the pinyin index, phrase index and bigram, is written then renamed individually (`Staged::immediate`). |
| Chunk open, header, payload, sync and close | `include/memory_chunk.h:536-557`; callers `pinyin.cpp:971,988`, `zhuyin.cpp:607,633` | `oxpinyin-input` `chunk::save`: open; one `write` per header word; one payload `write`; `fsync` and `close`, both results ignored. A failed header write is the pin's `assert`, reproduced as an abort (row 77, `ChunkHeaderWrite`). A short or failed payload write is the pin's soft `false` with no sync (`PersistenceError::Io`), tolerated as the pin's callers ignore it. No retry. |
| Staged DBM indexes | `facade_chewing_table2.h:93-96`, `facade_phrase_table3.h:92-95`; `chewing_large_table2_bdb.cpp:135-161`, `phrase_large_table3_bdb.cpp:150-176`; KC `chewing_large_table2_kyotodb.cpp:124-145`, `phrase_large_table3_kyotodb.cpp:139-160`; tkrzw `chewing_large_table2_tkrzwdb.cpp:111-129`, `phrase_large_table3_tkrzwdb.cpp:102-120` | Stale staging is unlinked, backend-native output is created, records are copied, and the backend save finishes before rename. KC dumps snapshots (its synchronize/close branch is `#if 0`); tkrzw synchronizes with `false` then closes; BDB calls the temporary DB's sync with `m_db`, then closes the temporary DB. |
| Staged bigram | `ngram_bdb.cpp:81-110`; `ngram_kyotodb.cpp:82-113`; `ngram_tkrzwdb.cpp:65-83`; facade `pinyin.cpp:1014-1022`, `zhuyin.cpp:680-690` | Last DBM, before the marker. Backend failures do not prevent the attempted rename. |
| Failed rename, including a directory at the final name | `pinyin.cpp:1059-1129`; `zhuyin.cpp:609-638,655-690` | The failed staging and destination stay; later files are still attempted; `rename %s to %s failed.` is printed without errno text; then marker and dirty reset. `save_reporting` collects failures; `ContextCore::save_user` prints them after persistence returns. |
| Marker after attempted renames | `pinyin.cpp:220-232,1141-1146`; `zhuyin.cpp:164-176,695-698`; `table_info.cpp:377-396` | In-place `user.conf` rewrite through the pin's `fopen`/`fprintf`/`fclose`. Pinyin keeps its counter; zhuyin writes counter zero. Open failure prints `write %s failed.`; the caller still clears dirty and returns true. |
| Fini | `pinyin.cpp:1194-1200`; `zhuyin.cpp:741-757` | Neither saves dirty dictionaries. Pinyin lowers the counter (`>1 ? counter-1 : 0`) and writes the marker; zhuyin only releases state. |
| Restart after interruption | `pinyin.cpp:173-217,922-1146`; `zhuyin.cpp:126-162,547-698`; `table_info.cpp:399-412` | Finals are loaded; `.tmp` is never replayed. No multi-file atomicity: pinyin before rename keeps old finals; either facade during renames can leave mixed generations; zhuyin even before all staging completes. |
| Same-backend content | `pinyin.cpp:963-1018`; `zhuyin.cpp:596-685` | Encoded chunk/diff content, indexed records, bigram values and reload results. Logical records, not physical bytes (below). |

Held by `crates/oxpinyin-user/tests/save_semantics_929.rs` and
`save_boundaries_929.rs` (both facade laws:
`failed_renames_report_exact_order_and_preserve_destinations`,
`interrupted_save_preserves_the_facade_specific_committed_prefix`,
`sync_rejecting_chunk_target_does_not_stop_later_files`,
`save_reload_keeps_bigrams_and_unsaved_changes_do_not_replace_them`),
`reporting_save_stops_at_a_chunk_header_write`, the `oxpinyin-input`
`chunk.rs` tests (`a_chunk_is_the_written_bytes`,
`an_unopenable_path_is_the_soft_failure`,
`a_header_write_failure_is_the_abort_site`) and the `save-929-*` contract
cases. The `save-929-boundary-*` cases interrupt at the marker write-open
(through a Linux read lease), after save and after fini; zhuyin has no fini
marker open, so it has no fini-marker case.

**GLib and libc boundary.** On Unix, `glib/gstdio.h:60-84` makes `g_rename`
the macro `rename` and `g_fsync` the macro `fsync` (GLib 2.84.4;
`glibconfig.h` defines `G_OS_UNIX`). The pin's chunk call therefore has no
GLib EINTR retry or full-sync upgrade; the separately compiled wrapper's
`F_FULLFSYNC` and EINTR branches (`glib/gstdio.c:1684-1702`) are not the
pin's call. oxpinyin calls `fsync` as the macro does. At glibc 2.41,
`rename` is one `rename` syscall with no retry
(`sysdeps/unix/sysv/linux/rename.c:25-35`), and `fsync` is a cancellation
point with no EINTR retry (`sysdeps/unix/sysv/linux/fsync.c:24-28`,
`nptl/cancellation.c:25-80`). The pin ignores the sync result and its
rename callers print a fixed message with no libc error text. On Windows,
GLib's `g_rename` converts both names to UTF-16 and calls `MoveFileExW`
with `MOVEFILE_REPLACE_EXISTING` (`glib/gstdio.c:1161-1194`), and maps
`GetLastError` to errno integers (`:99-140`); replacement of an existing
final is a required save effect.

**Logical records, not physical bytes.** A physical-byte comparison is
rejected as the criterion: the pin's own two saves of equivalent profiles
differ. In BDB the metadata UID at offset 52–71 (inode bytes 52–55, random
ID 60–63, process-seeded serial 64–67) is stamped at creation
(`src/fileops/fop_util.c:633-645`; `src/os/os_fid.c:35-131`;
`src/dbinc/db_page.h:73-94`), and creation reuses the metadata buffer for
the root or final bucket, so the UID residue stays in free space
(`src/btree/bt_open.c:538-569`; `src/hash/hash_open.c:417-446`; db5.3
5.3.28). LSNs are `{0,1}` on this path and do not differ. The pin save
unlinks and creates new containers before copying records
(`chewing_large_table2_bdb.cpp:135-161`,
`phrase_large_table3_bdb.cpp:150-176`, `ngram_bdb.cpp:81-110`). tkrzw
HashDBM writes a wall-clock microsecond timestamp at bytes 48–55
(`tkrzw_dbm_hash.cc:46,1184,1241,1296`), and TreeDBM uses an underlying
HashDBM. KC's `dump_snapshot` (`kcdb.h:2271-2328`) writes no file ID or
timestamp, but its record order follows bucket iteration
(`kcstashdb.h:442-468`). No byte is masked. The criterion is
`save-929-logical-cross-open-{pinyin,zhuyin}`: fresh pin→oxpinyin and
oxpinyin→pin workers open the donor's exact directory, recover the saved
tokens, re-add one existing reading with count zero (which keeps it,
`pinyin.cpp:561-582`, `zhuyin.cpp:449-469`), save and must give the same
logical rows; each receiver is checked against its donor and the other
receiver. `oxpinyin-capi`
`pinned_data_tests::the_pins_saved_profile_opens_with_its_records` and
`oxpinyin-zhuyin-capi`
`pinned_data_tests::a_pin_written_profile_opens_and_saves_with_its_logical_records`
hold this on the frozen pin profiles.

No language/runtime exception is established; the divergence register has
no G10 entry.

Open:

- interruption inside each DBM write and each rename, and writes after the
  marker open;
- backend sync/close errors (EIO, ENOSPC, EINTR) and power-loss durability;
- the interleaving of rename diagnostics with backend diagnostics;
- non-Linux execution, including the Windows `MoveFileExW` mapping.

### G11 — numeric parsing and locale

`SystemTableInfo2::load` and `UserTableInfo::load` read their fields with
`fscanf`. `atoi` cuts an addon index to `guint8`. The return of
`setlocale(LC_NUMERIC, "C")` is the new name, and every early return skips
the restore.

oxpinyin makes the pin's own calls. `oxpinyin-input` `config.rs` runs
`setlocale`, `fopen`, `fscanf`, `feof`, `atoi`, `fprintf` and `fclose`
through the `libc` crate, with the pin's format strings and call order.
`data/src/table_info.rs` (`TableConf`) and `data/src/user_files.rs`
(`UserTableInfo`) keep what the pin does with each answer. The host libc's
scanner therefore decides malformed spellings, as it does for the pin. That
is host dependence (policy row 78), not a divergence
([source policy, external I/O](../source-policy.md#external-io)).
`UserTableInfo::to_text` formats the two version fields through `i32`, so a
value outside `int` is written back signed, as the pin's `%d` does. An
unknown ordinary database name aborts, as the pin does (row 44).

Two class (b) refusals remain:

- A missing `source table format:` line reads the pin's uninitialised
  `char str[256]` (`table_info.cpp:228-229`;
  `TableConfError::UninitialisedSourceFormat`). oxpinyin refuses it
  deterministically.
- A five-word row merged with the next row's first word can make the pin
  read a NULL item and SIGSEGV (`table_info.cpp:254-287`). oxpinyin answers
  it with the declared result.

Neither refusal reproduces the uninitialised or invalid read.

`table.conf` partitions (glibc 2.41 sources; every case runs on both
facades):

| Partition | Verdict and source | Held by |
| --- | --- | --- |
| Directive literals and white space: a format space or `\n` matches any run of C white space, including none; one-line headers, indented `lambda`, CRLF and a missing final newline are accepted; leading bytes before the first literal do not match | contract; `vfscanf-internal.c:452-486` | `table-conf-919-header-{one-line,crlf,indented-lambda,no-final-newline,leading-space}` |
| Header order and missing fields: a missing `binary`/`model`/`lambda` directive is the pin's `false`; a missing `database format:` line reuses the `source` word and aborts; an empty source or database value makes the next word the value and aborts at the converter | contract and reproduced aborts; `:452-486`, `:1076-1259` | `table-conf-919-{binary-version,model-version}-line-missing`, `table-conf-abort-919-*-missing`, `table-conf-abort-source-format-line-missing` |
| `%d` grammar: white space, optional sign, at least one ASCII digit; no digits fails; base 10 leaves `x` after `0x` | contract; `:1452-1500`, `strtol_l.c:401-435` | `table-conf-919-versions-{plus-sign,no-digits}`, `versions-negative-overflow` |
| `%d` and `atoi` range: `strtol` saturates at `LONG_MIN`/`LONG_MAX`, the `int` store keeps the low 32 bits on amd64/arm64, an addon index is cut to `guint8` | contract; `strtol_l.c:483-519`, `vfscanf-internal.c:1963-1984` | `table-conf-919-versions-{positive-overflow,int-wrap,negative}`, `table-conf-919-addon-atoi-nondigit`, `table-conf-abort-919-addon-positive-overflow`, `user_files::tests::an_out_of_int_version_is_written_signed` |
| `%f` decimal: optional sign, digits with an optional dot, at least one digit, optional exponent; the scan does not backtrack | contract; `:2006-2320`, `strtod_l.c:607-940` | `table-conf-919-lambda-{dot-leading,dot-only,positive-sign,exponent-plus,decimal-underflow}` |
| `%f` hexadecimal: `0x`/`0X`, hex digits with an optional dot, optional `p` exponent; `e` is a hex digit; `0x` fails; `strtof` rounds to nearest even | contract; `:2140-2320`, `strtod_l.c:213-357,684-756,850-940` | `table-conf-919-lambda-{hex-no-exponent,hex-overflow}`, `lambda-hex*` |
| `%f` specials: case-insensitive `inf`, `infinity`, `nan`, `nan(n-char-sequence)`; `infin`, an unclosed `nan(` or an invalid payload fails; the payload is not observable | contract; `:2034-2160`, `strtod_l.c:640-684` | `table-conf-919-lambda-{infinity,nan-payload,nan-unclosed}`, `lambda-inf` |
| Incomplete exponents and `0x.` (below) | host-dependent (row 78) | the six `host_scanf` cases |
| `%255s` words: skip white space across newlines, take at most 255 bytes, keep NUL and non-UTF-8 bytes; the C string stops at the first NUL; a 256th byte starts the next word | contract; `:533-539`, `:1076-1259` | `table-conf-919-word-nul-truncates`, `table-conf-abort-919-word-width-overlong` |
| Six-word rows: words group across lines; a final short row is dropped; a merged row is class (b) (above) | contract; class (b) | `table-conf-919-short-row-merged`, `short-row-dropped`, `extra-word-dropped`, `comment-dropped` |
| Training λ: the tools read `table.conf` through the same `TableConf::load` (`oxpinyin-utils` `phrase_index::load_table_conf`) | tool contract | shared reader |

**Host scanner versions.** The reference glibc is 2.41. Its `%f`
lexer sets `got_e = got_dot = 1` at an exponent mark but does not clear
`got_digit` (`stdio-common/vfscanf-internal.c:2196-2201`). The post-loop
conversion check tests only the empty buffer and the bare `0x` prefix
(`:2545-2549`). glibc 2.42 rejects incomplete exponents, and 2.43 also
rejects `0x.` (`vfscanf-internal.c:2218-2219`, `:2571-2576` at 2.43-3).
2.41 therefore converts `1e`, `1e+`, `0x1p`, `0x1p+` and `0x1p-` (to 1.0)
and `0x.` (`strtof("0x.")` treats it as 0, `stdlib/strtod_l.c:690-768,1044-1048`),
and fails on `0x` and `infin`. These spellings are the `host_scanf` cases
of policy row 78, reported HOST-DEPENDENT on both facades:
`table-conf-lambda-exponent-without-digits`,
`table-conf-lambda-hex-exponent-without-digits`,
`table-conf-919-lambda-exponent-sign-without-digits`,
`table-conf-919-lambda-hex-exponent-sign-without-digits`,
`table-conf-919-lambda-hex-exponent-minus-without-digits` and
`table-conf-919-lambda-hex-dot-without-digits` (`host_scanf=('0x.',)`).
Any other difference stays DIFFER. The `%d` lexer is unchanged for the
base-10 table fields, and `%255s` is unchanged for the caller-provided
256-byte buffer.

`user.conf` partitions (pin `table_info.cpp:325-397`: `load` at
`:338,344,352,357`, `save` at `:372-397`, `is_conform` at `:399-413`,
`get_open_counter` at `:422-426`; the fixtures read the cell's `table.conf`
values, so every case is backend-independent):

| Partition | Verdict | Held by |
| --- | --- | --- |
| Version `%d`: LP64 `strtol` saturation, then the low 32 bits of the `int` store: `2147483648` is `-2147483648`, `4294967303` is `7`, `9223372036854775808` is `-1`, `-9223372036854775808` is `0` | contract | `oxpinyin-capi` `pinned_data_tests::user_conf_versions_past_int_take_the_scanners_int` |
| Open counter `%d`: the same conversion; a missing directive reads 0; a counter above `OPEN_COUNTER_LIMIT` is non-conforming and `get_open_counter` turns it into 0; the load raises `get_open_counter(existing) + 1` and fini lowers it | contract | `oxpinyin-data` `user_files::tests::marker_fields_follow_the_scanner` |
| `database format:%255s`: a known C string selects the backend; an unknown token aborts (row 44); EOF before the conversion leaves `UNKNOWN_FORMAT`; a missing literal or matching failure reads an unwritten `str` (class (b)); a NUL ends the C string, as the pin's `strcmp` does (`:122-131,350-354`) | contract; abort; class (b) | `marker_fields_follow_the_scanner`; `oxpinyin-capi` `boundary_tests::init_aborts_on_an_unknown_user_conf_database_format` |
| Missing or malformed version directive: `load` returns `false` with the reset defaults (`0/0/UNKNOWN/0`); `check_format` sees a non-conforming marker and wipes the profile | contract | `user_conf_versions_past_int_take_the_scanners_int` |
| Conformance: a conforming marker (including a wrapped version `7 + 2^32`, or a counter wrapping to 0) keeps every user file and learned count; a non-conforming marker follows `_clean_user_files`, and `write_marker` writes the system versions with the raised or lowered counter | contract | `user_conf_versions_past_int_take_the_scanners_int` (trained and imported unigrams after reopen) |
| Writer: `UserTableInfo::save` writes the versions and counter signed with `%d` and the database format canonically | contract | `user-conf-920-version-overflow-*`, `user-conf-920-counter-*`, `an_out_of_int_version_is_written_signed` |

**`LC_NUMERIC`.** Retained under the
[row 39 ruling](compatibility-policy.md#lc_numeric-after-init-row-39). The
mechanism is the pin's: `SystemTableInfo2::load`, `UserTableInfo::load` and
`UserTableInfo::save` save the return of `setlocale(LC_NUMERIC, "C")`,
which is the new name `"C"` (`table_info.cpp:197,291`; `:328,372`;
`:378,394`). The closing call re-installs `"C"`, and every early return
skips it. oxpinyin makes the same calls at the same points in
`oxpinyin-input` `config.rs`. The `setlocale` call comes before any parse
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
pin and in oxpinyin; the ruling accepts that concurrency impact unchanged.
An upstream fix would copy `setlocale(LC_NUMERIC, NULL)` first and restore
it on every path. oxpinyin changes only if libpinyin does.

### G12 — file I/O

`MemoryChunk` maps the file, checks the 8-byte header, the length word
and the checksum, and reads the payload (`include/memory_chunk.h:72-78,424-520,555`).
oxpinyin's mapping is `oxpinyin-input` `chunk::load` and the mapping source
(the pin's `open`, header reads and `mmap`), with the same header checks;
`unlink` belongs to `_clean_user_files` and the user-file rewrite;
`fprintf` is the raw stderr channel of the same paths. The writes are G10.

- Valid header, length, checksum, payload and mapped extent: identical.
- Short, truncated or unreadable system chunk: class (b) under row 76.
- A directory: below the header size it follows row 76; at or above it
  reaches `assert` at `include/memory_chunk.h:493`, reproduced as an abort
  (row 96). The split is filesystem-dependent.
- Sparse chunk mapping after fini: row 109, a pin resource defect with no
  compatibility exception
  ([record](upstream-divergences.md#a-sparse-chunks-whole-mapping-is-retained-after-fini-865-policy-row-109)).
- Other write, read and unlink failures and raw stderr: open.

### G13 — C primitives

`memset`/`memcpy`/`memmove`/`memcmp`/`strlen`/`strcmp`/`strchr` are
ordinary byte and string operations in the pin's parsers, chunk and
candidate code (e.g. `phrase_index.cpp:103-111,460-475`;
`pinyin.cpp:1635-1637,2041-2067`; `zhuyin.cpp:1096-1104,1360`). The Rust
equivalents are slices and `str`. There is no observable difference on
valid data. NULL or unreadable candidate text from a mismapped library or a
journal-removed item is class (b) at rows 80 and 106, which have different
outcomes (a warning refusal; omission with true). Other NULL, out-of-range
walks and malformed lengths are open.

### G14 — libm

The only reachable libm call is `log`. The n-best trellis and the phrase
lookup accumulate natural logs in `gfloat`. Three of the seven sites
(`phonetic_lookup.h:39,250`, `phrase_lookup.cpp:40`) pass a `float`, so
they resolve to `std::log(float)` in `<cmath>`. oxpinyin uses Rust's
standard maths (`f64::log2` in `core/src/cost.rs`, `ln` in
`engine/src/nbest.rs` and `engine/src/phrase.rs`). The class (a) register is
row 11. Basic arithmetic and float-to-integer comparator or frequency
conversion are not class (a) because floats occur; they are G5 and G6.

### G15 — punctuation

- `decode_puncts` walks a stored value in whole words and keeps each
  punctuation's stored words (`PhraseText`). The pin's
  `punct_table.cpp:57-94` walks bytes; see
  [Punctuation values contain whole words](upstream-divergences.md#punctuation-values-contain-whole-words).
- `prepend_punctuations` encodes them with `phrase_text_bytes`. That is the
  `g_string_append_unichar` form (GLib 2.84.4 `gstring.c:816-888` copies
  `g_unichar_to_utf8`, `gutf8.c:546-596`).
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
| Length not a multiple of 4 | `maxlen` is the `sar` of the byte difference and `begin < end` compares bytes: whole words, then one empty row after a terminated run | whole words only: no extra empty row. `misaligned-terminated` and `misaligned-short` are POLICY-ACCEPTED ([Punctuation values contain whole words](upstream-divergences.md#punctuation-values-contain-whole-words)) | `punct-decode/misaligned-terminated`, `punct-decode/misaligned-unterminated`, `punct-decode/misaligned-short` |
| Absent DBM entry | no row: `load_entry` leaves the chunk empty on a failed get (`punct_table_bdb.cpp:145,154-156`) | no row | — |
| Unopened table | no row: `pinyin_init` ignores a failed attach (`pinyin.cpp:437-441`); `load_entry` returns `false` with no database (`punct_table_bdb.cpp:141-142`) | no row | — |

The unterminated read is class (b), registered as
[Unterminated punctuation value over-read](upstream-divergences.md#unterminated-punctuation-value-over-read).
The rows match whenever the pin returns. Only the read and its possible
fault are not reproduced. The misaligned partition is the
language-artefact record above. The other partitions are ordinary
compatibility. No zhuyin punctuation export exists.

### G16 and G17 — tools and dead code

`pinyin_internal.h` includes `tag_utility.cpp`, but no exported entry
calls `taglib_*`. `utils/training` and `utils/storage` use it. `table_text_parser.h` and the `load_text` methods are
used only by `utils/storage/gen_binary_files.cpp`.
`PinyinLookup2` is never instantiated; `PhraseLookup` uses
`FacadePhraseTable3` and `FacadePhraseIndex`, not the old
`FacadePhraseTable2`/`ChewingLargeTable`. The runtime constructors at
`pinyin.cpp:351,363,406-425` and `zhuyin.cpp:295,306,348-355` use
`FacadeChewingTable2`, `FacadePhraseTable3`, `PhoneticLookup` and
`PhraseLookup`. The `#if 0` dumps and the `PunctTableEntry` writer methods
(`punct_table.cpp:41-54,96-146`) have no exported caller. G17 is excluded
from exported-surface coverage; it is not "identical", and the exclusion
authorises no deletion.

The tools are drop-in ports of the pin's utilities
([external I/O, stage 5](external-io-boundary.md#proposed-crate-and-staged-implementation)).
They make the pin's calls through `oxpinyin-input` `tool.rs`: `taglib` is
`oxpinyin-utils/src/utils_helper/taglib.rs`, and `atoi`, `atol`,
`g_strsplit_set`, `g_unichar_isspace` and `g_unichar_isgraph` are the
host's. So the separator, escape, category and numeric results are the
host's, as for the pin (pin `tag_utility.cpp:94-95,118-264`;
`table_text_parser.h:34-120`; `utils/utils_helper.h:51-72`).

- Ordinary tagged interpolation input: identical, the pin's taglib with
  the pin's GLib calls ([grammar record](interpolation2-grammar.md)).
- Quoted and dangling escapes, Unicode categories, unexpected or missing
  tags: the pin's calls (`g_unichar_isspace`, `g_unichar_isgraph`,
  `g_warning`); the macro checks are plain Rust without GLib's critical
  text ([Utility check macros write no message](upstream-divergences.md#utility-check-macros-write-no-message)).
- The segmented `token phrase` stream: `g_strsplit_set(line, " \t", 2)`,
  the empty-line null token and the macro's `abort()` on a missing
  separator (`utils/src/training/mod.rs`).
- Numeric fields in the segmented stream and K-mixture text: the host's
  `atoi`/`atol` through `libc`.
- Four-field generation records: `gen_binary_files` reads the pin's
  `table_text_read_pinyin_record` / `_punct_record` grammar in
  `utils/src/storage/table_text.rs`.

The [frozen tool cases](../runbooks/tool-diff.md) hold these.

## Exported C string inputs

The 25 exported functions with string inputs (29 parameters) convert at the
`oxpinyin-capi-marshal` boundary (classification row 9), with the pin's
invalid, non-scalar and NULL outcomes per parameter.

## Executable copies of external-library algorithms (2026-10-29 UTC)

This section follows the ruling "No ported external-library internals"
([compatibility policy](compatibility-policy.md#no-ported-external-library-internals)).
It covers `crates/` and `tools/`. All five items are replaced; the
[classification on main](#classification-on-main) gives the current code.
No other transcription of GLib, libstdc++, libc++ or glibc comparison or
probe order exists. `tools/audit/glib_call_sites.py` lists call sites; it
does not port them.

**Compliant, not ports:**

- `oxpinyin-data/src/table_entries.rs` calls `Vec::binary_search` on a
  token list that its own `insert` keeps sorted. That is the standard
  API with its documented semantics.
- The `oxpinyin-capi-marshal` UTF-8 codecs are ABI-prescribed boundary
  code, not external-algorithm ports: `input.rs` `utf8_to_ucs4`,
  `ucs4.rs` `decode`, and `phrase_text.rs` `encode_words`/`encode_word`.
  They give the C strings the bytes GLib's `g_utf8_to_ucs4` and
  `g_ucs4_to_utf8` would.

**Unpartitioned stored data (items C and D).** Unpartitioned stored data is
deliberately not checked for order, sorted or cleaned, because a per-lookup
sortedness check would make the search linear. On such data
`partition_point` stays memory-safe and deterministic for one toolchain,
but its result is unspecified and not the pin's; observable differences are
recorded as `POLICY-ACCEPTED`. The structural checks for truncated records,
invalid lengths and other unsafe conditions still apply in full.

| Item | Former copy and callers | Replacement | Parity | Inputs that differ; cases that become `POLICY-ACCEPTED` | Tests |
| --- | --- | --- | --- | --- | --- |
| A | `oxpinyin-engine/src/nbest.rs` `glib_merge_sort`, a GLib merge transcription, called by the candidate and prediction sorts (`pinyin.cpp:1678-1709,2286,2440-2445`; `zhuyin.cpp:1129-1144,1529,1610`). | `candidate::rank_by_frequency`: stable std sorting with the enabled length/span fields and native `rhs.wrapping_sub(lhs) as i32` frequency comparison. Each leading-key tie group is guarded; only cyclic groups use stable numeric descending fallback. | Exactly where every leading-key tie group's frequencies fit in a circular interval narrower than 2³¹, including wraparound across zero. Antipodal pairs are inconsistent and rejected by the guard. λ alone does not determine consistency. | Only cyclic groups may differ ([item A finding](upstream-divergences.md#candidate-sorting-guards-the-wrapping-frequency-order)); the four `row58-signed-{score,duplicate}-{off,on}` cases must MATCH, never POLICY-ACCEPTED. | `candidate.rs` guard tests (order laws at boundary triples, empty lists, ties, antipodal pairs, cycles, leading-field isolation); stable ties, disabled keys, dedup alignment and `session::tests::the_pinyin_length_key_wraps_at_eight_bits`. |
| B | `nbest.rs` `heap_push`, `heap_adjust`, `heap_pop`, `heap_make`, `heap_top`, a transcription of libstdc++ `stl_heap.h`. Callers: the two-slot node insert (`lookup/phonetic_lookup_heap.h:56-81`), beam selection (`lookup/phonetic_lookup.h:770-771`) and final-tail selection (`:331-337`). | `select_top`: `select_nth_unstable_by` at `k − 1`, then a sort of the `k`-prefix, on a complete key (`selection_compare`). `Node::keep`: direct two-slot transitions. | **Pinyin: everywhere.** The S1/S2 key (adjusted S, then batch position) is total, and the node transitions are derived from the pin. **Zhuyin:** wherever the comparator is a strict weak order and no two values tie on both length and score. | **Zhuyin only.** (1) λ outside `[0, 1]`, where NaN and finite scores of one length meet in one selection and libstdc++'s sequence decides. (2) Exact ties of length and score, where the heap's positional order differs from batch order. The accepted cases are in [Zhuyin selection uses length, raw score and input order](upstream-divergences.md#zhuyin-selection-uses-length-raw-score-and-input-order). | `nbest.rs` tests, including `selection_keeps_the_complete_key_at_each_limit` and `backtracking_uses_the_physical_slot_after_batch_selection`; `binary_heap_top` is a test-only comparison. |
| C | `oxpinyin-core/src/single_gram.rs` `lower_bound_records`, formerly a transcription of libstdc++ `__lower_bound`, behind `lower_bound` and `find`. It serves `get_freq`, `set_freq`, `insert_freq`, `remove_freq` and the search walk (`storage/ngram.cpp:161,185,215,239,259`; `include/stl_lite.h:29`). | `records.partition_point(\|r\| record_token(r) < token)` on the existing `[[u8; ITEM_LEN]]` view; unpartitioned data as above. | Wherever `item < probe` is partitioned over the stored items. That includes every gram the pin's writers produce, since `insert_freq` inserts at the lower bound (`ngram.cpp:185-200`). | Stored grams where an item below the probe follows one at or above it: foreign or hand-edited DBM values, or a user gram stored out of order and interleaved by `merge_single_gram` (`ngram.cpp:308-349`). The record names the toolchain. Accepted cases: [Stored gram partitions: phrase segmentation](upstream-divergences.md#stored-gram-partitions-phrase-segmentation) and [Stored gram partitions: training](upstream-divergences.md#stored-gram-partitions-training). | `single_gram.rs` tests, including `checked_search_asserts_short_and_faults_past_a_partial_tail`. |
| D | `oxpinyin-user/src/user_index.rs` `Entry::equal_range`, formerly a transcription of libstdc++ `equal_range`. Callers: `Entry::add_index` and `Entry::remove_index` (`storage/chewing_large_table2.h:208-255`, `phrase_exact_less_than2`). | `search_records::<N>`: two `partition_point`s over `as_chunks::<N>()` (design note 4); unpartitioned data as above. | Wherever the entry's items are ordered by `phrase_exact_less_than2`, as the pin's `add_index` keeps them (`:216-229`). | KC and tkrzw entry chunks whose order a mask walk broke by leaving a previous record's tail (module docs); the mask cases on the kc and tkrzw cells. | `user_index.rs` `entry_ops_keep_the_pins_order_and_report_presence` covers sorted entries. |
| E | A Python transcription of libstdc++ `__lower_bound` in the contract-case tooling chose the user gram for `user-gram-unsorted-phrase-segment`. | Removed with that tooling. The case's pin observations stay in [Stored gram partitions: phrase segmentation](upstream-divergences.md#stored-gram-partitions-phrase-segmentation). | The case's captured pin observations stay. | `user-gram-unsorted-phrase-segment` is POLICY-ACCEPTED. | — |

**Item A.** Candidate lists use stable `slice::sort_by` over cached enabled
keys and keep the dedup-only `m_freq`. Predicted lists sort their rows
directly. The eight-bit pinyin span and its defined unsigned wrap remain
contract. The maintainer decision retains the frequency comparison as
`rhs.wrapping_sub(lhs) as i32`. A linear guard on each equal length/span
group proves consistency before stable std sorting. Only cyclic groups use
the defined numeric-descending fallback. λ alone does not decide
consistency: wrapped/failed-token total updates can produce large scores at
stock λ. The proof is in
[Candidate sorting guards the wrapping frequency order](upstream-divergences.md#candidate-sorting-guards-the-wrapping-frequency-order).

**Item B: std selection and direct node slots.** `nbest.rs` has no heap
transcription. `Node::keep` writes physical slots directly. Beam and
final-tail selection call the same fixed-row
`select_top(Vec<BeamEntry>, count, nstore)` helper and comparator. The
comparator captures the mode, so there are no per-facade sort bodies.
S3's stable raw-score `tail_merge` orders the final tails. Zhuyin keeps the
pin's raw single-slot admission. Selection, then a sort of the retained
prefix, costs O(m + k log k) in place over the existing batch;
heapify-and-pop (rejected) costs O(m + k log m) and O(k) separate result
storage. Direct node transitions cost O(1) time and space.

**Design notes.** All cites are at the pin (`src/lookup/phonetic_lookup_heap.h`
blob `299ddedfa55e`, `src/lookup/phonetic_lookup.h` blob `c092e7610cb0`).

1. **Candidate sort key.** The key is explicit, with three fields
   compared in order: phrase length, pinyin span, frequency. Each is
   length/span descending, frequency by the guarded wrapping comparison,
   and a field whose sort bit is clear is zero
   (`pinyin.cpp:1684-1706`; zhuyin uses phrase length and frequency,
   `zhuyin.cpp:1134-1143`). Derived `RankKey::Ord` is not used: it orders
   ascending, and it includes `m_freq`, which the dedup reads
   (`pinyin.cpp:2092,2130`) but the sort ignores when `SORT_BY_FREQUENCY`
   is clear. The dedup keeps reading `m_freq`.
2. **Selection keys.** The keys are complete.
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

## Classification on main

This is the current classification. It follows two rulings in the
compatibility policy: [no ported external-library internals](compatibility-policy.md#no-ported-external-library-internals),
and no hand-written C or C++ shims, under which macros such as
`g_return_val_if_fail` and `G_STRFUNC` are not reproduced
([external I/O](compatibility-policy.md#external-io)). It covers every
crate under `crates/`: GLib, glibc and libstdc++ names, and sorts,
searches, joins, scanners and diagnostics shaped like them.

Buckets:

- **Boundary**: a real GLib or libc call the pin makes, through
  `glib-sys`, `libc` or an `extern "C"` declaration, or a C ABI shape,
  ownership rule, on-disk width or ABI-prescribed encoding.
- **Behaviour**: product behaviour, kept in plain Rust and named for it.
- **Imitation**: Rust that reproduces a GLib, libc or libstdc++
  implementation detail, or a macro that the shim ruling names.

| # | oxpinyin item (`crates/`) | Pin cite (`074a2219`) | Bucket | Reason |
| --- | --- | --- | --- | --- |
| 1 | `oxpinyin-capi/src/dict.rs`, `oxpinyin-zhuyin-capi/src/dict.rs`: `glib_sys::g_array_set_size` / `g_array_append_vals` on the caller's array | `pinyin.cpp:2805,2817`; `zhuyin.cpp:1797,1809` | Boundary | The caller owns a real `GArray`. |
| 2 | `ffi.rs` (both facades) `strdup`, `key_text`, `strconcat_aux`, `strjoinv`, `symbol_vector`, `sentence_text` (the pin's GLib calls), `malloc_cstr_with`, `take_owned_cstr`; the hand-declared `malloc` / `free` | `g_free` / `g_strfreev` ownership of returned strings (`pinyin.cpp:715-716,733`) | Boundary | Ownership and lifetime of returned C strings; `extern "C"` declarations are permitted. |
| 3 | `ffi.rs` (both facades) `log_warning` (`g_log` at warning level in the facade's domain) and the lines it carries for class (b) refusals and notices with no pin counterpart | none: the pin's `assert` / `abort()` sites, for example `pinyin.cpp:2507` (`pinyin_choose_candidate`) and `:3738` (`pinyin_remove_user_candidate`), abort in the C adapters and write no such line | Behaviour | The warning text is oxpinyin's own and is not compared with the pin. |
| 4 | `phrase.rs` (both facades), NULL `sentence`: GLib's `g_utf8_strlen` critical | `pinyin.cpp:1450`; `zhuyin.cpp:969`; GLib 2.84.4 `gutf8.c:240` | Imitation → Boundary | The pin's call, `glib_sys::g_utf8_strlen(sentence, -1)`, so GLib logs its own critical (domain `GLib`, `p != NULL \|\| max == 0`) and applies the caller's fatal mask. A hand-spelled copy of GLib's internal `g_return_val_if_fail` is rejected. |
| 5 | `phrase.rs` (both facades), invalid UTF-8: the pin's `g_return_if_fail_warning(NULL, "bool pinyin_phrase_segment(pinyin_instance_t*, const char*)", "num_of_chars == ucs4_len")` and the zhuyin twin | `pinyin.cpp:1454`; `zhuyin.cpp:973` | Imitation removed | Decision D1: the call answers `false` and keeps the earlier result, with no log call. [Phrase segmentation writes no invalid-UTF-8 critical](upstream-divergences.md#phrase-segmentation-writes-no-invalid-utf-8-critical). |
| 6 | `SubPhraseIndex::load`'s four `g_return_val_if_fail` checks | `phrase_index.cpp:354-356,361` | Imitation removed | Decision D1. `LayoutFault` keeps only `total_freq`, which the load, reload and save use for the pin's empty sub-index and totals. [Phrase-library layout faults write no critical](upstream-divergences.md#phrase-library-layout-faults-write-no-critical). |
| 7 | `oxpinyin-input` `config.rs`, `native.rs` (`setlocale`, `fopen`, `fscanf`, `feof`, `atoi`, `fprintf`, `fclose` through `libc`); the facades' locale calls | `table_info.cpp:197,208-291,328,338-359,372-394` | Boundary | External-I/O stage 1; `LC_NUMERIC` row 39 stands. |
| 8 | `oxpinyin-capi/src/types.rs` `GArray`, `GUint`, `GInt`, `GChar`; `oxpinyin-core` packed `ChewingKey`; the casts at exported parameters (`cursor.rs`, `iterators.rs`, `user_data.rs`) | `pinyin.h`, `zhuyin.h`; `chewing_key.h:111-113` | Boundary | The exported signatures prescribe the widths. |
| 9 | `oxpinyin-capi-marshal` `input.rs` `utf8_to_ucs4`, `scalar_text`, `parser_prefix`, `direct_zhuyin_reading`, `filename`; `ucs4.rs` `decode`, `prefix_text`; `phrase_text.rs` `phrase_text_bytes`, `encode_words`, `encode_word`, `stored_words_match` | `g_utf8_to_ucs4` / `g_ucs4_to_utf8` at the C string entries (`pinyin.cpp:568-571,1450-1454`) | Boundary | ABI-prescribed encodings. |
| 10 | `oxpinyin-core` `PhraseText`; `oxpinyin-data` `phrase_library` `phrase_text`, `phrase_codes`, `phrase_chars`; `row_format` `punct_text` | `lookup.cpp:39-60`; `pinyin.cpp:1627-1645` | Behaviour | Stored code points; the outward cut is row 9. |
| 11 | `oxpinyin-engine` `nbest.rs` `select_top`, `tail_merge`, `Node::keep` | `phonetic_lookup_heap.h:56-81,108-122`; `phonetic_lookup.h:141-172,331-337,770-771` | Behaviour | The selection rule and slot semantics, as std selection and sort (item B; S3). `binary_heap_top` is a test-only comparison. |
| 12 | `oxpinyin-engine` `candidate.rs` guarded frequency order; `session/mod.rs` `RankKey`, `compare_descending`, `for_sort_word`, `rank_candidates`; `oxpinyin-capi` `predict.rs` row sort | `pinyin.cpp:1678-1709,2286,2440-2445`; `zhuyin.cpp:1129-1144` | Behaviour | Stable std sort over the enabled keys; the `rhs.wrapping_sub(lhs) as i32` frequency order and the eight-bit span are retained (item A). |
| 13 | `session/mod.rs` `amplified_frequency`, `amplified_frequency_with_bigram` | `pinyin.cpp:1811-1824,1862-1866`; `zhuyin.cpp:1264-1268` | Behaviour | The ruled scaled `guint32` ranking key. |
| 14 | `session/mod.rs` `dedup_ranked_by_text`, `dedup_by_text_keep_first` | `pinyin.cpp:2128-2138`; `zhuyin.cpp:1406-1416` | Behaviour | One row per text. |
| 15 | `oxpinyin-capi` `predict.rs` `prepend_punctuations` | `pinyin.cpp:2468-2494` | Behaviour | Distinct punctuation strings before the predictions. |
| 16 | `oxpinyin-core` `single_gram.rs` `lower_bound`, `find` | `ngram.cpp:161,185,215,239,259` | Behaviour | `partition_point` (item C). |
| 17 | `oxpinyin-user` `user_index.rs` `Entry::equal_range` | `chewing_large_table2.h:208-255` | Behaviour | Two `partition_point`s (item D). |
| 18 | `oxpinyin-input` `path.rs` `build_filename`, `build_path`, `build_bytes` | `g_build_filename` at `pinyin.cpp:150-165,250-259,331-338,395-400,952-1121`; `zhuyin.cpp:104-119,196-203,276-282,337-342,585-687` | Boundary | The pin's call through `glib-sys` ([external-I/O stage 3](external-io-boundary.md#proposed-crate-and-staged-implementation)); decision D2. |
| 19 | `oxpinyin-data` `c_stderr.rs` `path_bytes`, `table_conf_load_failed`, `mmap_failed`, `rename_failed` through `oxpinyin_input::diagnostic` (`libc::fprintf` on the C library's `stderr`) | `table_info.cpp:201,332`; `pinyin.cpp:256,290,338,956,1059-1121,1265`; `zhuyin.cpp:200,282,589,609-687,800` | Boundary | External-I/O stage 4. |
| 20 | `oxpinyin-user` `persistence.rs` `save_reporting`, the rename order, `tolerate`, `write_marker`, `clean_user_files`, `.tmp` staging | `pinyin.cpp:149-170,922-1130`; `zhuyin.cpp:547-700`; `memory_chunk.h:536-549` | Boundary | Files on disk, their order and the save's answers (G10). The rename and unlink calls are the pin's, through `oxpinyin-input` `path.rs` (external-I/O stage 3). |
| 21 | `oxpinyin-input` `chunk.rs` `save`, called by `persistence.rs` `write_chunk_file` | `memory_chunk.h:536-557` | Boundary | The pin's `open`, two header `write`s, one payload `write`, `fsync` and `close` (external-I/O stages 2 and 3). |
| 22 | `oxpinyin-input` `chunk.rs` `load`; `oxpinyin-data` `phrase_library.rs` `map_chunk`, `directory_end` | `memory_chunk.h:420-427,470-520` | Boundary | The chunk format and the pin's mapping call (external-I/O stage 2, row 109). |
| 23 | `oxpinyin-data` `row_format.rs` `decode_puncts` | `punct_table.cpp:56-94` | Behaviour | Whole-word reading, the language-artefact record [Punctuation values contain whole words](upstream-divergences.md#punctuation-values-contain-whole-words). |
| 24 | `oxpinyin-data` `table_info.rs` `TableConf::parse`; `user_files.rs` `UserTableInfo::parse`, `to_text` | `table_info.cpp:122-131,194-294,338-394` | Boundary | The `table.conf` / `user.conf` reads and write are the pin's `fscanf` / `fprintf` calls through `oxpinyin-input`; the domain checks stay here. |
| 25 | `oxpinyin-capi` `aux_matrix.rs` `full`, `double`, `chewing`, `split_zhuyin` | `pinyin.cpp:3295-3576` | Behaviour | The byte delta used as a character offset is libpinyin's defect, reproduced (limb 4). |
| 26 | The tools' numeric fields, `printf("%f")` output and Unicode categories: `oxpinyin-input` `tool.rs` `atoi`, `atol`, `unichar_isgraph`, `fprintf` | `tag_utility.cpp:127-264`; `utils_helper.h:23-69`; `printf("%f")` at `estimate_interpolation.cpp:116-139`, `eval_correction_rate.cpp:201-203` | Boundary | The pin's calls through `libc` and `glib-sys` (external-I/O stage 5). |
| 27 | `oxpinyin-utils/src/utils_helper/taglib.rs`; `oxpinyin-input` `tool.rs`, `native.rs` | `tag_utility.cpp:70-299`; the tools' GLib and libc calls | Boundary | libpinyin's own taglib in Rust; its GLib and libc calls are real (`glib-sys`, `libc`). `Required` is a `HashMap`; no hash order reaches output. |
| 29 | Wrapping in-memory totals: `oxpinyin-runtime` unigram total; `oxpinyin-data` `phrase_libraries.rs`, `lm/mod.rs`; `oxpinyin-user` `store.rs`, `registry.rs`, `seed.rs`; the `guint8` pronunciation count in `store_libpinyin.rs` | `phrase_index.h:615,633`; `phrase_index.cpp:28` | Boundary | On-disk `guint32` / `guint8` widths; defined unsigned wrap. |
| 30 | `oxpinyin-engine` `nbest.rs` node index (`HashMap` beside the node `Vec`) | `phonetic_lookup.h:314-315` | Behaviour | Creation order lives in the `Vec`; the map is a lookup only. |

Out of scope: `oxpinyin-store`'s DBM calls and test-only code.
The table has no row 28; the other rows keep their numbers.

Result: Boundary rows 1, 2, 7–9, 18–22, 24, 26, 27 and 29. Behaviour rows
3, 10–17, 23, 25 and 30, each named for its behaviour. Imitation replaced by
the pin's call: row 4. Imitation removed: rows 5 and 6 (D1). No imitation
remains.

### Maintainer decisions

**D1 — the pin's own `g_return_val_if_fail` criticals (rows 5 and 6).**
A narrow exception to the library stderr ruling. For both facades'
`phrase_segment` and for `SubPhraseIndex::load`, oxpinyin keeps the pin's
return values, state changes and continuation exactly. It emits no
critical and substitutes no other log call. Two observable differences are
POLICY-ACCEPTED:

- the missing critical (to the caller's log handler, and to stderr under
  GLib's default handler);
- the missing abort under GLib fatal-critical settings
  (`G_DEBUG=fatal-criticals`, `g_log_set_always_fatal`), where the pin
  aborts and oxpinyin continues. This is an intentional observable
  difference, not exact compatibility.

This is not a general exception for copying GLib macros. It changes no
other native GLib call and no class (c) ruling.

**D2 — `g_build_filename` (row 18).** It is the pin's call, through
external-I/O stage 3. It keeps the pin's path behaviour, including a file
name that begins with a separator, which stays beneath its directory
(policy row 78). `std::path::Path::join` is rejected, because there an
absolute name replaces the directory.

## C-maths record

oxpinyin's floating-point sites call `std` directly: `core/src/cost.rs`
(`f64::log2`), `engine/src/nbest.rs` and `engine/src/phrase.rs` (`ln`),
and `utils/src/training/eval/decode.rs` (`ln`). The scaled candidate
frequency in `session/mod.rs` `amplified_frequency_with_bigram` is `f32`
arithmetic stored with `as u32`. Its saturation on out-of-range scores is
class (b). None of these is a C-maths imitation. A last-bit difference from
the pin's `gfloat` `log` is class (a), row 11.

## Negative coverage

Excluded from the exported surface:

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
| `zhuyin_parse_chewing` truncated UTF-8 | `g_utf8_next_char`, `g_utf8_strncpy` | `g_utf8_strncpy` uses `strncpy`, which stops at the NUL and pads; no read past the string. |
| tests | `glib_sys` use in test fixtures | test-only, not shipped. |

## Conclusions

- The valid-input side of every family is C-facing contract; the malformed
  side is the named class, record or host dependence.
- `LC_NUMERIC` is retained (row 39).
- The math record is row 11: `log` is class (a).
