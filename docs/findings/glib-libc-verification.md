# GLib/libc verification coverage

Status: **partial coverage, removal gate** ·
[Parent #915](https://github.com/shenghaoc/oxpinyin/issues/915).

This ledger records, for each family G1–G17 of the
[inventory](glib-libc-inventory.md) ([#1146](https://github.com/shenghaoc/oxpinyin/pull/1146)),
the verdict and how far the investigation goes. The inventory's
[call-site list](glib-libc-call-sites.tsv) gives the complete membership of
each family: 1,179 sites that an exported entry reaches, 190 that only
`utils/` programs reach, and 188 that no link keeps. Inventory membership does not prove coverage of every input.

## Evidence and decision rules

The #894 phrase-text records (non-scalar and NUL stored text) are
implemented in `PhraseText` and its consumers; their cases are ordinary
MATCH cases.

Citation conventions:

- All pin citations below are `file:line` at libpinyin
  `074a2219c90feaf962d0d24f034514033ece5f99`. Relative pin paths start at
  `src/` unless prefixed `utils/`. The pin source was read at `074a2219`,
  beside the cited existing findings.
- The inventory read GLib **2.84.4-3~deb13u5** and glibc
  **2.41-12+deb13u4** (recorded evidence), not the host libraries.
  The reference image is unpinned Debian testing.
- Rust paths below start at `crates/oxpinyin-`. They cite items, not line
  numbers, which move.
- “Both” means pinyin and zhuyin. Backend-independent source arguments
  cover bdb/kc/tkrzw only to the stated extent, not as three executed
  verification cells.

**Verdict and investigation are separate columns.** The terms mean:

- “Identical”: a source-derived equivalence for the named scope. It is
  not a test result.
- “Complete”: applies only to that bounded source investigation.
- “Partial”: a cited result exists, but the full partition/caller audit
  is still open.
- “Unverified; pending classification”: unread or unsettled work, not a
  policy question.
- “Needs a ruling to reverse”: reserved here for #923. The 2026-10-25 UTC ruling retains the pin's behaviour.

A child issue or an inventory label alone does not admit a quirk
exception.

Follow the [policy decision order](compatibility-policy.md#decision-order):
C contract first, classes (a)–(b) next, then individually recorded runtime
quirks; otherwise a defect to revert or an opt-in Stage 3 departure.
Differences link their records; identical paths need no discrepancy form.
New quirks use the [discrepancy template](language-runtime-discrepancy-template.md)
and register in [upstream-divergences](upstream-divergences.md).
`POLICY-ACCEPTED` never means `MATCH`.

Before removing/replacing an emulation, complete every affected partition,
including every caller of a shared helper, and pass the applicable
compatibility gates on the proposed code. Reuse a prior entry only after
checking that intervening changes have not invalidated its reasoning.
Keep real GLib ABI objects/allocations, `GArray`/`g_free` and #924.

Derive behavior from the versioned sources first. If macros, templates,
inlining, conversions, undefined behavior or evaluation order leave a
question open, read the compiled pin with `objdump -dlS -C`,
`gdb`'s `disassemble /s` and `readelf`, on both amd64 and arm64 and in the
backend build containing backend-specific code. Record symbol and
instruction offsets; amd64 remains the compatibility reference. Propose
a decompiler only for a specific unresolved case. Binary evidence below is
cited from the records that took it.

## G1 — UTF-8/UCS-4 conversion and category

Pin: `pinyin.cpp:614-652,698-768,1450-1454,1627-1645,1953-2138,3669-3710`;
`zhuyin.cpp:500-537,969-973,1096-1113,1294-1416`;
`lookup/lookup.cpp:27-69`; `storage/phrase_index.cpp:103-111,522-555`;
`storage/phrase_index.h` item storage; `storage/phrase_index.cpp` loaders.
Rust: `core/src/ucs4.rs`, `PhraseText`;
`data/src/phrase_library.rs`, `PhraseItemView::phrase_codes`/`phrase_text`;
`data/src/phrase_libraries.rs`, `phrase_char_count`/`trellis_item`;
`engine/src/nbest.rs`, `token_text`/`text`; both `capi/src/sentence.rs`
and `zhuyin-capi/src/sentence.rs`; `facade/src/export_rows.rs`,
`PhraseExportCursor`/`ExportRow`; `capi/src/iterators.rs`.

| Input partition | Verdict / record | Investigation and exercised scope |
| --- | --- | --- |
| Complete bounded phrase items, matching readings, nonzero scalar text; valid UTF-8 producers | Identical on the #894 text paths: GLib encoding and Rust scalar encoding agree. | Complete source trace in the #894 non-scalar and NUL records, including system/addon/user chunks, journals, import, remember, generation, training/save, candidate/sentence/export consumers. Six scalar controls exercised; not every producer tested. |
| Stored surrogates / above-Unicode values below `0x80000000`, with otherwise sound item/readings | Identical, ordinary MATCH (#894). GLib emits extended UTF-8; the Rust path carries the words and emits the same bytes. System data; malformed user-library items are tracked in #1198. | Complete source classification, now implemented by `PhraseText`. Exercised U+D800 and U+110000: ten candidate/sentence/pinyin-export cases MATCH. Direct token getters, addon/prediction alternatives and journals are source-traced, not separate exercised witnesses. |
| Embedded U+0000 in a declared phrase body | Identical, ordinary MATCH (#894). GLib stops per item; the Rust path ends the outward text at the first zero word and keeps the stored length. System data; malformed user-library items are tracked in #1198. | Complete source classification, now implemented. Five candidate/sentence/pinyin-export cases MATCH. Dedup, prefix slices and NULL-output variants have source reasoning, not separate exercised controls. |
| Values >= `0x80000000`; truncated, empty, overlong or inconsistent items; invalid readings; allocation failure | Values >= `0x80000000`: `phrase_segment` with a matching raw key is identical (record); the text getters still differ (pin true/declared length/NULL, Rust false/untouched), a distinct malformed-item difference not admitted by #894. Truncated/empty/overlong/inconsistent items: out-of-bounds reads are class (b) (rows 101/107); in-bounds declared-length reads can return following bytes. Invalid readings are a separate surface, not on the segment path. Allocation failure is host-dependent: conversion OOM refuses at the pin, later container OOM aborts, and Rust aborts on its conversion/copy allocations. | Source-complete for the values/items/readings and allocation partitions in the [G1 phrase-segment closure](#g1-phrase-segment-closure-918); no new equivalence or #918 removal clearance. |
| `phrase_segment`: empty; well-bounded rejected `ff fe`; valid scalar conversion | Identical for the entry/result lifecycle described in #918's audit; existing independent lookup/math records retain their scope. | Source established, both facades; inherited tests only, no new gate. Not a proof of every valid decoder path. |
| `phrase_segment`: GLib-accepted non-scalar, consistent scalar-only keys/items, sound grams and successful backing-store reads, including a matching scalar prefix | Identical return/result/getter/diagnostic behavior; no observable session or persistent change from prefix consultation. | Complete bounded source argument in #918's audit, three backend read paths. Inherited surrogate-in-`nihao` tests do not exercise every non-scalar or store state. |
| `phrase_segment`: matching prefix, reached sound system gram plus 1–3-byte user gram, later GLib-accepted non-scalar | Identical: both sides abort at `get_total_freq` (row 85). G1 entry; no new runtime-quirk exception. | Source-established, both facades/all three wrappers. Recorded evidence: bdb, kc and tkrzw 10/10 MATCH each; the `disable-raw-ucs4-lookup` mutation gave 4 DIFFER/6 MATCH on bdb. |
| `phrase_segment`: bounded matching raw non-scalar key/item | Identical: pin true/token and the raw `u32` key path true/token; no `char`/`str` from the non-scalar. Record, no exception. | Both facade contract cases MATCH on bdb; the path reads the item's declared length/frequency without decoding its text. |
| `phrase_segment`: over-skipping byte walk, partial/short unsafe gram reads | Existing class (b) governs illegal reads; no fabricated deterministic pin output. The raw span walk uses row 98's existing warning on a reached unsafe lookup gram. | Source and applicable inherited safety records; length-walk instructions read on amd64/arm64. |
| `phrase_segment`: faulting prefix stores | A returned read failure is no gram at the pin and in Rust, which continues the search and reads through real libdb on the pin's schedule. oxpinyin reproduces the pin's kc second-read assertion as an abort. | Named paths source-traced, including the KC check-then-get implementation. No runtime fault probes; a seam unit test covers the abort. |
| Neighboring string helpers; other corrupt bounds, extreme lengths and allocation failure | Caller-specific outcomes and defects in the [G1 phrase-segment closure](#g1-phrase-segment-closure-918); no blanket quirk admission or whole-process resource-failure equivalence. | Caller boundaries traced. The [G1 phrase-segment closure](#g1-phrase-segment-closure-918) settles the NULL/zero-length token lookup on all three cells, classifies extreme-length narrowing as class (b), and records the conversion/later-allocation split on allocation failure. Full G1 removal remains incomplete for other families. |
| Auxiliary UTF-8 substrings, tool Unicode categories, stored punctuation | Pending under G2/#925, G16/#926, G15/#1144 respectively. | Unverified outside their separately stated evidence; not covered by stored phrase text. |

### G1 phrase-segment closure (#918)

Pin citations: `pinyin.cpp:1446-1460`, `zhuyin.cpp:965-979`,
`lookup/phrase_lookup.cpp:120-157,255-428`, and GLib
`glib/gutf8.c:117-128,237-273,665-747,940-997`. The amd64/arm64 binaries
were read where source left the length walk or conversion unsettled.
These results close source questions; they do not clear removal of a GLib
conversion.

- **Bounded non-scalar input.** GLib accepts canonical historical encodings
  through `0x7fffffff` (`glib/gutf8.c:665-747`). The Rust decoder routes
  them to the same raw `u32` path; no `char` or `str` is built from a
  non-scalar. Sound-data equivalence is a length-sized zero-token result,
  and a matching raw key gives the pin's true/token.
- **Lead-byte walk past the allocation.** The pin's load is out of bounds,
  class (b); no deterministic return is assigned.
- **Short user gram with a system gram.** Both sides abort at
  `get_total_freq` (register row 85). Partial gram items and sub-total rows
  without a system gram are row 98; the raw span walk routes a reached
  unsafe predecessor through its existing one-warning refusal.
- **Inconsistent item lengths and token bounds.** `PhraseTableEntry::search`
  walks four-byte tokens to the raw value end
  (`storage/phrase_large_table3.cpp:29-51`); `get_phrase_item` trusts the
  declared length (`phrase_index.cpp:180-208`); `save_next_step` indexes by
  it (`phrase_lookup.cpp:348-377`). Out-of-range shapes are class (b) (rows
  101 and 107). An in-range inconsistent length can manufacture a path or
  token that the validating Rust reader refuses; that difference is not
  admitted and not normalized.
- **Stored values `>= 0x80000000`.** `phrase_segment` with a matching raw
  key is identical. The text getters still differ: the pin's
  `_token_get_phrase` returns true with the declared length and the NULL
  from `g_ucs4_to_utf8` (`pinyin.cpp:1627-1645`, `zhuyin.cpp:1096-1113`).
  Invalid readings past the last one are the uninitialised-key class (b) of
  rows 51 and 70; the segment path never renders a reading.
- **NULL, zero-length token-lookup key.** A GLib-rejected `*_lookup_tokens`
  call passes `search(0, NULL, tokens)`
  (`storage/facade_phrase_table3.h:111-138`). No cell dereferences the key:
  db5.3's get accepts a zero-sized DBT and `__bam_defcmp` bounds its loop by
  the smaller size (`src/btree/bt_compare.c:155-181`); kc's
  `ProtoDB::accept` tests the pointer at amd64 `+0xe5` and the size at
  `+0x310`; tkrzw copies the key with a zero-length `memcpy` that loads
  nothing. The pin returns `SEARCH_NONE`, leaves the caller's `GArray`
  empty and answers false, as oxpinyin does. An empty-key row is malformed
  table state; the pinned tables have none.
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
  remember pass raw bytes or convert at their own boundary. Their
  differences on malformed input (prefix-list and lookup lifecycle,
  zhuyin direct-reading chunks, accepted non-scalars with matching raw keys)
  are source findings, not admitted exceptions. Rows 19 and 21 hold the
  assert cases `c-string-input/pinyin-character-offset-invalid-past-matrix`
  and `c-string-input/pinyin-full-key-apostrophe-invalid`, where oxpinyin
  aborts as the pin does.

#894's implementation gate (recorded evidence): **24 MATCH** on the bdb
reference cell, including six scalar controls and three zhuyin
export-symbol absence checks.

## G2 — C strings, joins and vectors

Pin: `pinyin.cpp:698-768,822-865,3295-3379,3381-3576`;
`storage/punct_table.cpp:74-94`; full `g_strdup`, `g_free`, `g_string_*`
and vector membership in the inventory TSV.
Rust: `capi/src/ffi.rs`, the pin's `g_strdup`/`g_strdup_printf`/`g_strconcat`/
`g_strjoinv`/`g_ptr_array_*` calls (#1227,
[record](returned-string-allocation.md));
`capi/src/aux_matrix.rs`, `SharedMatrix` and `split_zhuyin`;
`capi/src/text.rs`, `chewing_aux_text`;
`capi/src/iterators.rs` export getters; `capi/src/predict.rs`,
`prepend_punctuations`; `input/src/path.rs`, `build_filename`.

| Partition | Verdict / record | Investigation |
| --- | --- | --- |
| Valid scalar phrase strings and their C marshalling | Identical within G1's valid-text scope. | Complete source argument there; other string consumers remain partial. |
| Non-scalar / NUL phrase text at marshalling or export | Accepted only as the #894 non-scalar / NUL records. | Source complete for those records, runtime limits as G1. |
| Auxiliary prefix/middle/postfix at every supported cursor: key interiors, key boundaries, tone digit/mark cuts, clamped overrun cursors | Identical visible bytes for the three auxiliary exports while the pin returns. `split_zhuyin` is an ordinary `str` scalar cut; the `g_strconcat` and `g_utf8_substring` shapes are gone. The pin's right-half call walks past the spelling at essentially every interior cursor (next rows). | Source trace below; bdb cases `aux-text-chewing-cursors`, `aux-text-chewing-zhuyin-cursors`, `aux-text-chewing-overrun-cursor` MATCH on visible bytes, and the aux family 11/11 MATCH (recorded evidence). |
| Missing middle: the `g_strconcat(prefix, NULL, postfix, NULL)` stop | Unreachable in a supported operation; the walk cannot pass the cursor without setting `middle`, so oxpinyin has no NULL-stop branch. | Source proof below. No case can witness it because the pin cannot reach it. |
| Chewing interior cursor (`len <= C`) | Visible right half identical to a pin that returns: the copied NUL terminates it at the spelling's end (empty at `len == C`). The call is class (b): `end_pos` is the absolute `m_raw_end`, so `raw_end - len > C - len` whenever `raw_end > C` (outside GLib's documented domain), which is essentially every interior cursor; the copy reads past the allocation at `raw_end >= C + 2`. | Source below; the new cases compare visible bytes, so a match is conditional on the pin not faulting. |
| Chewing cursor past the spelling (`len > C`) | Accepted class (b): the copy starts past the terminator, so the pin's visible right half is heap bytes (and can fault); oxpinyin returns an empty right half. Registered in [upstream divergences](upstream-divergences.md#chewing-auxiliary-substring-over-read). | Source below; not a differential case, because the pin side has no reproducible result. |
| Bigram export's unterminated pronunciation vector | Accepted class (b), [row 1 record](upstream-divergences.md#bigram-export-iterators-pinyin-buffer). | Established independent fault; #894 traces its text consumers but does not turn this fault into a deterministic phrase-text witness. |
| Raw path bytes: see G3 (#922). Embedded NUL in diagnostics; duplicate punctuation strings/order | Paths classified under G3; diagnostic NUL producers closed under #924 below; punctuation strings closed under G15 (#1144) and order/duplicates in the inventory's G2 notes (#928). | No blanket “malformed quirk” admission. |

### G2 auxiliary-text closure

The three exports read one matrix. `pinyin_parse_more_full_pinyins`
(`pinyin.cpp:1497-1517`), `pinyin_parse_more_double_pinyins`
(`:1540-1560`) and `pinyin_parse_more_chewings` (`:1582-1605`) fill it
through `fill_matrix` (`storage/phonetic_key_matrix.cpp:30-80`): one column
per consumed byte, each key at its `m_raw_begin`, zero keys in the gaps
(apostrophes), and the reserved zero key at `parsed_len`, so
`matrix.size() == parsed_len + 1`. Each export clamps the cursor with
`std_lite::min(cursor, instance->m_parsed_len)` (`:3390`, `:3449`, `:3527`),
leaving `cursor < matrix.size()`.

`_get_aux_text_prefix` and
`_get_aux_text_postfix` (`:3295-3379`) walk the same matrix, stop at the
reserved last column, and build their result with `g_strconcat` over
non-NULL pieces (`:3325`, `:3369`); the only NULL-argument stop is the final
`g_strconcat(prefix, middle, postfix, NULL)` (`:3431`, `:3509`, `:3569`).

**The NULL stop is unreachable.** For `full`, let `offset` be the loop
variable (`:3400-3429`); it starts at zero and `cursor >= 0`. Each step
computes `newoffset = _compute_pinyin_start(matrix, offset) >= offset`
(`:2903-2919`). If `offset <= cursor <= newoffset`, the boundary branch sets
`middle`. Otherwise `newoffset < cursor` (the only remaining case while
`offset <= cursor`), the walk assigns `offset = newoffset`, and
`key.begin == offset <= cursor`. If `begin < cursor < end`, the split branch
sets `middle`; otherwise `cursor >= end`, and `offset = key.end <= cursor`.
The invariant `offset <= cursor` therefore holds until the walk reaches the
reserved last column `size - 1 == parsed_len`, where
`newoffset == offset == parsed_len` and the boundary branch fires because
`cursor <= parsed_len`. `full` always sets `middle` before the loop ends.

`double` and `chewing` (`:3460-3507`, `:3538-3567`) maintain the same
invariant: if `cursor != offset`, then `offset < cursor`; the span at
`offset` starts there, so either it contains the cursor and the split branch
sets `middle`, or it ends at or before the cursor and `offset = end <= cursor`
with `end > offset`. The walk cannot step over `cursor`, so it reaches
`cursor == offset` no later than the reserved last column and sets `middle`.

A reached empty column is the pin's `assert(matrix.get_column_size(offset) >= 1)`
(`:3311`, `:3353`, `:3410`, `:3467`, `:3545`) and oxpinyin's `None`; it never
reaches the concat. A `middle == NULL` arm would therefore be dead, and
oxpinyin has none.

**`g_utf8_substring`'s absolute end position.** The chewing split calls
`g_utf8_substring(zhuyin, 0, len)` and `g_utf8_substring(zhuyin, len, end)`
(`:3557-3558`), where `len = cursor - begin` is a raw-input byte delta and
`end = m_raw_end` is an **absolute** raw-input byte offset. GLib 2.89.4-2
(recorded evidence: the Debian testing package that this reading used; tag `2.89.4`)
`g_utf8_substring` (`glib/gutf8.c:298-322`) resolves
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
`content_table` row (checked over all 441 rows at the pin: only the one-byte
incomplete `w`/`y` placeholders have `B < C`, and neither has an interior
cursor), so `raw_end > C` for every interior cursor except a first key whose
raw span equals its scalar count. For example, `nihao@3` cuts `hao`
(`begin 2`, `raw_end 5`, `C = 2`) at `len = 1`: the second call starts at
`ㄠ` and asks for `5 - 1 = 4` scalars, not the one that remains, so it walks
three scalars past the NUL and copies the NUL plus the bytes after it. The
`B >= C` relation places the NUL at or before the copied end; it does not
bound the copy. The zhuyin parser's spans are also absolute, so its later
keys reach the same condition.

The visible result is still deterministic while the pin returns.
`g_strconcat(left, "|", right, " ", NULL)` (`:3560`) measures `right` with
`strlen`. For `len <= C` the copied buffer begins at scalar `len` and ends
at or after the spelling's NUL, so its first NUL is the spelling's end. The
visible right half is the spelling from `len` to `C` (empty at `len == C`).
This matches `split_zhuyin`, although bytes after the NUL were read. For
`len > C` the copy begins after the NUL, so the visible right half is heap
bytes. There, `split_zhuyin` returns an empty right half as the
deterministic class (b) choice.

The class (b) difference is therefore:

- the absent out-of-allocation read, and possible fault, where the copied
  end passes the terminator (`raw_end >= C + 2`);
- the out-of-domain walk for `raw_end == C + 1`;
- the differing bytes for `len > C`.

The visible bytes for `len <= C` are equal only when the pin returns.
`g_strconcat` (`glib/gstrfuncs.c:572-609`) is a plain concatenation once
the NULL stop is removed.

**Cases.** `aux-text-chewing-cursors` visits every byte cursor on four
full-pinyin inputs plus the maximum `size_t`, covering both key boundaries,
interior cuts, the tone-1 terminator (`ni1@2`) and the tone-marked terminator
(`ni3hao4@6`). `nihao@3` is the `len < C` case that reads past the NUL
(`raw_end - C = 3`); `nihao@1` is the first key's `B == C`
in-bounds cut; `ni1@1` and `ni1@2` are the `raw_end == C + 1` walk that leaves
the string without reading its next byte; `ni3hao4@6` reads past it
(`raw_end - C = 4`). `aux-text-chewing-zhuyin-cursors` does the same over the
zhuyin parser (`su3cl3`): its first key's raw span equals its scalar count
(`B == C`), so its interior cuts stay in bounds, and its second key's
absolute end is `C + 3`, so its interior cuts read past. The Standard
keyboard spends one keystroke per zhuyin scalar plus the tone key, so an
interior cursor's `len` stays below `C`; the terminator falls on the key
boundary, which the `cursor == offset` branch takes rather than the substring
branch. The terminator cut is therefore exercised on the shared-matrix path
by `ni1@2` and `ni3hao4@6`. Longer keyboards such as CP26 can spend two
keys on one scalar, so their interior `len` can exceed `C`. `split_zhuyin`
applies the same class (b) clamp there.

`aux-text-chewing-overrun-cursor` uses one past, seven past and the maximum
`size_t` on both parse paths. All clamp to `parsed_len` at `:3527`. The
cases compare the visible bytes. A match is conditional on the pin
returning, and the out-of-allocation read itself is not reproduced. The
missing-middle and `len > C` output partitions have no case, for the
reasons above.

Output ownership is unchanged. The matrix-empty branch writes an allocated
`""` and returns `false` (`:3385-3388`, `:3444-3447`, `:3522-3525`). Every
other answer is `true` plus one caller-owned allocation, still marshalled
by the pin's `g_strdup`/`g_strconcat` (#1227). No preedit or candidate path
calls `split_zhuyin`. The only callers are `SharedMatrix::chewing` and
`chewing_aux_text`, which only `pinyin_get_chewing_auxiliary_text`
reaches.

## G3 — Path joining

Pin: `pinyin.cpp:164,176-226,252,268,286,308,331-336,353-368,395,400,419-439,952,968,985,997-1016,1052-1119,1260,1277`;
`zhuyin.cpp:118,130-170,196,212,230,252,275-280,297-300,585,601,605,626,630,684`.
GLib **2.84.4-3~deb13u5** (recorded evidence), `glib/gfileutils.c:1925-2025`,
`g_build_path_va` as `g_build_filename` calls it with `"/"` on Unix; the
`g_strdup` of the two directories at `pinyin.cpp:331-332` and
`zhuyin.cpp:275-276` is a byte copy.
Rust: `input/src/path.rs`, `build_filename`/`build_path`/`build_bytes`,
the pin's `g_build_filename` through `glib-sys` (external-I/O stage 3);
`capi-marshal/src/input.rs`, `filename`, the caller's directory bytes.
Owner #922; G10 covers rename/sync. Derived by reading; the pin was not run
to learn any result below.

The pin joins exactly two elements, `dir` and `name`, so the function reduces
to one law (checked against the source for every row): an empty `name` gives
`dir`, an empty `dir` gives `name`; otherwise `dir` keeps its leading
separators and loses its trailing ones, `name` loses its leading ones and
keeps its trailing ones, and one separator stands between the bodies. A part
made only of separators has no body (`dir` is then kept whole, `name` is
appended whole). Nothing inside a part is touched.

| Partition | Verdict | Investigation |
| --- | --- | --- |
| Ordinary directory/name pairs, empty `dir` (cwd-relative bare names), empty `name` | Same effective path as `Path::join`; the pin's call yields identical bytes. | Complete (source argument above; `glib-922-dir-*`, `glib-922-row-*`). |
| Leading `/` in a `table.conf` file name (`/x`, `//x`, `sub//x`) | Stays beneath the directory, never absolute: the pin's call keeps the pin's file. Not removable. | Complete; `glib-922-row-*` open the same library as the pin, `glib-922-row-missing-*` write the same `mmap … failed!` bytes. |
| Repeated or trailing separators on the directory (`/d///`) | The file is the same; the pin's printed path has one separator (`/d/x`), and oxpinyin writes that. | Complete. Held by `oxpinyin-input` `path::tests::names_stay_beneath_their_directory`. |
| Directory or name made only of separators (`/`, `//`) | Kept whole as the law says (`//` + `x` is `//x`; `/a` + `//` is `/a//`). Same file as a plain join on Unix, different bytes only in diagnostics. | Complete; held by `oxpinyin-input` `path::tests::names_stay_beneath_their_directory`. |
| `..` and `.` segments, leading `//` on the directory | Untouched by the pin's call; `Path` semantics resolve them identically. | Complete; held by `oxpinyin-input` `path::tests::names_stay_beneath_their_directory`. |
| User directory identity: `""` is a directory (cwd), NULL is none; `g_strdup` keeps the bytes | Unchanged. The marshalling is a byte copy into `OsStr`, the natural Rust form. | Complete; held by the existing #619/#642/#643 tests and both capi crates' `tests/userdir_arg.rs` `null_and_empty_user_dirs_are_different_arguments`. |
| Non-UTF-8 directory or name bytes | Raw bytes everywhere (`OsStr`, `path_bytes`); unchanged. | Complete; `e2e_tests::directories_with_non_utf8_names_open` (`oxpinyin-capi`) and `c_stderr::tests::paths_are_written_as_their_bytes` (`oxpinyin-data`). |
| `\` as a separator | An ordinary byte on Unix (GLib uses `/` only there). On Windows GLib accepts both; oxpinyin makes the same call, but no pin run covers Windows. | Unix complete; Windows unverified (no reference there). |
| User-directory file names (`user.conf`, the user bigram and index files and their `.tmp` siblings) | Same file always. With a directory ending in two or more separators (`/u///`), the pin's `g_build_filename(user_dir, name, NULL)` (`pinyin.cpp:164-226`, `zhuyin.cpp:118-170`; GLib `g_build_path_va` trims the directory's trailing separators and inserts one) gives `/u/user.conf`. `UserTableInfo::load` and `save` print that name raw (`table_info.cpp:332`, `:382`), and the save prints the `rename %s to %s failed.` pair (`pinyin.cpp:1061-1123`, `zhuyin.cpp:611-689`). `persistence.rs` builds every one of these through the pin's call (`build_path`). | Complete; unit `user_conf_path_is_the_join_the_pin_writes`, `glib-922-dir-user-dup-trailing`, `glib-922-save-write-failed-dup-trailing`. |

The system DBM opens (`data/src/dict.rs`, `runtime/src/lib.rs`) join a
constant name with `Path::join`. That is the same file as the pin's join,
and the pin opens those files through the backend without printing a path
of its own.

No discrepancy is registered: every difference found was a defect and is
fixed, and no valid configured file moves. Recorded evidence: the 39
`glib-922-*` cases MATCH on bdb, and the first 35 on kc and tkrzw.

## G4 — GArray/GPtrArray containers

Pin: `pinyin.cpp:2805,2817`; `zhuyin.cpp:1797,1809`;
`lookup/phonetic_lookup.h:231-239,298-339`; the complete 514-site container inventory
in the TSV. Rust: both `capi/src/dict.rs` and `zhuyin-capi/src/dict.rs`,
`*_lookup_tokens`/`*_token_get_nth_pronunciation`; internal
`engine/src/nbest.rs::Step`, session vectors and phrase-library slices.

| Partition | Verdict | Investigation |
| --- | --- | --- |
| Caller-owned real GArray mutation | Contract: MUST RETAIN GLib metadata/ownership, #924; direct `glib_sys` mutation is not emulation. | Source-backed ownership/lifecycle audit below (G9); conditional allocation failures remain distinct. |
| Valid internal step/candidate/result arrays | Inventory source conclusion: ordinary parity via vectors/slices. | Partial across all container sites; sorting separately G5/G6. |
| Out-of-range indices, key with no whole word, short chunk header | Pending per-site classification here; named existing (b) records are not a blanket container exception. | Unverified family-wide coverage; chunk specifics G12. |

## G5 — N-best tail sort

Pin: `lookup/phonetic_lookup.h:174-178,329-339` (`trellis_value_compare`,
`g_ptr_array_sort`). Rust: `engine/src/nbest.rs` `tail_merge`, a stable
`sort_by` on raw possibility with NaN last (`ordered_poss_compare`). Both
facades, all backends; #921.

| Partition | Verdict / record | Investigation |
| --- | --- | --- |
| Ordinary finite tails and stable/tied ordering | Language artefact (#1303): the pin's `gint` truncation ties scores under one unit apart, so its comparator is not transitive on finite scores. oxpinyin sorts without truncation. Registered site S3 in [compatibility-policy.md](compatibility-policy.md#registered-sites). | Source and binary (`cvttss2si`, symbol `0x96010`); [witness record](algorithm-precondition-witnesses-2026-10-28.md). |
| Transcendental accumulation near-ties | Accepted class (a) only within [row 11's record][math-record], not every comparator difference. | Existing math finding; G14 does not complete #921. |
| NaN, out-of-range float differences, non-transitive comparisons | NaN sorts last; signed zeros tie. The pin's NaN conversion is not observable. | amd64 binary read (`subss`, `xorps`, `cvttss2si`); arm64 not read. |

## G6 — Candidate and predicted-list sorting

Pin: `pinyin.cpp:1678-1709,1794-1868,2041-2138,2286,2443`;
`zhuyin.cpp:1129-1144,1229-1270,1375,1529,1610`.
Rust: `engine/src/candidate.rs` `rank_by_frequency`, called by
`engine/src/session/mod.rs` `rank_candidates` and `capi/src/predict.rs`
`sort_predicted`; `session/lookup.rs` `amplified_frequency_with_bigram`.
Owners #927/#928; preserve #913/#914 valid-input ranking.

| Partition | Verdict / record | Investigation |
| --- | --- | --- |
| Valid candidate order, equal keys, stable dedup winner and prediction order | Contract: stable std sorting with the pin's guarded wrapping frequency comparison ([1297-candidate-numeric-sort](1297-candidate-numeric-sort.md)). Dedup winner #913/#914/#928. | bdb controls `glib-sort-candidates-*`, `glib-sort-predicted` (recorded evidence). |
| NUL/non-scalar stored text affecting membership, dedup or ranking | Accepted only within the #894 NUL / non-scalar source traces. | Source analysis established; #894 isolated witnesses do not exercise every dedup collision. |
| Out-of-range unsigned frequency stores in #1137's five named cases | Identical to amd64 reference; arm64 pin portability defect, no oxpinyin exception. [Binary finding](arm64-pin-portability-1137.md). | Established source/binary result for those sites/cases; not all frequency-wrap or NaN cases. |
| Other `gint` frequency wrap, NaN scores, comparator ordering and shared-helper callers | Cyclic frequency groups use the defined numeric fallback ([1297-candidate-numeric-sort](1297-candidate-numeric-sort.md)). | Source; out-of-range λ cases are attributed in that record and in [1378-zhuyin-selection](1378-zhuyin-selection.md). |

Inherited binary citations: `_compute_frequency_of_items` in the bdb
`libpinyin.so.15.0.0`, amd64 symbol at `0x8fa20`, predicted conversion
at symbol `+0x2ae` (object `0x8fcce`), store `+0x14e` (`0x8fb6e`);
arm64 symbol at `0x72408`, conversion `+0x23c` (`0x72644`),
store `+0x128` (`0x72530`). In `libzhuyin.so.15.0.0`, same symbol,
amd64 start `0x8dee0`, conversion `+0x18d` (`0x8e06d`),
store `+0x192` (`0x8e072`); arm64 start `0x70804`, conversion
`+0x134` (`0x70938`), store `+0x138` (`0x7093c`). The linked finding
identifies the objects, commands and five tested cases. These are
unsigned frequency stores, not G5's signed tail comparator.

## G7 — Hash tables

Pin: `lookup/phonetic_lookup.h:212,235,263,307-315,350`;
`lookup/phrase_lookup.cpp:49,65,80,358-363,421`.
Rust: `engine/src/nbest.rs`, `Step::node`/`node_mut` and node vector;
`engine/src/phrase.rs`, `Step::relax`/`best`. Both facades, all backends.

| Partition | Verdict | Investigation |
| --- | --- | --- |
| Reachable step/token key lookup and update; new/repeated/missing keys | Identical at this internal boundary: hashes index stored nodes, while result order comes from content arrays, not hash iteration. No discrepancy record needed. | Complete narrow source argument from inventory plus lookup/insert sites; not a proof of surrounding scoring/sort behavior. |
| Malformed hash-specific input | No separate partition identified by the inventory. | Not an assertion that all malformed caller inputs are covered; their validation belongs to the consuming family. |

## G8 — Diagnostics

Pin: `pinyin.cpp:1454`; `zhuyin.cpp:973`;
`storage/phrase_index.cpp:354-356,361`; tool `g_warning` in G16.
Rust: both facades' `ffi.rs::log_warning`; `phrase.rs` calls the pin's
`g_utf8_strlen` through `glib-sys`. The six `g_return_val_if_fail` sites are
early returns of the pin's value with no critical (maintainer decision D1:
[1297-segment-critical](1297-segment-critical.md),
[1297-layout-criticals](1297-layout-criticals.md)).

| Partition | Verdict / record | Investigation |
| --- | --- | --- |
| Recorded #894 non-scalar candidate refusal versus NUL candidate omission | Accepted the #894 non-scalar / NUL records; warning only on the former witness. | Complete bounded records; G1's exact tested scope. |
| Registered pin assertion/abort sites | The pin's registered asserts and aborts are reproduced with `std::process::abort()`; class (b) refusals keep exactly one warning, correct domain/level. | Existing policy obligation; full G8 site audit partial. |
| `phrase_segment` conversion failure, critical precondition and previous result; diagnostic NUL/text/allocator cases | Return and previous result are MUST RETAIN; the invalid-byte critical is not written (D1, POLICY-ACCEPTED, [1297-segment-critical](1297-segment-critical.md)). NUL reachability is closed for current diagnostic producers; conditional OOM outcomes are traced below, without same-allocation failure injection. | #924 source proof and both-facade result-preservation cases; no blanket diagnostic exception. |

## G9 — Allocation/free and real GArray ABI

Pin: `pinyin.cpp:2805,2817`; `zhuyin.cpp:1797,1809`;
`pinyin.cpp:1452,1459,1644`; `lookup/lookup.cpp:54-65`; complete allocation
sites in TSV. Rust: both `ffi.rs` returned-string helpers (the pin's GLib
calls, #1227, [record](returned-string-allocation.md); the fallible
`malloc_cstr_with` for the `g_ucs4_to_utf8` sites), `dict.rs` GLib array
writers, `phrase.rs`; ownership slots in `libpinyin.alloc`/`libzhuyin.alloc`
(under their C facade crates).

| Partition | Verdict | Investigation |
| --- | --- | --- |
| Normal returned strings/vectors and caller arrays | MUST RETAIN: per-operation #924 matrix below covers all 38 registered slots and four caller-array operations. | Source-backed lifecycle audit and caller-mutation tests below; G1 retains its separate malformed-text scope. |
| NULL inputs/outputs, allocation failure/OOM, cleanup after partial allocation/conversion | #924 below traces conditional allocation outcomes and NULL caller-array diagnostics; malformed text retains G1 scope. | Four NULL-array defects found and closed (#1224); OOM outcomes traced below and injected by `just abi alloc-failure`. #894 excludes allocation failure. No ownership exception inferred. |

### #924 ABI ownership audit

**Decision: retain the GLib ABI.** No runtime exception is admitted. The
matrix settles the normal ownership boundaries, not every
malformed-input/diagnostic partition. Other families' rows retain their
own scope and status. Pin citations are to `074a2219` and start at `src/`;
GLib citations are to the GLib source tree.

`MUST RETAIN` applies to the outward contract, not the implementation's
choice of container. `MAY DIVERGE` here means internal implementation
freedom with identical outward results, **not** permission to change a
message, return, out-parameter, lifetime or allocator pairing. Reproducing
these contracts in ordinary Rust plus C adapters is possible. Opaque Rust
handles may use `Box`; Rust strings/vectors may build payloads; only the C
adapter transfers them to the matching foreign allocation. A Rust `CString`
from `into_raw` is not a substitute for a promised `g_free` buffer.

#### Exported allocation and lifetime matrix

`P` = `pinyin_`; `Z` = `zhuyin_`. A row naming both prefixes covers each
named exported operation independently. The two `.alloc` registers cover
26 pinyin slots and 12 zhuyin slots; they are retained byte-for-byte. The
four caller-supplied `GArray *` operations are additional to those registers
(they have no pointer return or `T **` slot).

| Exported operation | Allocation → library write → caller mutation/free: MUST RETAIN | Pin source; MAY DIVERGE internally |
| --- | --- | --- |
| P/Z `init`, `fini` | Caller lends directory strings for the call; library owns copies and context. Return opaque context, release only with matching `fini`; never `g_free` the handle. | `pinyin.cpp:326-441,1194-1224`; `zhuyin.cpp:270-357,741-765`. Pin `new/delete` and internal `g_strdup`/`g_free` may be Rust ownership, preserving path bytes and side effects. |
| P/Z `alloc_instance`, `free_instance` | Context must outlive instance; only matching destructor frees the handle and its internal candidate/result storage. No caller ownership of internal GLib arrays. | `pinyin.cpp:1310-1356`; `zhuyin.cpp:845-883`. Internal arrays may be Rust vectors. |
| P/Z `begin_add_phrases`, `end_add_phrases` | Opaque iterator borrows context; matching end releases it and performs the pin's compact/modified effects. | `pinyin.cpp:506-520,655-660`; `zhuyin.cpp:392-406,540-545`. Rust iterator representation is free. |
| P `begin_get_phrases`, `end_get_phrases` | Own iterator even when it has no rows; release with end, not `g_free`. | `pinyin.cpp:662-689,771-773`. Rust cursor is free. |
| P `begin_get_bigram_phrases`, `end_get_bigram_phrases` | Own iterator, matching end releases its buffers/arrays and handle. | `pinyin.cpp:775-790,913-919`. Rust row storage is free; existing row-1 over-read exception remains bounded. |
| P `get_context` | Borrowed exact originating context pointer; caller must not free a second time. | `pinyin.cpp:1358-1360`. No zhuyin twin. |
| P/Z `get_candidate` | Borrowed candidate, NULL on out-of-range false; invalidated with candidate storage, never caller-freed. | `pinyin.cpp:2851-2863`; `zhuyin.cpp:1843-1855`. Internal storage free subject to pointer validity. |
| P/Z `get_candidate_string` | Borrowed candidate-owned string; successful accessor allocates nothing and returns true. Do not mutate/free through the const pointer. | `pinyin.cpp:2873-2877`; `zhuyin.cpp:1865-1869`. Pin's internal text allocation may be Rust-owned, with the same visible bytes/lifetime. |
| P `get_pinyin_key`, Z `get_zhuyin_key` | Borrowed key, false-nulls; pin uses a function-static key overwritten by later calls. Caller does not free it. | `pinyin.cpp:2921-2941`; `zhuyin.cpp:1886-1905`. Representation may differ only without changing the established borrowed-slot behavior. |
| P `get_pinyin_key_rest`, Z `get_zhuyin_key_rest` | Borrowed rest slot, false-nulls; same static-slot constraint, not an owned GLib allocation. | `pinyin.cpp:2944-2964`; `zhuyin.cpp:1908-1927`. Same constraint as key. |
| P/Z `get_sentence` | Fresh caller-owned bytes via `g_free` when a sentence is produced; no guessed results means false and untouched out-pointer. Caller may write/reallocate its buffer. | `pinyin.cpp:1463-1481`; `zhuyin.cpp:982-999`; `lookup/lookup.cpp:27-69`. `ffi::sentence_text` makes the pin's `g_strdup`/`g_strconcat` chain over Rust-built phrases (#1227). |
| P `get_full_pinyin_auxiliary_text` | Owned string, including allocated empty string on false with empty matrix; free regardless of success. | `pinyin.cpp:3381-3437`. The pin's `g_strdup`/`g_strconcat` (#1227); G2 owns substring behavior. |
| P `get_double_pinyin_auxiliary_text` | Same ownership and false-allocation rule. | `pinyin.cpp:3440-3515`. Same internal freedom. |
| P `get_chewing_auxiliary_text` | Same ownership and false-allocation rule. | `pinyin.cpp:3518-3575`. Same internal freedom, subject to G2's recorded bounds. |
| P `get_zhuyin_string` | Fresh `g_free` buffer; zero-table-index key gives false and NULL. | `pinyin.cpp:2707-2715`; `storage/chewing_key.cpp:74-88`. Rust rendering is free. |
| P `get_pinyin_string` | Same owned-buffer/false-nulls rule. | `pinyin.cpp:2718-2726`; `storage/chewing_key.cpp:47-57`. Rust rendering is free. |
| P `get_luoma_pinyin_string` | Same owned-buffer/false-nulls rule. | `pinyin.cpp:2729-2737`; `storage/chewing_key.cpp:90-101`. Rust rendering is free. |
| P `get_secondary_zhuyin_string` | Same owned-buffer/false-nulls rule. | `pinyin.cpp:2740-2748`; `storage/chewing_key.cpp:103-114`. Rust rendering is free. |
| Z `get_zhuyin_string` | Same owned-buffer/false-nulls rule. | `zhuyin.cpp:1732-1740`; same ChewingKey renderer. |
| Z `get_pinyin_string` | Same ownership, selecting Hanyu/Luoma/secondary renderer from the context; false-nulls for zero key. | `zhuyin.cpp:1743-1765`; same ChewingKey renderers. |
| P `get_pinyin_strings` | Two independently owned `g_free` buffers; NULL output addresses are allowed independently; zero key leaves both outputs untouched and returns false. | `pinyin.cpp:2751-2762`; `storage/chewing_key.cpp:60-71`. Rust rendering is free; no shared allocation for two independently freed slots. |
| P/Z `token_get_phrase` | Optional length/text output addresses; missing item returns false without writing either. Successful text is an independent `g_free` allocation. | `pinyin.cpp:1627-1645,2776-2783`; `zhuyin.cpp:1096-1113,1768-1775`. Rust text conversion may replace internal UCS-4 buffer within G1's classified partitions. |
| P `iterator_get_next_phrase` | Two independent `g_free` buffers per row. Pin initializes outputs NULL/count -1, then writes row and returns true, including last row. Calling without a row is the existing row-62 abort/refusal exception, not an ordinary false. | `pinyin.cpp:698-768`. The phrase uses the fallible `malloc_cstr_with`; the reading is `ffi::strjoinv`, the pin's `g_strjoinv` (#1227). |
| P `bigram_iterator_get_next_phrase` | Two independent `g_free` copies. Last row returns false **after** writing both strings; caller must free them. | `pinyin.cpp:896-910`. Rust copying is free; separate existing iterator safety exceptions apply. |
| P/Z `in_chewing_keyboard` | Caller owns both NULL-terminated vector and each string; may replace strings or resize vector, finally `g_strfreev`. Unmapped key returns false with NULL vector. | `pinyin.cpp:1615-1624`; `zhuyin.cpp:1087-1093`; `storage/zhuyin_parser2.cpp:301-332,496-544,798-851`. `ffi::symbol_vector` makes the pin's `g_ptr_array_*` calls (#1227). |
| P/Z `lookup_tokens` | Caller supplies a **real** GArray of `phrase_token_t`. Library clears previous contents, appends in library order, preserves clear callback/element size/zero termination/refcount/capacity semantics. Caller may append, shrink, reuse, ref/unref or free it. No-match false still clears. | `pinyin.cpp:2652-2667`; `zhuyin.cpp:1678-1693`; `storage/phrase_large_table3.h:77-97`. Rust may construct token payloads; GLib must mutate the caller object. Input conversion is separate follow-up, not a reason to change the array. |
| P/Z `token_get_nth_pronunciation` | Real caller GArray of two-byte `ChewingKey`; clear **before** item lookup, including missing-token false. Valid reading appends phrase-length keys. Caller mutation/free remains legal. | `pinyin.cpp:2801-2818`; `zhuyin.cpp:1793-1810`; `storage/phrase_index.cpp:33-43`. Rust payload extraction is free; GLib owns metadata changes. Out-of-range reading uses the existing safety classification, not invented pin bytes. |
| P/Z `phrase_segment` | No allocation transferred to caller. Pin's temporary `g_utf8_to_ucs4` buffer is freed after lookup; diagnostic refusal precedes result replacement. | `pinyin.cpp:1446-1460`; `zhuyin.cpp:965-979`. `phrase.rs` uses Rust scalar/raw-UCS4 decoding: there is no GLib conversion allocation or `g_free` there. #918 owns that converter. |

Other temporary GLib arrays (parser matrices, per-library `PhraseTokens`,
candidate vectors, iterator scratch) do not cross the ABI as owned arrays.
The borrowed candidate/key rows above constrain their exposed data, not
which internal allocator or container implements it.

GLib's implementation explains why the adapter stays:
`glib/garray.c:63-74` has private element size/capacity, clear/terminator
flags, reference count and callback beyond public data/len;
`:605-625,773-795,890-916` update those and invoke caller clear callbacks.
`:524-574` implements `g_array_free(FALSE)`'s caller-owned detached payload
and shared-wrapper lifetime. Updating just public fields cannot preserve
that contract. `glib/gmem.c:95-111,206-210` pairs malloc/free;
`glib/gstrfuncs.c:2571-2582` frees every vector element and then the vector.
The returned-string helpers make the pin's GLib calls, and
`malloc_cstr_with` uses the same `malloc` family. Interior NUL rejection and
allocator failure are **not** proved compatible by successful allocator
pairing.

#### Diagnostic channels and macro-expanded sites

| Exported operation / trigger | MUST RETAIN outward result and channel | MAY DIVERGE / status |
| --- | --- | --- |
| P/Z `phrase_segment`, well-bounded `ff fe` after successful segmentation | Exactly one NULL-domain critical (level 8), `bool <facade>_phrase_segment(<facade>_instance_t*, const char*): assertion 'num_of_chars == ucs4_len' failed`; false; previous result/count/token outputs remain readable and identical. Pin `pinyin.cpp:1450-1459`, `zhuyin.cpp:969-978`; GLib `gutf8.c:950-990` leaves initialized converted length 0 on failure. | Since maintainer decision D1 the critical is not written; return and old result are kept. `glib-924-segment-critical-{pinyin,zhuyin}` and `glib-924-segment-fatal-critical-{pinyin,zhuyin}` are POLICY-ACCEPTED ([1297-segment-critical](1297-segment-critical.md)). |
| P/Z `init`, `load_phrase_library`, `save`, `mask_out`; additionally P `load_addon_phrase_library`, reached malformed sub-index separators/end | `storage/phrase_index.cpp:354-356,361` expands four `g_return_val_if_fail` checks; first failed reached check emits one NULL-domain critical and returns false from `SubPhraseIndex::load`. Facade outcomes are caller-specific: the load caller may ignore false; `save` may report true after a critical. Never convert all these into facade false or silence. | Since maintainer decision D1 the check is an early return of the pin's value with no critical ([1297-layout-criticals](1297-layout-criticals.md)); caller outcomes are the pin's. Source trace below separates bounded first-three checks from out-of-bounds/dangling-view paths; existing G8 exceptions remain independently scoped. |
| P/Z `token_get_nth_pronunciation`, NULL `keys`, valid instance and known token/in-range reading | GLib `g_array_set_size` precondition emits `GLib`-domain critical `g_array_set_size: assertion 'array' failed`; append emits a second critical `g_array_append_vals: assertion 'array' failed`; both return NULL locally, facade still returns true, no caller output exists. With unknown token: only first critical, facade false. Pin `pinyin.cpp:2805-2818`, `zhuyin.cpp:1797-1810`; GLib `garray.c:611,778`. | **Defect, closed by #1224:** the NULL array reaches GLib, which writes its own criticals, with the pin's returns. Witnesses `glib-924-null-array-pronunciation-{pinyin,zhuyin}`. |
| P/Z `lookup_tokens`, NULL output array, valid instance/scalar phrase | `reduce_tokens` calls set_size then append for each non-NULL prepared library array, even length zero: one plus the number of prepared arrays `GLib` criticals, each returning NULL locally; facade still returns `SEARCH_OK & retval`. No out-array is written. Pin `storage/phrase_large_table3.h:77-97`; GLib `garray.c:611-614,778`; facade citations above. | **Defect, closed by #1224:** GLib writes its own criticals, one plus the number of prepared library arrays, with the pin's returns. Do not hard-code one warning. Witnesses `glib-924-null-array-lookup-{pinyin,zhuyin}`; the fixture prepares seven library arrays, so both found/missing calls emit eight criticals. |
| Facade refusals mapped from unsafe reads | Existing class-(b) records retain each operation's trigger, false/NULL/out-state and exactly one facade-domain warning (level 16); `ffi::log_warning` uses `libpinyin`/`libzhuyin`. This channel is distinct from the pin's NULL-domain critical and raw stderr. | All call sites remain. No blanket permission to remove/rename messages. Full per-trigger re-audit of the existing warning register is not completed here; inherited witnesses are not new executions. |
| Both `ffi::log_warning`, hypothetical embedded NUL in message | `CString::new` drops the whole diagnostic on embedded NUL; GLib `%s` truncates at NUL. A concrete reachable trigger and complete outward proof are required before classifying any difference. | No reachable embedded-NUL producer found in the current C-facing call graph; source closure below. The helper difference is not an admitted outward difference. |

Macro expansion is part of the source proof: GLib
`glib/gmessages.h:663-674` calls `g_return_if_fail_warning` then returns the
specified value; `glib/gmessages.c:3193-3201` chooses critical and formats
the assertion. `glib/gmessages.h:323-325` defaults the domain to NULL;
`glib/gmacros.h:932-939` selects `__PRETTY_FUNCTION__` for this C++ build.
GLib's own array checks use its `GLib` domain (`glib/meson.build:421`) and C function spelling.
These are real diagnostics even though the pin source contains no explicit
`g_log` call. Pin `storage/tag_utility.cpp:137,202,211,239,246,256` is the
separate G16 tool-only macro/warning surface; no facade claim is inferred
from it. Raw file-I/O stderr belongs to G12/#929 and is not a `g_log`
substitute.


The warning entry-point inventory (**MUST RETAIN**, no per-trigger closure
implied) is:

| Adapter path | Exported operations reaching the warning channel |
| --- | --- |
| Both `context.rs` | P/Z `init`, `save`. |
| Both `config.rs`; pinyin `state.rs::load_addon`/`unload_addon` | P `set_full_pinyin_scheme`, `set_double_pinyin_scheme`, `set_zhuyin_scheme`; Z `set_full_pinyin_scheme`, `set_chewing_scheme`; P/Z `load_phrase_library`, `unload_phrase_library`, `mask_out`; P `load_addon_phrase_library`, `unload_addon_phrase_library`. |
| Both `cursor.rs` | P `get_pinyin_key`, `get_pinyin_key_rest`, `get_left_pinyin_offset`, `get_right_pinyin_offset`; Z `get_zhuyin_key`, `get_zhuyin_key_rest`, `get_left_zhuyin_offset`, `get_right_zhuyin_offset`. |
| Both `keys.rs` | P/Z `parse_full_pinyin`; P `get_pinyin_is_incomplete`. |
| Both `sentence.rs`, `phrase.rs` | P/Z `guess_sentence`, `guess_sentence_with_prefix`, `get_character_offset`, `phrase_segment`; P `get_sentence`, `guess_candidates`; Z `guess_candidates_before_cursor`, `guess_candidates_after_cursor`. `refuse_lookup_gram` and offset/tone validation are shared callers, not extra exported channels. |
| Both `candidates.rs` | P/Z `train`; P `get_candidate_nbest_index`, `remove_user_candidate`, `choose_candidate`, `choose_predicted_candidate`. |
| Both `iterators.rs` | P/Z `iterator_add_phrase`; P `begin_get_phrases`, `iterator_get_next_phrase`, `begin_get_bigram_phrases`, `bigram_iterator_get_next_phrase`. |
| P `predict.rs` | P `guess_predicted_candidates`, `guess_predicted_candidates_with_punctuations`. |
| P `text.rs::render_shared` | P `get_full_pinyin_auxiliary_text`, `get_double_pinyin_auxiliary_text`, `get_chewing_auxiliary_text`. |

These operations' existing pin-assertion citations and individual exception
obligations remain in the compatibility-policy register; merely forwarding
the same GLib channel proves neither the trigger nor the return/out state.
The matrix deliberately distinguishes this complete adapter-channel inventory
from the unfinished per-trigger G8 investigation.

#### Source closure of diagnostic producers

The six literal facade-reachable pin `g_return_val_if_fail` sites are the
two segment checks and four `SubPhraseIndex::load` checks above. The latter
first delete the old owning chunk, read total/offsets, test three separators,
install views, then test the end (`storage/phrase_index.cpp:335-361`).
`FacadePhraseIndex::load` subtracts the previous total first and adds the
new total only on success (`:237-248`). A fresh sub-index starts with empty
views (`storage/phrase_index.h:281-283`); a first-three-check refusal retains
the newly read local total but installs no views. This is not universally
an empty-object refusal: reloading a populated object deletes its old chunk
before checking the replacement, leaving stale views on an early refusal.
Subsequent reads through those views are unsafe, not a deterministic output
contract to copy into Rust.

The exported load/init callers ignore the inner false on successfully read
files and can return success (`pinyin.cpp:232-323,370-394,477-495`;
`zhuyin.cpp:176-267`). Save's diff uses a fresh baseline and ignores its load
false (`storage/phrase_index.cpp:270-283`; `pinyin.cpp:963`;
`zhuyin.cpp:596`), so a critical is compatible with successful save and
partial file writes. Mask reload also ignores false before merge/compact
(`pinyin.cpp:1224-1295`; `zhuyin.cpp:759-830`); a fresh failed library and a
previously populated library must not be conflated. The fourth check
`index_three <= end` cannot fail on these callers' `[0, chunk->size())`
bounds without the preceding `buf_begin + index_three - 1` separator read
already leaving the payload. `include/memory_chunk.h:499-520` checks mapped
payload length. There is no additional safe fourth-check fixture to invent.
The existing second/third-separator init witnesses passed for both facades
(four MATCH); this is bounded runtime coverage, not a run of unsafe reloads.
Since maintainer decision D1, oxpinyin writes none of these criticals
([1297-layout-criticals](1297-layout-criticals.md)).

Indirect GLib checks were also traced. `g_utf8_strlen(NULL, -1)` emits a
GLib-domain critical and returns zero (`glib/gutf8.c:237-242`), but segment
then passes NULL to `g_utf8_to_ucs4`, whose `:950-952` dereference is unsafe:
there is no defined false-return/out-state promise for that input. A NULL
`g_strjoinv` vector guard (`glib/gstrfuncs.c:2646`) cannot arise from the
normal terminated scratch vectors: their construction allocations are
fatal. The separately recorded unterminated bigram vector remains an
unsafe over-read, not a NULL-vector guard. `g_vasprintf`'s NULL output-address
guard (`glib/gprintf.c:330`) is unreachable through `g_strdup_vprintf`,
which supplies a local pointer address (`glib/gstrfuncs.c:510-517`). Internal
array pointers are successfully constructed or allocation terminates; the
four exported caller-array NULL paths are the reachable checks tested above.
GLib invokes the handler before enforcing the fatal mask
(`glib/gmessages.c:1274-1313`), so under fatal criticals the pin dies of
SIGTRAP at the segment critical; oxpinyin does not (D1, POLICY-ACCEPTED).

Diagnostic NUL reachability is closed for the C-facing producers, not for
arbitrary Rust calls to `log_warning(&str)`. Literal messages,
exported-operation names and assertion expressions are NUL-free;
`TableConfError` warnings and `ChunkHeaderWrite` expressions are static.
The dynamic init message in both `context.rs` formats a typed `OpenError`
and a debug-formatted system path. Input paths come through CStr;
configured filenames stop at the first NUL. Dictionary/layout errors
format fixed text or numeric bounds, not raw stored phrase bytes. Backend
messages terminate at CStr. User-profile open errors either become fixed
typed failures or go to raw stderr and degrade the store, not arbitrary
GLib warning strings. Thus the hypothetical drop-whole-message versus `%s`
truncation has no current exported trigger. Stored-phrase NUL remains G1's
separate returned-text contract. No helper-only artificial input establishes
an outward discrepancy.

#### Allocation-failure partitions

GLib fatal allocation and fallible conversion are different contracts.
`g_malloc`/`g_realloc` call fatal `g_error` (`glib/gmem.c:95-106,164-177`),
whereas conversion's `try_malloc_n` uses the nonfatal allocator and returns
NULL (`glib/gutf8.c:905-912,977-990,1034-1068`). All these pin conversions
pass NULL error pointers: conversion OOM itself emits no diagnostic and
leaves `items_written` untouched. The reference GLib build defines
`GLIB_USING_SYSTEM_PRINTF`; its `g_vasprintf` object calls system vasprintf
and aborts on ENOMEM, matching `glib/gprintf.c:339-357`: minimal raw stderr,
not `g_log`. Other printf failures set the output NULL.

| Allocation boundary / exported operations | Conditional pin result, return/out-state/diagnostic | Subject and disposition |
| --- | --- | --- |
| Real caller arrays, all four operations | GLib growth failure terminates through fatal allocation; no normal return. | Actual GLib growth retained. Rust payload preparation may terminate earlier. MUST RETAIN ABI; identical allocation graphs are not claimed. |
| P/Z key renderers, P dual pinyin strings and auxiliary text | Duplication/concatenation OOM is fatal; formatted renderer ENOMEM aborts with raw stderr as above. | The pin's `g_strdup`/`g_strdup_printf`/`g_strconcat` (#1227): the same fatal boundary. Earlier Rust allocations can terminate first. |
| P/Z keyboard vector | GLib vector/string allocation OOM is fatal, no returned partial vector. | `ffi::symbol_vector`, the pin's `g_ptr_array_*` and `g_strdup` (#1227): the same fatal boundary. Earlier Rust allocation can terminate. |
| P/Z `token_get_phrase` | Conversion allocation failure writes declared length and NULL text, returns true, no warning (`pinyin.cpp:1627-1645`; `zhuyin.cpp:1096-1113`). | Both `dict.rs` getters write the length and NULL and return true, as the pin does. |
| P/Z `get_sentence` | Each token conversion is fallible; a NULL conversion can leave NULL before a prefix exists or preserve an existing prefix, omitting that token. Later dup/concat allocation can terminate; converter otherwise returns true (`lookup/lookup.cpp:27-69`). No conversion warning. | `ffi::sentence_text`: the fallible encoder per phrase, then the pin's `g_strdup`/`g_strconcat`/`g_free` chain, `true` even when NULL ([record](returned-string-allocation.md)). |
| P unigram iterator | Phrase conversion can yield NULL; if later fatal-allocation stages succeed, row still writes reading/count and returns true (`pinyin.cpp:698-768`). Reading allocation OOM terminates. | Independent final allocations may yield NULL phrase and/or reading while returning true and advancing. No partial-allocation rollback is promised by either interface; reading OOM differs. |
| P bigram iterator | Internal pair conversion can yield NULL first phrase (`g_strconcat` returns NULL) or omit a NULL second phrase; later returned-string duplication OOM is fatal (`pinyin.cpp:822-836,896-910`; `glib/gstrfuncs.c:576-578`). | `BigramHandle::convert_loaded_pair` uses the fallible encoder and `ffi::strconcat_pair`; the returned copies are the pin's `g_strdup`. Internal unsafe unterminated vector remains its existing exception; no new OOM admission. |
| P/Z `phrase_segment` temporary conversion | Positive nonempty input with failed conversion leaves length zero: critical (not written by oxpinyin, D1), false, old result retained. Empty input has zero equal lengths and proceeds to empty successful result, if subsequent fatal allocations succeed (`lookup/phrase_lookup.cpp:36-52,129-157,382-433`). | Rust conversion allocation can terminate; no identical-site fallible conversion exists. #918 owns converter, not this audit. |
| P/Z `iterator_add_phrase`, `remember_user_input`; P `get_character_offset` | Failed conversion leaves initialized length zero; return false before adding a phrase/remembering/writing offset (P `:628-646,3678-3681,3210-3213`; Z `:513-532,2165-2168`). Parser allocation after add-phrase conversion can terminate first. | Rust earlier allocations can terminate. Preserve ordinary return/output contracts, no same-pressure equivalence inferred. |
| P/Z `lookup_tokens` temporary conversion | Zero initialized length on conversion failure searches zero-length input, then clears output and returns false (G1 closure; P `:2652-2667`, Z `:1678-1693`). | Caller-array ownership is independent of this converter partition. |
| P/Z `guess_sentence_with_prefix` | Failed conversion skips prefix token additions, continues lookup with sentence_start and returns lookup result, no conversion warning. P frees prior prefix, stores NULL but leaves prior prefix length unchanged on failure; Z uses local zero length and frees temporary (P `:1389-1443`, Z `:912-962`). | Rust allocation can terminate before this fallback; no attempt to imitate C allocation staging in Rust. |
| Context/instance/iterator handles | Opaque C++ new allocation failure has no successful handle transfer. | Rust Box is appropriate; handle destructors retain ownership. Resource pressure thresholds need not match. |

`malloc_cstr_with` frees and refuses a failed fill or embedded NUL. A
checked-size overflow returns NULL before allocation; realistic exported
payload bounds do not license inventing a reachable usize overflow.

#### Remaining scope and verification

Broader G8 registered unsafe/assertion refusals, borrowed/static-slot
identity and malformed backing-item text retain their individual records;
the channel inventory is not a per-trigger certification of them. No
difference has been proved impossible or unsafe to reproduce at a safe
C-adapter boundary, so none is admitted in `upstream-divergences.md`.

The `GArray` contract is held by the `glib-924-array-lifecycle-*` cases
and the capi `boundary_tests` named below: a seeded clear callback, 257
appended caller elements, shrink, a refusing lookup, clearing and zero
termination, zero-fill growth, `g_array_free(FALSE)` and `g_free`. The
allocator consumers mutate or reallocate every owned string and replace or
append vector elements before `g_strfreev`. The gates are `just abi
alloc-pairing` (26 pinyin + 12 zhuyin slots, with LeakSanitizer negative
controls) and `just abi exports` (79 + 52 symbols).

### #924 Allocation-failure injection follow-through — 2026-10-25 UTC

The Linux C-consumer harness and its use (`just abi alloc-failure`) are in
[`tools/abi/alloc-failure.md`](../../tools/abi/alloc-failure.md).
Source expectations were read before the fault-injected pin comparisons.

GLib `glib/gmem.c:95-106` calls malloc and invokes fatal `g_error` on a
positive-size failure. `:164-177` does the same for realloc.
`glib/garray.c:605-625,1071-1100` grows a real GArray through that realloc;
`glib/gstrfuncs.c:315-329` duplicates through `g_new`/g_malloc. Structured
`g_error` expands through `glib/gmessages.h:328-334`; the writer is invoked
before fatal termination (`glib/gmessages.c:1874-1910`). Legacy logging also
calls its handler before fatal termination (`:1274-1313`). The harness
observes both channels, because a legacy handler alone misses structured
GLib allocation errors. Error level is 4, unlike precondition critical 8.

The HSU mapping is source-defined (`storage/zhuyin_table.h:209-232`), with
ordered strdup/append of both entries in
`storage/zhuyin_parser2.cpp:496-544`. The regular single-symbol keyboard
path has the same fatal allocation contract (`:301-332`); Dachen CP26 is
`:798-851`. Pin vector capacity growth also goes through fatal g_realloc
(`glib/garray.c:1890-1916`). oxpinyin's `ffi::symbol_vector` makes the
same GLib calls (#1227).

The matrix separates the final foreign allocation from earlier Rust String,
CString and Vec allocations. Ordinary infallible Rust allocations may
terminate before the final transfer is attempted; this harness does not
inject those earlier allocations or claim identical allocation graphs.
A same-resource-pressure comparison is not established.

| Exported caller(s) | Pin source and conditional result | Subject final-allocation result / classification |
| --- | --- | --- |
| P `get_pinyin_string`, `get_zhuyin_string` | `pinyin.cpp:2707-2726`; `storage/chewing_key.cpp:47-57,74-88`: g_strdup failure is fatal; formatted-tone path uses printf (see limit below). | `cursor.rs` allocates with `ffi::key_text`, the pin's `g_strdup` calls (#1227, [record](returned-string-allocation.md)); the silent-NULL **defect** is closed. Zhuyin string with first tone tested. |
| P `get_luoma_pinyin_string`, `get_secondary_zhuyin_string` | `pinyin.cpp:2729-2748`; `storage/chewing_key.cpp:90-114`: fatal duplicate/formatted allocation. | `keys.rs::display_string_getter` allocates with `ffi::key_text`, the pin's calls (#1227); the silent-NULL **defect** is closed. |
| Z `get_zhuyin_string`, `get_pinyin_string` | `zhuyin.cpp:1732-1765`; same ChewingKey renderers. | `keys.rs::display_string_getter` allocates with `ffi::key_text` (#1227); the silent-NULL **defect** is closed. First-tone zhuyin rendering tested. |
| P `get_pinyin_strings` | `pinyin.cpp:2751-2762`; `storage/chewing_key.cpp:60-71`: each requested output is independently duplicated; allocation failure terminates, potentially after writing the first output. | `cursor.rs::pinyin_get_pinyin_strings` calls `ffi::strdup` per requested half (#1227); the **defect** is closed. |
| P three auxiliary-text getters | `pinyin.cpp:3381-3575`: dup/concat allocation fatal, including allocated empty string on false. | `text.rs::write_aux` calls `ffi::strdup` / `ffi::strconcat_aux` (#1227); the **defect** is closed. |
| P/Z `in_chewing_keyboard` | `pinyin.cpp:1615-1624`, `zhuyin.cpp:1087-1093`; parser citations above: positive malloc/realloc failure terminates, no normal partial vector return. | `parse.rs` allocates with `ffi::symbol_vector` (#1227, [record](returned-string-allocation.md)); the silent-NULL **defect** is closed. Vector, first string and second string failures tested. |
| P/Z `token_get_phrase` | `pinyin.cpp:1627-1645,2776-2783`, `zhuyin.cpp:1096-1113,1768-1775`: fallible UCS-4 conversion failure can write length + NULL and still return true, no conversion diagnostic. | Both `dict.rs` getters write the allocation result, length + NULL included, and return true, even when it is NULL, as the pin does. **Same nonfatal outcome.** Source-only here. |
| P/Z `get_sentence` | `lookup/lookup.cpp:27-69`: per-item fallible conversion, then fatal duplication/concatenation; missing converted item may leave NULL or preserve an existing prefix. P `:1463-1481`, Z `:982-999`. | `ffi::sentence_text` makes the pin's per-phrase chain (#1227); the **defect** is closed. |
| P `bigram_iterator_get_next_phrase` | `pinyin.cpp:896-910`: final two g_strdup calls fatal; last-row false still follows writes. | `ffi::strdup_raw` / `ffi::strdup` per row, the pin's `g_strdup` (#1227); the **defect** is closed. |
| P/Z `lookup_tokens`, `token_get_nth_pronunciation` GArray growth | `pinyin.cpp:2652-2667,2801-2818`; `zhuyin.cpp:1678-1693,1793-1810`; `storage/phrase_large_table3.h:77-97`: GLib owns real caller-array growth; failure terminates via GLib error. | Both dict adapters invoke real GLib growth. **Same fatal boundary**, tested through nth pronunciation on an empty real array. Lookup's additional scratch/payload allocations are not injected. |

Earlier Rust `String`/`CString` preparation allocations come before the
final GLib allocation. Interior-NUL refusal is independent of OOM; existing
malformed-text records retain their scope. `malloc_cstr_with` serves the
`g_ucs4_to_utf8` sites (the P unigram phrase, `*_token_get_phrase`, the
bigram and sentence conversions): fallible, silent, NULL on failure, as the
pin's `g_try_malloc_n` conversion. Pin `pinyin.cpp:698-768` can return true
with a NULL phrase from conversion, while reading allocation is fatal.

The GLib conversion allocator is explicitly nonfatal:
`glib/gutf8.c:905-912,977-990,1034-1068` uses try_malloc_n and returns NULL;
the pin supplies NULL GError pointers, so the failed allocation itself does
not log. Do not classify these conversion failures as g_malloc aborts.
The reference image's g_vasprintf build follows
`glib/gprintf.c:339-357` (ENOMEM abort with minimal raw stderr); `just abi
alloc-failure` fails `vasprintf`'s malloc on both sides.

## Results and remaining scope

`just abi alloc-failure` expects GLib's `g_error` then SIGTRAP or SIGABRT
on both sides for every injected GLib allocation, and the `gprintf.c`
abort for `vasprintf` ([harness](../../tools/abi/alloc-failure.md)). A
failed `GArray` growth has no contract case: both facades grow a caller's
array only through GLib's `g_array_set_size` and `g_array_append_vals`, so
the failure happens inside GLib's `g_realloc` on both sides.
`oxpinyin-capi`'s `boundary_tests::lookup_tokens_resets_the_callers_array`
and `oxpinyin-zhuyin-capi`'s
`boundary_tests::caller_arrays_keep_their_metadata_and_clear_through_the_callback`
cover oxpinyin's part, returning a real GLib-owned `GArray`.

Remaining: earlier Rust allocations, and individual runtime coverage of
the source-only caller rows. No divergence is admitted.

## G10 — Rename and sync

Audit #929; pin source read from a checkout of
`074a2219c90feaf962d0d24f034514033ece5f99`. The C-facing path is
`pinyin_save` / `zhuyin_save` → `facade::ContextCore::save_user` →
`UserStore::save_reporting_with_baseline` → `persistence::save_reporting`.
The reporting save is the only save.
`UserConfLaw` selects staged pinyin versus interleaved zhuyin writes.
Paths in this section are relative to the identified repository root
(oxpinyin, pinned libpinyin, or the named GLib tag); installed reference-image
files use absolute paths rather than repository paths.

| Path / side effect | Pin citation at 074a2219 | Mandatory result and subject audit |
| --- | --- | --- |
| No user directory or clean context | `src/pinyin.cpp:1132-1137`; `src/zhuyin.cpp:547-552` | Return false before compaction, staging or marker write. A clean retry after a soft save failure does not repair the failed file. `UserStore::save_reporting_inner` retains these guards. |
| Loaded default libraries, ascending index 1..15, skipping absent, NOT_USED and NULL user names | `src/pinyin.cpp:922-994,1025-1086`; `src/zhuyin.cpp:559-644` | Exact library order is mandatory; SYSTEM_FILE/DICTIONARY writes a diff against the currently mapped system chunk, USER_FILE writes a chunk. `libraries_in_pin_order`, `stage_rest` and the baseline hook implement this. No filename policy change (#922). |
| Pinyin `.tmp` staging and rename pass | `src/pinyin.cpp:971,988,996-1022,1059,1078,1094,1108,1121` | Stage **all** selected libraries, user pinyin index, user phrase index, user bigram, then rename in that same order. `pin_rename_order`/`save_reporting` implement the pass. A failure never licenses reordering later files. |
| Zhuyin `.tmp` staging and immediate renames | `src/zhuyin.cpp:607-609,633-635,646-690` | Write then rename each library, then write/rename pinyin index, phrase index and bigram individually. `Staged::immediate` preserves this difference. |
| Chunk open, header, payload, sync and close | `src/include/memory_chunk.h:536-557`; callers `src/pinyin.cpp:971,988`, `src/zhuyin.cpp:607,633` | Open failure and payload short/error returns are ignored by the caller. Each failed four-byte header write asserts; the assert is reproduced as an abort (row 77). Successful payload is followed by `g_fsync`, then close; both results ignored. `write_chunk_file` calls `oxpinyin-input` `chunk::save`, the pin's `open`/`write`/`fsync`/`close` sequence (#1221, external-I/O stages 2 and 3). Successful write/rename alone does not establish durability. |
| Staged DBM indexes | `src/storage/facade_chewing_table2.h:93-96`, `src/storage/facade_phrase_table3.h:92-95`; `src/storage/chewing_large_table2_bdb.cpp:135-161`, `src/storage/phrase_large_table3_bdb.cpp:150-176`; KC `src/storage/chewing_large_table2_kyotodb.cpp:124-145`, `src/storage/phrase_large_table3_kyotodb.cpp:139-160`; tkrzw `src/storage/chewing_large_table2_tkrzwdb.cpp:111-129`, `src/storage/phrase_large_table3_tkrzwdb.cpp:102-120` | Unlink stale staging, create backend-native output, copy records, finish backend save before rename. KC active code dumps snapshots (its explicit synchronize/close branch is `#if 0`); tkrzw synchronizes with `false` then closes. BDB invokes the temporary DB's sync function with `m_db`, then closes the temporary DB. Do not silently describe that as a temporary-file fsync. Same-backend format/record bytes remain mandatory; backend error paths require separate verification. |
| Staged bigram | `src/storage/ngram_bdb.cpp:81-110`; `src/storage/ngram_kyotodb.cpp:82-113`; `src/storage/ngram_tkrzwdb.cpp:65-83`; facade calls `src/pinyin.cpp:1014-1022`, `src/zhuyin.cpp:680-690` | Last DBM, before marker. BDB has the same sync receiver detail; KC dumps a snapshot; tkrzw HashDBM synchronizes with `false` and closes. Backend failures do not prevent the facade's attempted rename. Native save/reload checks retained counts; this is not a whole-file byte-identity proof. |
| Failed rename, including a directory at the final name | `src/pinyin.cpp:1059-1129`; `src/zhuyin.cpp:609-638,655-690` | Retain failed staging and destination; attempt later files, print `rename ... to ... failed.` without errno text, then marker and dirty reset. `save_reporting` collects failures; `facade::ContextCore::save_user` prints them after persistence returns; interleaving with backend diagnostics remains an ordering follow-up. Native tests block every destination and assert ordered failures and sentinel preservation; contract cases cover chunk and all three DBM finals on both facades. |
| Marker after attempted renames | `src/pinyin.cpp:220-232,1141-1146`; `src/zhuyin.cpp:164-176,695-698`; `src/storage/table_info.cpp:377-396` | In-place `user.conf` rewrite, not a `.tmp` commit record. Pinyin retains its context counter; zhuyin writes fresh counter zero. Open failure prints `write ... failed.`; caller still clears dirty and returns true. fprintf/fclose failures are unchecked upstream. Subject `write_marker` writes through the pin's `fopen`/`fprintf`/`fclose` (`oxpinyin-input` `config.rs`); write-error diagnostic equivalence after a successful open remains unverified. Parsing is #920. |
| Fini with or without save | `src/pinyin.cpp:1194-1200`; `src/zhuyin.cpp:741-757` | Neither saves dirty dictionaries. Pinyin lowers counter (`>1 ? counter-1 : 0`) and writes marker; zhuyin only releases state. `FiniGuard`/`persistence::fini` follow this. Native and contract round trips include loss of an intentionally unsaved suffix, not loss of committed data. |
| Restart after interruption | `src/pinyin.cpp:173-217,922-1146`; `src/zhuyin.cpp:126-162,547-698`; `src/storage/table_info.cpp:399-412` | Load finals, never replay `.tmp`; conformance/counter can trigger cleanup. No multi-file atomicity: pinyin before rename retains old finals; either facade during renames can leave mixed generations, zhuyin even before all staging completes. Native children cover the earlier-log prefix and the later addon/network chunk prefix. Seven C-facade cases cover save marker-open, after save, after fini, and pinyin fini marker-open; all completed data survives restart. Individual DBM/rename interiors remain open. No power-cut durability claim. |
| Same-backend bytes and no data loss on valid completed saves | `src/pinyin.cpp:963-1018`; `src/zhuyin.cpp:596-685`; DBM writers cited above | Mandatory: encoded chunk/diff content, indexed records, bigram values and reload results. Native round trips also compare every data file byte-for-byte before fini and after conforming reopen (excluding only the lifecycle marker). An authorized pin-only repeat control demonstrates variable BDB file IDs and unused-page residue, not changed records. Two replacement C-facade logical cases compare every key/value byte and reciprocal cross-open/re-save results. Detailed classification is below; interior crash-order coverage remains open. Do not mask an unexplained byte difference as a filesystem quirk. |
| GLib/platform boundary and errno wording | Call sites `src/pinyin.cpp:1059-1124`, `src/zhuyin.cpp:609-690`, `src/include/memory_chunk.h:555`; GLib tag `2.84.4`, `glib/gstdio.h:60-84`, `glib/gstdio.c:99-140,1157-1198,1684-1702`; glibc tag `glibc-2.41`, `sysdeps/unix/sysv/linux/rename.c:25-35`, `sysdeps/unix/sysv/linux/fsync.c:24-28` | Source trace completed below for Unix macro selection, Windows wrapper behavior and the Linux libc `rename`/`fsync` wrappers. Rename diagnostics at these callers contain no errno text; syscall/backend diagnostic equivalence and non-Linux execution remain open. No new platform exception is admitted. |

Source follow-through (recorded evidence): GLib tag [`2.84.4`](https://github.com/GNOME/glib/tree/2.84.4), commit
`41eca60845d3fc309af361f5e7f801ba339099aa`, the upstream version of the
`libglib2.0-0t64` package **2.84.4-3~deb13u5** that this reading used,
read through the authorized GitHub mirror, not by executing the pin. The
trixie source package `glib2.0_2.84.4-3~deb13u5` (snapshot.debian.org; archives
checked against the `.dsc` SHA-256 entries) carries `glib/gstdio.c` and
`glib/gstdio.h` byte-identical to the tag, and no patch in its
`debian/patches` modifies either file; two patches only show
`#include <glib/gstdio.h>` as context. The trixie `libgio-2.0-dev`
2.84.4-3~deb13u5 amd64 package, which ships the GLib headers, installs
`/usr/include/glib-2.0/glib/gstdio.h` byte-identical to the tag and defines
`G_OS_UNIX` at `/usr/lib/x86_64-linux-gnu/glib-2.0/include/glibconfig.h:120`.
Windows branches below are the upstream tag's source, not a tested Debian
Windows build.

| GLib source path at tag 2.84.4 | Source-derived result and scope |
| --- | --- |
| `glib/gstdio.h:60-84` (`g_rename` at :75, `g_fsync` at :82); reference `/usr/lib/x86_64-linux-gnu/glib-2.0/include/glibconfig.h:120` | Ordinary Unix callers get macros: `g_rename` is `rename`, `g_fsync` is `fsync`. The pin's chunk call has no GLib EINTR retry or full-sync upgrade. The tkrzw linux/amd64 pin objects import `fsync@plt` in `MemoryChunk::save` (cited in `oxpinyin-input` `native.rs`). |
| `glib/gstdio.c:26,1157-1198` | GLib enables its own wrapper definitions internally. The non-Windows rename wrapper simply calls `rename` (:1196); the Windows wrapper (:1161-1194) converts both filenames to UTF-16, refuses failed conversion with EINVAL, and calls `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` (:1182). Replacement of an existing final is a required save effect, not a dispensable C-runtime quirk. Filename conversion policy remains #922. |
| `glib/gstdio.c:99-140,1182-1194` | Windows failure maps `GetLastError` to errno: sharing/access denial to EACCES, existing destination to EEXIST, missing path/file to ENOENT, cross-device to EXDEV, default to EIO; the map also covers handle, parameter and memory errors. Success sets errno to zero. These are integer mappings, not formatted messages. Pin rename callers test only zero/nonzero and emit the fixed path diagnostic (`src/pinyin.cpp:1059-1124`, `src/zhuyin.cpp:609-690`); they do not call strerror or format errno. Backend-generated diagnostics remain separate open coverage. No Windows errno-text exception is needed for these caller messages. |
| `glib/gstdio.c:1684-1702` | The callable wrapper uses `_commit` on Windows (:1688). Its Unix implementation uses `F_FULLFSYNC` when configured, otherwise fsync, and retries EINTR (:1689-1698); without either facility it returns zero (:1700). **These Unix wrapper branches are not the reference pin's macro-expanded call.** The Unix header also prevents attributing the wrapper's macOS stronger-sync branch to ordinary pin callers. oxpinyin calls `fsync` as the pin's macro does. |
| Pin `src/include/memory_chunk.h:549-557`, `src/pinyin.cpp:971,988`, `src/zhuyin.cpp:607,633` | After a complete payload write, invoke sync, ignore its result, close, ignore that result, return true. Callers ignore the chunk save return and continue according to each facade's ordering. oxpinyin's `chunk::save` does the same. |

Recorded evidence: the libc of this reading is `libc6` **2.41-12+deb13u4**.
Source read at the authorized
`bminor/glibc` tag `glibc-2.41`, commit
`74f59e9271cbb4071671e5a474e7d4f1622b186f`. The trixie source package
`glibc_2.41-12+deb13u4` (snapshot.debian.org; archives checked against the
`.dsc` SHA-256 entries) has an orig tarball whose copies of every file cited
below are byte-identical to the tag. Of its 81 patches, only the stable-branch
`debian/patches/git-updates.diff` touches one of them: in
`nptl/cancellation.c` it widens `__syscall_cancel`'s local `r` from `int` to
`long int`, which does not change `fsync`'s 0 or −1 result. The same patch
narrows the SIGCANCEL handler's asynchronous-cancellation test in
`nptl/pthread_cancel.c`, which matters only to a thread being cancelled.
Linux x86_64:

| glibc source at tag `glibc-2.41` | Source-derived result |
| --- | --- |
| `sysdeps/unix/sysv/linux/rename.c:25-35`; `sysdeps/unix/sysv/linux/x86_64/64/arch-syscall.h:253`; `sysdeps/unix/make-syscalls.sh:45-70`; `sysdeps/unix/syscalls.list:55`; `sysdeps/unix/sysv/linux/sysdep.h:42-48` | `rename` is a C wrapper. x86_64 defines `__NR_rename` (82), so it issues `INLINE_SYSCALL_CALL (rename, old, new)` (:29), not `renameat` or `renameat2`. The generic `syscalls.list` template entry is dropped because the higher-priority Linux directory supplies `rename.c`. `INLINE_SYSCALL` returns the kernel result, or −1 with `errno` set to the negated result. There is no EINTR retry, and `rename` is not a cancellation point. |
| `sysdeps/unix/sysv/linux/fsync.c:24-28`; `sysdeps/unix/sysdep.h:202-206,243-253`; `nptl/cancellation.c:25-80` | `fsync` is `SYSCALL_CANCEL (fsync, fd)`, which in libc (not rtld) calls `__syscall_cancel`, so `fsync` is a cancellation point. In a single-threaded process, or with cancellation disabled or exiting, the syscall is issued directly (:37-45); otherwise it goes through `__syscall_cancel_arch`, and the thread is cancelled only when the syscall returns EINTR with cancellation enabled and pending (:52-61). Failure returns −1 with `errno` set (:69-80). EINTR is never retried. |

Consequence for G10: on the reference, neither GLib's macro (`g_fsync` is
`fsync`) nor libc retries an interrupted `fsync`, and `rename` is one syscall
with no retry. The pin ignores the sync result (`src/include/memory_chunk.h:555`),
so an EINTR or other sync failure is silent at the pin, and its rename callers
print a fixed message without libc error text. This admits no libc
error-string rule. Kernel and filesystem semantics beneath these wrappers,
backend error strings and Windows/macOS execution remain open.

Coverage limits (all native tests below run both facade laws):

| Requested failure/restart coverage | Existing test and exact scope | Still missing / reason |
| --- | --- | --- |
| Failed rename | `failed_renames_report_exact_order_and_preserve_destinations`: blocks every final with a sentinel-bearing directory; checks ordered failures, retained `.tmp`, marker success and clean retry. Eight `save-929-rename-*` cases cover four individual final roles through both C facades. | Error combinations with backend diagnostics and interruption at each individual rename remain unverified; ordered error lists do not establish all syscall/diagnostic interleavings. |
| Failed sync | `sync_rejecting_chunk_target_does_not_stop_later_files` and two `save-929-sync-rejecting-*` cases use a chunk staging symlink to `/dev/null`, verify the target rejects sync, then verify save continuation. | These are outward-result tests, not injection into the subject's sync. EIO/ENOSPC/EINTR, backend sync/close failures and power-loss durability remain untested. |
| Interrupted save | `interrupted_save_preserves_the_facade_specific_committed_prefix`: child process exits with code 73 without unwinding before library 2, 3 and 4; parent waits for completion, checks every earlier log and every reloaded library delta. | Later coverage is in `crates/oxpinyin-user/tests/save_boundaries_929.rs` and seven `save-929-boundary-*` C-facade cases, detailed below. Interruption inside index/bigram writes, each individual rename, marker writes after open, and power cuts remain open. |
| Save/reload | `save_reload_keeps_bigrams_and_unsaved_changes_do_not_replace_them`: saved count 37 survives an unsaved overwrite to 91; two reopens retain all data-file bytes. Two `save-929-reload-*` cases compare imported phrase tokens and normalized file content across saved/unsaved suffixes. | The C helper normalizes chunk-header bytes and DBM records. Physical bytes are not a valid criterion (below); the logical-record and reciprocal cross-open cases replace them, with no masks or accepted exceptions. |

No necessary language/runtime exception is established, so the divergence
register has no G10 entry. Rust expresses per-file staging, replacement,
explicit sync and ignored error results naturally; there is no reason to
imitate GLib containers or C ownership mechanisms here.

Tests: `crates/oxpinyin-user/tests/save_semantics_929.rs` exercises both
facade laws. The `/dev/null` sync-rejection case proves only the outward
continuation result. The contract cases are the `save-929-*` family
(recorded evidence: all MATCH on bdb, and a subject mutation that retained
dirty after `UserStore::save_reporting_inner` made 10 DIFFER).

### Later save/fini boundaries and physical bytes

`crates/oxpinyin-user/tests/save_boundaries_929.rs` holds two tests/six
child exits under both facade laws: after addon/network chunks in a configured
profile, after completed save, and after fini. The configured late system row
uses the pin's independent row-name/type parsing
(`src/storage/table_info.cpp:145-156,266-286`) and ascending save walk
(`src/pinyin.cpp:927-994`; `src/zhuyin.cpp:560-644`). Restart checks old indexes
and bigram after the chunk interruption, committed phrases and bigram after
completed saves, and exact preservation of all data bytes across reopen.

The C-facade cases for these boundaries (recorded evidence):

| New case family | Source-derived boundary | BDB result |
| --- | --- | --- |
| `save-929-boundary-save-marker-{pinyin,zhuyin}` | Data renames precede marker open (`src/pinyin.cpp:1141-1143`; `src/zhuyin.cpp:695`; `src/storage/table_info.cpp:377-396`). A Linux read lease blocks the marker's write-open; its SIGIO notification tells the parent to kill and reap the saving child before releasing the lease. The marker stays intact; all committed phrase data survives restart with no `.tmp` files. | 2 MATCH |
| `save-929-boundary-fini-marker-pinyin` | Save completes before acquiring the lease. Import another unsaved phrase, then interrupt fini at marker write-open (`src/pinyin.cpp:1194-1200`). The committed prefix survives; the unsaved phrase does not. Zhuyin has no fini marker open (`src/zhuyin.cpp:741-757`), so no fictitious zhuyin marker boundary is tested. | 1 MATCH |
| `save-929-boundary-{after-save,after-fini}-{pinyin,zhuyin}` | Child exits without unwinding after the named C call. Both finies omit dictionary save; pinyin lowers the marker counter (`src/pinyin.cpp:1194-1200`), zhuyin does not write it (`src/zhuyin.cpp:741-757`). Reopen retains two saved tokens, loses the unsaved third, and preserves every data byte. | 4 MATCH |
| `save-929-physical-bytes-{pinyin,zhuyin}` | Identical fresh profiles, two identical phrase imports and save, then an unsaved import, fini and reopen. Observe each complete file as length plus hex bytes, including checksum, DBM metadata and marker. `MemoryChunk::save` writes length/checksum/payload (`src/include/memory_chunk.h:536-557`); bytes 4–7 are a checksum, **not a timestamp** (`:136-159,545-547`). | Retired: 2 DIFFER, and the pin differs from itself (below); replaced by the logical cross-open cases |

### Physical-byte classification and corrected criterion

A maintainer-authorized pin-only repeat control ran the
`save-929-physical-bytes-{pinyin,zhuyin}` scenario with the pin DSO in both
runner positions, from independent, equivalent fresh profiles. All three
BDB containers differ from the pin's own second output on both facades.
Chunks, diff logs, marker contents, lengths and recovered tokens agree.

The observed pin/subject byte differences are classified below. The repeat
control varies individual bytes within those same fields (including offset
65 in some IDs); the table describes observed regions, **not comparison
masks**.

| File / exact earlier differing offsets | Demonstrated cause; no logical-record change |
| --- | --- |
| All three containers: 52, 60–64 | Metadata UID occupies 52–71. Its inode bytes are 52–55, randomized ID 60–63 and process-seeded serial 64–67. The files are independently created. |
| `user_phrase_index.bin` (8192 bytes): 4148, 4156–4160 | Root page starts at 4096. The metadata buffer is reused for that page, retaining the UID at relative 52–71 in unused space. The observed page has 8 index entries and free space `[42,4032)`; all differing bytes lie in that free space. |
| `user_pinyin_index.bin` (8192 bytes): 4156–4160 | Same root-buffer reuse; 16 index entries leave free space `[58,3968)`. Index entries overwrite relative 52–57, so only later UID residue differs. |
| `user_bigram.db` (12288 bytes): 8244, 8252–8256 | Final hash bucket starts at 8192 and reuses the metadata buffer. Zero entries leave free space `[26,4096)`, including the UID residue. |
| Page allocation and LSNs | Both sides have 4096-byte pages and identical page counts/types for this fixture. Metadata/root/final-bucket LSNs are `{file=0, offset=1}`; the unused hash page is zero. No LSN byte differs. LSNs or page-allocation differences are **not** explanations for this observation. |

Recorded evidence: the BDB of this reading is `libdb5.3t64` **5.3.28+dfsg2-9**;
the control ran on that version.
Source inspection used
[db5.3 5.3.28+dfsg2-11](https://snapshot.debian.org/archive/debian/20260831T000000Z/pool/main/d/db5.3/db5.3_5.3.28+dfsg2-11.dsc)
with its Debian patch series applied. The -9/-11 patch sets do not
change the cited implementation files; the sole patch to the cited page header,
`005-pg_crypt_size.patch`, is identical. The source archives were checked
against the `.dsc` SHA-256 entries. References below are package-root paths:

| BDB source | Consequence |
| --- | --- |
| `src/fileops/fop_util.c:633-645`; `src/os/os_fid.c:35-46,62-66,80-86,98-131`; `src/os/os_uid.c:33-54` | Database creation requests a unique file ID. Inode/device, process identity, time, stack address, random value and serial contribute; the ID is stamped once when the database is created. |
| `src/dbinc/db_page.h:73-94`; `src/btree/bt_open.c:406-440`; `src/hash/hash_open.c:277-299` | The metadata's 20-byte UID is at offset 52 and is copied from the newly generated file ID. |
| `src/btree/bt_open.c:538-569`; `src/hash/hash_open.c:417-446`; `src/dbinc/db_page.h:270-290,525-540` | Creation writes metadata, then reuses its buffer for the root/final bucket. Full clearing occurs only under DIAGNOSTIC; P_INIT changes the page header, not unused bytes. Entry count and high-free offset delimit the record-free region. Observed later-page residue equals the metadata ID, except where the index array overwrote it. |
| `src/dbinc/db_int.in:1038-1044`; `src/btree/bt_open.c:541,562`; `src/hash/hash_open.c:420,440` | This nonlogged creation path writes LSN `{0,1}`. No transaction-LSN variability is inferred for this fixture. |

The pin save path unlinks and creates new containers before copying records:
`src/storage/chewing_large_table2_bdb.cpp:135-161`,
`src/storage/phrase_large_table3_bdb.cpp:150-176`,
`src/storage/ngram_bdb.cpp:81-110` at `074a2219`.
Copying one identical starting profile for both sides therefore cannot make
**new saves** physically identical: each save replaces the containers with
newly identified files. Sharing one writable profile between the two saves
would couple operations and change the experiment. The existing isolated
profile handoff, however, permits a meaningful cross-open test of the exact
file another implementation wrote, without requiring reproducible file IDs.

The criterion is logical records: `save-929-logical-cross-open-pinyin` and
`save-929-logical-cross-open-zhuyin`. `oxpinyin-capi`'s
`pinned_data_tests::the_pins_saved_profile_opens_with_its_records` and
`oxpinyin-zhuyin-capi`'s
`pinned_data_tests::a_pin_written_profile_opens_and_saves_with_its_logical_records`
hold them on the frozen pin profiles. The cases compared DBM rows with
`db5.3_dump` on BDB, and compared
all key/value bytes including empty values, with no padding or metadata mask.
The fixture uses two-syllable imported phrases and a nonempty bigram row
with total/count 37. The existing crafted-profile helper supplies the same
row to both sides; the pin loads and saves it by copying records
(`src/storage/ngram_bdb.cpp:47-110`).
Fresh pin→subject and subject→pin workers run on each donor's exact
directory. The receiving C facade opens it, recovers the
saved tokens, re-adds one existing reading with count zero, saves and must
produce exactly the same logical rows. The zero-count operation preserves the
existing reading (`src/pinyin.cpp:561-582`; `src/zhuyin.cpp:449-469`), while
forcing the facade's load/save path. Each receiver is checked against its own
donor as well as the other receiver; equal corruption cannot become MATCH.

This corrects a criterion the pin itself does not satisfy; it is **not an
accepted Rust difference**. No bytes are masked. The native tests and the
C-facade cross-open cases both check nonempty saved bigram counts.

Other backends, source classification only:

| Backend and inspected version | Save-container behavior |
| --- | --- |
| KC 1.2.80-2+b1, trixie's `libkyotocabinet16v5` (source 1.2.80-2; its `libkyotocabinet-dev` installs `kcdb.h` and `kcstashdb.h` byte-identical to the orig tarball, and no Debian patch touches either) | Pin `src/storage/ngram_kyotodb.cpp:82-105`, `src/storage/chewing_large_table2_kyotodb.cpp:124-145`, `src/storage/phrase_large_table3_kyotodb.cpp:139-160` use `dump_snapshot`; the native-file alternative is disabled. `kcdb.h:2271-2307,2316-2328` writes magic, key/value lengths and bytes, and terminator, without file IDs, page allocation or timestamps. `kcstashdb.h:442-468` iterates buckets/chains, so serialized record order remains a separate dependency. KC does **not** have the demonstrated BDB metadata cause; same-record/same-order snapshots have no source-visible per-save identity field. No unrun physical-identity result is claimed. |
| tkrzw 1.0.32-1+b1, trixie's `libtkrzw1t64` (source 1.0.32-1, no Debian patches); sound-runtime source 9ee84164 (1.0.34) checked too | Pin `src/storage/ngram_tkrzwdb.cpp:65-83` creates HashDBM; the two index writers create TreeDBM (`src/storage/chewing_large_table2_tkrzwdb.cpp:111-129`, `src/storage/phrase_large_table3_tkrzwdb.cpp:102-120`). In both inspected tkrzw versions, `tkrzw_dbm_hash.cc:46,1184,1241,1296` writes a wall-clock microsecond timestamp at bytes 48–55. `tkrzw_dbm_tree.cc:379,432,954` uses an underlying HashDBM and synchronizes it. Thus tkrzw also has per-save variable physical metadata, for a different reason from BDB. No new timestamp mask or parity exception is introduced. |

Recorded evidence: BDB `--issue 929` reported **21 MATCH**, including both
logical cases and all four reciprocal cross-open workers.

Open: individual DBM/rename interiors, writes after marker open, backend
sync/close errors, power-loss durability and non-Linux execution. The glibc
`rename` and `fsync` wrappers are traced above at `glibc-2.41`, trixie's
libc; they add no retry and no error text.

## G11 — Numeric parsing and locale

Pin: `storage/table_info.cpp:160,197-291,328-394` (scanner sites
`:208,214,220,228,232,255,338,344,352,357`; locale
`:197,291,328,372,378,394`). Rust: `input/src/config.rs` and
`input/src/native.rs` make the pin's `setlocale`, `fopen`, `fscanf`, `feof`,
`atoi`, `fprintf` and `fclose` calls through the `libc` crate, with the pin's
format strings and order (external-I/O stage 1); `data/src/table_info.rs`
(`TableConf`) and `data/src/user_files.rs` (`UserTableInfo`, `to_text`) keep
what the pin does with each answer. Because the call is the pin's, the
verdicts below describe the host scanner on both sides; a scanner version
difference is host dependence (policy row 78), not a divergence. An unknown
ordinary database name aborts, as the pin does (row 44).

Recorded evidence: the scanner of this reading is glibc **2.41-12+deb13u4** from the Debian
`glibc-source` package: `stdio-common/vfscanf-internal.c` for the
`%d`/`%f`/`%255s` state machines and `stdlib/strtod_l.c` for the float
value. The source walk below is the basis for each verdict. “Complete”
means the behaviour has a contract case or a unit test; every case named
below runs on both facades unless marked otherwise.

| Partition | Verdict / record | Completion |
| --- | --- | --- |
| Directive literals and whitespace: a format space or `\n` matches any run of C white space, including none; leading bytes before the first literal do not match; one-line headers, indented `lambda`, CRLF and a missing final newline are accepted. | Contract; no exception. Source: `stdio-common/vfscanf-internal.c:452-486` (literal and white-space loop). | Complete: `table-conf-919-header-{one-line,crlf,indented-lambda,no-final-newline,leading-space}` and the missing-line cases. |
| Header order and missing fields: a missing `binary`/`model`/`lambda` directive is the pin’s ordinary `false`; a missing `database format:` line reuses the `source` word and aborts; an empty source or database value makes the next word the value and aborts at the converter; a missing `source` line reads an uninitialised `char str[256]` (class (b), not reproduced); a missing user-file row is the existing directory-read split. | Contract, except the missing-source class (b) refusal and the converter abort sites (reproduced aborts). Source: `stdio-common/vfscanf-internal.c:452-486` and `:1076-1259` (`%255s`); the missing-source class (b) is the pin's uninitialised `str` at `table_info.cpp:228-229`, not a glibc rule. | Complete: `table-conf-919-{binary-version,model-version}-line-missing`, `table-conf-abort-919-{database-format-line,database-format-value,source-format-value}-missing`, `table-conf-abort-919-source-format-line-missing`, existing `table-conf-abort-source-format-line-missing`. |
| `%d` grammar: leading white space, optional `+`/`-`, at least one ASCII decimal digit, stop at the first non-digit; no digits is a conversion failure; base 10 leaves `x` after `0x`; a truncated line leaves the same bytes. | Contract. Source: `stdio-common/vfscanf-internal.c:1452-1500` (sign/digit lexer); `stdlib/strtol_l.c:401-435` (digit loop). | Complete: `table-conf-919-versions-{plus-sign,no-digits}`, `versions-negative-overflow`. |
| `%d` and `atoi` range: `strtol` saturates at `LONG_MIN`/`LONG_MAX`, the store into `int` is the low 32 bits on amd64/arm64, and an addon index is cut to `guint8`; `UserTableInfo::save` formats the two system versions with `%d`, so a `table.conf` value outside `int` is written back signed. | Contract; the writer formats through `i32`. Source: `stdlib/strtol_l.c:483-519` (overflow/saturation); `stdio-common/vfscanf-internal.c:1963-1984` (`int` store). | Complete: `table-conf-919-versions-{positive-overflow,int-wrap,negative}`, `table-conf-919-addon-atoi-nondigit`, `table-conf-abort-919-addon-positive-overflow`; `user_files::tests::an_out_of_int_version_is_written_signed`. |
| `%f` decimal grammar: optional sign; digits with an optional dot on either side; at least one digit; `e`/`E` exponent with optional sign and digits; a second dot or no digit is rejected; a bare exponent mark is accepted by trixie 2.41 (it does not clear `got_digit`) but rejected by 2.43-3 and by the subject; the scan is non-backtracking. | Contract. Source: `stdio-common/vfscanf-internal.c:2006-2320` (lexer); `stdlib/strtod_l.c:607-940` (sign, digits, fraction, exponent). | Complete: `table-conf-919-lambda-{dot-leading,dot-only,positive-sign,exponent-plus,exponent-sign-without-digits,decimal-underflow}`, `table-conf-lambda-{exponent-without-digits,hex-exponent-without-digits}`. |
| `%f` hexadecimal: `0x`/`0X`; hex digits with an optional dot; at least one hex digit; optional `p`/`P` exponent with optional sign and digits; `e` is a hex digit, not the exponent; `0x` fails; `0x.` converts to 0 in trixie 2.41 and is rejected by 2.43-3 and the subject; `0x1p`, `0x1p+` and `0x1p-` convert to 1.0 in trixie 2.41 and are rejected by 2.43-3; `strtof` rounds to nearest even, including subnormals and overflow. | Contract. Source: `stdio-common/vfscanf-internal.c:2140-2320`; `stdlib/strtod_l.c:684-700,732-756,850-940`; rounding at `strtod_l.c:213-357`. | Complete: `table-conf-919-lambda-{hex-no-exponent,hex-overflow,hex-dot-without-digits,hex-exponent-sign-without-digits,hex-exponent-minus-without-digits}`, `lambda-hex*`. |
| `%f` specials: case-insensitive `inf`, `infinity`, `nan`, and `nan(n-char-sequence)`; an incomplete `infin`, an unclosed `nan(`, or an invalid payload fails; λ is only consumed in NaN arithmetic whose result is zero frequency/NaN costs, so the payload is not observable. | Contract. Source: `stdio-common/vfscanf-internal.c:2034-2160`; `stdlib/strtod_l.c:640-684`. | Complete: `table-conf-919-lambda-{infinity,nan-payload,nan-unclosed}`, `lambda-inf`. |
| `%f` range and host libc: decimal overflow/underflow, hex subnormal/overflow and rounding match the reference; trixie 2.41 accepts `1e`, `1e+`, `0x1p`, `0x1p+`, `0x1p-` and `0x.` where 2.43-3 rejects them, and the registered `host_scanf` cases report HOST-DEPENDENT rather than DIFFER. | Host dependence of the pin's own call (policy row 78). Source: `stdlib/strtod_l.c:213-357` (round/overflow) and `:878-940` (exponent); `stdio-common/vfscanf-internal.c:2196-2201` (2.41 exponent mark keeps `got_digit`) and `:2545-2549` (2.41 conversion check has no `!got_digit`); 2.43-3 adds `:2218-2219` and `:2571-2576`. | Complete: `table-conf-919-lambda-{hex-overflow,decimal-underflow}`, the `host_scanf` cases and the 919 exponent controls per facade; the reference is the oracle image’s glibc 2.41-12+deb13u4. |
| `%255s` words: skip white space across newlines, take at most 255 bytes, retain NUL and non-UTF-8 bytes, then the C string stops at the first NUL; EOF after white space is an input failure; a 256th byte starts the next word. | Contract. Source: `stdio-common/vfscanf-internal.c:533-539` (width) and `:1076-1259` (`%s`). | Complete: `table-conf-919-word-{nul-truncates}`, `table-conf-abort-919-word-width-overlong`, `short-row-dropped`/`extra-word-dropped`; file names cover non-UTF-8. |
| Six-word rows: words group across line boundaries; a final short row is dropped; a five-word row followed by the next row’s first word forms a six-word row; a merged row that mismatches the library and table makes the pin read a NULL item and die of SIGSEGV, while the subject returns the declared result (class (b)). | Contract on defined rows; the merged-row crash is class (b). Source: `stdio-common/vfscanf-internal.c:1076-1259` (`%s` stops at white space); the six-word loop is pin `table_info.cpp:254-287`. | Complete: `table-conf-919-short-row-merged` (declared class-(b) result), `short-row-dropped`, `extra-word-dropped`, `comment-dropped`. |
| Consumer mapping: system/user versions, addon index, λ `f32` bits, library names/types and file-type roles reach the same dictionary load/save and candidate results. | Contract. Source: the conversions above (`vfscanf-internal.c`/`strtod_l.c`); no additional scanner path. | Complete: the 68 #919 cases plus the 88 valid `table-conf-*` cases. |
| Training λ reader: the tools read `table.conf` through the same `TableConf::load` stream as the library (`oxpinyin-utils` `phrase_index::load_table_conf`). | Tool-only contract with the pin’s loader; no C-ABI change. Source: the scanner citations above; no independent scanner. | Complete: the tools share the library reader; no facades entry. |
| Process `LC_NUMERIC`: the pin sets it to `C` and early returns skip the restore; oxpinyin makes the same calls and leaves it at `C`. | **Needs a ruling to reverse**, [policy row 39](compatibility-policy.md), #923; retain. Not a scanner verdict: pin `table_info.cpp:197`; #923. | Not changed. A maintainer ruling governs it; it is not an unread path. Held by both capi `tests/locale.rs`. |

**Host scanner versions.** The reference scanner is glibc 2.41. Its `%f`
does not clear `got_digit` at the exponent mark, and its post-loop check
does not require a digit after it, so `1e`, `1e+`, `0x1p`, `0x1p+` and
`0x1p-` convert (to 1.0); `0x` and `infin` fail. `0x.` reaches
`strtof("0x.")`, which treats it as 0 and returns with the read pointer
after `0` (`stdlib/strtod_l.c` at 2.41: hex prefix `:690-700`, the `0x.`
branch `:732-768`, the zero-digit return `:1044-1048`). glibc 2.42 rejects
the incomplete exponents and 2.43 also rejects `0x.`
(`vfscanf-internal.c:2571-2576`). The affected cases, on both facades, are
`table-conf-lambda-exponent-without-digits`,
`table-conf-lambda-hex-exponent-without-digits`,
`table-conf-919-lambda-exponent-sign-without-digits`,
`table-conf-919-lambda-hex-exponent-sign-without-digits`,
`table-conf-919-lambda-hex-exponent-minus-without-digits` and
`table-conf-919-lambda-hex-dot-without-digits` (`host_scanf=('0x.',)`,
maintainer ruling 2026-10-25 UTC). Their `host_scanf` disposition reports
HOST-DEPENDENT; any other difference stays DIFFER, and matching
observations stay MATCH. The `%d` and `%255s` rows are unchanged for the
scanned fields.

### `user.conf` — #920

Pin: `storage/table_info.cpp:325-397` (`UserTableInfo::load` four
`fscanf`s at `:338,344,352,357`; `save` at `:372-397`;
`is_conform` at `:399-413`; `get_open_counter` at `:422-426`). Rust:
`input/src/config.rs` (the pin's `fscanf`/`fprintf` calls),
`data/src/user_files.rs` (`UserTableInfo`) and `user/src/persistence.rs`
(`check_format`, `write_marker`, `clean_user_files`). The fixture helpers read the running cell's
`table.conf` values; no backend name is written into a case.

| Partition | Verdict / record | Completion |
| --- | --- | --- |
| Version `%d`: C white space, optional `+`/`-`, every ASCII digit; `strtol` saturates at LP64 `LONG_MIN`/`LONG_MAX` and the store through `int *` keeps the low 32 bits, so `2147483648` is `-2147483648`, `4294967303` is `7`, `9223372036854775808` is `-1`, and `-9223372036854775808` is `0`. | Contract; the pin's signed `int` bits are preserved. Source: `stdio-common/vfscanf-internal.c:1452-1500`, `stdlib/strtol_l.c:483-519`, `vfscanf-internal.c:1983-1984`. | Complete: `pinned_data_tests::user_conf_versions_past_int_take_the_scanners_int` (`oxpinyin-capi`). |
| Open counter `%d`: the same conversion into `int`; a missing/failed directive reads 0; a counter above `OPEN_COUNTER_LIMIT` is non-conforming and `get_open_counter` turns it into 0; negative/zero counters conform; the load raises `get_open_counter(existing) + 1` and fini lowers it. | Contract; state-preserving on conforming wraps, wiping on non-conforming ones. Source: `vfscanf-internal.c:1452-1500`, `strtol_l.c:483-519`; pin `table_info.cpp:357-359,422-426`. | Complete: unit `user_files::tests::marker_fields_follow_the_scanner` (`oxpinyin-data`). |
| `database format:%255s`: a known C string selects the backend; an unknown token aborts (`to_table_database_format_type`); an EOF before the conversion leaves `UNKNOWN_FORMAT`; a missing literal or matching failure reaches the mapper on `str` that no conversion wrote (class (b)); a NUL ends the C string even though `%s` kept reading the word. | Contract for known/EOF; reproduced abort for unknown (row 44), class (b) for the unwritten `str`. Source: `vfscanf-internal.c:1076-1259` (`%s`); pin `table_info.cpp:350-354` and `strcmp` at `:122-131`. | Complete: unit `user_files::tests::marker_fields_follow_the_scanner` (`oxpinyin-data`); `boundary_tests::init_aborts_on_an_unknown_user_conf_database_format` (`oxpinyin-capi`) for the unknown-format abort. |
| Missing/malformed version directive: the pin's `load` returns `false`, leaves the reset defaults (`0/0/UNKNOWN/0`), `check_format` sees a non-conforming marker and wipes the profile; a `%d` with no digit after an optional sign is that failure; a foreign byte after a version is what the next directive meets. | Contract; the wipe is the pin's, not a Rust error policy. Source: `vfscanf-internal.c:452-486` (literal/EOF) and `:1452-1500`; pin `table_info.cpp:338-348`. | Complete: `boundary_tests::init_aborts_on_an_unknown_user_conf_database_format` (`oxpinyin-capi`) is the abort neighbour; `pinned_data_tests::user_conf_versions_past_int_take_the_scanners_int` covers the wipe. |
| State preservation and conformance: a conforming marker (including a wrapped version equal to the system's, `7 + 2^32`, or a counter wrapping to 0) retains every user file and learned count; a non-conforming marker follows `_clean_user_files`, and `write_marker` writes the system versions with the raised/lowered counter. | Contract; #920's guard. | Complete: `pinned_data_tests::user_conf_versions_past_int_take_the_scanners_int` (`oxpinyin-capi`) asserts the trained and imported unigrams after reopen; `user_listing` compares the files and canonical `user.conf`. |
| Round-trip writer: `UserTableInfo::save` writes versions with `%d` (signed), the database format canonically, and the counter signed; a marker read from a version outside `int` writes back the same signed values the pin does. | Contract; builds on #1178. Source: pin `table_info.cpp:372-397`; `user.conf` writer uses `i32` in `data/src/user_files.rs`. | Complete: `user-conf-920-version-overflow-*`, `user-conf-920-counter-*`, `user_files::tests::an_out_of_int_version_is_written_signed`. |
| Backend identity: the same four directives are used on bdb, kc and tkrzw; only the `database format:` value changes, and it is read from the cell's `table.conf`. | Contract; fixtures are backend-independent. | Complete: every #920 fixture derives binary/model/counter values from the cell and edits the database token generically; no literal backend name. |
| Process `LC_NUMERIC`: as #919/#923; the pin sets it to `C` and early returns skip the restore, oxpinyin makes the same calls. | **Needs a ruling to reverse**, policy row 39/#923. | Not changed. |

Recorded evidence: `user-conf-920-*` **16/16 MATCH** on bdb, kc and
tkrzw; the named `nul-database-token` mutation made the two NUL cases
DIFFER.

## G12 — File I/O and MemoryChunk

Pin: `include/memory_chunk.h:72-78,424-520,555`;
`storage/phrase_index.cpp:354-361`; save/cleanup paths in G10.
Rust: `input/src/chunk.rs` `load`/`save` (the pin's calls);
`data/src/phrase_library.rs`, `map_chunk`; `user/src/persistence.rs`;
`data/src/c_stderr.rs` raw-byte diagnostics.

| Partition | Verdict / record | Investigation |
| --- | --- | --- |
| Valid file header, length/checksum, payload and mapped extent | Inventory source conclusion: compatible bytes/checks. | Partial full I/O audit; no claim to all read/write failures. |
| Short/truncated/unreadable system chunk leading to empty-chunk fault | Accepted class (b) only within policy row 76 and its [register](upstream-divergences.md). | Established named fault, not every malformed file; wider path coverage pending. |
| Directory with lseek size below header versus at least header size | Below header follows row 76; at least header reaches `assert` at `include/memory_chunk.h:493`, reproduced as an abort, [policy row 96 record](compatibility-policy.md). | Established filesystem-dependent split, not uniformly class (b). Row 96 records affected init/load/save/mask/addon callers and prior ext4/btrfs checks. |
| Sparse chunk retained mapping after fini | Policy row 109: closed pin resource defect, **no compatibility exception**; [record](upstream-divergences.md#a-sparse-chunks-whole-mapping-is-retained-after-fini-865-policy-row-109). | Established bounded source result: pin unmaps header plus capacity; Rust drops its mapped extent. |
| Other write/read/unlink failures and raw stderr | Pending #929 and per-site classification. | Unverified complete family; G10 owns save ordering. |

Rows 96 and 109 above use the current policy/register, refining the
inventory matrix's compressed “class (b) rows 76/96/109” label.

## G13 — C primitives

Pin: e.g. `storage/phrase_index.cpp:103-111,460-475`;
`pinyin.cpp:1635-1637,2041-2067`; `zhuyin.cpp:1096-1104,1360`;
all `memset`/`memcpy`/`memmove`/`memcmp`/`strlen`/`strcmp`/`strchr` sites
in TSV. Rust: `data/src/row_format.rs` byte decoders,
`core/src/single_gram.rs` gram operations, `capi/src/ffi.rs` C-string
helpers and engine candidate dedup.

| Partition | Verdict / record | Investigation |
| --- | --- | --- |
| Valid bounded bytes and NUL-terminated scalar strings | Inventory source conclusion: ordinary slice/string equivalence; no independent primitive discrepancy. | Partial across all callers; G1/G2 limits still apply. |
| NULL/unreadable candidate text from mismapped library or journal-removed item | Accepted class (b) only at [policy row 80/106 records](compatibility-policy.md). Row 80's warning refusal and row 106's omission/true are different outcomes. | Established scoped records, not permission to collapse them. |
| Other NULL, out-of-range walks and malformed lengths | Pending classification. | Unverified whole-family trace. |

## G14 — Maths

Pin: `lookup/phonetic_lookup.h:39,250,663,693`;
`lookup/phrase_lookup.cpp:40,321,342` (`log`, seven reachable sites).
Rust: `core/src/cost.rs`, `surprisal`, `cost_of_probability` (`f64::log2`);
`engine/src/nbest.rs` and `engine/src/phrase.rs` (`ln`).
Owner [#1152](https://github.com/shenghaoc/oxpinyin/issues/1152); **std
maths is the direction**.

| Partition | Verdict / record | Investigation |
| --- | --- | --- |
| Last-bit differences following transcendental accumulation/std maths | Accepted class (a), [row 11 record][math-record] and current math rule. | Existing classification established; no new replacement evaluated here. |
| Std maths: ordinary probabilities, zero/special values, cost scale/rounding/saturation, ranking consequences | Pending exact implementation assessment under #1152; preserve required special-value handling and record permitted numerical changes. | `core/src/cost.rs` already calls `f64::log2`; the per-site assessment under #1152 is not done. |
| Basic arithmetic and float-to-integer comparator/frequency conversion | Not admitted by class (a) merely because floats occur. | G5/G6 investigation, not closed by this math entry. |

## G15 — Punctuation decoding

Pin: `storage/punct_table.cpp:56-94,148-156`, backend `load_entry`
implementations; consumer `pinyin.cpp:2456-2498`.
Rust: `data/src/row_format.rs::punct::decode_puncts`,
`data/src/punct.rs::PunctTable::punctuations`,
`runtime/src/lib.rs::RuntimeDict::punctuations`,
`capi/src/predict.rs::prepend_punctuations`. Pinyin only; #1144, with
order/dedup #928. No zhuyin punctuation-export twin is implied.

| Partition | Verdict / evidence | Investigation |
| --- | --- | --- |
| Well-formed, nonempty, scalar, terminated fields | Identical strings. | `punct-decode/valid` MATCH on bdb (#1144). |
| Non-u32-aligned byte length | Whole words only; the pin's extra empty row after a terminated run is not reproduced. POLICY-ACCEPTED, [1297-punct-words](1297-punct-words.md). | `punct-decode/misaligned-terminated`, `punct-decode/misaligned-short` (policy evidence). |
| Empty punctuation field | Pin rows reproduced: a zero first word is an empty string. | `punct-decode/empty`, `punct-decode/empty-leading` MATCH on bdb (#1144). |
| Non-scalar code point | Pin bytes reproduced through `phrase_text_bytes`; no quirk record needed. | `punct-decode/surrogate`, `punct-decode/above-unicode` MATCH on bdb (#1144). |
| Unterminated stored value | Pin rows reproduced; the one-word over-read is class (b), [registered](upstream-divergences.md#unterminated-punctuation-value-over-read). | `punct-decode/unterminated`, `punct-decode/unterminated-zero` MATCH on bdb (#1144). |
| Absent DBM entry | No row on either side. | Source: `load_entry` leaves the chunk empty on a failed get (`punct_table_bdb.cpp:145,154-156`), and `get_all_punctuations` returns `false` on a zero-size chunk (`punct_table.cpp:78-79`). |
| Zero-length stored value | No row on either side. | `punct-decode/zero-length` MATCH on bdb (#1144). |
| Unopened table | No row on either side. | Source: `pinyin_init` ignores a failed attach (`pinyin.cpp:437-441`); `load_entry` returns `false` with no database (`punct_table_bdb.cpp:141-142`). |

The full partition is the inventory's
[G15](glib-libc-inventory.md#g15--punctuation).
Order and duplicates are the inventory's G2 notes (#928).

## G16 — Tool-interface check

Pin: `storage/tag_utility.cpp:94-95,118-264` (`split_line`, `taglib_read`);
`storage/table_text_parser.h:34-74,76-120`;
`utils/utils_helper.h:51-72` (`TAGLIB_PARSE_SEGMENTED_LINE`);
`utils/storage/gen_binary_files.cpp:68-72,103` (`load_text` callers).
Rust: `kmm/src/taglib.rs` (the pin's taglib, re-exported by
`oxpinyin-utils`; [grammar record](interpolation2-grammar.md));
`utils/src/training/mod.rs` (`TAGLIB_PARSE_SEGMENTED_LINE`);
`utils/src/storage/table_text.rs`; the tool boundary `input/src/tool.rs`
(`g_strsplit_set`, `g_unichar_isspace`, `g_unichar_isgraph`, `atoi`,
`atol`, `fprintf` through `glib-sys` and `libc`). Owner #926.

The tools are drop-in ports of the pin's utilities under their own names
([external I/O, stage 5](external-io-boundary.md#proposed-crate-and-staged-implementation)).
Every call at the tool's edge is the pin's call, so the separator, escape,
category and numeric results are the host's, as for the pin.

| Tool input partition/interface | Verdict | Investigation |
| --- | --- | --- |
| Ordinary tagged interpolation input; spaces, printable tokens, required/ignored tags | Identical: the pin's taglib with the pin's GLib calls ([grammar record](interpolation2-grammar.md)). | The frozen [tool cases](../runbooks/tool-diff.md). |
| Quoted escapes, dangling escapes, Unicode whitespace/graph categories, unexpected/missing tags | The pin's calls (`g_unichar_isspace`, `g_unichar_isgraph`, `g_warning`); the macro checks are plain Rust without GLib's critical text ([eio-tool-check-messages](eio-tool-check-messages.md)). | Source and tool cases. |
| Segmented `token phrase` stream, empty lines, repeated separators | Reproduced: `g_strsplit_set(line, " \t", 2)`, the empty-line null token and the macro's `abort()` on a missing separator. | Source; tool cases. |
| Signed/overflowing/malformed numeric fields in the segmented stream and K-mixture text | The host's `atoi`/`atol` through `libc`, as the pin. | Source; tool cases. |
| Four-field generation records, field-length limits, integer ranges and rejected rows | `gen_binary_files` reads the pin's `table_text_read_pinyin_record` / `_punct_record` grammar in `storage/table_text.rs`. | Source; tool cases. |

The inventory excludes these sites from the shared objects' exported
call graph; it does not exclude the tools' input/output contracts from
#926. Inclusion of `tag_utility.cpp` through headers does not itself
establish an exported call.

## G17 — Dead-code exclusion

Pin evidence: inventory [negative coverage](glib-libc-inventory.md#negative-coverage)
and the `dead` rows in the TSV; runtime constructors at
`pinyin.cpp:351,363,406-425`, `zhuyin.cpp:295,306,348-355` use
`FacadeChewingTable2`, `FacadePhraseTable3`, `PhoneticLookup` and
`PhraseLookup`. `storage/punct_table.cpp:41-54,96-146` contains the excluded
`PunctTableEntry::escape`/`append_punctuation`/`remove_punctuation`;
`:74-94` is the separately reachable G15 reader.

| Partition | Verdict | Investigation |
| --- | --- | --- |
| Uninstantiated `lookup/pinyin_lookup2.cpp`, old `storage/chewing_large_table.cpp`, `phrase_large_table2.cpp`, `facade_chewing_table.h`, `facade_phrase_table2.h` | Excluded from exported-surface compatibility coverage, not “identical.” No shipped counterpart required. | Inventory reachability exclusion established; constructors cross-checked here. Not a new linker/binary reachability audit or deletion authorization. |
| PunctTableEntry writer methods and compiled/commented-out debug dumps | Same exclusion for the inventory's dead sites. | Inherited negative coverage; no claim about hypothetical future callers or generator behavior. Tool-only load_text remains G16. |

## Completion

Before #915 closes, verify combined main, reconcile shared-helper
changes, and account for every inventory entry. This ledger and an
inventory of all families do not mean all their investigations are
complete. #923 is ruled: retain (2026-10-25 UTC). G14 tracks #1152.

[math-record]: compatibility-policy.md#classification-table
