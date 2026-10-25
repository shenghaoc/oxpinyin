# GLib/libc verification coverage

Status: **partial coverage, removal gate** · 2026-10-25 UTC ·
[Parent #915](https://github.com/shenghaoc/oxpinyin/issues/915).

This ledger is keyed to G1–G17 of the merged
[inventory](glib-libc-inventory.md) ([#1146](https://github.com/shenghaoc/oxpinyin/pull/1146)).
Its [call-site list](glib-libc-call-sites.tsv) supplies the complete membership
of each family: 1,224 exported-path sites, 115 tool-only sites and 173 dead
sites. Inventory membership is not proof that every input was investigated.
The entries below preserve that distinction.

## Evidence and decision rules

Source baseline: oxpinyin `801593723f62a36e1a5d2a7857209cc45d50bbe3`.
The #894 records are now on main via [#1153](https://github.com/shenghaoc/oxpinyin/pull/1153);
its reviewed head remains
[`a3cf66514a22eb25a8ec636d4f03f18bd7efbce1`](https://github.com/shenghaoc/oxpinyin/commit/a3cf66514a22eb25a8ec636d4f03f18bd7efbce1).
The immutable [non-scalar record][non-scalar] and [NUL record][nul] are the
source of the established #894 conclusions below. No #1153 implementation
is copied or changed by this coverage document.

All pin citations below are `file:line` at libpinyin
`074a2219c90feaf962d0d24f034514033ece5f99`; relative pin paths start at
`src/` unless prefixed `utils/`. The pin source was read at `074a2219`,
alongside the cited existing findings. The
inventory's reference is GLib **2.89.4-2**, glibc **2.43-3**, Debian snapshot
`20260831T000000Z`, not the host libraries. Rust paths below start at
`crates/oxpinyin-`; they cite items, not moving line numbers. “Both” means
pinyin and zhuyin; backend-independent source arguments cover bdb/kc/tkrzw
only to the stated extent, not three executed verification cells.

**Verdict and investigation are separate columns.** “Identical” states a
source-derived equivalence for the named scope; it is not a test result.
“Complete” applies only to that bounded source investigation. “Partial”
means a cited result exists but the full partition/caller audit remains
open. “Unverified; pending classification” is unread or unsettled work,
not a policy question. “Needs a ruling” is reserved here for #923.
A child or an inventory label alone does not admit a quirk exception.

Follow the [policy decision order](compatibility-policy.md#the-languageruntime-quirk-exception--outside-the-classes):
C contract first, classes (a)–(c) next, then individually recorded runtime
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
a decompiler only for a specific unresolved case. This docs pass runs no
new binary analysis, runtime experiments, builds, tests or measurements;
inherited binary evidence is explicitly cited below, not a new result.

## G1 — UTF-8/UCS-4 conversion and category

Pin: `pinyin.cpp:614-652,698-768,1450-1454,1627-1645,1953-2138,3669-3710`;
`zhuyin.cpp:500-537,969-973,1096-1113,1294-1416`;
`lookup/lookup.cpp:27-69`; `storage/phrase_index.cpp:103-111,522-555`;
`storage/phrase_index.h` item storage; `storage/phrase_index.cpp` loaders.
Rust: `data/src/phrase_library.rs`, `PhraseItemView::phrase_chars`;
`data/src/phrase_libraries.rs`, `phrase_char_count`/`trellis_item`;
`engine/src/nbest.rs`, `token_text`/`text`; both `capi/src/sentence.rs`
and `zhuyin-capi/src/sentence.rs`; `facade/src/export_rows.rs`,
`PhraseExportCursor`/`ExportRow`; `capi/src/iterators.rs`.

| Input partition | Verdict / record | Investigation and exercised scope |
| --- | --- | --- |
| Complete bounded phrase items, matching readings, nonzero scalar text; valid UTF-8 producers | Identical on the #894 text paths: GLib encoding and Rust scalar encoding agree. | Complete source trace in [non-scalar][non-scalar] and [NUL][nul] records, including system/addon/user chunks, journals, import, remember, generation, training/save, candidate/sentence/export consumers. Six scalar controls exercised; not every producer tested. |
| Stored surrogates / above-Unicode values below `0x80000000`, with otherwise sound item/readings | Accepted runtime quirk, [non-scalar][non-scalar]. GLib emits extended UTF-8; Rust refuses the item. | Complete recorded source classification for the bounded text mechanism. Exercised U+D800 and U+110000 only: ten marked candidate/sentence/pinyin-export cases. Direct token getters, addon/prediction alternatives and journals are source-traced, not separate exercised witnesses. |
| Embedded U+0000 in a declared phrase body | Accepted runtime quirk, [NUL][nul]. GLib stops per item; Rust retains zero until C marshalling refuses it. | Complete recorded source classification. Five marked candidate/sentence/pinyin-export cases. Dedup, prefix slices and NULL-output variants have source reasoning, not separate exercised controls. |
| Values >= `0x80000000`; truncated, empty, overlong or inconsistent items; invalid readings; allocation failure | Values >= `0x80000000`: `phrase_segment` with a matching raw key is identical ([record](918-nonscalar-token-result.md)); the text getters still differ (pin true/declared length/NULL, Rust false/untouched), a distinct malformed-item difference not admitted by #894. Truncated/empty/overlong/inconsistent items: out-of-bounds reads are class (b) (rows 101/107); in-bounds declared-length reads can return following bytes. Invalid readings are a separate surface, not on the segment path. Allocation failure is host-dependent: conversion OOM refuses at the pin, later container OOM aborts, and Rust aborts on its conversion/copy allocations. | Source-complete for the values/items/readings and allocation partitions in the [G1 closure](918-boundary-follow-through.md#g1-partition-closure); no new equivalence or #918 removal clearance. |
| `phrase_segment`: empty; well-bounded rejected `ff fe`; valid scalar conversion | Identical for the entry/result lifecycle described in [#918's audit](918-phrase-segment-source-audit.md); existing independent lookup/math records retain their scope. | Source established, both facades; inherited tests only, no new gate. Not a proof of every valid decoder path. |
| `phrase_segment`: GLib-accepted non-scalar, consistent scalar-only keys/items, sound grams and successful backing-store reads, including a matching scalar prefix | Identical return/result/getter/diagnostic behavior; no observable session or persistent change from prefix consultation. | Complete bounded source argument in [#918's audit](918-phrase-segment-source-audit.md), three backend read paths. Inherited surrogate-in-`nihao` tests do not exercise every non-scalar or store state. |
| `phrase_segment`: matching prefix, reached sound system gram plus 1–3-byte user gram, later GLib-accepted non-scalar | Identical under class (c): process the raw span and use row 85's ordinary warning/false/old-result refusal. [G1 entry](918-prefix-lookup.md); no new runtime-quirk exception. | Source-established, both facades/all three wrappers; bdb, kc and tkrzw contract gates 10/10 each; raw-lookup mutation 4 DIFFER/6 MATCH on bdb; restored 10/10 MATCH; touched-crate tests, clippy and fmt passed. |
| `phrase_segment`: bounded matching raw non-scalar key/item | Identical: pin true/token and the raw `u32` key path true/token; no `char`/`str` from the non-scalar. [Record](918-nonscalar-token-result.md), no exception. | Both facade contract cases MATCH on bdb; the path reads the item's declared length/frequency without decoding its text. |
| `phrase_segment`: over-skipping byte walk, partial/short unsafe gram reads | Existing class (b) governs illegal reads; no fabricated deterministic pin output. The raw span walk uses row 98's existing warning on a reached unsafe lookup gram. | Source and applicable inherited safety records; length-walk instructions read on amd64/arm64. |
| `phrase_segment`: faulting prefix stores | The [bdb format error](918-prefix-bdb-format-error.md) read is restored through real libdb, so its diagnostic and environment panic happen as at the pin; the surviving result lifecycle is the separate error-lifecycle entry. The [kc second-read assertion](918-prefix-kc-read-assert.md) is class (c): false, one warning, old result kept. | Named paths traced in [follow-through](918-boundary-follow-through.md), including the bdb binary branch on both architectures and the KC check-then-get implementation. No runtime fault probes; seam unit tests cover the class-(c) mapping. |
| Neighboring string helpers; other corrupt bounds, extreme lengths and allocation failure | Caller-specific outcomes and defects in [follow-through](918-boundary-follow-through.md); no blanket quirk admission or whole-process resource-failure equivalence. | Caller boundaries traced. The [G1 closure](918-boundary-follow-through.md#g1-partition-closure) settles the NULL/zero-length token lookup on all three cells, classifies extreme-length narrowing as class (b), and records the conversion/later-allocation split on allocation failure. Full G1 removal remains incomplete for other families. |
| Auxiliary UTF-8 substrings, tool Unicode categories, stored punctuation | Pending under G2/#925, G16/#926, G15/#1144 respectively. | Unverified outside their separately stated evidence; not covered by stored phrase text. |

#918 source follow-through (reading baseline `1c5d96b9`, rebased onto
`ae516625` before the prefix implementation): both `phrase.rs::segment_text` and
`*_phrase_segment`; neighboring `ffi.rs` helper callers are listed in
[the source audit](918-phrase-segment-source-audit.md#neighboring-helpers-and-shared-callers).
Its pin citations include `pinyin.cpp:1446-1460`, `zhuyin.cpp:965-979`,
`lookup/phrase_lookup.cpp:120-157,255-428`, and GLib
`glib/gutf8.c:117-128,237-273,665-747,940-997`. Unlike the initial
ledger pass, this follow-through reads the exact amd64/arm64 binaries
where source left length-walk/conversion or BDB abort configuration
unsettled; symbols and instruction offsets are in the linked records.
No oracle experiments or measurements derived these findings. The
new entry reports native tests, the executed bdb, kc and tkrzw gates (10/10 each) and
the raw-lookup mutation (4 DIFFER/6 MATCH). The ordinary scalar path
already emits row 85's warning on `ShortLookupPredecessor`; the
[G1 prefix entry](918-prefix-lookup.md) extends that refusal to the
reached short-gram/non-scalar partition and returns the pin's true/token
for a matching raw key. Sound-data equivalence is a length-sized
zero-token result, not a zero-length result. No removal is cleared.

GLib source evidence in #894 is `glib/gutf8.c:1035-1047,1054-1060`
(`g_ucs4_to_utf8`), `:549-607` (`g_unichar_to_utf8`) and `:940-990`
(`g_utf8_to_ucs4`) from the stated Debian package. The earlier #894 issue's
2.80.0 citations and blanket assertion that C input cannot introduce
non-scalars are superseded: **valid UTF-8 C input** cannot; malformed
input can. No binary ambiguity was needed for these recorded arguments.

#894's historical amd64 bdb gate: **15 POLICY-ACCEPTED, 9 MATCH**, no
skips/failures, on 24 cases. Nine MATCH means six scalar controls plus
three checks that zhuyin export symbols are absent, not three extra
malformed-text equivalences. The PR also reports the same selection on
kc after its fixture correction; no tkrzw or arm64 execution is claimed
for #894. The `nul-as-replacement` subject mutation yielded five DIFFER,
ten POLICY-ACCEPTED and nine MATCH. The records report the preceding
135 harness tests; the final #1153 PR reports 138 after the KC fixture
correction. These are inherited results, not executions by this PR.

Reproduction at the cited #1153 head (substitute `kc` for its fixture check):

```sh
bash tools/oracle/compat.sh --runtime docker --cell bdb --runners contract-diff --issue 894
uv run python tools/bisection/contract-diff.test.py
```
The NUL record describes the temporary subject mutation; do not mutate
the pin or use these tests to discover its behavior.

## G2 — C strings, joins and vectors

Pin: `pinyin.cpp:698-768,822-865,3295-3379,3381-3576`;
`storage/punct_table.cpp:74-94`; full `g_strdup`, `g_free`, `g_string_*`
and vector membership in the inventory TSV.
Rust: `capi/src/ffi.rs`, `owned_cstr`/`owned_cstr_list`;
`capi/src/aux_matrix.rs`, `SharedMatrix::{prefix,postfix,full,double,chewing}`
and `split_zhuyin`; `capi/src/text.rs`, `chewing_aux_text`;
`capi/src/iterators.rs` export getters; `capi/src/predict.rs`,
`prepend_punctuations`; `data/src/pin_stderr.rs`, `build_filename`.

| Partition | Verdict / record | Investigation |
| --- | --- | --- |
| Valid scalar phrase strings and their C marshalling | Identical within G1's valid-text scope. | Complete source argument there; other string consumers remain partial. |
| Non-scalar / NUL phrase text at marshalling or export | Accepted only as [non-scalar][non-scalar] / [NUL][nul]. | Source complete for those records, runtime limits as G1. |
| Auxiliary prefix/middle/postfix at every supported cursor: key interiors, key boundaries, tone digit/mark cuts, clamped overrun cursors | Identical for the three auxiliary exports. `split_zhuyin` is an ordinary `str` scalar cut; the `g_strconcat` and `g_utf8_substring` shapes are gone. | Source trace below; new bdb cases `aux-text-chewing-cursors`, `aux-text-chewing-zhuyin-cursors`, `aux-text-chewing-overrun-cursor` MATCH, and the existing aux family 11/11 MATCH. |
| Missing middle: the `g_strconcat(prefix, NULL, postfix, NULL)` stop | Unreachable in a supported operation; the walk cannot pass the cursor without setting `middle`, so the NULL-stop branch was dead and is removed. | Source proof below. No case can witness it because the pin cannot reach it. |
| `g_utf8_substring` at the spelling's end (`len ==` scalar count) | Identical visible result: the right half is empty, which `split_zhuyin` produces at the terminator. | Source below; `ni1@2` and `ni3hao4@6` in the new cases. |
| `len >` scalar count: a cursor inside a long key past its zhuyin spelling | Accepted class (b): `g_utf8_offset_to_pointer` walks past the allocation, so no deterministic bytes exist; oxpinyin keeps the right half empty. Registered in [upstream divergences](upstream-divergences.md#chewing-auxiliary-substring-over-read). | Source below; not a differential case, because the pin side has no reproducible result. |
| Bigram export's unterminated pronunciation vector | Accepted class (b), [row 1 record](upstream-divergences.md#bigram-export-iterators-pinyin-buffer). | Established independent fault; #894 traces its text consumers but does not turn this fault into a deterministic phrase-text witness. |
| Raw path bytes: see G3 (#922). Embedded NUL in diagnostics; duplicate punctuation strings/order | Paths classified under G3; the rest pending #924/#928/#1144, with G8/G15. | No blanket “malformed quirk” admission. |

### G2 auxiliary-text closure — 2026-10-25 UTC

The three exports read one matrix. `pinyin_parse_more_full_pinyins`
(`pinyin.cpp:1497-1517`), `pinyin_parse_more_double_pinyins`
(`:1540-1560`) and `pinyin_parse_more_chewings` (`:1582-1605`) fill it
through `fill_matrix` (`storage/phonetic_key_matrix.cpp:30-80`): one column
per consumed byte, each key at its `m_raw_begin`, zero keys in the gaps
(apostrophes), and the reserved zero key at `parsed_len`, so
`matrix.size() == parsed_len + 1`. Each export clamps the cursor with
`std_lite::min(cursor, instance->m_parsed_len)` (`:3390`, `:3449`, `:3527`),
leaving `cursor < matrix.size()`. `_get_aux_text_prefix` and
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
reaches the concat. The old helper's `middle == NULL` arm was therefore dead,
and its removal changes no supported output.

**`g_utf8_substring`'s end of string.** The chewing split calls
`g_utf8_substring(zhuyin, 0, len)` and `g_utf8_substring(zhuyin, len, end)`
(`:3557-3558`), where `len = cursor - begin` is a raw-input byte delta and
`end = m_raw_end` an absolute raw byte offset. GLib 2.89.4-2
`g_utf8_substring` (`glib/gutf8.c:298-322`) resolves
`start = g_utf8_offset_to_pointer(str, len)`, then
`end = g_utf8_offset_to_pointer(start, end_pos - len)`, and copies
`end - start` bytes plus a terminator. `g_utf8_offset_to_pointer`
(`:376-386`) advances by `g_utf8_next_char`, a byte-width step from
`g_utf8_skip` that does not stop at the terminator. With `C` the spelling's
scalar count:

- `len < C`: `left` is the first `len` scalars. The right copy requests
  `end - len` scalars; for every splittable row `end >= end - begin = B >= C`
  (checked against all 441 `content_table` rows at the pin: only the
  one-byte incomplete `w` and `y` placeholders have `B < C`, and neither has
  an interior cursor), so the copy includes the terminator and the visible
  `right` string is the remaining scalars.
- `len == C`: `start` is the terminator, so `right` is empty; `left` is the
  whole spelling.
- `len > C`: `start` is already past the terminator and the result is heap
  bytes. That is an out-of-allocation read, undefined with no reproducible
  bytes.

`split_zhuyin` cuts the spelling at scalar `len`, or leaves an empty right
half when `len >= C`. It therefore produces exactly the first two cases with
ordinary `str` operations and answers the third deterministically with an
empty right half. The third is the class (b) difference registered in
[upstream divergences](upstream-divergences.md#chewing-auxiliary-substring-over-read);
the pin's bytes there cannot be reproduced safely. `g_strconcat`
(`glib/gstrfuncs.c:572-609`) is a plain concatenation once the NULL stop is
removed.

**Cases.** `aux-text-chewing-cursors` visits every byte cursor on four
full-pinyin inputs plus the maximum `size_t`, covering both key boundaries,
interior cuts, the tone-1 terminator (`ni1@2`) and the tone-marked terminator
(`ni3hao4@6`). `aux-text-chewing-zhuyin-cursors` does the same over the
zhuyin parser (`su3cl3`). Its Standard-keyboard keys spend one keystroke per
zhuyin scalar plus the tone key, so an interior cursor's `len` stays below
`C`; the terminator falls on the key boundary, which the `cursor == offset`
branch takes rather than the substring branch. The terminator cut is
therefore exercised on the shared-matrix path by `ni1@2` and `ni3hao4@6`.
(Longer keyboards such as CP26 can spend two keys on one scalar, so their
interior `len` can exceed `C`; `split_zhuyin` applies the same class (b)
clamp there.)
`aux-text-chewing-overrun-cursor` uses one past, seven past and the maximum
`size_t` on both parse paths; all clamp to `parsed_len` at `:3527`. The
missing-middle and `len > C` partitions have no case for the reasons above.
Output ownership is unchanged: the matrix-empty branch writes an allocated
`""` and returns `false` (`:3385-3388`, `:3444-3447`, `:3522-3525`); every
other answer is `true` plus one caller-owned allocation, still marshalled by
`owned_cstr` under #924. No preedit or candidate path calls `split_zhuyin`;
the only callers are `SharedMatrix::chewing` and `chewing_aux_text`, both
reached only from `pinyin_get_chewing_auxiliary_text`.

## G3 — Path joining

Pin: `pinyin.cpp:164,176-226,252,268,286,308,331-336,353-368,395,400,419-439,952,968,985,997-1016,1052-1119,1260,1277`;
`zhuyin.cpp:118,130-170,196,212,230,252,275-280,297-300,585,601,605,626,630,684`.
GLib 2.89.4 (identical to 2.90.0), `glib/gfileutils.c`, `g_build_path_va` as
`g_build_filename` calls it with `"/"` on Unix (the reference image's GLib;
the `g_strdup` of the two directories at `pinyin.cpp:331-332` and
`zhuyin.cpp:275-276` is a byte copy).
Rust: `data/src/table_info.rs`, `pin_join`/`join_bytes` (used by
`FileName::under`); `data/src/pin_stderr.rs`, `build_filename`;
`user/src/store_libpinyin.rs`, the fini `write … failed.` line;
`capi/src/ffi.rs`, `cstr_to_path` and the zhuyin counterpart.
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
| Ordinary directory/name pairs, empty `dir` (cwd-relative bare names), empty `name` | Same effective path as `Path::join`; `pin_join` yields identical bytes. | Complete (source argument above; `glib-922-dir-*`, `glib-922-row-*`). |
| Leading `/` in a `table.conf` file name (`/x`, `//x`, `sub//x`) | Stays beneath the directory, never absolute: `pin_join` keeps the pin's file. Not removable. | Complete; `glib-922-row-*` open the same library as the pin, `glib-922-row-missing-*` write the same `mmap … failed!` bytes. |
| Repeated or trailing separators on the directory (`/d///`) | The file is the same; the pin's printed path has one separator (`/d/x`). `pin_join` and `build_filename` now write that. They wrote `/d///x` (and `/` for a `//` directory) before. | Complete for the joins through `pin_join`/`build_filename`; see the residual row. `glib-922-row-missing-dup-trailing-dir`, `glib-922-table-conf-missing-slashes`, `glib-922-fini-write-failed-dup-trailing`. |
| Directory or name made only of separators (`/`, `//`) | Kept whole as the law says (`//` + `x` is `//x`; `/a` + `//` is `/a//`). Same file as a plain join on Unix, different bytes only in diagnostics. | Complete; unit `file_names_stay_beneath_their_directory`, `glib-922-table-conf-missing-{slash,slashes,many-slashes}`. |
| `..` and `.` segments, leading `//` on the directory | Untouched by the pin and by `pin_join`; `Path` semantics resolve them identically. | Complete; `glib-922-dir-both-{dotdot,dot-segments,leading-dup}`. |
| User directory identity: `""` is a directory (cwd), NULL is none; `g_strdup` keeps the bytes | Unchanged. `cstr_to_path` is already a byte copy into `OsStr`, the natural Rust form; nothing to simplify. | Complete; existing #619/#642/#643 cases plus `glib-922-dir-*`: same file set, train/save/reopen. |
| Non-UTF-8 directory or name bytes | Raw bytes everywhere (`OsStr`, `path_bytes`); unchanged. | Complete; existing `stderr-non-utf8-user-dir`. |
| `\` as a separator | An ordinary byte on Unix (GLib uses `/` only there). On Windows GLib accepts both; `pin_join` follows that but no pin run covers Windows. | Unix complete; Windows unverified (no reference there). |
| Residual: the other `dir.join(<constant name>)` sites, listed below | Same file always. Derived from the join rule and the pin, not run: with a directory ending in two or more separators (`/u///`), `Path::join` appends the name without adding one (`/u///user.conf`), while the pin's `g_build_filename(user_dir, USER_TABLE_INFO, NULL)` (`pinyin.cpp:175-176`, `zhuyin.cpp:129-130`; GLib `g_build_path_va` trims the directory's trailing separators and inserts one) gives `/u/user.conf`, and `UserTableInfo::load` prints that name raw in `open %s failed.` (`table_info.cpp:332`). So the line differs in bytes for such a directory and for no other. Not fixed here: #1190 edits these files. | Pending; follow-up after #1190 replaces these joins with `pin_join` and adds the user-directory witness to `glib-922-*`. |

Remaining `dir.join(<constant name>)` sites (items, not lines): in
`user/src/persistence.rs`, `check_format` (`user.conf` read and its open line),
the `user.conf` marker write, `user_conf_write_failed`, the user bigram,
pinyin-index and phrase-index loads, saves and `.tmp` renames; in
`data/src/dict.rs`, the system pinyin, phrase and addon index opens; in
`runtime/src/lib.rs`, the `table.conf` open, the system index and punctuation
opens and the `user.conf` text at the user-directory error; in
`user/src/store_libpinyin.rs`, the user bigram existence check (prints
nothing). Those that print a path (the `open`, `write` and `rename` lines)
differ only as the row above says.

No discrepancy is registered: every difference found is a defect fixed above
or the pending residual, and no valid configured file moves.

Verification: the 35 `glib-922-*` cases match on the bdb, kc and tkrzw
cells (tkrzw against a sound libtkrzw), and against the parent build on bdb
`--expect-parent` finds the 11 non-control cases differ and the 24 controls
match.

## G4 — GArray/GPtrArray containers

Pin: `pinyin.cpp:2805,2817`; `zhuyin.cpp:1797,1809`;
`lookup/phonetic_lookup.h:231-239,298-339`; the complete 514-site container inventory
in the TSV. Rust: both `capi/src/dict.rs` and `zhuyin-capi/src/dict.rs`,
`*_lookup_tokens`/`*_token_get_nth_pronunciation`; internal
`engine/src/nbest.rs::Step`, session vectors and phrase-library slices.

| Partition | Verdict | Investigation |
| --- | --- | --- |
| Caller-owned real GArray mutation | Contract: MUST RETAIN GLib metadata/ownership, #924; direct `glib_sys` mutation is not emulation. | Established ownership constraint; full lifecycle/allocator audit still partial (G9). |
| Valid internal step/candidate/result arrays | Inventory source conclusion: ordinary parity via vectors/slices. | Partial across all container sites; sorting separately G5/G6. |
| Out-of-range indices, key with no whole word, short chunk header | Pending per-site classification here; named existing (b)/(c) records are not a blanket container exception. | Unverified family-wide coverage; chunk specifics G12. |

## G5 — N-best tail sort

Pin: `lookup/phonetic_lookup.h:174-178,329-339` (`trellis_value_compare`,
`g_ptr_array_sort`). Rust: `engine/src/nbest.rs`, `glib_merge_sort`,
`tail_compare`, `c_float_to_gint`. Both facades, all backends; #921.

| Partition | Verdict / record | Investigation |
| --- | --- | --- |
| Ordinary finite tails and stable/tied ordering | Pending replacement classification; order is valid-input contract. | Partial inventory trace; full comparator/caller proof unverified. |
| Transcendental accumulation near-ties | Accepted class (a) only within [row 11's record][math-record], not every comparator difference. | Existing math finding; G14 does not complete #921. |
| NaN, out-of-range float differences, non-transitive comparisons | Pending classification. | Unverified; source does not alone settle narrowing/evaluation. Both-target binary symbol/offset evidence must be supplied before a proposed removal. G6's different unsigned conversion is not a substitute. |

## G6 — Candidate and predicted-list sorting

Pin: `pinyin.cpp:1678-1709,1794-1868,2041-2138,2286,2443`;
`zhuyin.cpp:1129-1144,1229-1270,1375,1529,1610`.
Rust: `engine/src/session/mod.rs`, `glib_rank_sort`, `RankKey::pin_compare`,
`guint32_of_score`; `capi/src/predict.rs`, `sort_predicted`.
Owners #927/#928; preserve #913/#914 valid-input ranking.

| Partition | Verdict / record | Investigation |
| --- | --- | --- |
| Valid candidate order, equal keys, stable dedup winner and prediction order | Inventory records preservation; complete replacement verdict pending #927/#928. | Partial, not permission to substitute an unstable sort. |
| NUL/non-scalar stored text affecting membership, dedup or ranking | Accepted only within [NUL][nul] / [non-scalar][non-scalar] source traces. | Source analysis established; #894 isolated witnesses do not exercise every dedup collision. |
| Out-of-range unsigned frequency stores in #1137's five named cases | Identical to amd64 reference; arm64 pin portability defect, no oxpinyin exception. [Binary finding](arm64-pin-portability-1137.md). | Established source/binary result for those sites/cases; not all frequency-wrap or NaN cases. |
| Other `gint` frequency wrap, NaN scores, comparator ordering and shared-helper callers | Pending classification. | Unverified beyond the bounded findings above. |

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
Rust: both facades' `ffi.rs::log_warning`/`log_critical` and
`phrase.rs::segment_text` (`g_return_if_fail_warning`, GLib conversion).

| Partition | Verdict / record | Investigation |
| --- | --- | --- |
| Recorded #894 non-scalar candidate refusal versus NUL candidate omission | Accepted [non-scalar][non-scalar] / [NUL][nul]; warning only on the former witness. | Complete bounded records; G1's exact tested scope. |
| Registered pin assertion/abort sites | Class (c) only at its [registered sites](compatibility-policy.md#c-availability--upstream-aborts-on-caller-input): false/Err and exactly one warning, correct domain/level. | Existing policy obligation; full G8 site audit partial. |
| `phrase_segment` conversion failure, critical precondition and previous result; diagnostic NUL/text/allocator cases | Pending #918/#924. Required diagnostics are observable contract. | Unverified whole lifecycle; neither silence nor generic “quirk” classification is admitted. |

## G9 — Allocation/free and real GArray ABI

Pin: `pinyin.cpp:2805,2817`; `zhuyin.cpp:1797,1809`;
`pinyin.cpp:1452,1459,1644`; `lookup/lookup.cpp:54-65`; complete allocation
sites in TSV. Rust: both `ffi.rs::owned_cstr`/`owned_cstr_list`, `dict.rs`
GLib array writers, `phrase.rs::segment_text`; ownership slots in
`libpinyin.alloc`/`libzhuyin.alloc` (under their C facade crates).

| Partition | Verdict | Investigation |
| --- | --- | --- |
| Normal returned strings/vectors and caller arrays | Contract: malloc-compatible strings freeable with `g_free`, vectors with `g_strfreev`, real GArray metadata retained. MUST RETAIN, #924. | Inventory establishes boundary; full allocation/lifetime audit partial. G1 proves its text records preserve ownership. |
| NULL inputs/outputs, allocation failure/OOM, cleanup after partial allocation/conversion | Pending exact per-site classification. | Unverified full family; #894 explicitly excludes allocation failure. No ownership exception inferred. |

## G10 — Rename and sync

Pin: `pinyin.cpp:915-1130` (renames `:1059,1078,1094,1108,1121`);
`zhuyin.cpp:585-693`; `include/memory_chunk.h:555`.
Rust: `user/src/persistence.rs`, `save_with_bigram_reporting`,
`pin_rename_order`; `data/src/phrase_library.rs` chunk saves. #929.

| Partition | Verdict | Investigation |
| --- | --- | --- |
| Successful valid-profile save, per-library then index/bigram order | Inventory source conclusion: same file bytes/order. | Partial; proposed changes need same-backend persistence gates. |
| Per-file write, rename or sync failure, partial saved state, return value | Pending classification. | Unverified complete failure ordering across both facades/backends; no quirk admission. |

## G11 — Numeric parsing and locale

Pin: `storage/table_info.cpp:160,197-291,328-394` (scanner sites
`:208,214,220,228,232,255,338,344,352,357`; locale
`:197,291,328,372,378,394`). Rust: `data/src/table_info.rs`, `Scan`,
`strtol_digits`, `atoi_guint8`, `hex_float`; `data/src/user_files.rs`,
`Scan::int` and `UserTableInfo::to_text`; both facade
`locale.rs::pin_table_info_locale`; `training/src/segment/model.rs`,
`parse_table_conf_lambda`.

The reference scanner is glibc **2.43-3** from the Debian
`glibc-source` package: `stdio-common/vfscanf-internal.c` for the
`%d`/`%f`/`%255s` state machines and `stdlib/strtod_l.c` for the float
value. The source walk below is the basis for each verdict; the corpus
runs are confirmation only. “Complete” means the behaviour has a contract
case or a unit test; every case named below runs on both facades unless
marked otherwise.

| Partition | Verdict / record | Completion |
| --- | --- | --- |
| Directive literals and whitespace: a format space or `\n` matches any run of C white space, including none; leading bytes before the first literal do not match; one-line headers, indented `lambda`, CRLF and a missing final newline are accepted. | Contract; no exception. Source: `stdio-common/vfscanf-internal.c:452-486` (literal and white-space loop). | Complete: `table-conf-919-header-{one-line,crlf,indented-lambda,no-final-newline,leading-space}` and the missing-line cases. |
| Header order and missing fields: a missing `binary`/`model`/`lambda` directive is the pin’s ordinary `false`; a missing `database format:` line reuses the `source` word and aborts; an empty source or database value makes the next word the value and aborts at the converter; a missing `source` line reads an uninitialised `char str[256]` (class (b), not reproduced); a missing user-file row is the existing directory-read split. | Contract, except the missing-source class (b) refusal and the class (c) converter abort sites. Source: `stdio-common/vfscanf-internal.c:452-486` and `:1076-1259` (`%255s`); the missing-source class (b) is the pin's uninitialised `str` at `table_info.cpp:228-229`, not a glibc rule. | Complete: `table-conf-919-{binary-version,model-version}-line-missing`, `table-conf-abort-919-{database-format-line,database-format-value,source-format-value}-missing`, `table-conf-abort-919-source-format-line-missing`, existing `table-conf-abort-source-format-line-missing`. |
| `%d` grammar: leading white space, optional `+`/`-`, at least one ASCII decimal digit, stop at the first non-digit; no digits is a conversion failure; base 10 leaves `x` after `0x`; a truncated line leaves the same bytes. | Contract. Source: `stdio-common/vfscanf-internal.c:1452-1500` (sign/digit lexer); `stdlib/strtol_l.c:401-435` (digit loop). | Complete: `table-conf-919-versions-{plus-sign,no-digits}`, existing `versions-negative-overflow`, unit `integers_saturate_at_long_min_like_strtol`. |
| `%d` and `atoi` range: `strtol` saturates at `LONG_MIN`/`LONG_MAX`, the store into `int` is the low 32 bits on amd64/arm64, and an addon index is cut to `guint8`; `UserTableInfo::save` formats the two system versions with `%d`, so a `table.conf` value outside `int` is written back signed. | Contract; the writer was fixed here. Source: `stdlib/strtol_l.c:483-519` (overflow/saturation); `stdio-common/vfscanf-internal.c:1963-1984` (`int` store). | Complete: `table-conf-919-versions-{positive-overflow,int-wrap,negative}`, `table-conf-919-addon-atoi-nondigit`, `table-conf-abort-919-addon-positive-overflow`; `user_files::tests::an_out_of_int_version_is_written_signed`. |
| `%f` decimal grammar: optional sign; digits with an optional dot on either side; at least one digit; `e`/`E` exponent with optional sign and digits; a bare exponent mark, a second dot, or no digit is rejected because the scanner’s `got_digit` is cleared; the scan is non-backtracking. | Contract. Source: `stdio-common/vfscanf-internal.c:2006-2320` (lexer); `stdlib/strtod_l.c:607-940` (sign, digits, fraction, exponent). | Complete: `table-conf-919-lambda-{dot-leading,dot-only,positive-sign,exponent-plus,exponent-sign-without-digits,decimal-underflow}`, existing `table-conf-lambda-{exponent-without-digits,hex-exponent-without-digits}` and `float_spellings_read_like_glibc`. |
| `%f` hexadecimal: `0x`/`0X`; hex digits with an optional dot; at least one hex digit; optional `p`/`P` exponent with optional sign and digits; `e` is a hex digit, not the exponent; `0x`, `0x.`, `0x.p1` and a `p` without digits fail; the Rust `hex_float` rounds to nearest even, including subnormals and overflow. | Contract. Source: `stdio-common/vfscanf-internal.c:2140-2320`; `stdlib/strtod_l.c:684-700,732-756,850-940`; rounding at `strtod_l.c:213-357`. | Complete: `table-conf-919-lambda-{hex-no-exponent,hex-overflow,hex-dot-without-digits,hex-exponent-sign-without-digits,hex-exponent-minus-without-digits}`, existing `lambda-hex*`, unit `hex_floats_round_like_strtof`. |
| `%f` specials: case-insensitive `inf`, `infinity`, `nan`, and `nan(n-char-sequence)`; an incomplete `infin`, an unclosed `nan(`, or an invalid payload fails; the payload bits are not retained by Rust’s `f32::NAN`, but λ is only consumed in NaN arithmetic whose result is zero frequency/NaN costs, so the payload is not observable. | Contract; NaN payload is an unobservable internal difference, not an exception. Source: `stdio-common/vfscanf-internal.c:2034-2160`; `stdlib/strtod_l.c:640-684`. | Complete: `table-conf-919-lambda-{infinity,nan-payload,nan-unclosed}`, existing `lambda-inf`, unit `float_spellings_read_like_glibc`. |
| `%f` range and host libc: decimal overflow/underflow, hex subnormal/overflow and rounding match the reference; glibc 2.39 on Ubuntu 24.04 accepts `1e` and `0x1p` where 2.43-3 rejects them, and the four existing `host_scanf` cases report HOST-DEPENDENT rather than DIFFER. | Contract; host-libc dependent only for the registered spellings. Source: `stdlib/strtod_l.c:213-357` (round/overflow) and `:878-940` (exponent); `stdio-common/vfscanf-internal.c:2567-2576` (`got_digit`). | Complete: `table-conf-919-lambda-{hex-overflow,decimal-underflow}`, existing four `host_scanf` cases; the reference is the oracle image’s glibc 2.43-3. |
| `%255s` words: skip white space across newlines, take at most 255 bytes, retain NUL and non-UTF-8 bytes, then the C string stops at the first NUL; EOF after white space is an input failure; a 256th byte starts the next word. | Contract. Source: `stdio-common/vfscanf-internal.c:533-539` (width) and `:1076-1259` (`%s`). | Complete: `table-conf-919-word-{nul-truncates}`, `table-conf-abort-919-word-width-overlong`, existing `short-row-dropped`/`extra-word-dropped`; file names already cover non-UTF-8. |
| Six-word rows: words group across line boundaries; a final short row is dropped; a five-word row followed by the next row’s first word forms a six-word row; a merged row that mismatches the library and table makes the pin read a NULL item and die of SIGSEGV, while the subject returns the declared result (class (b)). | Contract on defined rows; the merged-row crash is class (b). Source: `stdio-common/vfscanf-internal.c:1076-1259` (`%s` stops at white space); the six-word loop is pin `table_info.cpp:254-287`. | Complete: `table-conf-919-short-row-merged` (declared class-(b) result), existing `short-row-dropped`, `extra-word-dropped`, `comment-dropped`. |
| Consumer mapping: system/user versions, addon index, λ `f32` bits, library names/types and file-type roles reach the same dictionary load/save and candidate results. | Contract. Source: the conversions above (`vfscanf-internal.c`/`strtod_l.c`); no additional scanner path. | Complete: the 68 #919 cases plus the existing 88 valid `table-conf-*` cases; the version writer fix is above. |
| Training λ reader: the old line-based `parse_table_conf_lambda` accepted only a line-prefixed plain decimal; it now parses the same full `table.conf` stream as the pin through `TableConf::parse` (hex, leading whitespace, one-line headers, incomplete-exponent rejection). The file is read as bytes. | Tool-only contract with the pin’s loader; no C-ABI change. Source: it reuses the full `TableConf` stream and therefore the scanner citations above; no independent scanner. | Complete: `training/src/segment/model.rs::the_lambda_reader_follows_the_pin_stream`, `paths::load_lambda` reads bytes; no facades entry. |
| Process `LC_NUMERIC`: the pin sets it to `C` and early returns skip the restore; oxpinyin leaves it at `C` and does not reproduce the process side effect. | **Needs a ruling to reverse**, [policy row 39](compatibility-policy.md), #923; retain. Not a scanner verdict: pin `table_info.cpp:197`; #923. | Not changed; real policy STOP, not an unread path. |

Completion gate (2026-10-24 UTC, linux/amd64, bdb): `cargo test -p
oxpinyin-data -p oxpinyin-training` passed, clippy and fmt passed, and
`--issue 919` ran **68/68 MATCH**. The named
`unsigned-user-conf-versions` mutation (the writer back to `%u`) made the
three version partitions DIFFER on both facades (**6 DIFFER / 62 MATCH**);
restored and rebuilt, the same 68 cases returned **68/68 MATCH**.

Confirmation only, not the basis of the verdicts: the glibc differential
corpus (2,648,804 mixed strings, 90,031 valid decimal/hex spellings,
24,687 `%d` strings) agreed with the source-derived lexers on values and
consumed bytes; the only differences were four NaN payload bits, which are
unobservable. The corpus is local test data and is not committed.

After the database-format-value fixture was made backend-independent
(match `database format:` and any following value), the same 68 cases ran
once on each cell: **bdb 68/68, kc 68/68, tkrzw 68/68**. This is a
targeted rerun for the fixture, not a three-cell gate.


### `user.conf` — #920

Pin: `storage/table_info.cpp:325-397` (`UserTableInfo::load` four
`fscanf`s at `:338,344,352,357`; `save` at `:372-397`;
`is_conform` at `:399-413`; `get_open_counter` at `:422-426`). Rust:
`data/src/user_files.rs` (`UserTableInfo`, `Scan::int`, `upstream_db_format`)
and `user/src/persistence.rs` (`check_format`, `write_marker`,
`clean_user_files`). The fixture helpers read the running cell's
`table.conf` values; no backend name is written into a case.

| Partition | Verdict / record | Completion |
| --- | --- | --- |
| Version `%d`: C white space, optional `+`/`-`, every ASCII digit; `strtol` saturates at LP64 `LONG_MIN`/`LONG_MAX` and the store through `int *` keeps the low 32 bits, so `2147483648` is `-2147483648`, `4294967303` is `7`, `9223372036854775808` is `-1`, and `-9223372036854775808` is `0`. | Contract; the pin's signed `int` bits are preserved. Source: `stdio-common/vfscanf-internal.c:1452-1500`, `stdlib/strtol_l.c:483-519`, `vfscanf-internal.c:1983-1984`. | Complete: `user-conf-920-version-overflow-{conforms,nonconforms}`, `user-conf-920-negative-overflow-nonconforms`; unit `the_version_directives_read_as_fscanfs_percent_d`. |
| Open counter `%d`: the same conversion into `int`; a missing/failed directive reads 0; a counter above `OPEN_COUNTER_LIMIT` is non-conforming and `get_open_counter` turns it into 0; negative/zero counters conform; the load raises `get_open_counter(existing) + 1` and fini lowers it. | Contract; state-preserving on conforming wraps, wiping on non-conforming ones. Source: `vfscanf-internal.c:1452-1500`, `strtol_l.c:483-519`; pin `table_info.cpp:357-359,422-426`. | Complete: `user-conf-920-counter-overflow-{conforms,nonconforms}`, `user-conf-920-counter-missing`; unit `the_counter_reads_as_fscanfs_percent_d`. |
| `database format:%255s`: a known C string selects the backend; an unknown token aborts (`to_table_database_format_type`); an EOF before the conversion leaves `UNKNOWN_FORMAT`; a missing literal or matching failure reaches the mapper on `str` that no conversion wrote (class (b)/(c)); a NUL ends the C string even though `%s` kept reading the word. | Contract for known/EOF; class (c) for unknown, class (b) for the unwritten `str`. Source: `vfscanf-internal.c:1076-1259` (`%s`); pin `table_info.cpp:350-354` and `strcmp` at `:122-131`. | Complete: `user-conf-920-nul-database-token`, `user-conf-920-database-missing-at-eof`; existing unknown-format abort case. |
| Missing/malformed version directive: the pin's `load` returns `false`, leaves the reset defaults (`0/0/UNKNOWN/0`), `check_format` sees a non-conforming marker and wipes the profile; a `%d` with no digit after an optional sign is that failure; a foreign byte after a version is what the next directive meets. | Contract; the wipe is the pin's, not a Rust error policy. Source: `vfscanf-internal.c:452-486` (literal/EOF) and `:1452-1500`; pin `table_info.cpp:338-348`. | Complete: existing `user-conf-abort-database-format-unknown` is the class-(c) neighbour; the #920 cases `version-overflow-nonconforms` and `negative-overflow-nonconforms` cover the wipe. |
| State preservation and conformance: a conforming marker (including a wrapped version equal to the system's, `7 + 2^32`, or a counter wrapping to 0) retains every user file and learned count; a non-conforming marker follows `_clean_user_files`, and `write_marker` writes the system versions with the raised/lowered counter. | Contract; #920's guard. | Complete: all eight `#920` cases assert the trained and imported unigrams after reopen; `user_listing` compares the files and canonical `user.conf`. |
| Round-trip writer: `UserTableInfo::save` writes versions with `%d` (signed), the database format canonically, and the counter signed; a marker read from a version outside `int` writes back the same signed values the pin does. | Contract; builds on #1178. Source: pin `table_info.cpp:372-397`; `user.conf` writer uses `i32` in `data/src/user_files.rs`. | Complete: `user-conf-920-version-overflow-*`, `user-conf-920-counter-*`, `user_files::tests::an_out_of_int_version_is_written_signed`. |
| Backend identity: the same four directives are used on bdb, kc and tkrzw; only the `database format:` value changes, and it is read from the cell's `table.conf`. | Contract; fixtures are backend-independent. | Complete: every #920 fixture derives binary/model/counter values from the cell and edits the database token generically; no literal backend name. |
| Process `LC_NUMERIC`: unchanged from #919/#923; the pin sets it to `C` and early returns skip the restore, oxpinyin leaves it at `C`. | **Needs a ruling to reverse**, policy row 39/#923. | Not changed. |

Completion gate (2026-10-24 UTC, linux/amd64, bdb):
`user-conf-920-*` **16/16 MATCH**. The named
`nul-database-token` mutation (remove the C-string truncation in
`upstream_db_format`) made the two NUL cases DIFFER and the other 14
controls MATCH; restored and rebuilt, **16/16 MATCH**. Because the
fixtures read the cell's own `table.conf`, the same 16 cases also ran once
on kc and tkrzw: **kc 16/16, tkrzw 16/16** (a targeted validation, not a
three-cell gate). The 24,687-case `%d` corpus from #919 is confirmation
only; it is not the basis of these verdicts and is not committed.

## G12 — File I/O and MemoryChunk

Pin: `include/memory_chunk.h:72-78,424-520,555`;
`storage/phrase_index.cpp:354-361`; save/cleanup paths in G10.
Rust: `data/src/phrase_library.rs`, `map_chunk`, `MappedFile`,
`directory_chunk_fault`; `user/src/persistence.rs`;
`data/src/pin_stderr.rs` raw-byte diagnostics.

| Partition | Verdict / record | Investigation |
| --- | --- | --- |
| Valid file header, length/checksum, payload and mapped extent | Inventory source conclusion: compatible bytes/checks. | Partial full I/O audit; no claim to all read/write failures. |
| Short/truncated/unreadable system chunk leading to empty-chunk fault | Accepted class (b) only within policy row 76 and its [register](upstream-divergences.md). | Established named fault, not every malformed file; wider path coverage pending. |
| Directory with lseek size below header versus at least header size | Below header follows row 76; at least header reaches `assert` at `include/memory_chunk.h:493`, accepted class (c), [policy row 96 record](compatibility-policy.md). | Established filesystem-dependent split, not uniformly class (b). Row 96 records affected init/load/save/mask/addon callers and prior ext4/btrfs checks. |
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
Rust: `core/src/cost.rs`, `log2_fixed`, `surprisal`,
`cost_of_probability`; `engine/src/nbest.rs::poss`.
Owner [#1152](https://github.com/shenghaoc/oxpinyin/issues/1152), added after
#1146's “no replacement child” wording; **std maths is the direction**.

| Partition | Verdict / record | Investigation |
| --- | --- | --- |
| Last-bit differences following transcendental accumulation/std maths | Accepted class (a), [row 11 record][math-record] and current math rule. | Existing classification established; no new replacement evaluated here. |
| Proposed std replacement: ordinary probabilities, zero/special values, cost scale/rounding/saturation, ranking consequences | Pending exact implementation assessment under #1152; preserve required special-value handling and record permitted numerical changes. | Unverified proposed code/measurement; filing #1152 alone is not authorization to implement. |
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
| Well-formed, nonempty, scalar, terminated fields | Inventory source conclusion: identical strings. | Partial: inherited BEHAVIOR-COVERED applies only to exercised valid values; not all lookup/order states. |
| Non-u32-aligned byte length | Pending pin/subject classification; Rust decoder Err -> runtime empty vector. | Unverified pin load_entry/unescape outcome per backend. |
| Empty punctuation field | Pending pin/subject classification; Rust decoder Err -> runtime empty vector. | Unverified pin whole path per backend. |
| Non-scalar code point | Inventory identifies extended UTF-8 versus scalar rejection mechanism; pending complete discrepancy record #1144. | Partial mechanism only; G1's phrase-text record does not admit this independent producer. |
| Unterminated stored value | Inventory identifies class-(b) over-read candidate at `unescape`'s next-word access; complete outcome/classification pending #1144. | Partial source defect, unverified per-backend whole path. |
| Absent DBM entry | Pending comparative verdict; Rust `Ok(Vec::new())`, not decoder error. | Unverified pin per-backend outcome. |
| Zero-length stored value | Pending comparative verdict; Rust `Ok(Vec::new())`, bypasses decoder. | Unverified pin per-backend outcome. |
| Unopened table | Pending comparative verdict; Rust `Ok(Vec::new())`, bypasses decoder. | Unverified pin per-backend outcome. |

For the Rust empty-vector paths, prediction prepends no punctuation and
returns its normal true; existing predicted candidates need not be empty.
Neither that C result nor the shared empty vector proves the pin result.
No malformed partition above is certified by valid-only differential
coverage. No pending partition is a request for a human ruling yet.

## G16 — Tool-interface check

Pin: `storage/tag_utility.cpp:94-95,118-264` (`split_line`, `taglib_read`);
`storage/table_text_parser.h:34-74,76-120`;
`utils/utils_helper.h:51-72` (`TAGLIB_PARSE_SEGMENTED_LINE`);
`utils/storage/gen_binary_files.cpp:68-72,103` (`load_text` callers).
Rust: `data/src/interp.rs::parse_interpolation2_from_reader`,
`data/src/interp_grammar.rs::read`/`Tokens`/`is_graph`;
`training/src/counter/parse.rs::parse_ngseg`/`parse_segmented_line`/`atoi`;
`dictool/src/format.rs::parse`/`split_classic`. Owner #926.

| Tool input partition/interface | Verdict | Investigation |
| --- | --- | --- |
| Ordinary tagged interpolation input; spaces, printable tokens, required/ignored tags | Pending full interface equivalence. | Checked interface mapping and `taglib_read`/Rust grammar entry points; not a completed value/diagnostic/output audit. |
| Quoted escapes, dangling escapes, Unicode whitespace/graph categories, unexpected/missing tags | Pending classification. | Unverified against the pinned GLib category tables and all consuming tools; Rust `is_graph` alone is not proof. |
| Segmented `token phrase` stream, empty lines, repeated separators | Pending complete tool verdict; source maps two-piece space/tab split and empty-line null token. | Partial: the pin macro also validates token against phrase; reading the Rust parser is not proof of the full training pipeline. |
| Signed/overflowing/malformed numeric fields, missing separators | Pending classification. | Unverified `atoi`/abort/error and downstream file effects; no generic tool-scope quirk admission. |
| Four-field generation records, field-length limits, integer ranges and rejected rows | Pending whole `load_text` consumer verdict. | Source interface checked: table parser uses `g_ascii_strtoull`, bounded copies, four fields; generator callers identified. Full Rust datagen/output audit unverified. |
| Classic dictool two/three-field dictionary interchange | Pending against its own interface contract, [dictool format](dictool-format.md). | `split_classic` targets the ibus frontend's three-piece split, not the pin's four-field generator parser. Inventory grouping does not make the formats interchangeable. |

The inventory excludes these sites from the shared objects' exported
call graph; it does not exclude the tools' input/output contracts from
#926. This source-reading check identifies interfaces and limits, and
runs no tool or tests. Inclusion of `tag_utility.cpp` through headers does
not itself establish an exported call.

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

## Completion and next child

After all children land, verify combined main, reconcile shared-helper
changes, and account for every inventory entry before closing #915.
The third sweep alone does not close it. This ledger and an inventory
of all families do not mean all their investigations are complete.

At this record's date, #916/#917 and #894 are closed; #1153 is merged.
The next eligible child in #915's listed order is
[#918](https://github.com/shenghaoc/oxpinyin/issues/918): open, unassigned,
no issue comments or matching open PR found in the ownership check.
Start with both `phrase_segment` lifecycles and shared conversion callers,
not a deletion. Re-check ownership when work resumes. #923 stays stopped;
G14 tracks #1152. This docs PR implements no child.

Documentation validation: reviewed cited records, source sites, item names
and links; `git diff --check`. No new runtime or measurement result.

[non-scalar]: https://github.com/shenghaoc/oxpinyin/blob/a3cf66514a22eb25a8ec636d4f03f18bd7efbce1/docs/findings/894-non-scalar-phrase-text.md
[nul]: https://github.com/shenghaoc/oxpinyin/blob/a3cf66514a22eb25a8ec636d4f03f18bd7efbce1/docs/findings/894-nul-phrase-text.md
[math-record]: upstream-divergences.md#n-best-trellis-accumulates-gfloat-log-costs--not-reproducible-in-fixed-point-frozen-as-a-permanent-stage-1-divergence
