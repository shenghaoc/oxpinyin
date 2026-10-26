# GLib and libc at the pin: reachable calls and oxpinyin counterparts

Status: **current audit** (#917) · 2026-10-24 UTC · Parent: #915.
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
| G1 | UTF-8/UCS-4 conversion and category: `pinyin.cpp:628,715,1397,1450-1454,2658,3210,3678`; `lookup.cpp:54,58`; `zhuyin.cpp:918,969-973,1684,2165`; `zhuyin_parser2.cpp:871,879`; `punct_table.cpp:48` (dead); `tag_utility.cpp` (tool) | both (punct pinyin only); all | `capi/src/ffi.rs` `cstr_to_string` / `cstr_to_parsed_prefix` / `cstr_to_strict`; `capi/src/phrase.rs` `segment_text`; `data/src/phrase_library.rs` `phrase_chars`; `data/src/interp_grammar.rs` `is_graph`; `data/src/row_format.rs` `decode_puncts` | valid: identical strings. malformed/non-scalar: `g_utf8_to_ucs4` accepts surrogates and values above U+10FFFF; Rust `str` cannot. Embedded U+0000 stops the pin's conversion. | valid: contract. malformed: quirk (#894, #918); tool: quirk (#926); punct: quirk/class (b) (#1144) | #894, #918, #925, #926, #1144 |
| G2 | C strings, joins and vectors: `g_strdup` 39, `g_free` 155, `g_strdup_printf` 14, `g_strconcat` 16, `g_strjoinv` 3, `g_strndup` 7, `g_strfreev` 10, `g_strv_length`/`g_strv_contains` 2, `g_string_*` 4 | both (punct subset pinyin only); all | `capi/src/ffi.rs` `owned_cstr` / `owned_cstr_list`; `capi/src/aux_matrix.rs` `join`; `capi/src/predict.rs` `prepend_punctuations`; `capi/src/iterators.rs` export rendering; `data/src/pin_stderr.rs` `build_filename` | valid: identical bytes. malformed: raw path bytes, missing middle aux component, NUL in a diagnostic, duplicate punctuation. | valid: contract. malformed: quirk (#922) or #925/#928; punctuation strings #1144 | #922, #924, #925, #928, #1144; export row 1 |
| G3 | Path joining: `g_build_filename`, 68 sites, e.g. `pinyin.cpp:164,252,268,286,308,395,400,952,968,985,1052`; `zhuyin.cpp:118,196,212,230,252,337,342,585,601,605,626,630,684` | both; all | `data/src/table_info.rs` `FileName::under` / `pin_join`; `data/src/pin_stderr.rs` `build_filename`; `capi/src/ffi.rs` `cstr_to_path` | valid: same resolved path. malformed: leading `/`, empty name, `..`, duplicate separators, non-UTF-8 bytes. | valid: contract. malformed: quirk (#922) | #922 |
| G4 | GArray/GPtrArray containers: `g_array_*` and `g_ptr_array_*`, 514 sites (`g_array_index` 101, `g_array_new` 64, `g_array_free` 68, `g_ptr_array_index` 58, `g_array_set_size` 57, `g_array_append_val` 54, ...) | both; all | `capi/src/dict.rs` and zhuyin twin keep real `GArray` for the ABI; internal steps are `Vec`/slices in `engine/src/nbest.rs`, `engine/src/session/*`, `data/src/phrase_library.rs` | valid: identical candidate/result arrays. malformed: out-of-range indices, a key with no whole word, a short chunk header. | valid: contract. malformed: class (b)/(c) rows; #924 for the ABI arrays | #924 |
| G5 | n-best tail sort: `phonetic_lookup.h:337` `g_ptr_array_sort(tails, trellis_value_compare)` | both; all | `engine/src/nbest.rs` `glib_merge_sort`, `tail_compare`, `c_float_to_gint` | valid: the `gint` truncation ties scores under one unit apart, so the comparator is not transitive on finite scores. malformed: NaN and ±∞ from λ outside `[0, 1]`. | contract; merge kept, [record](921-927-glib-sorts.md) | #921 |
| G6 | candidate and predicted list sorts: `pinyin.cpp:2286,2443` `g_array_sort_with_data(compare_item_with_sort_option)`; `zhuyin.cpp:1529,1610` `g_array_sort`; phrase-string dedup `pinyin.cpp:2067`, `zhuyin.cpp:1375` | both; all | `engine/src/nbest.rs` `glib_merge_sort` under `engine/src/session/mod.rs` `glib_rank_sort`, `RankKey::pin_compare` and `capi/src/predict.rs` `sort_predicted` | valid: a total order (frequencies below 2²⁴); stable order and dedup winner. malformed: λ outside `[0, 1]` wraps the `gint` frequency key into cycles. | contract; merge kept, [record](921-927-glib-sorts.md); dedup #913/#914 | #927, #928 |
| G7 | hash tables: `g_hash_table_new` 2, `insert` 4, `lookup_extended` 4, `destroy` 2; `g_direct_hash`/`equal` | both; all | `engine/src/nbest.rs` `Step`, `engine/src/session/*` vectors; no exposed iteration order | valid: lookup only; no order reaches the ABI. malformed: none. | contract (internal; no observable order) | — |
| G8 | diagnostics: `g_return_val_if_fail`, 6 sites (`pinyin.cpp:1454`, `zhuyin.cpp:973`, `phrase_index.cpp:354-356,361`); `g_warning` only in the tool-only tag utility | both; all | `capi/src/ffi.rs` `log_warning` / `log_critical`; `capi/src/phrase.rs` `g_log` / `g_return_if_fail_warning`; zhuyin twin | valid: class (c) sites keep one warning at warning level and the pin's domain. malformed: a GLib conversion failure logs the critical. | (c) where the pin aborts; #924 for the channel; #918 for the segment gate | #918, #924 |
| G9 | allocation/free and real GArray ABI: `g_free` 155, `g_strdup` 39, `g_strdup_printf` 14; C `malloc`/`free`/`realloc` 6; `g_array_set_size`/`g_array_append_vals` into caller arrays in `pinyin.cpp:2805,2817` and `zhuyin.cpp:1797,1809` | both; all | `capi/src/ffi.rs` `owned_cstr` / `owned_cstr_list` (C `malloc`/`g_free` contract); `capi/src/dict.rs` and zhuyin twin on `glib_sys`; `libpinyin.alloc` / `libzhuyin.alloc` | valid: caller may `g_free` / `g_strfreev`; the real `GArray` keeps GLib's private metadata. malformed: NULL or OOM paths. | contract; MUST RETAIN | #924 |
| G10 | rename and sync: `g_rename` 10 (`pinyin.cpp:1059,1078,1094,1108,1121`; `zhuyin.cpp:609,635,655,672,687`); `g_fsync` 1 (`memory_chunk.h:555`) | both; all | `user/src/persistence.rs` `save_with_bigram_reporting`, `pin_rename_order`; `data/src/phrase_library.rs` chunk save | valid: same file order and bytes. failure: per-file write/rename failures and the save's return. | contract on valid profiles; #929 for failure ordering | #929 |
| G11 | numeric parsing and locale: `fscanf` 10 (`table_info.cpp:208,214,220,228,232,255,338,344,352,357`); `atoi` 1 (`:160`); `setlocale` 6 (`:197,291,328,372,378,394`) | both; all | `data/src/table_info.rs` `Scan`, `strtol_digits`, `atoi_guint8`, `hex_float`; `data/src/user_files.rs` `Scan::int`; `capi/src/locale.rs` | valid: same table values, versions, λ bits and library mapping. malformed/host: glibc 2.43-3 `%d`/`%f`/`%255s` grammar, saturation, signed user.conf versions, hex/NaN/Inf, incomplete exponents, NUL/width and row grouping; class (b): missing source buffer and merged-row SIGSEGV. | valid: contract. malformed: contract except the class (b) refusals; host scanner (#919/#920); process side effect ruling (#923) | #919, #920, #923 |
| G12 | file I/O and `MemoryChunk`: `mmap` 10, `munmap` 2, `lseek` 4, `read` 5, `write` 3, `open` 3, `close` 12, `unlink` 22, `fopen` 3, `fclose` 8, `fprintf` 36 | both; all | `data/src/phrase_library.rs` `map_chunk`, `MappedFile`, chunk header; `user/src/persistence.rs`; `data/src/pin_stderr.rs` raw bytes | valid: same mapped extent, header check and file bytes. malformed: short/truncated/directory chunk; write failures. | contract on valid files. malformed: class (b) rows 76/96/109; #929 for writes | rows 76, 96, 109; #929 |
| G13 | C primitives: `memset` 124, `memcpy` 28, `strlen` 18, `strcmp` 18, `memcmp` 10, `memmove` 7, `strchr` 2 | both; all | Rust slices/`str`; `data/src/row_format.rs`, `core/src/single_gram.rs`, `capi/src/ffi.rs` | valid: same bytes. malformed: NULL/unreadable phrase strings and out-of-range walks are class (b) rows. | contract; class (b) at the registered NULL/over-read rows | rows 80, 106 |
| G14 | libm: `log` 7 (`phonetic_lookup.h:39,250,663,693`; `phrase_lookup.cpp:40,321,342`) | both; all | `core/src/cost.rs` `cost_of_probability` / `surprisal` (fixed-point `log2_fixed`); `engine/src/nbest.rs` `poss` | valid: class (a) last-bit differences are accepted. | class (a), row 11; record only | no child (math record) |
| G15 | punctuation decoding: `punct_table.cpp:56-72` `unescape`, `:74-94` `get_all_punctuations`, reached at `pinyin.cpp:2474` | pinyin; all | `data/src/row_format.rs` `decode_puncts`; `data/src/punct.rs` `PunctTable::punctuations` (`Err(DictError::Parse)` for malformed non-empty; `Ok(Vec::new())` for unopened/absent/empty); `runtime/src/lib.rs:721` (`RuntimeDict::punctuations` uses `unwrap_or_default`); `capi/src/predict.rs:308` | valid: identical punctuation strings. decoder errors and lookup states are partitioned below; they are not one case. | valid: contract. malformed: class (b) over-read, non-scalar quirk, and empty results separately; no ruling | **#1144** |
| G16 | tool-only GLib: `tag_utility.cpp` (`g_strsplit_set`, `g_unichar_*`, `g_strv_length`, `g_utf8_get_char/next_char`, `g_ptr_array_foreach`, `g_hash_table_foreach_steal`, `g_warning`); `table_text_parser.h` (`g_ascii_strtoull`, `g_strlcpy`); the `load_text` methods | not C ABI | `data/src/interp.rs`, `data/src/interp_grammar.rs`; `training/src/counter/parse.rs`; `dictool/src/format.rs` | valid: tool input. malformed: separator, escape, category and `atoi` quirks. | quirk, tool scope | #926 |
| G17 | dead code: `pinyin_lookup2.cpp`, `chewing_large_table.cpp`, `phrase_large_table2.cpp`, `facade_chewing_table.h`, `facade_phrase_table2.h`, `PunctTableEntry::escape/append/remove`, `#if 0` dumps | none | no shipped counterpart; the runtime uses `PhoneticLookup`, `PhraseLookup` and the large-table-3/chunk path | n/a | negative coverage | — |

## Family notes

### G1 — UTF-8 and UCS-4

The pin converts with GLib's `g_utf8_to_ucs4` and `g_ucs4_to_utf8` in
three candidate/sentence/export paths (`pinyin.cpp:1627-1645` and
`lookup.cpp:27-60`; the export is `pinyin.cpp:698-716`). GLib 2.89.4
encodes surrogates and values above U+10FFFF and stops at an embedded
U+0000. Rust `char`/`str` cannot represent the first two, and
`PhraseItemView::phrase_chars` treats them as unreadable; `decode_puncts`
rejects them. The valid-input path is byte-identical; the malformed
cases are #894 (phrase text) and #1144 (punctuation). `phrase_segment` is
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

`g_build_filename` is the only path constructor. `pin_join` keeps the
name beneath the directory even when it starts with `/`; `build_filename`
keeps raw bytes for stderr; `cstr_to_path` keeps the caller's bytes.
`g_rename` and `g_fsync` are G10.

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

All of these stay GLib's merge (#921, #927): one implementation,
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
save results remain unchanged. Linux uses `fsync` through std.

No new test was added: this function owns a concrete `File`; the existing
injected `Write` seam only observes header writes and cannot observe sync.
The existing `save-929-sync-rejecting-{pinyin,zhuyin}` contract cases check
continued saving when `/dev/null` rejects sync, explicitly not whether a
sync was issued. Source inspection establishes the added call and its
position. Crash/power-loss behavior and macOS runtime execution were not
tested. Existing save/reload cases cover output and persistence behavior.

### G11 — numeric parsing and locale

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

`decode_puncts` is the decoder for a **stored, non-empty** value. A
malformed value returns `Err(DictError::Parse)`; `PunctTable::punctuations`
propagates it, and `RuntimeDict::punctuations` turns it into an empty
vector through `unwrap_or_default`. An unopened table, an absent DBM
entry and a zero-length stored value bypass `decode_puncts` and return
`Ok(Vec::new())` directly.

| Partition | oxpinyin result | `RuntimeDict::punctuations` | C API |
| --- | --- | --- | --- |
| Stored value length is not a multiple of 4 | `decode_puncts` -> `Err(DictError::Parse)` | empty vector | no punctuation prepended; `pinyin_guess_predicted_candidates_with_punctuations` returns its normal `true`, not an empty candidate list |
| Empty punctuation field (zero code point while the current field is empty) | `decode_puncts` -> `Err(DictError::Parse)` | empty vector | as above |
| Code point is not a Unicode scalar value | `decode_puncts` -> `Err(DictError::Parse)` | empty vector | as above |
| Unterminated stored value | `decode_puncts` -> `Err(DictError::Parse)` | empty vector | as above |
| Absent DBM entry | `PunctTable::punctuations` -> `Ok(Vec::new())` | empty vector | no punctuation prepended; normal `true` |
| Zero-length stored value | `PunctTable::punctuations` -> `Ok(Vec::new())` | empty vector | no punctuation prepended; normal `true` |
| Unopened table | `PunctTable::punctuations` -> `Ok(Vec::new())` | empty vector | no punctuation prepended; normal `true` |

The three findings stay separate:

- **Class (b), over-read.** The unterminated value is the row where the
  pin's `PunctTableEntry::unescape` reads `punct[index]` after advancing
  `index`, one `ucs4_t` past the value. That is #1144's class (b) half.
- **Language/runtime quirk, non-scalar conversion.** A surrogate or a
  value above U+10FFFF is encoded by the pin's
  `g_string_append_unichar` into invalid UTF-8; Rust's `char` cannot
  hold it, so `decode_puncts` refuses the value.
- **Empty results.** An absent entry, a zero-length value and an unopened
  table are lookup states, not decoder errors. They are not evidence
  that a malformed value was decoded.

The pin's result for each malformed partition through `load_entry` and
`unescape`, including the bdb/kc/tkrzw differences, is deliberately left
to #1144. This inventory records the Rust result and the C-visible
outcome only.

`BEHAVIOR-COVERED` applies only to the inputs the differential
exercised (valid punctuation values). It does not establish any
malformed partition above.

### G16 and G17 — tools and dead code

`tag_utility.cpp` is included by `pinyin_internal.h` but no exported
entry calls `taglib_*`; it is used by `utils/training` and
`utils/storage`. `table_text_parser.h` and the `load_text` methods are
used only by `utils/storage/gen_binary_files.cpp`.
`PinyinLookup2` is never instantiated; `PhraseLookup` uses
`FacadePhraseTable3` and `FacadePhraseIndex`, not the old
`FacadePhraseTable2`/`ChewingLargeTable`. The `#if 0` dumps and the
`PunctTableEntry` writer methods have no exported caller.

## oxpinyin GLib emulations in `crates/`

The table covers semantic counterparts, not only names containing
`glib`. Each row is an internal re-expression of a C/GLib behaviour; the
child column is the audit that must decide it.

| Counterpart | Emulates | Pin cite | Child |
| --- | --- | --- | --- |
| `capi/src/ffi.rs` `cstr_to_string`, `cstr_to_parsed_prefix`, `cstr_to_strict`, `cstr_to_path` | `g_utf8_to_ucs4` / `g_utf8_strlen` gates; raw C path bytes | `pinyin.cpp:1450-1454`; `zhuyin.cpp:969-973`; `pinyin.cpp:1498-1509` | #918 |
| `capi/src/phrase.rs` `segment_text` | `g_utf8_strlen` + `g_utf8_to_ucs4` gate and `g_return_val_if_fail` | `pinyin.cpp:1450-1454`; `zhuyin.cpp:969-973` | #918 |
| `data/src/phrase_library.rs` `phrase_chars` | `g_ucs4_to_utf8` on stored phrase text | `pinyin.cpp:1627-1645`; `lookup.cpp:27-60`; `pinyin.cpp:698-716` | #894 |
| `data/src/row_format.rs` `decode_puncts` | `PunctTableEntry::unescape` / `get_all_punctuations` | `punct_table.cpp:56-94` | #1144 |
| `capi/src/aux_matrix.rs` `AuxMatrix::chewing`, `join` | `g_utf8_substring`, `g_strconcat` with a NULL middle | `pinyin.cpp:3518-3576` | #925 |
| `capi/src/predict.rs` `prepend_punctuations`, `seen` | `g_strv_length` + `g_strv_contains` + reverse prepend | `pinyin.cpp:2456-2498` | #928 |
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
| `data/src/interp_grammar.rs` `is_graph`, `interp.rs` | `taglib_read` + `g_unichar_isgraph` | `tag_utility.cpp:171-264` | #926 |
| `dictool/src/format.rs` `split_classic`; `training/src/counter/parse.rs` `atoi`, `splitn` | `g_strsplit_set` / C `atoi` in tool input | `tag_utility.cpp`, `table_text_parser.h`; utils | #926 |
| `capi/src/dict.rs`, `zhuyin-capi/src/dict.rs` direct `g_array_*` | real GLib `GArray` mutation | `pinyin.cpp:2805,2817`; `zhuyin.cpp:1797,1809` | #924 |
| `capi/src/ffi.rs` `owned_cstr`, `owned_cstr_list`, `log_warning`, `log_critical`; `phrase.rs` `g_log` / `g_free` | `g_strdup` / `g_strfreev` / `g_log` / `g_return_val_if_fail` | `pinyin.cpp` diagnostics; `phrase_index.cpp:354-356` | #924 |

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
| `engine/src/session/mod.rs:423-456` `amplified_frequency`, `guint32_of_score` | C `float` multiply/truncate and the low 32 bits | `f32` arithmetic plus `as i64 as u32` (already Rust) | record only |
| `capi/src/predict.rs:482` `guint32_of_score` | the same `cvttss2si` / low-32-bit store | same | record only |

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
