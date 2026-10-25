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
| Auxiliary prefix/middle/postfix at every supported cursor: key interiors, key boundaries, tone digit/mark cuts, clamped overrun cursors | Identical visible bytes for the three auxiliary exports while the pin returns. `split_zhuyin` is an ordinary `str` scalar cut; the `g_strconcat` and `g_utf8_substring` shapes are gone. The pin's right-half call walks past the spelling at essentially every interior cursor (next rows). | Source trace below; new bdb cases `aux-text-chewing-cursors`, `aux-text-chewing-zhuyin-cursors`, `aux-text-chewing-overrun-cursor` MATCH on visible bytes, and the existing aux family 11/11 MATCH. |
| Missing middle: the `g_strconcat(prefix, NULL, postfix, NULL)` stop | Unreachable in a supported operation; the walk cannot pass the cursor without setting `middle`, so the NULL-stop branch was dead and is removed. | Source proof below. No case can witness it because the pin cannot reach it. |
| Chewing interior cursor (`len <= C`) | Visible right half identical to a pin that returns: the copied NUL terminates it at the spelling's end (empty at `len == C`). The call is class (b): `end_pos` is the absolute `m_raw_end`, so `raw_end - len > C - len` whenever `raw_end > C` (outside GLib's documented domain), which is essentially every interior cursor; the copy reads past the allocation at `raw_end >= C + 2`. | Source below; the new cases compare visible bytes, so a match is conditional on the pin not faulting. |
| Chewing cursor past the spelling (`len > C`) | Accepted class (b): the copy starts past the terminator, so the pin's visible right half is heap bytes (and can fault); oxpinyin returns an empty right half. Registered in [upstream divergences](upstream-divergences.md#chewing-auxiliary-substring-over-read). | Source below; not a differential case, because the pin side has no reproducible result. |
| Bigram export's unterminated pronunciation vector | Accepted class (b), [row 1 record](upstream-divergences.md#bigram-export-iterators-pinyin-buffer). | Established independent fault; #894 traces its text consumers but does not turn this fault into a deterministic phrase-text witness. |
| Raw path bytes: see G3 (#922). Embedded NUL in diagnostics; duplicate punctuation strings/order | Paths classified under G3; diagnostic NUL producers closed under #924 below; punctuation remains #928/#1144, with G15. | No blanket “malformed quirk” admission. |

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

**`g_utf8_substring`'s absolute end position.** The chewing split calls
`g_utf8_substring(zhuyin, 0, len)` and `g_utf8_substring(zhuyin, len, end)`
(`:3557-3558`), where `len = cursor - begin` is a raw-input byte delta and
`end = m_raw_end` is an **absolute** raw-input byte offset. GLib 2.89.4-2
`g_utf8_substring` (`glib/gutf8.c:298-322`) resolves
`start = g_utf8_offset_to_pointer(str, len)` and
`end = g_utf8_offset_to_pointer(start, end_pos - len)`, then copies
`end - start` bytes and appends a terminator. `g_utf8_offset_to_pointer`
(`glib/gutf8.c:376-386`) steps `g_utf8_next_char` `end_pos - start_pos` times
and does not check for the terminator. The second call therefore asks for
`raw_end - len` scalars from scalar `len`, not for the `C - len` scalars that
remain; when `raw_end > C` the call is outside GLib's documented domain and
the walk passes the NUL; the copy reads beyond the allocation once the end
pointer is at least one byte past the NUL (`raw_end >= C + 2`).

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

The visible result is nevertheless deterministic while the pin returns.
`g_strconcat(left, "|", right, " ", NULL)` (`:3560`) measures `right` with
`strlen`, and for `len <= C` the copied buffer begins at scalar `len` and
ends at or after the spelling's NUL, so its first NUL is the spelling's end.
The visible right half is the spelling from `len` to `C` (empty at
`len == C`), matching `split_zhuyin`, even though bytes after the NUL were
read. For `len > C` the copy begins after the NUL, so the visible right half
is heap bytes; `split_zhuyin` returns an empty right half there as the
deterministic class (b) choice. The class (b) difference is therefore the
absent out-of-allocation read and possible fault where the copied end passes
the terminator (`raw_end >= C + 2`), the out-of-domain walk for
`raw_end == C + 1`, and the differing bytes for `len > C`; the visible bytes
for `len <= C` equal only when the pin returns. `g_strconcat`
(`glib/gstrfuncs.c:572-609`) is a plain concatenation once the NULL stop is
removed.

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
by `ni1@2` and `ni3hao4@6`.
(Longer keyboards such as CP26 can spend two keys on one scalar, so their
interior `len` can exceed `C`; `split_zhuyin` applies the same class (b)
clamp there.)
`aux-text-chewing-overrun-cursor` uses one past, seven past and the maximum
`size_t` on both parse paths; all clamp to `parsed_len` at `:3527`. The
cases compare the visible bytes; a match is conditional on the pin returning,
and the out-of-allocation read itself is not reproduced. The missing-middle
and `len > C` output partitions have no case for the reasons above.
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
| Caller-owned real GArray mutation | Contract: MUST RETAIN GLib metadata/ownership, #924; direct `glib_sys` mutation is not emulation. | Source-backed ownership/lifecycle audit below (G9); conditional allocation failures remain distinct. |
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
| `phrase_segment` conversion failure, critical precondition and previous result; diagnostic NUL/text/allocator cases | The bounded invalid-byte critical is MUST RETAIN and verified in the #924 matrix below; NUL reachability is closed for current diagnostic producers; conditional OOM outcomes are traced below, without same-allocation failure injection. | New #924 source proof and both-facade result-preservation cases; no blanket diagnostic exception. |

## G9 — Allocation/free and real GArray ABI

Pin: `pinyin.cpp:2805,2817`; `zhuyin.cpp:1797,1809`;
`pinyin.cpp:1452,1459,1644`; `lookup/lookup.cpp:54-65`; complete allocation
sites in TSV. Rust: both `ffi.rs::owned_cstr`/`owned_cstr_list`, `dict.rs`
GLib array writers, `phrase.rs::segment_text`; ownership slots in
`libpinyin.alloc`/`libzhuyin.alloc` (under their C facade crates).

| Partition | Verdict | Investigation |
| --- | --- | --- |
| Normal returned strings/vectors and caller arrays | MUST RETAIN: per-operation #924 matrix below covers all 38 registered slots and four caller-array operations. | Source-backed lifecycle audit and caller-mutation tests below; G1 retains its separate malformed-text scope. |
| NULL inputs/outputs, allocation failure/OOM, cleanup after partial allocation/conversion | #924 below traces conditional allocation outcomes and NULL caller-array diagnostics; malformed text retains G1 scope. | Four NULL-array defects verified; OOM outcomes source-derived without per-allocation injection. #894 excludes allocation failure. No ownership exception inferred. |

### #924 ABI ownership audit — 2026-10-25 UTC

**Decision: retain the GLib ABI.** This audit changes tests only. No new
runtime exception is admitted; `upstream-divergences.md` therefore needs no
new entry. The matrix settles the normal ownership boundaries, not every
malformed-input/diagnostic partition. **Refs #924**, not Fixes: the explicit
follow-ups below remain. Other families' rows above and below retain their
own scope and status.

Source trees read for this audit: libpinyin
`074a2219c90feaf962d0d24f034514033ece5f99` (detached checkout), GLib tag
`2.89.4`, commit `fa41d356ee4936264c45cf11fa6c2640a89fbdda` (the upstream
version of reference package 2.89.4-2), and oxpinyin `6be364d9`.
GLib citations in this subsection are to that tag; pin citations are to
074a2219 and start at `src/`. Expectations below were derived from these
sources before executing the new contract cases. Reference package metadata
was checked in the pinned dependency image: GLib **2.89.4-2**, glibc
**2.43-3**. GLib's packaged `changelog.Debian` says -2 only re-uploaded the
orig tarball with no source changes; -1 imported 2.89.4 and dropped included
patches. The actual `g_vasprintf` build branch was also inspected below.
The authorized `bminor/glibc` GitHub mirror cloned successfully, but fetching
`refs/tags/glibc-2.43` returned “couldn't find remote ref”; its newest release
tag was 2.42; the additionally authorized fetch of `release/2.43/master`
also returned “couldn't find remote ref” (HEAD `04e750e75b`, 2026-01-20). Neither session nor reference
image has an available deb-src configuration. **Open source-evidence gap:**
the matching glibc 2.43 allocator/vasprintf implementation and complete
Debian patch comparison have not been read. The packaged 2.43-2 changelog
mentions an arm64 malloc sysfs change; -3 lists resolver, wordexp, loader
and POWER10 memchr changes, not an amd64 malloc contract change. Changelog
inspection is not a substitute for reading that patch set. This gap does
not prevent the pin/GLib conditional traces or the permitted tests below;
no older glibc tag is cited as if it were 2.43.

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
| P/Z `get_sentence` | Fresh caller-owned bytes via `g_free` when a sentence is produced; no guessed results means false and untouched out-pointer. Caller may write/reallocate its buffer. | `pinyin.cpp:1463-1481`; `zhuyin.cpp:982-999`; `lookup/lookup.cpp:27-69`. Rust sentence construction plus `owned_cstr`/`write_owned_sentence` may replace repeated GLib joins. |
| P `get_full_pinyin_auxiliary_text` | Owned string, including allocated empty string on false with empty matrix; free regardless of success. | `pinyin.cpp:3381-3437`. `owned_cstr` may replace `g_strdup`/`g_strconcat`; G2 owns substring behavior. |
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
| P `iterator_get_next_phrase` | Two independent `g_free` buffers per row. Pin initializes outputs NULL/count -1, then writes row and returns true, including last row. Calling without a row is the existing row-62 abort/refusal exception, not an ordinary false. | `pinyin.cpp:698-768`. `malloc_cstr_with` writes final bytes directly; no need to recreate temporary GPtrArray, `g_strjoinv`, `g_strfreev`. |
| P `bigram_iterator_get_next_phrase` | Two independent `g_free` copies. Last row returns false **after** writing both strings; caller must free them. | `pinyin.cpp:896-910`. Rust copying is free; separate existing iterator safety exceptions apply. |
| P/Z `in_chewing_keyboard` | Caller owns both NULL-terminated vector and each string; may replace strings or resize vector, finally `g_strfreev`. Unmapped key returns false with NULL vector. | `pinyin.cpp:1615-1624`; `zhuyin.cpp:1087-1093`; `storage/zhuyin_parser2.cpp:301-332,496-544,798-851`. `owned_cstr_list` can directly allocate the vector instead of building/detaching GPtrArray. |
| P/Z `lookup_tokens` | Caller supplies a **real** GArray of `phrase_token_t`. Library clears previous contents, appends in library order, preserves clear callback/element size/zero termination/refcount/capacity semantics. Caller may append, shrink, reuse, ref/unref or free it. No-match false still clears. | `pinyin.cpp:2652-2667`; `zhuyin.cpp:1678-1693`; `storage/phrase_large_table3.h:77-97`. Rust may construct token payloads; GLib must mutate the caller object. Input conversion is separate follow-up, not a reason to change the array. |
| P/Z `token_get_nth_pronunciation` | Real caller GArray of two-byte `ChewingKey`; clear **before** item lookup, including missing-token false. Valid reading appends phrase-length keys. Caller mutation/free remains legal. | `pinyin.cpp:2801-2818`; `zhuyin.cpp:1793-1810`; `storage/phrase_index.cpp:33-43`. Rust payload extraction is free; GLib owns metadata changes. Out-of-range reading uses the existing safety classification, not invented pin bytes. |
| P/Z `phrase_segment` | No allocation transferred to caller. Pin's temporary `g_utf8_to_ucs4` buffer is freed after lookup; diagnostic refusal precedes result replacement. | `pinyin.cpp:1446-1460`; `zhuyin.cpp:965-979`. Current `phrase.rs::segment_text` already uses Rust scalar/raw-UCS4 decoding: there is no remaining GLib conversion allocation or `g_free` there. #918 owns that converter. |

The last row corrects the old inventory's description of `phrase.rs` at the
new baseline; it is not a proposal to remove a still-live foreign free.
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
Both `ffi.rs::owned_cstr` and `owned_cstr_list`, plus pinyin
`malloc_cstr_with`, use that allocation family. Partial-list failure frees
all preceding strings and the vector internally, never transfers a partial
list. Interior NUL rejection and allocator failure are **not** proved
compatible by successful allocator pairing; see the follow-ups.

#### Diagnostic channels and macro-expanded sites

| Exported operation / trigger | MUST RETAIN outward result and channel | MAY DIVERGE / status |
| --- | --- | --- |
| P/Z `phrase_segment`, well-bounded `ff fe` after successful segmentation | Exactly one NULL-domain critical (level 8), `bool <facade>_phrase_segment(<facade>_instance_t*, const char*): assertion 'num_of_chars == ucs4_len' failed`; false; previous result/count/token outputs remain readable and identical. Pin `pinyin.cpp:1450-1459`, `zhuyin.cpp:969-978`; GLib `gutf8.c:950-990` leaves initialized converted length 0 on failure. | Rust decode/Result control flow is free; exact text is easy and safe to reproduce, so keep it. New `glib-924-segment-critical-{pinyin,zhuyin}` asserts trigger, text, domain, level, return and old result, not just stderr. |
| P/Z `init`, `load_phrase_library`, `save`, `mask_out`; additionally P `load_addon_phrase_library`, reached malformed sub-index separators/end | `storage/phrase_index.cpp:354-356,361` expands four `g_return_val_if_fail` checks; first failed reached check emits one NULL-domain critical and returns false from `SubPhraseIndex::load`. Facade outcomes are caller-specific: the load caller may ignore false; `save` may report true after a critical. Never convert all these into facade false or silence. | Existing `LayoutFault::critical_message`, context/config reporting and inherited G8/compatibility records govern these paths. Precise C++ signature imitation is formatting in an adapter, not a Rust-language impossibility. Source trace below separates bounded first-three checks from out-of-bounds/dangling-view paths; existing G8 exceptions remain independently scoped. |
| P/Z `token_get_nth_pronunciation`, NULL `keys`, valid instance and known token/in-range reading | GLib `g_array_set_size` precondition emits `GLib`-domain critical `g_array_set_size: assertion 'array' failed`; append emits a second critical `g_array_append_vals: assertion 'array' failed`; both return NULL locally, facade still returns true, no caller output exists. With unknown token: only first critical, facade false. Pin `pinyin.cpp:2805-2818`, `zhuyin.cpp:1797-1810`; GLib `garray.c:611,778`. | **Unresolved defect/follow-up in reserved dict.rs:** current Rust early false is silent. Not a permitted diagnostic-only difference (the valid-token return also differs), not a pin crash. No changes to either dict.rs in this lane. Source-derived proof then `glib-924-null-array-pronunciation-{pinyin,zhuyin}`: both DIFFER. Pin known-token true/two criticals, unknown-token false/one critical; Rust both false/silent. |
| P/Z `lookup_tokens`, NULL output array, valid instance/scalar phrase | `reduce_tokens` calls set_size then append for each non-NULL prepared library array, even length zero: one plus the number of prepared arrays `GLib` criticals, each returning NULL locally; facade still returns `SEARCH_OK & retval`. No out-array is written. Pin `storage/phrase_large_table3.h:77-97`; GLib `garray.c:611-614,778`; facade citations above. | **Unresolved reserved-file follow-up:** current silent false is not equivalent. Number of append diagnostics depends on loaded libraries; do not hard-code one warning or claim the pin dereferences NULL here. Source proof then `glib-924-null-array-lookup-{pinyin,zhuyin}`: both DIFFER. The fixture prepares seven library arrays, so both found/missing calls emit eight criticals; pin returns true/false respectively, Rust false/false silently. |
| Facade refusals mapped from registered asserts/unsafe reads | Existing class-(b)/(c) records retain each operation's trigger, false/NULL/out-state and exactly one facade-domain warning (level 16); `ffi::log_warning` uses `libpinyin`/`libzhuyin`. This channel is distinct from the pin's NULL-domain critical and raw stderr. | All call sites remain. No blanket permission to remove/rename messages. Full per-trigger re-audit of the existing warning register is not completed here; inherited witnesses are not new executions. |
| Both `ffi::log_warning`/`log_critical`, hypothetical embedded NUL in message | `CString::new` currently drops the whole diagnostic on embedded NUL; GLib `%s` truncates at NUL. A concrete reachable trigger and complete outward proof are required before classifying any difference. | No reachable embedded-NUL producer found in the current C-facing call graph; source closure below. The mechanical helper difference is not an admitted outward difference or a reason to edit reserved zhuyin ffi.rs. |

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


The retained warning entry-point inventory (Rust item names at the stated
baseline; **MUST RETAIN**, no per-trigger closure implied) is:

| Adapter path | Exported operations reaching the warning channel |
| --- | --- |
| Both `context.rs` | P/Z `init`, `save`; context load-fault reporting additionally uses the NULL-domain critical channel. |
| Both `config.rs`; pinyin `state.rs::load_addon`/`unload_addon` | P `set_full_pinyin_scheme`, `set_double_pinyin_scheme`, `set_zhuyin_scheme`; Z `set_full_pinyin_scheme`, `set_chewing_scheme`; P/Z `load_phrase_library`, `unload_phrase_library`, `mask_out`; P `load_addon_phrase_library`, `unload_addon_phrase_library`. |
| Both `cursor.rs` | P `get_pinyin_key`, `get_pinyin_key_rest`, `get_left_pinyin_offset`, `get_right_pinyin_offset`; Z `get_zhuyin_key`, `get_zhuyin_key_rest`, `get_left_zhuyin_offset`, `get_right_zhuyin_offset`. `warn_on_empty_matrix` supplies the operation-specific name. |
| Both `keys.rs` | P/Z `parse_full_pinyin`; P `get_pinyin_is_incomplete`. |
| Both `sentence.rs`, `phrase.rs` | P/Z `guess_sentence`, `guess_sentence_with_prefix`, `get_character_offset`, `phrase_segment`; P `get_sentence`, `guess_candidates`; Z `guess_candidates_before_cursor`, `guess_candidates_after_cursor`. `refuse_lookup_gram` and offset/tone validation are shared callers, not extra exported channels. |
| Both `candidates.rs` | P/Z `train`; P `get_candidate_nbest_index`, `remove_user_candidate`, `choose_candidate`, `choose_predicted_candidate`. |
| Both `iterators.rs` | P/Z `iterator_add_phrase`; P `begin_get_phrases`, `iterator_get_next_phrase`, `begin_get_bigram_phrases`, `bigram_iterator_get_next_phrase`. |
| P `predict.rs::guess_predicted_result` | P `guess_predicted_candidates`, `guess_predicted_candidates_with_punctuations`. Reserved file, unchanged. |
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
Rust `phrase_library.rs:783` constructs fixed-expression layout faults;
`phrase_libraries.rs:117-139` queues them for context/config reporting,
while save reports at its callback. Further repairs in #1190's persistence
path are follow-ups, not missing pin source.

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
(`glib/gmessages.c:1274-1313`). Both new
`glib-924-segment-fatal-critical-{pinyin,zhuyin}` cases MATCH (SIGTRAP),
retaining the real channel's caller-selected fatal behavior.

Diagnostic NUL reachability is closed for this baseline's C-facing
producers, not for arbitrary Rust calls to `log_warning(&str)`. Literal
messages, exported-operation names and assertion expressions are NUL-free.
`LayoutFault.expression` is static (`oxpinyin-data/src/phrase_library.rs:82-97`);
`TableConfError` warnings are static (`table_info.rs:280,322`), as are
`ChunkHeaderWrite` expressions (`oxpinyin-user/src/persistence.rs:405`).
The dynamic init message (`oxpinyin-capi/src/context.rs:81`, zhuyin `:102`)
formats a typed `OpenError` and debug-formatted system path. Input paths
come through CStr; configured filenames stop at the first NUL
(`oxpinyin-data/src/table_info.rs:523,661-662`). Dictionary/layout errors
format fixed text or numeric bounds, not raw stored phrase bytes. Backend
messages terminate at CStr (`oxpinyin-store/src/bdb/ffi.rs:178-187`,
`kyotocabinet/ffi.rs:547-558`, `tkrzw/mod.rs:213-224`). User-profile open
errors either become fixed typed failures or go to raw stderr and degrade
the store (`oxpinyin-runtime/src/lib.rs:2133-2189`), not arbitrary GLib
warning strings. Thus the hypothetical drop-whole-message versus `%s`
truncation has no current exported trigger. Stored-phrase NUL remains G1's
separate returned-text contract. No helper-only artificial input establishes
an outward discrepancy.

#### Allocation-failure partitions (source-derived, not failure-injected)

GLib fatal allocation and fallible conversion are different contracts.
`g_malloc`/`g_realloc` call fatal `g_error` (`glib/gmem.c:95-106,164-177`),
whereas conversion's `try_malloc_n` uses the nonfatal allocator and returns
NULL (`glib/gutf8.c:905-912,977-990,1034-1068`). All these pin conversions
pass NULL error pointers: conversion OOM itself emits no diagnostic and
leaves `items_written` untouched. The reference package 2.89.4-2 defines
`GLIB_USING_SYSTEM_PRINTF`; its `g_vasprintf` object calls system vasprintf
and aborts on ENOMEM, matching `glib/gprintf.c:339-357`: minimal raw stderr,
not `g_log`. Other printf failures set the output NULL. This build branch
was checked in the reference dependency image, without running the pin to
infer expectations.

| Allocation boundary / exported operations | Conditional pin result, return/out-state/diagnostic | Subject and disposition |
| --- | --- | --- |
| Real caller arrays, all four operations | GLib growth failure terminates through fatal allocation; no normal return. | Actual GLib growth retained. Rust payload preparation may terminate earlier. MUST RETAIN ABI; identical allocation graphs are not claimed. |
| P/Z key renderers, P dual pinyin strings and auxiliary text | Duplication/concatenation OOM is fatal; formatted renderer ENOMEM aborts with raw stderr as above. | `owned_cstr` first allocates Rust CString, then libc storage. The former can terminate; the latter returns NULL. P renderers/dual getters can return true with NULL, Z renderer false/NULL; auxiliary writer false/NULL. Unaccepted conditional differences; adapter follow-up, including reserved zhuyin ffi.rs. |
| P/Z keyboard vector | GLib vector/string allocation OOM is fatal, no returned partial vector. | Checked vector allocation or element libc failure returns false/NULL after freeing every prior string and vector. Earlier Rust allocation can terminate. No leaked partial-list transfer; successful pairing is correct, OOM returns are not equivalent. |
| P/Z `token_get_phrase` | Conversion allocation failure writes declared length and NULL text, returns true, no warning (`pinyin.cpp:1627-1645`; `zhuyin.cpp:1096-1113`). | Subject writes length and NULL but returns false. Reserved dict.rs follow-up, not an accepted quirk. |
| P/Z `get_sentence` | Each token conversion is fallible; a NULL conversion can leave NULL before a prefix exists or preserve an existing prefix, omitting that token. Later dup/concat allocation can terminate; converter otherwise returns true (`lookup/lookup.cpp:27-69`). No conversion warning. | Final subject buffer failure returns false/NULL; no per-token partial assembly. Conditional difference requires adapter follow-up, not C-like temporary allocations in the engine. |
| P unigram iterator | Phrase conversion can yield NULL; if later fatal-allocation stages succeed, row still writes reading/count and returns true (`pinyin.cpp:698-768`). Reading allocation OOM terminates. | Independent final allocations may yield NULL phrase and/or reading while returning true and advancing. No partial-allocation rollback is promised by either interface; reading OOM differs. |
| P bigram iterator | Internal pair conversion can yield NULL first phrase (`g_strconcat` returns NULL) or omit a NULL second phrase; later returned-string duplication OOM is fatal (`pinyin.cpp:822-836,896-910`; `glib/gstrfuncs.c:576-578`). | Final allocations can yield NULL while retaining last-row return/advance. Internal unsafe unterminated vector remains its existing exception; no new OOM admission. |
| P/Z `phrase_segment` temporary conversion | Positive nonempty input with failed conversion leaves length zero: critical, false, old result retained. Empty input has zero equal lengths and proceeds to empty successful result, if subsequent fatal allocations succeed (`lookup/phrase_lookup.cpp:36-52,129-157,382-433`). | Rust conversion allocation can terminate; no identical-site fallible conversion exists. #918 owns converter, not this audit. |
| P/Z `iterator_add_phrase`, `remember_user_input`; P `get_character_offset` | Failed conversion leaves initialized length zero; return false before adding a phrase/remembering/writing offset (P `:628-646,3678-3681,3210-3213`; Z `:513-532,2165-2168`). Parser allocation after add-phrase conversion can terminate first. | Rust earlier allocations can terminate. Preserve ordinary return/output contracts, no same-pressure equivalence inferred. |
| P/Z `lookup_tokens` temporary conversion | Zero initialized length on conversion failure searches zero-length input, then clears output and returns false (G1 closure; P `:2652-2667`, Z `:1678-1693`). | Caller-array ownership is independent of this converter partition; reserved dict.rs follow-up remains. |
| P/Z `guess_sentence_with_prefix` | Failed conversion skips prefix token additions, continues lookup with sentence_start and returns lookup result, no conversion warning. P frees prior prefix, stores NULL but leaves prior prefix length unchanged on failure; Z uses local zero length and frees temporary (P `:1389-1443`, Z `:912-962`). | Rust allocation can terminate before this fallback; no attempt to imitate C allocation staging in Rust. |
| Context/instance/iterator handles | Opaque C++ new allocation failure has no successful handle transfer. | Rust Box is appropriate; handle destructors retain ownership. Resource pressure thresholds need not match. |

Subject source: pinyin `ffi.rs:150-243`, zhuyin `ffi.rs:123-180`;
`malloc_cstr_with` additionally frees/refuses a failed fill or embedded NUL.
A checked-size overflow returns NULL before allocation; realistic exported
payload bounds do not license inventing a reachable usize overflow.
The pin and upstream GLib call sites in the allocation families above have been read; the exact glibc/Debian source gap remains explicitly open above.
There is **no existing per-allocation failure injector** in the contract
driver: a process-wide memory cap could fail the harness or an earlier
allocation, so it cannot verify these conditional outcomes. That is a
verification-tooling limitation, not unavailable source or an accepted
runtime exception. No new driver or speculative OOM experiment was added.

#### Remaining scope and verification

The NULL-array and conditional OOM differences need implementation work;
reserved dict.rs/zhuyin ffi.rs prevent those repairs in this lane. No source
access or authority is needed to finish their traces, now recorded above.
Broader G8 registered unsafe/assertion refusals, borrowed/static-slot
identity and malformed backing-item text retain their individual existing
records; channel inventory is not a new per-trigger certification of them.
No new difference has been proved impossible or unsafe to reproduce at a
safe C-adapter boundary, so none is admitted in `upstream-divergences.md`.
**Refs #924** remains appropriate.

New contract cases use the existing `contract-diff.py` driver. Array cases
exercise each facade's lookup and nth-pronunciation output after a seeded
clear callback, append 257 caller elements, shrink, call a refusing lookup,
verify clearing and zero termination, grow with zero fill, detach with
`g_array_free(FALSE)` and release with `g_free`. Both existing allocator
consumers now mutate/reallocate every owned string, and replace/append
vector elements before `g_strfreev`; borrowed and handle frees are untouched.


Initial checkpoint checks on linux/amd64, bdb, 2026-10-25 UTC: both capi package
test suites passed (150 tests total), fmt and clippy passed; **4/4 MATCH**
for `--issue 924`; allocator-pairing **26 + 12 slots passed**, including
both LeakSanitizer negative controls; ABI exports **79 + 52 passed**;
contract harness **142 tests passed**. Native GLib was **2.84.4**; source
expectations above use reference-version **2.89.4**, not host source.
The pin libraries and generated data were verified by `tools/cloud/setup.sh`.
The named subject-only `segment-critical-expression` mutation replaced
pinyin's assertion-expression literal: **1 INVALID, 3 MATCH**, rejecting
the changed message by the witness's source-derived assertion. Restoring
and rebuilding gave **4/4 MATCH**. No mutation touched reserved files.
After rebasing onto `a4546f67`, `tools/bisection/cells-for-diff.py origin/main`
selected **bdb**: no changed store path, backend-feature line or backend
Cargo.toml check fired. No kc/tkrzw run was selected or performed. Main's
intervening changes were CI workflows/actions and packaging documentation;
no tested production or contract-driver source changed in the rebase.
The expanded selection at checkpoint `371de917` had **6 MATCH, 4 DIFFER**: array lifecycle,
nonfatal segment and fatal segment pairs match; all four NULL-array witnesses
fail as source predicted. These four are real unresolved defects, not policy-accepted results.

The subsequent explicit maintainer ruling uses the removal-and-issue
fallback: the existing driver has no general opt-in-only registration
mechanism (its axis-c exclusion belongs to the separate replacement suite).
The four NULL-array definitions and registrations were removed unchanged
from the active driver and preserved in the prepared bug-report body;
they remain recoverable verbatim from `371de917`,
`tools/bisection/contract-diff.py::glib_924_null_array` and its registration
loop. Publication is pending: the required REST duplicate-issue lookup
returned Forbidden, so no issue was created or guessed. After #1190 merges,
a separate fix in both dictionary adapters must restore these four cases
to the default suite with their comparisons unchanged.

The remaining #924 selection is **6/6 MATCH**, all default-suite members.
The default selection contains **3061 cases**, including none of the four
removed witnesses; this was checked by `select_cases`, **not** by executing
the entire suite. Driver regressions: **142 passed**. Final rebase onto `c83024a6` preserved
the concurrent #926 ledger rows; main changed tool tokenizer/training code
and findings, not these facade binaries or the contract driver. The selector
still chose bdb only (all three backend-expansion checks did not fire), and
the six audit cases passed again after rebase. No generic selection
semantics or expected results changed. Exact glibc 2.43/Debian source
inspection remains open; no older-release equivalence is inferred.
There are no shipping-code changes or new runtime divergence records.

Reproduction (after cloud provisioning and sourcing its environment; one
bdb target directory, managed CPython per the setup guide):

```sh
export CARGO_TARGET_DIR="$PWD/target/bdb"
just heavy -- cargo test --locked -p oxpinyin-capi -p oxpinyin-zhuyin-capi
just heavy -- cargo clippy --locked -p oxpinyin-capi -p oxpinyin-zhuyin-capi --all-targets -- -D warnings
just fmt --all --check
just heavy -- cargo build --locked -p oxpinyin-capi -p oxpinyin-zhuyin-capi
just heavy -- tools/bisection/run-contract-diff.sh bdb "$PINYIN_ORACLE_PREFIX" \
  "$CARGO_TARGET_DIR/debug/libpinyin_capi.so" \
  "$CARGO_TARGET_DIR/debug/libzhuyin_capi.so" -- --issue 924
just heavy -- tools/abi/check-alloc-pairing.sh
just heavy -- tools/abi/check-exports.sh
uv run --no-project --managed-python --python cpython@3.15+gil \
  python -X lazy_imports=normal tools/bisection/contract-diff.test.py
```

## G10 — Rename and sync

Audit #929, 2026-10-25 UTC, subject base `6be364d9`; pin source read from a
checkout of `074a2219c90feaf962d0d24f034514033ece5f99`. No save implementation
or fsync/rename policy is changed. The C-facing path is `context::save_context`
/ `zhuyin_save` → `facade::ContextCore::save_user_with_layout_faults` →
`UserStore::save_reporting_with_baseline` → `persistence::save_reporting`.
The separate strict Rust `UserStore::save`/`persistence::finish_save` path
stops and cleans pending files on error; it is not the C facade's contract.
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
| Chunk open, header, payload, sync and close | `src/include/memory_chunk.h:536-557`; callers `src/pinyin.cpp:971,988`, `src/zhuyin.cpp:607,633` | Open failure and payload short/error returns are ignored by the caller. Each failed four-byte header write asserts; existing class (c) refusal applies. Successful payload is followed by `g_fsync`, then close; both results ignored. **Gap:** `write_chunk_file` writes and drops the file without sync. This is not a necessary Rust difference: `File::sync_all` is available. Follow-up [#1221](https://github.com/shenghaoc/oxpinyin/issues/1221), sequenced after #1190; this lane does not edit persistence or change policy. Successful write/rename alone does not establish durability. |
| Staged DBM indexes | `src/storage/facade_chewing_table2.h:93-96`, `src/storage/facade_phrase_table3.h:92-95`; `src/storage/chewing_large_table2_bdb.cpp:135-161`, `src/storage/phrase_large_table3_bdb.cpp:150-176`; KC `src/storage/chewing_large_table2_kyotodb.cpp:124-145`, `src/storage/phrase_large_table3_kyotodb.cpp:139-160`; tkrzw `src/storage/chewing_large_table2_tkrzwdb.cpp:111-129`, `src/storage/phrase_large_table3_tkrzwdb.cpp:102-120` | Unlink stale staging, create backend-native output, copy records, finish backend save before rename. KC active code dumps snapshots (its explicit synchronize/close branch is `#if 0`); tkrzw synchronizes with `false` then closes. BDB invokes the temporary DB's sync function with `m_db`, then closes the temporary DB. Do not silently describe that as a temporary-file fsync. Same-backend format/record bytes remain mandatory; backend error paths require separate verification. |
| Staged bigram | `src/storage/ngram_bdb.cpp:81-110`; `src/storage/ngram_kyotodb.cpp:82-113`; `src/storage/ngram_tkrzwdb.cpp:65-83`; facade calls `src/pinyin.cpp:1014-1022`, `src/zhuyin.cpp:680-690` | Last DBM, before marker. BDB has the same sync receiver detail; KC dumps a snapshot; tkrzw HashDBM synchronizes with `false` and closes. Backend failures do not prevent the facade's attempted rename. Native save/reload checks retained counts; this is not a whole-file byte-identity proof. |
| Failed rename, including a directory at the final name | `src/pinyin.cpp:1059-1129`; `src/zhuyin.cpp:609-638,655-690` | Retain failed staging and destination; attempt later files, print `rename ... to ... failed.` without errno text, then marker and dirty reset. `save_reporting` collects failures; `facade::ContextCore::save_user_with_layout_faults` prints them after persistence returns; interleaving with backend diagnostics remains an ordering follow-up. Native tests block every destination and assert ordered failures and sentinel preservation; contract cases cover chunk and all three DBM finals on both facades. |
| Marker after attempted renames | `src/pinyin.cpp:220-232,1141-1146`; `src/zhuyin.cpp:164-176,695-698`; `src/storage/table_info.cpp:377-396` | In-place `user.conf` rewrite, not a `.tmp` commit record. Pinyin retains its context counter; zhuyin writes fresh counter zero. Open failure prints `write ... failed.`; caller still clears dirty and returns true. fprintf/fclose failures are unchecked upstream. Subject `write_marker` uses `std::fs::write`; write-error diagnostic equivalence after a successful open remains unverified. Parsing is #920. |
| Fini with or without save | `src/pinyin.cpp:1194-1200`; `src/zhuyin.cpp:741-757` | Neither saves dirty dictionaries. Pinyin lowers counter (`>1 ? counter-1 : 0`) and writes marker; zhuyin only releases state. `FiniGuard`/`persistence::fini` follow this. Native and contract round trips include loss of an intentionally unsaved suffix, not loss of committed data. |
| Restart after interruption | `src/pinyin.cpp:173-217,922-1146`; `src/zhuyin.cpp:126-162,547-698`; `src/storage/table_info.cpp:399-412` | Load finals, never replay `.tmp`; conformance/counter can trigger cleanup. No multi-file atomicity: pinyin before rename retains old finals; either facade during renames can leave mixed generations, zhuyin even before all staging completes. Native children exit without unwinding before library 2, 3 or 4, verify the complete earlier-log staging prefix (pinyin) or already-renamed prefix (zhuyin), and verify every system-library reload delta. No power-cut durability claim. |
| Same-backend bytes and no data loss on valid completed saves | `src/pinyin.cpp:963-1018`; `src/zhuyin.cpp:596-685`; DBM writers cited above | Mandatory: encoded chunk/diff content, indexed records, bigram values and reload results. Native round trips also compare every data file byte-for-byte before fini and after conforming reopen (excluding only the lifecycle marker). This proves the subject preserves its own saved bytes across reopen, not physical-file identity between pin and subject; full cross-implementation backend-specific byte comparison and crash-order coverage remain open. Do not mask an unexplained byte difference as a filesystem quirk. |
| GLib/platform boundary and errno wording | Call sites `src/pinyin.cpp:1059-1124`, `src/zhuyin.cpp:609-690`, `src/include/memory_chunk.h:555`; GLib tag `2.89.4`, `glib/gstdio.h:60-82`, `glib/gstdio.c:99-140,1169-1208,1706-1722` | Source trace completed below for Unix macro selection and Windows wrapper behavior. Rename diagnostics at these callers contain no errno text; syscall/backend diagnostic equivalence and non-Linux execution remain open. No new platform exception is admitted. |

Source follow-through (2026-10-25 UTC): GLib tag
[`2.89.4`](https://github.com/GNOME/glib/tree/2.89.4), commit
`fa41d356ee4936264c45cf11fa6c2640a89fbdda`, read through the authorized GitHub
mirror, not by executing the pin. The reference image installs Debian
`libglib2.0-dev` **2.89.4-2**; its `glib/gstdio.h` is byte-identical to the tag.
The installed `/usr/share/doc/libglib2.0-0t64/changelog.Debian.gz` entries for 2.89.4-2 and -1 say respectively
“no source changes” and new upstream release/drop incorporated patches
(uncompressed lines 1–13). This verifies the reference header
selection; it is not a claim that every Debian patch was examined. Windows
branches below are the upstream tag's source, not a tested Debian Windows
build. Host 2.84.4 headers were not substituted.

| GLib source path at tag 2.89.4 | Source-derived result and scope |
| --- | --- |
| `glib/gstdio.h:60-82`; reference `/usr/lib/x86_64-linux-gnu/glib-2.0/include/glibconfig.h:118` | Ordinary Unix callers get macros: `g_rename` is `rename`, `g_fsync` is `fsync`. Both pinned Linux facade DSOs import the libc symbols, not `g_rename`/`g_fsync`. The pin's chunk call has no GLib EINTR retry or full-sync upgrade. |
| `glib/gstdio.c:26,1169-1208` | GLib enables its own wrapper definitions internally. The non-Windows rename wrapper simply calls `rename`; the Windows wrapper converts both filenames to UTF-16, refuses failed conversion with EINVAL, and calls `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`. Replacement of an existing final is a required save effect, not a dispensable C-runtime quirk. Filename conversion policy remains #922. |
| `glib/gstdio.c:99-140,1193-1205` | Windows failure maps `GetLastError` to errno: sharing/access denial to EACCES, existing destination to EEXIST, missing path/file to ENOENT, cross-device to EXDEV, default to EIO. Success sets errno to zero. These are integer mappings, not formatted messages. Pin rename callers test only zero/nonzero and emit the fixed path diagnostic (`src/pinyin.cpp:1059-1124`, `src/zhuyin.cpp:609-690`); they do not call strerror or format errno. Backend-generated diagnostics remain separate open coverage. No Windows errno-text exception is needed for these caller messages. |
| `glib/gstdio.c:1706-1722` | The callable wrapper uses `_commit` on Windows. Its Unix implementation uses `F_FULLFSYNC` when configured, otherwise fsync, and retries EINTR; without either facility it returns zero. **These Unix wrapper branches are not the reference pin's macro-expanded call.** The Unix header also prevents attributing the wrapper's macOS stronger-sync branch to ordinary pin callers. #1221 owns the proposed natural Rust sync and its documented macOS stronger-sync consideration. |
| Pin `src/include/memory_chunk.h:549-557`, `src/pinyin.cpp:971,988`, `src/zhuyin.cpp:607,633` | After a complete payload write, invoke sync, ignore its result, close, ignore that result, return true. Callers ignore the chunk save return and continue according to each facade's ordering. The missing subject sync is a real open implementation issue (#1221), not excused by the ignored result. |

The reference libc remains Debian **2.43-3**. Its installed changelog lists
stable-branch updates in `debian/patches/git-updates.diff` (`/usr/share/doc/libc6/changelog.Debian.gz`, uncompressed lines 1–34), so an
upstream release alone would not establish all Debian behavior. The authorized
`bminor/glibc` mirror did not advertise `refs/tags/glibc-2.43` (exact-tag fetch
reported no such ref); neither the host nor reference deps image has deb-src
configured. Exact libc implementation/patch inspection therefore remains an
**open source gap**, not a download blocker or an equivalence claim based on a
nearby tag. G10's fixed rename message and ignored sync result are settled by
the pin/GLib call sites above without importing a libc error-string rule.
Platform syscall behavior, backend error strings, Windows/macOS execution and
full Debian patch verification remain open. No TLS/network settings changed.

Coverage limits (all native tests below run both facade laws):

| Requested failure/restart coverage | Existing test and exact scope | Still missing / reason |
| --- | --- | --- |
| Failed rename | `failed_renames_report_exact_order_and_preserve_destinations`: blocks every final with a sentinel-bearing directory; checks ordered failures, retained `.tmp`, marker success and clean retry. Eight `save-929-rename-*` cases cover four individual final roles through both C facades. | Error combinations with backend diagnostics and interruption at each individual rename remain unverified; ordered error lists do not establish all syscall/diagnostic interleavings. |
| Failed sync | `sync_rejecting_chunk_target_does_not_stop_later_files` and two `save-929-sync-rejecting-*` cases use a chunk staging symlink to `/dev/null`, verify the target rejects sync, then verify save continuation. | The subject issues no chunk sync, so these are outward-result tests, not injection into an actual subject sync. Successful sync issuance, EIO/ENOSPC/EINTR, backend sync/close failures and power-loss durability remain untested. The implementation and any sync test seam are follow-up #1221, after #1190; `crates/oxpinyin-user/src/persistence.rs` remains outside this lane's edit authorization. |
| Interrupted save | `interrupted_save_preserves_the_facade_specific_committed_prefix`: child process exits with code 73 without unwinding before library 2, 3 and 4; parent waits for completion, checks every earlier log and every reloaded library delta. | No C-facade interrupted-save case; no interruption at user-chunk, index/bigram or individual rename boundaries; no power-cut test. The existing public baseline callback exposes system-library boundaries only. Additional instrumentation is needed for those missing boundaries; no new driver or persistence hook was added. |
| Save/reload | `save_reload_keeps_bigrams_and_unsaved_changes_do_not_replace_them`: saved count 37 survives an unsaved overwrite to 91; two reopens retain all data-file bytes. Two `save-929-reload-*` cases compare imported phrase tokens and normalized file content across saved/unsaved suffixes. | The C helper normalizes chunk-header bytes and DBM record representation; this is not full physical-byte identity between pin and subject. KC/tkrzw were not selected, so no execution claim is made for them. |

No new necessary language/runtime exception is established, so the divergence
register is unchanged. In particular the missing chunk sync, unchecked marker
write differences and overly broad “previous profile intact” comments in
`crates/oxpinyin-user/src/persistence.rs` are follow-ups, not accepted exceptions. Rust can express
per-file staging, replacement, explicit sync and ignored error results naturally;
there is no reason to imitate GLib containers or C ownership mechanisms here.

Verification and reproduction: `just heavy -- cargo test -p oxpinyin-user`,
`just heavy -- cargo clippy -p oxpinyin-user --all-targets -- -D warnings`,
`cargo fmt --all --check`. The four new tests in
`crates/oxpinyin-user/tests/save_semantics_929.rs` exercise both facade laws. The `/dev/null`
sync-rejection case proves only the outward continuation result, **not that the
subject issued a sync**. The contract selection is `--issue 929` in the existing
`tools/bisection/run-contract-diff.sh` runner; cell selection is
`tools/bisection/cells-for-diff.py origin/main`. On this Linux amd64 host, 231 user-crate tests and 142 contract-harness tests
passed; fmt/clippy passed. The selector chose bdb only; all 12 new contract
cases MATCH on bdb. A temporary subject-only mutation retaining dirty after
`UserStore::save_reporting_inner` produced 10 DIFFER and two unchanged reload
controls; restoring the subject restored 12 MATCH. No mutation remains in
the diff. KC/tkrzw were provisioned and hash-verified, not selected for this
diff. The GLib wrapper source trace is complete; exact libc source/patch
inspection, non-Linux execution and the full durability/byte audit remain open
as distinguished above. This is Refs #929, not closure.


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
| Classic dictool two/three-field dictionary interchange | Equivalent `str::splitn` replacement under the existing [dictool format](dictool-format.md), including empty fields and retained count tail. | [#926 source audit](926-tool-tokenizers.md): current safe-store import/export path, ASCII versus Unicode separators, literal escapes/categories and preflight errors traced. The older description of dictool calling the C ABI is historical. |

#926's [bounded tokenizer audit](926-tool-tokenizers.md) also replaces the
segmented helper's two-piece iterator with `split_once` and borrows its numeric
prefix without changing existing results. It inventories all six active pin
split/category calls and preserves `interp_grammar.rs::is_graph` unchanged.
The follow-through traces nonzero phrase validation, signed numeric inputs,
amd64 numeric narrowing, NUL and invalid UTF-8 through model output. It cites
GLib tag 2.89.4 and the installed Debian 2.89.4-2 splitter, and libc 2.43-3
headers/disassembly. Implementation/parity gaps, exact libc tag/patchset
source inspection, and DBM durability after assertion remain open, as does
the separate full four-field generator audit. No equivalent refactor closes
those gaps or admits a runtime exception. The approved interpolation-category
difference remains separate.

Bounded completion gate, 2026-10-25 UTC: native dictool/training tests,
fmt and clippy passed; both named splitter mutations failed and their
restorations passed. BDB live counter values (138096 unigrams, 199 bigrams)
matched for the ordinary and classified streams; existing import differential
and classic frontend interchange matched in both directions. Commands and
limits are in the audit; this does not promote the remaining G16 rows.

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
