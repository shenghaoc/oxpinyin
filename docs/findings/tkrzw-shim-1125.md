# tkrzw concrete-class shim: evaluation (#1125)

## Empty-message fast path dropped; retained counts and instruction paths; STOP

Recorded 2026-10-24T08:22:48Z. This is the current status, superseding the earlier
queued fast path and build/generation status below. The human requested
retained-profile analysis only, dropping the empty-message optimization
and stopping for a maintainer decision. No new runtime capture, build,
policy regeneration or measurement was started under this ruling.

### Source checkpoint and policy status

In crates/oxpinyin-store/src/tkrzw/shim.cc, finish again unconditionally
assigns status.GetMessage() to last_message after recording the code.
The guarded exception boundary, standard/nonstandard catches, noexcept
entry points and best-effort fail remain unchanged. No key copy was added.
This is an unbuilt checkpoint on ef12b1134ef160b6085b7ea1c4f02e561a83565d;
the approved comparison base remains d7788072. Only shim.cc and this
findings document are changed in this checkpoint.

After the restoration, git diff against the original measured findings
checkpoint 92b7d61bb60a46c41cc85a0ffed70cf7c354e702 is empty outside this
findings file. This is a static source/tree comparison, not a new build
or a performance result for this restored checkpoint. The previous
clippy/tests and twice-regenerated policy results below belong to the
now-dropped fast-path source. The checked-in order files are left untouched;
their latest regeneration used that fast path. They are **stale for this
checkpoint and require authorized regeneration**. No claim of a newly
ordered or measured restored binary is made, even though the order-file
bytes also match the historical measured files. Original measurement
provenance and retained originals below are preserved.

### Collected memcmp calls and exclusive Ir per call

These counts come only from the six original retained profiles at
$R/campaign/store-work/callgrind/get_raw.tree.{miss,retain}.{base,control,candidate}.out.
The profiles have positions: line and events: Ir, without operand addresses.
Base and control match exactly, including every exclusive source-line row.
The libc leaf has no outgoing call; exclusive leaf Ir reconciles with the
previous caller report. Status/message contributes zero to this leaf.

| operation | collected calls base/control | collected calls prototype | exclusive Ir base/control | exclusive Ir prototype | Ir/call base/control | Ir/call prototype | extra Ir/call |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| get_raw.tree.miss | 377,678 | 377,678 | 8,431,677 | 8,501,700 | 22.325041437 | 22.510445406 | +0.185403968 |
| get_raw.tree.retain | 599,488 | 599,488 | 13,018,822 | 13,187,264 | 21.716568138 | 21.997544571 | +0.280976433 |

Collected calls are counted by the single endbr64 at libc
__memcmp_avx2_movbe+0x0 (ELF offset 0x19bf20, assembly source line 73).
The next two single-instruction rows, cmp length and jb at +0x4/+0x8,
have the same counts; so do all five instructions of the short-operand
page guard at +0x2e5 through +0x2f3. All collected calls reach that guard:
comparison lengths are 2 through 31 bytes. The benchmark's
support::ops::measured is the collection toggle boundary; setup is outside
that boundary, and memcmp contains no collection toggle. These entry
instruction counts therefore supply the denominator for collected Ir.

The raw calls= edges must be kept separate. As documented in
[the benchmark runbook](../runbooks/benches.md), Callgrind call counters
can include calls outside collection. They are not selected-call counts.

| operation | raw stub-to-memcmp calls base/control | raw stub-to-memcmp calls prototype | raw LexicalKeyComparator-to-stub calls (all arms) | raw HashDBMImpl::ProcessImpl-to-stub calls (all arms) |
| --- | ---: | ---: | ---: | ---: |
| get_raw.tree.miss | 377,709 | 377,678 | 377,423 | 255 |
| get_raw.tree.retain | 7,123,202 | 7,123,171 | 7,122,240 | 931 |

The raw leaf counter differs by 31 in each operation, while the two named
key-search caller counters and collected entry counts are equal. The
retained output does not identify the uncollected provenance of those 31;
it would be wrong to call them extra measured tree searches, or divide
collected retain Ir by more than seven million setup-inclusive calls.
There is no measured-call-count discrepancy requiring a different-search-
path explanation. Equal counts alone do not establish identical operand
addresses or an identical sequence of comparisons.

### Executed path explaining the extra instructions

The existing libc ELF and its matching split debug information were read,
without executing it. libc build ID is
5c6ef14e93d7793dc6b155982295fddfbc7f5eb9; the debug file is
/usr/lib/debug/.build-id/5c/6ef14e93d7793dc6b155982295fddfbc7f5eb9.debug.
The assembly line names embedded in that ELF refer to glibc-2.44-1ubuntu2,
sysdeps/x86_64/multiarch/memcmp-avx2-movbe.S.

At +0x2e5/+0x2e7 the code combines the two pointer low bits with OR;
+0x2e9 masks by 4095, +0x2ee compares with 4064, and +0x2f3 jumps to
+0x320 when ((uintptr_t(a) | uintptr_t(b)) & 4095) > 4064.
Otherwise it uses a 32-byte AVX comparison at +0x2f5, masking the result
by the actual length. The guard conservatively avoids a potentially
page-crossing wide load. An OR of the low bits can exceed the threshold
even when neither individual offset does; these counts must not be
reported as the number of operands actually straddling a page.

| retained branch entry (single instruction) | miss base/control | miss prototype | retain base/control | retain prototype |
| --- | ---: | ---: | ---: | ---: |
| usual short AVX path, line 414 / +0x2f5 | 339,321 | 269,240 | 568,671 | 536,561 |
| page-boundary fallback, line 429 / +0x320 | 38,357 | 108,438 | 30,817 | 62,927 |
| fallback 4-7 byte path, line 438 / +0x333 | 20,235 | 37,748 | 19,529 | 42,581 |
| fallback 8-15 byte path, line 476 / +0x380 | 18,122 | 70,690 | 10,720 | 20,346 |
| fallback 16-31 byte path, line 512 / +0x3a3 | 0 | 0 | 284 | 0 |
| fallback 2-3 byte path, line 553 / +0x3e0 | 0 | 0 | 284 | 0 |

Fallback entries increase by 70,081 for miss and 32,110 for retain;
normal short AVX entries decrease by exactly those amounts. Below are
**exclusive**, disjoint assembly-row deltas; they reconcile every changed
leaf row, including return instructions. No inclusive edges are added.

| exclusive assembly path / source rows | miss delta Ir | retain delta Ir |
| --- | ---: | ---: |
| vector mismatch return: 156,166-168,171 | -420,486 | -148,692 |
| short AVX compare/equal return: 414-417,420-423 | -420,486 | -214,644 |
| fallback length dispatch: 429-430,432-433,435-436 | +315,350 | +174,544 |
| fallback 4-7 byte compare/return: 438-446,448,450 | +175,130 | +235,007 |
| fallback nonzero return: 463-464,466 | +210,243 | +75,198 |
| fallback 8-15 byte compare/return: 476-482,484,486 | +210,272 | +54,129 |
| fallback 16-31 / 2-3 byte compare/return: 512-516,520-525,527,529,553-562,564,566 | 0 | -7,100 |
| **sum** | **+70,023** | **+168,442** |

Thus the retained instruction paths prove the immediate cost comes from
libc's address-low-bit guard selecting different short-key comparison
paths more often. They do not record which argument, allocation or tree
record caused the different predicate outcomes. In particular, the PLT
runtime addresses below identify code locations, not the key operands.
Attributing this to the shim's query buffer alignment, a DBM-owned record
buffer, or a specific allocation layout would exceed the retained evidence.
The previous pin-construction comparison found no explicit flags, tuning,
comparator or file-type discrepancy; that source result remains unchanged.

### Key-copy proposal: premise and limits

The actual base path is db_get in crates/oxpinyin-store/src/tkrzw/mod.rs
at d7788072: it passes key.as_ptr() and a checked nonnegative length to
ffi::tkrzw_dbm_process. In the certified upstream
[tkrzw_langc.cc at 9ee84164](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_langc.cc),
tkrzw_dbm_process constructs a borrowed string_view and forwards it to
Process; it does not copy that input key. The shim's oxtkrzw_dbm_process
also constructs a borrowed string_view. Other C APIs that copy keys are
not this measured path. The installed tkrzw_dbm_process disassembly agrees:
+0x1a preserves the input pointer in rbx, +0x6b passes it in rcx to the
virtual Process call at +0x85. There is no input-key allocation or copy.
The optional strlen path is bypassed for the Rust caller's checked length.

There is therefore no old C-API key copy to retain or restore. Adding a
new copy could change a query operand's low bits, but an ordinary string
copy guarantees no suitable address and cannot fix the unknown other
operand. Even deliberately aligned scratch storage would only control
the copied operand; neither the number of avoided fallback calls nor the
net benefit follows from these profiles. A copy adds work linear in key
length and temporary key storage, possibly allocation for an owning
string; reusable scratch still needs copying and bounds/alignment handling.
No implementation or measured Ir/wall/space cost for such a copy exists
in the retained run, so a numerical cost or a claim that it removes the
regression is unresolved. No speculative copy optimization was made.

### Commands and stopping point

The local analysis reads compressed function IDs, calls= associations and
absolute/relative source positions, excludes inclusive association-cost
rows, verifies base/control equality and partitions all changed exclusive
leaf rows. Analysis scripts and raw outputs remain local, uncommitted.

```sh
$PY "$R/analysis/memcmp-callers.py"
$PY "$R/analysis/count-summary.py"
objdump -dl --no-show-raw-insn --start-address=0x19bf20 --stop-address=0x19c320 /usr/lib/x86_64-linux-gnu/libc.so.6
objdump -d -C --no-show-raw-insn --start-address=0x194690 --stop-address=0x1947d0 "$TKRZW_PREFIX/lib/libtkrzw.so.1.77.0"
git -C "$R/worktree" show d7788072:crates/oxpinyin-store/src/tkrzw/mod.rs
git -C "$R/worktree" diff 92b7d61bb60a46c41cc85a0ffed70cf7c354e702 -- . ':!docs/findings/tkrzw-shim-1125.md'
git -C "$R/worktree" diff --check
```

The first objdump command above is the combined range equivalent of the
read-only 0x19bf20-0x19c300 and 0x19c300-0x19c345 reads actually used.
Source retrieval used the GitHub read-only fetch action at the exact
upstream URL above. STOP for the maintainer's decision; no PR or review.
No new workload was launched. The remaining causal gap is operand-address
and allocation provenance; no new capture is authorized by this ruling.


## Caller boundary: key-search memcmp contribution; STOP for ruling

Recorded 2026-10-24T07:49:15Z. This section supersedes the queued follow-up status
below. The new human boundary requires reporting key-search attribution
and pin construction/tuning comparison before proceeding further.
Key search contributes, so no further build, regeneration or measurement
will start until a ruling.

### Exact category split from retained original call graph

| original operation | memcmp base/control exclusive Ir | prototype exclusive Ir | key-search delta | status/message delta |
| --- | ---: | ---: | ---: | ---: |
| get_raw.tree.miss | 8,431,677 | 8,501,700 | +70,023 | 0 |
| get_raw.tree.retain | 13,018,822 | 13,187,264 | +168,442 | 0 |

__memcmp_avx2_movbe is a leaf in each retained profile: it has no outgoing
call edge. Its sole incoming edge, from libtkrzw's memcmp PLT stub, equals
the leaf's exclusive cost exactly. That stub has only two incoming callers:
tkrzw::LexicalKeyComparator and tkrzw::HashDBMImpl::ProcessImpl, the hash
page-record key lookup below TreeDBM. There is no status/message caller.
The script verifies that the sum of incoming stub-edge costs equals the
stub's exclusive cost plus its outgoing descendant costs in every file.
Base/control match. Thus the category split above accounts for all the
leaf cost without assigning any of it to finish or its string temporary.

For graph reconciliation only, these are the incoming edge costs, which
include the stub and descendants and MUST NOT be added to the exclusive
Ir table:

| operation / edge caller | base/control | prototype |
| --- | ---: | ---: |
| miss / LexicalKeyComparator | 9,181,675 | 9,251,698 |
| miss / HashDBMImpl::ProcessImpl | 5,358 | 6,217 |
| retain / LexicalKeyComparator | 14,217,624 | 14,386,066 |
| retain / HashDBMImpl::ProcessImpl | 174 | 174 |

The miss graph has an extra lazy resolver traversal below the shared stub.
Because the capture merges callers at that stub, these inclusive edges
alone are not used to invent an exact leaf split between the two key-search
parents. No operand-address/alignment explanation is proved. If required
after the ruling, a scoped instruction-position capture with separated
caller contexts and operand-address observations would resolve that finer
question. No diagnostic runtime capture was run under this new boundary.

### Source comparison with libpinyin 074a2219

The pin was read with git show, not from the checkout's current HEAD:
ChewingLargeTable2::attach at src/storage/chewing_large_table2_tkrzwdb.cpp:75-88;
its load_db/save_db at :92-126; PhraseLargeTable3::attach at
src/storage/phrase_large_table3_tkrzwdb.cpp:65-79 and load_db/save_db at
:83-117; attach_options at src/storage/tkrzwdb_utils.h:32-45.

| input | pin 074a2219 | shim / existing Rust adapter |
| --- | --- | --- |
| TreeDBM constructor | new TreeDBM for attach; automatic TreeDBM for file-copy temporary | std::make_unique<TreeDBM>(); no constructor arguments |
| file implementation | default TreeDBM constructor | same default constructor: MemoryMapParallelFile |
| read-only existing file | Open(path, false, OPEN_NO_CREATE) | Open(path, false, OPEN_NO_CREATE) |
| writable/create file | Open(path, true, OPEN_DEFAULT) | Open(path, true, OPEN_DEFAULT) |
| writable existing without create | attach_options can select true, OPEN_NO_CREATE | shim independently accepts writable=true, no_create=true; trait create uses true,false |
| open tuning | Open's default OpenAdvanced TuningParameters() | same Open default; no tuning setter |
| comparator input | no explicit comparator | no explicit comparator; key_comparator=nullptr default |
| tree tuning supplied | max_page_size, max_branches, max_cached_pages = -1; page_update_mode=PAGE_UPDATE_DEFAULT | identical defaults |
| inherited hash tuning supplied | update_mode/CRC/compression/restore = DEFAULT; offset_width, align_pow, num_buckets, fbp_capacity, min_read_size, cache_buckets = -1 | identical defaults |

The installed tree header documents the MemoryMapParallelFile default and
Open -> OpenAdvanced delegation. Its comparator default is lexical;
existing files restore the stored comparator through the same library
metadata path. The exact certified upstream
[TreeDBM implementation at 9ee84164](https://raw.githubusercontent.com/estraier/tkrzw/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_tree.cc)
confirms the constructor and metadata comparator selection. No custom file,
cache, page, comparator or hash tuning is supplied by this shim. OPEN_DEFAULT
is 0 and OPEN_NO_CREATE is 2 in the installed file header.

The pin's attach allocation is on the heap, as is the shim's. The pin's
load/save temporary is automatic storage while the shim always owns its
handle on the heap. This is an ownership/allocation-shape distinction,
not a supplied tuning or file-option difference, and is not claimed to
explain the memcmp counts. Matching parameters does not establish their
runtime instruction-path cause. No constructor/tuning discrepancy was
found that can be corrected from this source comparison.

### Work already completed before the new boundary

Under the prior explicit release, fast-path source checkpoint `974f98bc`
was pushed before heavy work. The fast path was already compiled for
clippy, 59 tkrzw tests, the standalone actual-shim status/exception check,
and unordered shipping inputs. These passed; standard/nonstandard throws
and allocation failure during error recording remained contained. Policy
regeneration completed twice; the outputs matched each other and the
checked-in policies byte-for-byte, with zero missing C++ paths.

The status edit was folded into implementation
`bb43abcd900aac7be409be82160d063b6493aed5`, directly on unchanged base
`d7788072`. The complete tree stayed identical to the pre-fold checkpoint
(tree `95541cd379d6a4aedd5d08a575a2444aa2a61b48`). All findings content was
preserved. Checkpoint `5bfdd4a5e9728ab84eb738358c23b93ad532b0f8` was pushed
and remote=head verified. No ordered follow-up build or follow-up
measurement had started when the new boundary arrived. The completed
validation/generation job exited normally; no heavy job remains owned by
this lane. The prepared follow-up scripts were not launched further.
The status change has no measured performance result.

```sh
$PY "$R/analysis/memcmp-callers.py"
git -C /home/sheng/repos/libpinyin show 074a2219:src/storage/chewing_large_table2_tkrzwdb.cpp
git -C /home/sheng/repos/libpinyin show 074a2219:src/storage/phrase_large_table3_tkrzwdb.cpp
git -C /home/sheng/repos/libpinyin show 074a2219:src/storage/tkrzwdb_utils.h
```

The original profiles, follow-up validation output and exact scripts remain
local; no raw capture is committed or uploaded. No PR or review action.


## Retained exclusive attribution and queued status fast path

Recorded 2026-10-24T07:33:36Z, against the original measured base/prototype benchmarks
from the Ubuntu campaign above, before rebuilding or running another case.
The human cleared the earlier STOP for bounded completion; it remains the
result of that original run. Base remains fixed at `d7788072`.

The six retained Callgrind files are
`$R/campaign/store-work/callgrind/get_raw.tree.{miss,retain}.{base,control,candidate}.out`.
The local analysis script `$R/analysis/exclusive.py` resolves compressed
function/object IDs, keeps inline source rows under their enclosing function,
and excludes the cost row following each calls= association. For each file,
the sum of exclusive function costs equals its totals: field exactly.
Base/control maps match function by function after normalizing the binary
pathname and identifying unresolved stubs by their unique outgoing callee.
That outgoing edge supplies identity only; no inclusive edge cost is added.

The profiles call PLT entries "???" with a runtime address. Entries leading
to __tls_get_addr and memcmp are independently checked against the retained
runtime ELF's .plt.sec disassembly: static offsets 0x6a7a0 and 0x66ce0,
respectively. They establish the same load bias within each arm
(0x4ad8000 base, 0x4adc000 prototype). Each normal stub is endbr64 plus an
indirect jump. Other stubs below are labeled by their unique outgoing
callee, without claiming that their exclusive instructions belong to that
callee's body.

All nonzero exclusive deltas follow. Counts are whole profiled-region Ir;
all omitted functions have delta zero. This reconciles the entire increase,
including loader/allocator effects, without overlapping inclusive totals.

### get_raw.tree.miss: 20,000 calls

| exclusive function / object | base Ir | prototype Ir | delta |
| --- | ---: | ---: | ---: |
| finish(tkrzw::Status) [clone .isra.0] / bench | 0 | 1,160,000 | +1,160,000 |
| tkrzw_dbm_process / libtkrzw.so.1.77.0 | 880,000 | 0 | -880,000 |
| oxtkrzw_dbm_process / bench | 0 | 640,000 | +640,000 |
| Processor::ProcessEmpty(string_view) / bench | 0 | 600,000 | +600,000 |
| RecordProcessorWrapper::ProcessEmpty(string_view) / libtkrzw.so.1.77.0 | 500,000 | 0 | -500,000 |
| __tls_get_addr / ld-linux-x86-64.so.2 | 480,000 | 0 | -480,000 |
| tkrzw::PolyDBM::Process(string_view, tkrzw::DBM::RecordProcessor*, bool) / libtkrzw.so.1.77.0 | 400,000 | 0 | -400,000 |
| stub -> __tls_get_addr / ??? | 80,000 | 0 | -80,000 |
| __memcmp_avx2_movbe / libc.so.6 | 8,431,677 | 8,501,700 | +70,023 |
| stub -> free / ??? | 173,732 | 133,732 | -40,000 |
| do_lookup_x / ld-linux-x86-64.so.2 | 11,592 | 12,106 | +514 |
| _dl_lookup_symbol_x / ld-linux-x86-64.so.2 | 23,378 | 23,512 | +134 |
| sbrk / libc.so.6 | 955 | 1,075 | +120 |
| _dl_fixup / ld-linux-x86-64.so.2 | 4,692 | 4,799 | +107 |
| strcmp / ld-linux-x86-64.so.2 | 196 | 294 | +98 |
| check_match / ld-linux-x86-64.so.2 | 2,351 | 2,418 | +67 |
| __glibc_morecore / libc.so.6 | 400 | 460 | +60 |
| systrim.constprop.0 / libc.so.6 | 204 | 250 | +46 |
| _int_free_maybe_trim / libc.so.6 | 19,068 | 19,110 | +42 |
| _dl_runtime_resolve_xsave / ld-linux-x86-64.so.2 | 1,886 | 1,927 | +41 |
| bcmp / libc.so.6 | 0 | 25 | +25 |
| brk / libc.so.6 | 306 | 324 | +18 |
| __memcpy_avx_unaligned_erms / libc.so.6 | 888,245 | 888,239 | -6 |
| stub -> __memcmp_avx2_movbe / ??? | 755,356 | 755,361 | +5 |

Sum of exclusive deltas: +91,294 Ir.

### get_raw.tree.retain: 32,000 calls

| exclusive function / object | base Ir | prototype Ir | delta |
| --- | ---: | ---: | ---: |
| finish(tkrzw::Status) [clone .isra.0] / bench | 0 | 1,856,000 | +1,856,000 |
| tkrzw_dbm_process / libtkrzw.so.1.77.0 | 1,408,000 | 0 | -1,408,000 |
| oxtkrzw_dbm_process / bench | 0 | 1,024,000 | +1,024,000 |
| Processor::ProcessFull(string_view, string_view) / bench | 0 | 992,000 | +992,000 |
| RecordProcessorWrapper::ProcessFull(string_view, string_view) / libtkrzw.so.1.77.0 | 832,000 | 0 | -832,000 |
| __tls_get_addr / ld-linux-x86-64.so.2 | 768,000 | 0 | -768,000 |
| tkrzw::PolyDBM::Process(string_view, tkrzw::DBM::RecordProcessor*, bool) / libtkrzw.so.1.77.0 | 640,000 | 0 | -640,000 |
| __memcmp_avx2_movbe / libc.so.6 | 13,018,822 | 13,187,264 | +168,442 |
| stub -> __tls_get_addr / ??? | 128,000 | 0 | -128,000 |
| stub -> free / ??? | 256,300 | 192,300 | -64,000 |
| _int_malloc / libc.so.6 | 1,071,760 | 1,071,881 | +121 |
| __libc_malloc2 / libc.so.6 | 159,114 | 159,137 | +23 |
| __memcpy_avx_unaligned_erms / libc.so.6 | 1,259,994 | 1,260,006 | +12 |
| malloc / libc.so.6 | 964,437 | 964,429 | -8 |

Sum of exclusive deltas: +200,590 Ir.

### What the counts establish

Status conversion in finish costs 58 Ir per record: 1,160,000 for miss and
1,856,000 for retain. The direct wrapper costs 32 per record, and its
Processor callback adapter costs 30 (empty) or 31 (full). The removed C
wrapper costs 44, its processor 25/26, PolyDBM::Process 20, TLS lookup 24,
and the removed TLS/free stubs another 6 per record. Their net is +1 Ir
per record: +20,000 / +32,000 respectively. These are exclusive rows,
not a subtraction of overlapping call-tree totals.

The libc byte-comparison function __memcmp_avx2_movbe accounts for another
+70,023 / +168,442 Ir. Calls enter it through the same runtime's memcmp
stub; source lines in that assembly routine have different exclusive
execution counts. The retained line profiles do not record the comparison
operand addresses, so an address-alignment or cache-layout cause is not
proved and is not asserted. Its exact instruction-path/operand cause needs
a queued scoped capture with instruction positions and operand-address
observations if still material after the safe status change.

Rust get_value/callback and Vec growth/copy functions have unchanged
exclusive counts. libc memcpy changes only −6 / +12 Ir total, so the extra
Ir is not a material record-buffer copy. The remaining miss +1,271 and
retain +148 Ir are fully named loader/allocator/stub residuals in the tables.
No __cxa_throw or unwind routine contributes recorded Ir. The wrapper's
32 instructions include ordinary setup, virtual invocation and Status
cleanup; the line-only profile cannot charge them separately to exception
landing-pad scaffolding. They are not labeled "exception handling cost".

Existing-binary disassembly of finish (symbol offset 0x26d440 in the
prototype bench) shows construction and move assignment of the temporary
string returned by Status::GetMessage even when message_ is null. The
installed tkrzw_lib_common.h GetMessage returns an empty string in that
case; HasMessage tests non-null and a non-NUL first byte. This is source
and existing-object inspection, not a new runtime probe.

The only source edit adds a HasMessage branch in finish: copy a nonempty
message as before; otherwise clear the thread-local string directly.
An empty message is preserved for both null and empty-string message
storage, including failures with no message. Nonempty success/error
messages are preserved. guarded still surrounds every call, both catches
are unchanged, and fail still contains any exception while recording one.
No exception boundary, callback validation, Status code or ownership rule
was weakened; no dependency was added. This is an unbuilt change with no
new performance or compatibility claim. Its actual gain and any residual
memcmp delta await the authorized Ubuntu campaign and exact controls.

```sh
# No build, policy regeneration or runtime workload in this analysis step.
$PY "$R/analysis/exclusive.py"
objdump -d -C --no-show-raw-insn "$PR_BENCH"
objdump -d -C --no-show-raw-insn -j .plt.sec "$CERTIFIED_LIBTKRZW"
```

Before any later PR, rebase onto main including #1140 and regenerate the
tkrzw policies on the combined tree; carrying these order files through a
textual rebase is insufficient. The fixed-base evaluation does not rebase
first. No PR or review is authorized by this checkpoint.


## Ubuntu results: STOP on store-operation instructions

Recorded 2026-10-24T07:16:00Z. The facade campaign completed; the store-operation
campaign was canceled under the requested no-time-for-residency STOP rule.
`get_raw.tree.miss` executes 4,215.77705 Ir/call against base/control
4,211.21235: +0.108394%, with exactly equal base/control whole counts.
`get_raw.tree.retain`, already in flight during cancellation, also completed
with an exact-control increase. No source tuning or rerun followed.
The heavy-slot supervisor reported `cancelled; child process groups
terminated and reaped`, exit 130. This is neither PASS nor SKIP for the
incomplete store campaign. Remaining operations have no result.
No PR, review request or artifact upload was made. Adoption is undecided.

### Source, policies and build provenance

The authoritative fetched checkpoint was `b9b8a31bee471b940fdaecc4c0cc10ebea0c93b2`,
with implementation `2b188ae85a0e0d037e6235e10cbe9cb48aba4a9c` directly on
base `d7788072d3b8078aaa500cb5fcb98e0542fe7278` (#1114 included).
The earlier handoff `2d1b7ade` was not substituted for it. Explicit diff
showed equal source/test, build inputs, shipping tools and policy trees;
only #1114 findings and the benches retention rule differed. Those external
changes were preserved. No rebase onto later main was made. The inspected
later main (`7fdab25f`) changes tools, workflow, documentation, Rust comments
and doc attributes; it changes no shipping code. It is not the baseline.

Regeneration ran twice using the same unordered shipping libraries and
static archives, with the existing teardown profiles and generator on ext4.
Both facade policy files were byte-identical between passes. The generated
policies were folded into the current implementation, producing
`b3018335cce3e1d080f7790716bf88a4726247a2`, still directly on `d7788072`.
The four existing findings commits were replayed without content changes;
checkpoint `20dc2615055ea4bae2fc86f253a03966414de6d1` was pushed with the exact
`b9b8a31b` lease and remote=head verified. The tree difference from `b9b8a31b`
is exactly the two tkrzw policies. These policies were not present at `b9b8a31b`.
Both identities are Shenghao Chen <shenghaoc@outlook.com>.

The linked-object verifier reports ordered base pinyin/zhuyin over 260/240
paths and ordered prototype/control over 274/254 paths. Both prototype
policies include three measured C++ paths. Every resolver reports zero
missing C++ paths (0 of 3 for the prototype); no shipping measurement uses
an unordered fallback. Ordering was explicitly disabled only for the
regeneration inputs. Prototype dynamic imports include all four concrete
constructors and no PolyDBM or tkrzw_dbm_* C entry point. Base imports
`tkrzw_dbm_set`, confirming the #1114 load path in the baseline.

Ubuntu Linux x86-64, Rust 1.97.1, GCC 15.3.0, GNU ld 2.47, cargo-c
0.10.24, Valgrind 3.27.1. The existing cargo-c installation was selected
from `/home/sheng/.cache/oxpinyin/fo-1018/cargo-c/bin`; none was installed.
The selected linker supports section ordering, so a compatibility-image
build was unnecessary. Setup verified all three oracle hashes and data
manifests, and the cached tkrzw probe passed. Both pin and subject facades,
controls, store binaries and private measured copies used the certified
runtime guard. The runtime's before/after SHA-256 is
`36c6aa4d77edb3bd6a628f02fc456f0a67096bdd451c04999141d7144f144d0e`.

Separate base/prototype targets and stages, measured libraries, runtime,
system-data/profile paths, work paths and TMPDIR were checked with findmnt
on `/dev/nvme0n1p2`, ext4. Normal host-wide heavy admission covered setup,
regeneration, shipping builds and the single measuring job sequentially;
no private slot lock was introduced. The facade campaign confirms all
fixture manifests and library hashes unchanged at completion. No shipping
source changed after the cloud correctness gates: only policy files did.
The cloud correctness results below remain recorded at their actual SHAs.

### Same-run M1 memory

Base / prototype / same-bytes prototype control / pin, KiB. Twenty paired
rounds per facade/profile after two warm-ups, rotating four-arm order.
The delta columns are paired medians with 95% bootstrap intervals, not
subtractions of independent medians. The control rule is teardown.py's
median and whole interval inside ±max(64 KiB, 1% of prototype median).

| facade/profile | base | prototype | control | pin | prototype − base [95%] | prototype − pin [95%] |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| pinyin/phrases | 9383 | 9118.5 | 9118.5 | 9272.5 | -264 [-265, -264] | -157.5 [-210.5, -113] |
| pinyin/journals | 9327 | 9065 | 9066 | 9132 | -261.5 [-263, -259] | -69 [-179.5, -65.5] |
| zhuyin/phrases | 8395 | 8075 | 8075 | 8105 | -320.5 [-322.5, -318.5] | -30 [-33, -30] |
| zhuyin/journals | 8447 | 8127 | 8127 | 8118 | -320.5 [-322, -317.5] | 8.5 [4.5, 11.5] |

Total-Pss control − prototype, paired median [95%], and bound, KiB:

- pinyin/phrases: 0 [-1.5, 1.5], bound ±91.185; VALID.
- pinyin/journals: 0 [-3, 3], bound ±90.65; VALID.
- zhuyin/phrases: 0 [-2.5, 1.5], bound ±80.75; VALID.
- zhuyin/journals: 1 [-1.5, 4], bound ±81.27; VALID.

Resident executable text, Rss KiB, at the same M1 checkpoints:

| facade/profile | facade base / prototype / pin | libtkrzw base / prototype / pin |
| --- | ---: | ---: |
| pinyin/phrases | 508 / 508 / 464 | 1048 / 732 / 782 |
| pinyin/journals | 508 / 508 / 464 | 1084 / 768 / 768 |
| zhuyin/phrases | 520 / 520 / 448 | 1084 / 768 / 892 |
| zhuyin/journals | 520 / 520 / 448 | 1084 / 768 / 888 |

Prototype/control text readings are identical in every pair. libtkrzw text
falls by 316 KiB in all four cells; facade text stays equal to base.
This is the same-run page-residency observation, independent of the older
#1098 attribution run. No gain is claimed against that older run's numbers.

### Same-run instructions and stripped size

Init Ir is the existing coordinator's constructors/init scope, excluding
its first-candidate toggles. Every prototype init control is exact.

| facade/profile | base Ir | prototype Ir | control Ir | pin Ir | prototype/base |
| --- | ---: | ---: | ---: | ---: | ---: |
| pinyin/phrases | 10,406,235 | 10,207,527 | 10,207,527 | 338,876,836 | 0.980905 |
| pinyin/journals | 10,437,692 | 10,238,312 | 10,238,312 | 338,949,787 | 0.980898 |
| zhuyin/phrases | 10,018,978 | 9,889,759 | 9,889,759 | 338,538,947 | 0.987103 |
| zhuyin/journals | 10,048,609 | 9,918,512 | 9,918,512 | 338,613,938 | 0.987053 |

First-candidate control differences are −26, −144, −88 and +110 Ir;
steady differences are −6,935, +4,889, +7,405 and −12,122 Ir, respectively.
All are INVALID under the exact-count rule; no comparison is claimed for
those scopes. All eight warm/cold init wall-time controls fail the existing
[0.95, 1.05] stability rule, so wall-time effects remain inconclusive.
The coordinator's cold samples evict the private facade copy only; no
whole-data cold-time result is claimed. Original timing samples were retained.

| stripped shipping library | base B | prototype/control B | pin B | increase over base |
| --- | ---: | ---: | ---: | ---: |
| libpinyin | 2,487,016 | 2,499,472 | 867,216 | +12,456 (+0.501%) |
| libzhuyin | 2,245,352 | 2,249,616 | 838,520 | +4,264 (+0.190%) |

### Store operations: partial, STOP

The existing store-ops runner compares base, a same-bytes base control,
and prototype against the same verified tkrzw system data. It has no pin
store-trait arm; pin figures above cover the facade workloads and size.
Every completed Callgrind row below has exactly equal base/control totals.
No timing result is inferred from the profile-only worker's clocks.

| operation | Ir/call base = control | prototype | prototype/base |
| --- | ---: | ---: | ---: |
| contains_raw.tree | 2561.40900000 | 2543.83920000 | 0.99314057 |
| get_raw.tree.empty | 2568.47270000 | 2566.24065000 | 0.99913098 |
| get_raw.tree.small | 2625.72620000 | 2621.84375000 | 0.99852138 |
| get_raw.tree.medium | 2764.90380000 | 2746.36380000 | 0.99329452 |
| get_raw.tree.large | 7850.43090000 | 7823.69900000 | 0.99659485 |
| get_raw.tree.miss | 4211.21235000 | 4215.77705000 | 1.00108394 |
| get_raw.tree.retain | 2885.38634375 | 2891.65478125 | 1.00217248 |

The first detected increase, tree.miss, is 91,294 additional instructions
over 20,000 calls. tree.retain completed while cancellation was in progress:
200,590 additional instructions over 32,000 calls. The paired controls
are exact in each case. These instruction increases accompany lower
resident memory, triggering the task's explicit STOP. Later operations
were not completed, and no attempt was made to explain away, tune or rerun
the increases. No adoption or PR decision is made here.

### Commands and retained local outputs

Task root `R=/home/sheng/Documents/Codex/2026-10-24/task-3`.
The exact orchestration and retention scripts are retained locally under
`$R/campaign/records`, alongside their original output. No capture is
committed or uploaded. Retention wrappers delegate to the existing
coordinators without changing workloads, arguments, counters or statistics;
they preserve original stdout/stderr, paused-worker smaps and DHAT files.
The stock facade tool's final "no raw records retained" line describes
its own scratch cleanup; the local wrappers retained originals separately.

```sh
export TMPDIR="$R/tmp"
OXPINYIN_ORACLE_ENV="$R/setup.env" just heavy -- tools/cloud/setup.sh --cells 'bdb kc tkrzw'
# Source only the task-owned setup output in each campaign shell.
source "$R/setup.env"
export PATH=/home/sheng/.cache/oxpinyin/fo-1018/cargo-c/bin:$PATH
just heavy -- "$R/prepare-policies.sh"
# install.sh with OXPINYIN_FUNCTION_ORDER_POLICY= builds unordered inputs;
# contract-diff.py --teardown-stage provision prepares the pin fixtures;
# generate-function-order.py runs twice, followed by cmp on both facades.
just heavy -- bash "$R/build-ordered.sh"
just heavy -- bash "$R/run-measurements.sh"
# The last command invokes measure-function-order.py through its local
# retention wrapper with --subject base=... --subject pr=...
# --subject control=... --subject pin=..., both explicit base policies,
# --cpu 2 --rounds 20, the generator's system identity and pinned profiles.
# Then store-ops.py through its retention wrapper uses --profile-only
# --keep-profiles --cpu 2, with isolated base/prototype bench binaries.
```

The exact measuring script SHA-256 is
`64ca422a84e339cf88a8b8c682d48640831567620b67066a516347cbbe44d4a8`.
Source/build-input/staged-library hashes, original profiler totals, timing
samples and the exact as-run scripts remain in `$R/campaign/records`.
Store Callgrind files remain in `$R/campaign/store-work/callgrind`.
No archive was created. The kernel-notification watcher detected the STOP;
SIGTERM went to the verified tools.dev slot supervisor, which reaped its
owned process groups. No further heavy job was started.

One initial inspection batch containing
`cat /home/sheng/.oxpinyin-oracle.env` was denied by automatic approval
review for possible credential/session-material exposure. The batch did
not execute; that read was omitted. Supported setup wrote a task-owned
environment file instead. No private environment content was printed.


Initial record 2026-10-24T04:04:52Z. Branch `wip/1125-tkrzw-shim` was based directly on
`wip/1114-tkrzw-record-copy` at `838dabc2fb86c9b221ba3bde914584596de075da`.
Source checkpoint `58566b3c56c738981bdf5a199e954eb9598a0651` is an unfinished
prototype. The cloud checks below supersede its original uncompiled status.
At that cloud checkpoint, no performance result existed for this prototype.
The cloud-era authorized rebase placed implementation commit `bceb13c0` directly
on verified #1114 head `1dfd3e52e6fb02155c014856075aaf3955c8ab29`.
The own implementation diff is unchanged; the rebase verification is below.
The original local prototype `af357bdef8b993169f829a52583a7a7fe367db0e`
is preserved under `local/1125-unvalidated-af357bde`.

## Cloud status and deferred policies

Recorded 2026-10-24T04:16:00Z in the fresh Codex Debian 13 x86-64 session.
The source checkpoint was fetched from the authorized branch and checked
to have exactly `838dabc2` as parent, with Shenghao Chen as both author
and committer. Work uses `/tmp/oxpinyin-1125`; the shared checkout stays
on its original branch. No checkout `.agents/skills` directory exists.

Function-order regeneration is **deferred, not passed**. The required
`docs/packaging.md` Regeneration workflow uses ext4, teardown-profile
provisioning and startup/steady profiling through the teardown boundary.
The maintainer clarified that regeneration moves to the first Ubuntu-slot
step, before measurements, and cloud correctness continues on debug
builds only. This host also reports
tmpfs for the worktree/TMPDIR and overlay for `/workspace`. No generator,
ordered shipping relink or layout-resolution claim has been made. No
unordered shipping fallback was used. The existing tkrzw policies remain
stale. Final regenerated policies must be folded into the prototype code
commit there. Stale policies are expected at the cloud checkpoint.

Native completion checks at `58566b3c` passed:

- `cargo fmt --all --check`, `git diff --check`, and
  `bash -n tools/packaging/relink-versioned.sh`.
- Through `just heavy`, `cargo clippy --locked -p oxpinyin-store
  --all-targets -- -D warnings` and `cargo test --locked -p oxpinyin-store`:
  57 tests passed; four existing real-data BDB tests ignored.
- The same clippy and test commands with `--no-default-features --features
  tkrzw`: 59 tests passed. C++ compilation emits upstream header unused
  parameter warnings; no warning suppression was introduced. Bindgen and
  linking succeeded. Before execution, the three test executables were
  checked through `bash tools/tkrzw/runtime.sh --print-path tkrzw 1125
  subject OBJECT` against setup's certified library.
- `uv run --no-project --managed-python --python cpython@3.15+gil python
  -B -X lazy_imports=normal tools/bisection/contract-diff.test.py
  TkrzwSavedBigramTests`: six passed. The comparator and its mutation
  assertions are unchanged from #1114.

Native targets are `/workspace/oxpinyin-1125-targets/bdb` and `.../tkrzw`.
Each shell sources the existing native activation, unsets its fixed
`CARGO_BUILD_JOBS`, `RUST_TEST_THREADS` and `PKG_CONFIG_SYSROOT_DIR`, and
selects the certified tkrzw `PKG_CONFIG_PATH` and `LD_LIBRARY_PATH`.
TMPDIR is `/tmp/oxpinyin-1125-tmp`. The writable host-wide slot directory
is `/workspace/oxpinyin-env/locks`; the default under the read-only home
failed before starting a process. `just` 1.59.0 was installed through
the same Python slot implementation to bootstrap `just heavy`.
Rust 1.97.1 was already present. Setup provisions checksum-verified uv
0.13.1 and managed CPython 3.15.0 with the GIL in task-owned paths.
TLS verification stays enabled; `OXPINYIN_PROXY_CA` names the existing
session CA bundle `/etc/ssl/certs/ca-certificates.crt`.

The explicit certified-runtime probe passed: 18 comparator GOT
relocations, hash/tree/skip create/read, removal, comparator type 1,
`RESULT : healthy`. Both pin facades resolve through `runtime.sh` to
`/workspace/oxpinyin-env/cloud-cache/tkrzw/lib/libtkrzw.so.1.77.0`.

An isolated C++ boundary unit check compiled the actual `shim.cc` from
stdin with `just heavy -- c++ -std=c++17 -I<CERTIFIED>/include
-I<STORE>/src/tkrzw -x c++ - -L<CERTIFIED>/lib -ltkrzw
-o /tmp/oxpinyin-1125-shim-exceptions`, then launched it through
`runtime.sh`. It installed a throwing record processor on a TinyDBM:
`std::runtime_error`, `throw 42`, and a `std::exception` with a long
`what()` message after making global `operator new` throw `bad_alloc`.
All three calls returned false and SYSTEM_ERROR. The final error message
was empty when recording it could not allocate. Allocation was restored,
and Count and Close succeeded on that handle. Result: PASS for all three
containment cases. This checks the boundary's source-derived failure
contract; it does not run the pin or measure performance.

Static comparison with the parent also confirmed unchanged bodies for
load_db, save_db, db_set_one, holds_no_record, ends_before_records,
record_base and the collision-chain regression. db_set_record changes
only the FFI callee, retaining overwrite=true. No health/record-section
guard or comparator assertion was weakened.

After fetching current `origin/main` (`06195c1a`), rule 2 path checks give:

```text
git diff --name-only origin/main...HEAD -- crates/oxpinyin-store/
crates/oxpinyin-store/Cargo.toml
crates/oxpinyin-store/build.rs
crates/oxpinyin-store/src/tkrzw/ffi.rs
crates/oxpinyin-store/src/tkrzw/mod.rs
crates/oxpinyin-store/src/tkrzw/shim.cc
crates/oxpinyin-store/src/tkrzw/shim.h

git diff --name-only origin/main...HEAD -- ':!*.md' | xargs -r grep -lE 'feature *= *"(bdb|tkrzw|kyotocabinet)"'
crates/oxpinyin-store/build.rs

git diff -U0 origin/main...HEAD -- '*Cargo.toml' | grep -E '^[+-][^+-].*(bdb|tkrzw|kyotocabinet)'
+# Compiles the Kyoto Cabinet and tkrzw C++ shims (#1118, #1125).
```

Checks 1 and 2 select bdb, kc and tkrzw. Check 3 prints a comment;
its existing regex does not match the tkrzw feature line itself (#1128).
That separate issue is not changed here.

### Completed cloud differential checks

Recorded 2026-10-24T04:37:41Z. Subject artifacts were built in debug,
opt-level 1 at `49ea0fc9`; source equals `58566b3c`. The supported
cell-artifacts build records the worktree, cell, profile and artifact
identities separately for each target. A first `--no-build` request on
the manually built revert target was refused for having no identity;
the supported build path then recorded and verified it. No identity was
handwritten and no target was shared across worktrees.

`tools/cloud/setup.sh --cells 'bdb kc tkrzw'` completed unchanged:
both oracle facade hashes and both data manifests per cell verified.
Both subject tkrzw facades, both pin facades and the named-mutant facades
resolve through `runtime.sh` to the same certified library. `nm -D -C`
on each subject shows the HashDBM, TreeDBM, TinyDBM and BabyDBM constructor
imports, and no PolyDBM or `tkrzw_dbm_*` import. Thus these debug libraries
contain the concrete-class prototype.

The existing #1114 selection ran with:

```sh
just heavy -- tools/bisection/run-contract-diff.sh tkrzw "$PIN" \
  "$TARGET/tkrzw/debug/libpinyin_capi.so" \
  "$TARGET/tkrzw/debug/libzhuyin_capi.so" -- --issue 1114
```

Both `issue1114-collision-load-save-pinyin` and
`issue1114-collision-load-save-zhuyin` MATCH. The existing runner compares
saved bytes directly and executes pin-to-subject and subject-to-pin
reload/save in fresh workers: eight successful workers total. Only the
named timestamp field at bytes 48–55 is masked. Byte 56, lengths, all
bucket pointers, links and payloads remain exact. The native collision
regression checks bucket-chain order independently. Neither comparator
nor mutation assertions changed from #1114.

Full suites use:

```sh
uv run --no-project --managed-python --python cpython@3.15+gil \
  python -X lazy_imports=normal tools/bisection/run-all.py \
  --runners contract-diff,same-data-dir-diff --cells bdb,kc,tkrzw \
  --oracle-root /workspace/oxpinyin-env/cloud-cache/oracle \
  --target-root /workspace/oxpinyin-1125-targets \
  --runner-timeout contract-diff=14400 \
  --runner-timeout same-data-dir-diff=7200 \
  --log-dir /tmp/oxpinyin-1125-gate-output
```

The first same-data runs failed to compile `tie-order-diff.c` because
GLib's pkg-config paths name `/usr`, while its installed headers are in
the pre-existing task-native prefix. Adding the two provisioned GLib
include directories to task-local CPATH fixes that environment error.
The full same-data runner then passed all 15 drivers on bdb, kc and
tkrzw, including dynamic-adjust non-vacuity and both on/off comparisons.
That rerun used the command above with `--runners same-data-dir-diff
--no-build` and a separate local output directory. No subject source,
driver, expectation or compiler warning setting changed.

The full BDB contract result is 2,893 MATCH, 2 SKIP, 4 HOST-DEPENDENT,
0 DIFFER: 2,899 total. Revert-and-check built the exact parent `838dabc2`
in `/tmp/oxpinyin-1125-base`, using its own
`/workspace/oxpinyin-1125-base-targets/bdb`. Its full BDB contract suite
also passed with exactly those counts and verdicts, using the command
above with `--runners contract-diff --cells bdb` and that target root.
The prototype has no observable effect on BDB in these gates.

A named tkrzw mutation, "force OPEN_NO_CREATE on HashDBM saves", was
prepared in a third isolated worktree/target. It changes only the shim's
open-option expression to select OPEN_NO_CREATE for every HashDBM.
Source predicts failure when save_db removes and then tries to create
the output file. The unchanged #1114 selection detects changed saved
bytes, but its first-byte-mismatch branch attributes the error to the
pin result and reports two INVALID cases. That is not a successful
DIFFER gate. No comparator or assertion was changed.

The same built mutation then ran the existing ordinary cases
`issue677-mask-save-sequence-pinyin` and
`issue677-mask-save-sequence-zhuyin`, with `--expect-parent`. Both workers
on both sides exited 0, both cases reported DIFFER and expected_ok=true,
and the runner exited 0. Their source reads the raw saved bigram row
after mask/save: preventing creation leaves the old row observable.
This is the source-derived effect of the named open-flag mutation.
The mutation source was restored with `git restore --source=58566b3c`,
and `git diff --exit-code` plus an empty status confirmed exact
restoration. Its isolated worktree was removed. The unchanged normal
prototype then reported MATCH for these two cases and both #1114 cases
(four MATCH, twelve successful workers). Mutant artifacts never entered
the normal targets or the authorized branch.

Full cloud gates completed at 2026-10-24T04:59:55Z:

| cell | MATCH | SKIP | HOST-DEPENDENT | DIFFER | total | same-data |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| bdb | 2,893 | 2 | 4 | 0 | 2,899 | IDENTICAL, 15 drivers |
| kc | 2,888 | 7 | 4 | 0 | 2,899 | IDENTICAL, 15 drivers |
| tkrzw | 2,890 | 5 | 4 | 0 | 2,899 | IDENTICAL, 15 drivers |

Every full contract row has expected_ok=true; none is INVALID. The
initial aggregate exits 1 because of its earlier same-data compiler
failures; its three contract rows all PASS. The corrected three-cell
same-data rerun exits 0. These component results, not the failed initial
aggregate exit, establish the completed gates.

The execution transport briefly disconnected. Reconnection inspection
confirmed both active contracts survived and finished normally; no gate
was canceled or replaced. The baseline worktree cleanup completed during
that interruption, and its outcome was inspected before proceeding.

The maintainer supplied replacement parent
`1dfd3e52e6fb02155c014856075aaf3955c8ab29`, tree
`9fddaf5f9cd754ac4a42eb83e6b8b9f96e00cae2`. The remote branch was checked.
Its actual diff from `838dabc2` changes only KC's include override from
push to insert(0) in build.rs, plus two findings documents. There is no
tkrzw, BDB or differential change. Rebase and the consequent native
checks are recorded below. No measurements have started and no
adoption conclusion is made.

### Verified rebase and final cloud checkpoint

Recorded 2026-10-24T05:07:22Z. The green pre-rebase checkpoint is
`be997c1782a3f7103dfb4733a69425f440bf0c3d`. Before rebasing, both the full
own patch and the non-Markdown implementation patch were saved as normal
local `git diff --binary 838dabc2 HEAD` outputs. The remote parent branch
was checked to be exactly `1dfd3e52e6fb02155c014856075aaf3955c8ab29`.
Only this branch's four own commits were replayed:

```sh
git -c user.name='Shenghao Chen' -c user.email='shenghaoc@outlook.com' \
  rebase --onto 1dfd3e52e6fb02155c014856075aaf3955c8ab29 \
  838dabc2fb86c9b221ba3bde914584596de075da wip/1125-tkrzw-shim
git diff --binary 1dfd3e52 HEAD
git log --format='%H %an <%ae> / %cn <%ce> %s' 1dfd3e52..HEAD
```

The rebase was conflict-free. The resulting checkpoint is `eccacefd`:
implementation `bceb13c0`, followed by own findings commits `98147792`,
`104114e3` and `eccacefd`. All retain Shenghao Chen
<shenghaoc@outlook.com> as both author and committer. The log contains no
old #1114 copies. The parent branch was not rewritten by this lane.
The own full-patch stable ID before and after is
`e5de3ab2cfccc46dff0683877a44f37c53e1aa37`; the implementation-patch ID is
`49e90d72f20b1bb801d991508acf4cd043e46f29`. Direct normalized-hunk
comparison also passed for both patches, removing only blob-ID lines
and hunk coordinates. No own change was lost or shadowed, including
#1114's Set path, #1110's guards and the shim functions.

The rewritten checkpoint was pushed with the exact lease
`--force-with-lease=refs/heads/wip/1125-tkrzw-shim:be997c1782a3f7103dfb4733a69425f440bf0c3d`.
This final findings update is documentation only; the own implementation
patch retains the ID above.

Rule 7: the rebase brings the KC header-precedence fix into build.rs,
a file this lane touches. Native clippy (`--all-targets -- -D warnings`)
and `cargo test -p oxpinyin-store` were rerun through `just heavy` for
default/BDB, tkrzw, and kyotocabinet. Results: 57 / 59 / 50 tests passed,
respectively; BDB has four existing ignored real-data tests. All clippy
runs and fmt passed. The tkrzw test binaries passed the certified-runtime
guard before execution. The two status getters are allocation-free;
the common guarded boundary and best-effort reporting checked earlier
are unchanged by the rebase.

Both facades were rebuilt in debug opt-level 1 for every cell through
`cell-artifacts.sh`. The pinyin rebuild and same-data rerun used the full
command above with `--runners same-data-dir-diff` and
`--log-dir /tmp/oxpinyin-1125-post-rebase-same-data-output`. The zhuyin
rebuild used, per cell, `just heavy -- bash
tools/bisection/cell-artifacts.sh --artifacts zhuyin-capi`. The appropriate
PINYIN_ORACLE_DBM and CARGO_TARGET_DIR were set in each shell.
Same-data passed again on all three cells, all 15 drivers each.

All six rebuilt debug facade SHA-256s equal their fully gated pre-rebase
values (`sha256sum --check /tmp/oxpinyin-1125-debug-before-rebase.sha256`,
six OK). Values, pinyin / zhuyin:

| cell | libpinyin_capi.so | libzhuyin_capi.so |
| --- | --- | --- |
| bdb | c85a28bc7f7400e1d247bebd2d2c75a29f0a8be9a499329b6103524a991ac03a | 8b14812a5d726e0d12a85c0b7ee66cf52776b2ebde09636686c6c90fd9e841aa |
| kc | 9829730bf31b11ede42964528a0444bcf933dcb43d0b08918cd414e849ace025 | 9ee18ada0372acefe9d48e04f385a63f926af97971bc58b1b4cf2ddbac8c29c0 |
| tkrzw | bb4284919eb9026769f92a5b00af9dda9811cc7266ba05f43d2bec608f9bba6c | a04867f582e4bf6d1acbe5d3d2f452e56d932eaa8087e3b30355f93f6d2cbe5a |

Both tkrzw facades on both sides were checked again with runtime.sh and
resolve to the same certified libtkrzw.so.1.77.0. Full contracts were not
repeated: the rebuilt artifacts have exactly the same SHA-256s as those
that passed all three full suites, so the table above applies to the
rebased source's debug outputs. Revert-and-check and mutation were not
repeated; neither the implementation diff nor differential changed.

Cloud correctness is complete. Stop for the Ubuntu slot. Function-order
regeneration remains its first step, with policies folded into the code
commit before measurements. No policy resolution, ordered shipping
relink, performance result, adoption decision or PR is claimed here.

## Source inspection

[Issue #1125](https://github.com/shenghaoc/oxpinyin/issues/1125) is open.
[KC shim #1121](https://github.com/shenghaoc/oxpinyin/pull/1121) is merged
as `6755fc5a257cf68da58f3743e450aa02cad212b9`.
[Issue #1114](https://github.com/shenghaoc/oxpinyin/issues/1114) remains
open; its exact checkpoint above is the parent of this prototype.

The pin's source was read with `git show 074a2219:<path>` from
`/home/sheng/repos/libpinyin`, whose checked-out HEAD is not the pin.

- `src/storage/ngram_tkrzwdb.cpp:48-80`: default TinyDBM without Open;
  load from HashDBM with OPEN_NO_CREATE; save to a new default HashDBM,
  copy in native iteration order and soft-sync before closing.
- `src/storage/tkrzwdb_utils.h`: attach_options maps read/write/create
  flags; copy_tkrzwdb walks source records and calls destination Set.
- `src/storage/chewing_large_table2_tkrzwdb.cpp:87-126` and
  `src/storage/phrase_large_table3_tkrzwdb.cpp:77-117`: default TreeDBM
  for files and default BabyDBM for the in-memory container.
- `crates/oxpinyin-store/src/kyotocabinet/{shim.h,shim.cc}`, its build.rs
  module, and `docs/findings/kc-tkrzw-pss-1098.md` (libtkrzw) were read.
- tkrzw 9ee8416's [C callback adapter](https://raw.githubusercontent.com/estraier/tkrzw/9ee8416/tkrzw_langc.cc)
  and [PolyDBM implementation](https://raw.githubusercontent.com/estraier/tkrzw/9ee8416/tkrzw_dbm_poly.cc)
  were read. The latter skips Open for empty-path memory containers and
  delegates record operations to their concrete DBM.

Cloud source verification used separate local checkouts exactly at
libpinyin `074a2219c90feaf962d0d24f034514033ece5f99` and tkrzw
`9ee84164cf45e441aa8186418c03a02c9b89f08b`. The installed five DBM
headers compare byte-identical to that tkrzw source. HashDBM and TreeDBM
Open delegate to OpenAdvanced with default TuningParameters
(`tkrzw_dbm_hash.h:356-375`, `tkrzw_dbm_tree.h:259-278`); their default
constructors use MemoryMapParallelFile (`tkrzw_dbm_hash.cc:2401-2403`,
`tkrzw_dbm_tree.cc:2558-2560`). TinyDBM's default -1 uses
DEFAULT_NUM_BUCKETS (`tkrzw_dbm_tiny.h:155`, `.cc:208-218`), with no Open
on the pin's memory container. BabyDBM defaults to LexicalKeyComparator
(`tkrzw_dbm_baby.h:149`), also without Open. The pin calls no alternative
tuning on these paths. The shim can reproduce all four classes' flags
and tuning. Its Set retains the C++ default overwrite=true
(`tkrzw_dbm.h:1123-1132`). These are source conclusions, independent of
the differential gates above.

## Local changes

`crates/oxpinyin-store/src/tkrzw/shim.h` and `shim.cc` implement 18 C
entry points over concrete HashDBM, TreeDBM, TinyDBM and BabyDBM objects.
The boundary contains C++ exceptions and returns failure; error text is
best effort. The shim supplies status, callback tags, point processing,
ProcessMulti, Set, count, health, synchronize, rebuild and iterators.
The loading path retains #1114's db_set_record and calls the shim Set
entry point with overwrite=true. No single-record ProcessMulti batch is
introduced into load_db. #1114's collision-chain and value tests remain.

The tkrzw feature enables the already locked cc dependency. build.rs
compiles C++17 at O2 with hidden visibility and generates only shim C
bindings. The shipping relink includes libstdc++. No cxx dependency or
Cargo.lock change was introduced.

Static text comparison against the #1114 base confirmed unchanged bodies
for TkrzwUserBigramDb's load_db and save_db, db_set_one, holds_no_record,
ends_before_records and record_base. db_set_record changes only the FFI
callee to oxtkrzw_dbm_set, retaining the overwrite=true argument. is_healthy
still calls the backend health operation through the shim. Existing guard
and collision tests were retained.

## Commands and checks

Working directory:
`/home/sheng/Documents/Codex/2026-10-24/task-3/worktree`.
Home-disk TMPDIR:
`/home/sheng/Documents/Codex/2026-10-24/task-3/tmp`.

```sh
git -C /home/sheng/repos/oxpinyin fetch origin main
git -C /home/sheng/repos/oxpinyin worktree add -b wip/1125-tkrzw-shim \
  /home/sheng/Documents/Codex/2026-10-24/task-3/worktree origin/main
export TMPDIR=/home/sheng/Documents/Codex/2026-10-24/task-3/tmp
cargo fmt --all
cargo fmt --all --check
git diff --check
bash -n tools/packaging/relink-versioned.sh
```

At the original local prototype, the final three checks exited 0. An initial fmt check reported formatting
differences; cargo fmt corrected them before the passing check. These are
light checks, not a completion gate.

## Cloud handoff before Ubuntu campaign (historical)

Superseded by the Ubuntu results above; retained for checkpoint provenance.

The checked-in tkrzw policies have not been regenerated for this prototype.
Regeneration is the first Ubuntu-slot step, before measurement; fold those
policies into the code commit. No ordered shipping relink is claimed here.
Ubuntu admission remains held for #1114, then #1031, then #1073.
All base/prototype/pin measurements remain unrun.
The saved-byte mask remains limited to HashDBM META_OFFSET_TIMESTAMP
zero-based bytes 48–55 inclusive; byte 56 and every other byte must match.
Strict bucket-chain and saved-file checks remain required.

The measurement requirements remain one run with base, prototype and pin,
certified libtkrzw on every build and launch, resident libtkrzw and facade
text, paired-control total Pss at M1, exact-control init and store-operation
Ir, and stripped size. Build in the compatibility image if the host linker
lacks --section-ordering-file. Target and stage paths must be distinct,
and measured binaries must be verified to contain the actual prototype.
A time-for-residency trade remains a STOP. No adoption conclusion is made.
The final report must stop for the maintainer's decision without a PR.
