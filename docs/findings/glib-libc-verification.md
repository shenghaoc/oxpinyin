# GLib/libc verification coverage

Status: **partial coverage, removal gate** · 2026-10-24 UTC ·
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
| Values >= `0x80000000`; truncated, empty, overlong or inconsistent items; invalid readings; allocation failure | Values >= `0x80000000`: pin true/declared length/NULL, Rust refuses the item (false/untouched); a distinct malformed-item difference, not admitted by #894. Truncated/empty/overlong/inconsistent items: out-of-bounds reads are class (b) (rows 101/107); in-bounds declared-length reads can return following bytes. Invalid readings are a separate surface, not on the segment path. Allocation failure is host-dependent: conversion OOM refuses at the pin, later container OOM aborts, and Rust aborts on its conversion/copy allocations. | Source-complete for the values/items/readings and allocation partitions in the [G1 closure](918-boundary-follow-through.md#g1-partition-closure); no new equivalence or #918 removal clearance. |
| `phrase_segment`: empty; well-bounded rejected `ff fe`; valid scalar conversion | Identical for the entry/result lifecycle described in [#918's audit](918-phrase-segment-source-audit.md); existing independent lookup/math records retain their scope. | Source established, both facades; inherited tests only, no new gate. Not a proof of every valid decoder path. |
| `phrase_segment`: GLib-accepted non-scalar, consistent scalar-only keys/items, sound grams and successful backing-store reads, including a matching scalar prefix | Identical return/result/getter/diagnostic behavior; no observable session or persistent change from prefix consultation. | Complete bounded source argument in [#918's audit](918-phrase-segment-source-audit.md), three backend read paths. Inherited surrogate-in-`nihao` tests do not exercise every non-scalar or store state. |
| `phrase_segment`: matching prefix, reached sound system gram plus 1–3-byte user gram, later GLib-accepted non-scalar | Identical under class (c): process the prefix and use row 85's ordinary warning/false/old-result refusal. [G1 entry](918-prefix-lookup.md); no new runtime-quirk exception. | Source-established, both facades/all three wrappers; bdb contract gate 8/8 MATCH; skip-prefix mutation 2 DIFFER/6 MATCH; restored 8/8 MATCH; touched-crate tests, clippy and fmt passed. |
| `phrase_segment`: bounded matching raw non-scalar key/item | Pin true/token vs Rust false/zeros; [proposed separate runtime quirk](918-nonscalar-token-result.md), not admitted. | Bounded source path complete; no new case/gate. |
| `phrase_segment`: over-skipping byte walk, partial/short unsafe gram reads | Existing class (b) governs illegal reads; no fabricated deterministic pin output. The new prefix walk uses row 98's existing warning on a reached unsafe lookup gram. | Source and applicable inherited safety records; length-walk instructions read on amd64/arm64. |
| `phrase_segment`: faulting prefix stores | [bdb format error](918-prefix-bdb-format-error.md) is an unadmitted diagnostic/state difference; [kc second-read assertion](918-prefix-kc-read-assert.md) owes class-(c) warning. Baseline silent kc first-check/tkrzw returned errors had bounded equivalence; the new prefix read exposes the separate error-lifecycle difference traced in the follow-through. | Named paths traced in [follow-through](918-boundary-follow-through.md), including conditional bdb binary branch on both architectures. No runtime fault probes. |
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
new prefix entry separately reports native tests and the executed bdb
gate. The ordinary scalar path already emits row 85's warning on `ShortLookupPredecessor`;
the [G1 prefix entry](918-prefix-lookup.md) extends that refusal to the
reached short-gram/non-scalar partition under the new ruling. Sound-data equivalence is a length-sized zero-token result,
not a zero-length result. No removal is cleared.

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

Pin: `pinyin.cpp:698-768,822-865,3421-3431,3482-3509,3518-3576`;
`storage/punct_table.cpp:74-94`; full `g_strdup`, `g_free`, `g_string_*`
and vector membership in the inventory TSV.
Rust: `capi/src/ffi.rs`, `owned_cstr`/`owned_cstr_list`;
`capi/src/aux_matrix.rs`, `AuxMatrix::chewing`/`join`;
`capi/src/iterators.rs` export getters; `capi/src/predict.rs`,
`prepend_punctuations`; `data/src/pin_stderr.rs`, `build_filename`.

| Partition | Verdict / record | Investigation |
| --- | --- | --- |
| Valid scalar phrase strings and their C marshalling | Identical within G1's valid-text scope. | Complete source argument there; other string consumers remain partial. |
| Non-scalar / NUL phrase text at marshalling or export | Accepted only as [non-scalar][non-scalar] / [NUL][nul]. | Source complete for those records, runtime limits as G1. |
| Ordinary aux prefix/middle/postfix; missing middle; UTF-8 substring boundaries | Inventory says `join` retains the NULL-middle drops-postfix law; full classification pending #925. | Partial; no removal clearance across all aux callers. |
| Bigram export's unterminated pronunciation vector | Accepted class (b), [row 1 record](upstream-divergences.md#bigram-export-iterators-pinyin-buffer). | Established independent fault; #894 traces its text consumers but does not turn this fault into a deterministic phrase-text witness. |
| Raw path bytes; embedded NUL in diagnostics; duplicate punctuation strings/order | Pending #922/#924/#928/#1144, with G3/G8/G15. | Unverified whole paths; no blanket “malformed quirk” admission. |

## G3 — Path joining

Pin: `pinyin.cpp:164,252,268,286,308,395,400,952,968,985,1052`;
`zhuyin.cpp:118,196,212,230,252,337,342,585,601,605,626,630,684`.
Rust: `data/src/table_info.rs`, `FileName::under`/`pin_join`;
`data/src/pin_stderr.rs`, `build_filename`; `capi/src/ffi.rs`, `cstr_to_path`
and the zhuyin counterpart. Owner #922; G10 covers rename/sync.

| Partition | Verdict | Investigation |
| --- | --- | --- |
| Ordinary directory/name pairs | Inventory source conclusion: same resolved path; contract retained. | Partial; not a completed all-caller audit. |
| Leading `/`, empty name, `..`, duplicate separators, non-UTF-8 directory/name bytes | Pending classification. The current helpers preserve raw bytes and pin joining rather than establishing permission to remove it. | Unverified full load/save/stderr trace under #922. |

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
`Scan::int`; both facade `locale.rs::pin_table_info_locale`.

| Partition | Verdict / record | Investigation |
| --- | --- | --- |
| Valid table.conf versions, library mapping and lambda bits | Inventory source conclusion: same values; contract. | Partial #919; no complete scanner-removal proof. |
| Valid user.conf versions/state | Inventory source conclusion: same values; contract. | Partial #920; preservation of user state must be traced. |
| Overflow/saturation, signed/truncated integers, `atoi` index narrowing, hexadecimal floats, incomplete/non-backtracking exponents, missing/malformed fields | Pending per-partition classification #919/#920. Existing four `host_scanf` HOST-DEPENDENT cases do not admit arbitrary parser changes. | Unverified complete scanner/consumer audit; G6's lambda frequency issue is distinct. |
| Process LC_NUMERIC left at C, including early returns | **Needs a ruling to reverse**, [policy row 39](compatibility-policy.md), #923. Retain current behavior. | Established earlier human ruling; this is a real policy STOP, not an unread-path placeholder. |

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
