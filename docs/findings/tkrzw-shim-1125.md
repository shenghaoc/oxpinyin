# tkrzw concrete-class shim: evaluation (#1125)

## Adoption preparation on current main: validation pending

The maintainer authorized adoption as a documented trade only if the final
combined build meets the stated gates and guardrails. Prior STOP results
below remain historical at their cited measured SHAs and artifact hashes;
they are not carried forward as final-build results. No final adoption or
PR decision is made at this checkpoint.

Before rewriting, the source-only annotated tag
`provenance/tkrzw-shim-measured-2026-10-24` was pushed and fetched back from
origin. Its peeled anchor is `c0078254fc9c706fd2e7778601e6888689b747c9`. The anchor's tree
is exactly pre-adoption findings head `9d81c9e29e3714f2836a295174a768318136560f`;
its multiple parents retain the disjoint historical graphs rather than
claiming a tag on one old tip covers rewritten commits. This is not a
merge of source changes into the implementation branch. It also retains
the two cited upstream source-pin graphs. An ancestry check against the
**fetched remote tag** passed for every commit listed below. The audit
conservatively includes non-measured commit citations too. Build IDs,
artifact hashes, patch IDs and tree IDs were distinguished from commits.
All pre-existing measured SHA and artifact-hash text below is preserved.
The tag retains source objects, **not binaries**; artifact hashes identify
the original local measured outputs and do not recreate or preserve them.

| cited source commit covered by remote tag |
| --- |
| `06195c1a6ceb6e2ea97d0679ca2b8fe0f26fb1c2` |
| `074a2219c90feaf962d0d24f034514033ece5f99` |
| `0e7f98ac785b81837b74a4015eaebc6d63223241` |
| `104114e3fbf7a303dcfc69eadd0f002fc14e11a1` |
| `1dfd3e52e6fb02155c014856075aaf3955c8ab29` |
| `20dc2615055ea4bae2fc86f253a03966414de6d1` |
| `2b188ae85a0e0d037e6235e10cbe9cb48aba4a9c` |
| `2d1b7ade67b861e95da9e402a996dffaffb6aa52` |
| `41816464a3d360df023c343a87074d05d40d1871` |
| `49ea0fc9340b1d8f9dd625a4de6929664feca088` |
| `58566b3c56c738981bdf5a199e954eb9598a0651` |
| `5bfdd4a5e9728ab84eb738358c23b93ad532b0f8` |
| `6755fc5a257cf68da58f3743e450aa02cad212b9` |
| `6e50b77ef9b380f36ca0cd6d3fb76732c094baaf` |
| `7fdab25fa29b6b2a48820f5b82c0a629f93f4bb3` |
| `838dabc2fb86c9b221ba3bde914584596de075da` |
| `92b7d61bb60a46c41cc85a0ffed70cf7c354e702` |
| `974f98bc57eff6d47e89834cecf4582c5f354632` |
| `9814779235a91ae621f5b79e8605b48e1d19e0fc` |
| `9d81c9e29e3714f2836a295174a768318136560f` |
| `9ee84164cf45e441aa8186418c03a02c9b89f08b` |
| `af357bdef8b993169f829a52583a7a7fe367db0e` |
| `b3018335cce3e1d080f7790716bf88a4726247a2` |
| `b9b8a31bee471b940fdaecc4c0cc10ebea0c93b2` |
| `bb43abcd900aac7be409be82160d063b6493aed5` |
| `bceb13c0039ca1d902c81758a68825a81f88a6a5` |
| `be997c1782a3f7103dfb4733a69425f440bf0c3d` |
| `c4cc6d0ca1063704ca2905966a481647b8bf164b` |
| `d7788072d3b8078aaa500cb5fcb98e0542fe7278` |
| `eccacefd61a04db3fdf8e031dbd86960fc464ce6` |
| `ef12b1134ef160b6085b7ea1c4f02e561a83565d` |

The new baseline is current main
`f9257eae495e06677d74026e93c285b42c6c34e8`. The own shim patch applied
cleanly, retaining main's changed files and all regression tests. Its own
shipping changes remain the store adapter/FFI, handwritten C++ shim and
header, cc build integration and tkrzw-only C++ relink. The empty-message
fast path remains removed, and #1110's health/record-section guards and
#1114's single-record Set load path remain. No new shim optimization or
fault investigation was introduced. The tkrzw policies were regenerated twice
on this combined tree, from fresh unordered shipping inputs. Both passes
matched byte-for-byte. These are freshly derived combined-tree policies,
not copied historical files. The seven own source/build/adapter files are
byte-identical to the historically measured 9d81c9e2 tree; main's changes
around them and the fresh policies are the combined-tree differences.

Since old baseline d7788072, main changed candidate collection to omit
unused pronunciation pricing (#1140), with Dictionary candidate lookup
interfaces and implementations in core/data/user, engine lookup callers
and regression tests. Main also regenerated tkrzw policies, changed Rust
documentation/cfg-doc attributes, and added/updated verification, native
startup measurement, platform/oracle selection and documentation tooling.
These surrounding changes are main's, not attributed to the shim. Hence
old facade performance is not a measurement of this combined source.
The old store table will stand only if both timed benchmark binaries,
runtime, fixture inputs and build flags match; otherwise it stays explicitly
historical. Current preparation has no such binary-equivalence result yet.

Fmt, clippy and store tests (default and tkrzw) passed on the same
shipping source. Main then added cgroup-v1 admission accounting only; the
clean rebase did not change shipping source. Main's subsequent bilingual
README and contributor documentation change also leaves shipping source and
profiling inputs unchanged. Current rule-2 checks 1, 2
and 3 all select bdb,kc,tkrzw. The combined code commit is `08e178829f991469c864b98a980286a2f2a899d3`. The regenerated policies
are folded into that code commit. Generated policy SHA256 values are
`c1ddbcde97704e10b82c7671fc501f1d4adfd15cc8b34472b8e9b341c8321660`
(pinyin) and
`1713876b7cf3faf03c7695fdd2746a5318ad40083627e835d0e58dcf474d299b`
(zhuyin). Selected-cell differentials passed as described below. The remaining check is one Ubuntu
final-build campaign against this main plus the pin. Essential guardrails
must be conclusive under their existing exact/range/timing rules. No new
cold-init tolerance is assumed. A worse/uncertain trade or essential
INCONCLUSIVE guardrail means STOP with no PR. A draft PR is permitted only
after all conditions pass; no review request or merge is authorized.

The maintained four-arm controller now has optional `--emit-samples` JSON
output, after each worker exits, retaining original timing/resource,
memory and steady readings with round, arm order and library identity.
It changes neither worker scopes nor cache preparation, and defaults off.
The controller's 44 tests pass, including original-value/order retention,
missing fields versus zero, disabled output and write failures. This
allows the final timing controls to use paired original samples instead
of independently sorted summaries. No private measurement controller is
used. The runbook documents the option.

Same-data differentials passed in bdb, kc and tkrzw. Full contract results
cover all 2,899 registered cases at the unchanged shipping checkpoint:
bdb 2,897 MATCH / 2 expected SKIP; kc 2,892 MATCH / 7 expected SKIP;
tkrzw 2,894 MATCH / 5 expected SKIP. No case TIMEOUT or failing verdict was
recorded. These use the supported two-case pool, the unchanged 300-second
per-case deadline and a 3,600-second aggregate runner deadline. The original
full job survived loss of its transport and completed; its final records,
rather than an incomplete console view, establish the result. A subsequent
full retry was cancelled and reaped after 19 completed cases. Exact
reconciliation against the default registry leaves zero unfinished or
timed-out IDs, so no selected case needs to be repeated.

The final clean rebase also includes main's #894 contract fixtures and
recorded-discrepancy verifier, and conservative cgroup-v1 admission fixes.
It changes no shipping, build, profile, worker or policy-generation input.
The completed gates stand under the proportional re-verification rule;
this lane does not claim to have rerun main's new #894 cases.

Main then upgraded cc 1.4.3 → 1.9.0 and find-msvc-tools 0.1.11 → 0.1.15.
Both tkrzw policies were regenerated twice again under that lockfile; the
two new passes matched and are the committed policies identified above.
Their Rust instantiating-crate identities differ from the earlier policies,
so the earlier files were not carried forward. Each facade still selects
all three C++ paths (two init and one steady). The cc archive member name
changed, but the extracted shim object is byte-identical: 44,056 bytes,
SHA256 `f2a5243806846d5e2df0562d95f7be5aa388a407c4ae1e72c3c74a9276e2b6bb`.
This is object identity, not a facade performance claim. Fmt, both clippy
configurations and store tests passed again: tkrzw 52 unit + 7 integration,
default 50 unit + 7 integration, with four existing ignored fixture tests.
Both exact current-main shipping facades also built with their own checked-in
policies and no unordered fallback. Final linked-object ordering checks and
the final campaign remain pending.



## Cold-page diagnosis: records absent; same-setup one-pass entry point blocked

Recorded 2026-10-24T11:03:31.939909+00:00. STOP remains. This is diagnosis only
from findings head 0e7f98ac785b81837b74a4015eaebc6d63223241 and the exact
measured base/prototype artifacts below. No subject worker was launched;
no build, source/policy change, regeneration, rebase or new driver was made.
The two additional major-fault events are **not identified to pages** by
this investigation. Unknown page sets are not reported as empty sets.

### Retained page evidence and checkpoint scope

The 176 original cold native pinyin timing workers (phrases/journals,
22 rounds including warmups, four arms) retain their stdout resource
counts but no per-file mincore vectors or page-index runs at M0/M1.
The original measure-function-order.py cache_pages function constructs a
mincore vector but returns only `(resident_count, total_pages)`; its
cold precondition validates zero residency **before launch**, not a bitmap
at either checkpoint. The vector is discarded. This aggregate cannot
identify which pages became resident later.

The companion memory workers retained smaps and rollup snapshots, not
mincore vectors. As established above, those workers used warmed facade
copies despite the wrapper's stale `cold=true` context label. Their
segment RSS/Pss totals and address maps cannot reconstruct a per-file
cache bitmap. The campaign's Callgrind executed-page records are not
file-cache residency records either. There are no retained cold page
sets from which to calculate PR-only M0-to-M1 pages.

Checkpoint names also matter. In the measured memory workload, M0 follows
dlopen/resolve, and M1 follows init **plus configuration, allocation and
first candidate**. M0-to-M1 would therefore not isolate init even if its
bitmaps had survived. The existing native worker's `init` mode instead
pauses at M0 and **I1**, immediately after the init export, before those
later operations. No retained init-mode run exists in this campaign.

The additional two major faults are process-wide getrusage events. They
do not name a file, segment or instruction. Newly resident mincore pages
would include cache population/readahead, not necessarily executed code
or the faulting addresses. A page difference is not a one-to-one mapping
to major-fault events. Across different base/prototype ELF layouts, equal
page indices are equal file offsets, not necessarily equal functions.

### Existing-worker capability and exact blocker

The low-level pieces exist: teardown.py resident_pages(fd, ranges=True)
returns page-index runs; residency uses that option for SUBJECT in init
mode; Campaign.run(mode='init') can read them at paused M0/I1. That does
not mean the requested one-pass invocation exists as a supported entry
point with the required cache and binary constraints.

- The original measure-function-order.py CLI runs the >=20-round campaign,
  with time-mode cold facade copies and warm memory workers. It exposes no
  one-pass init bitmap-retention option.
- contract-diff.py exposes teardown cache choices `warm` and `cold` only.
  Its `cold` path evicts the facade, resolved dependencies, system data and
  user files via cold_files/evict_files. That is broader than this fixed
  campaign's **cold facade only** setup.
- Its teardown startup calls build_worker and provisioning; there is no
  CLI switch to reuse the exact retained worker and existing fixtures for
  one sample. The init-attribute stage runs the full hardcoded multiple-
  probe schedule (including 30 count rounds plus warmup, sampling, fault
  tracing, syscall tracing and Lackey), not one permitted residency pass.
  Pilot/confirm memory samples retain aggregate per-file residency through
  file_residency, not per-page facade runs, and also run multiple samples.

**Blocked action:** a single existing-worker pass which retains M0/init-end
per-page facade residency, reuses the fixed measured artifacts and worker,
and applies only the original cold-library setup. It was **not attempted**,
not denied by an approval system. Composing a bespoke pause/evict/mincore
controller, monkeypatching the eviction scope or round schedule, or adding
a driver option would cross the explicit no-new-driver/source-capability
boundary. Running the broader cold CLI or full attribution campaign would
silently change the experiment and exceed one pass. None was done.

### Static placement from the exact measured ELF and policy records

This independent static result does not need a new subject execution. The
files match their published completed-campaign hashes. Policy bytes are
those used by the campaign, not current-main policies. The existing linked
object verifier was read-only and passed for both objects.

| arm | measured facade SHA256 | policy SHA256 | init-tier virtual-address span [start,end) | whole ordered init+steady span |
| --- | --- | --- | --- | --- |
| base | 92a769b8d4c57a61c6190e7e50699deb44f022d87228cb5f96ac00dd16b0031e | d82dedd278c029f5ca1453802ebaf01e134e3e31298724f86a538c2458d6dab9 | 0x11060,0x79ebf | 0x11060,0x7bba7 |
| pr | d040918c1da19056ef4e8646cb82be7e7151884766025610e83bad08ba5d2837 | 419b2c2b130d2013267ed953deb01142370c5ef220dbaddb680bd2c069f92bec | 0x111c0,0x7ac1f | 0x111c0,0x7c9b7 |

The tier spans are derived separately from actual policy tiers and sized
linked symbols. They must not be confused with teardown.py policy_prefix,
which obtains **all** policy names, including the steady tier. A function
outside the init tier can still be correctly ordered in the steady tier.

PT_LOAD mappings from readelf on these exact files:

| arm | file offset | ELF virtual address | file bytes | memory bytes | flags |
| --- | ---: | ---: | ---: | ---: | --- |
| base | 0x0 | 0x0 | 0x103c8 | 0x103c8 | R |
| base | 0x11000 | 0x11000 | 0x1e9ea5 | 0x1e9ea5 | R E |
| base | 0x1fb000 | 0x1fb000 | 0x5606c | 0x5606c | R |
| base | 0x251cb0 | 0x252cb0 | 0xcd50 | 0xce00 | RW |
| pr | 0x0 | 0x0 | 0x10b40 | 0x10b40 | R |
| pr | 0x11000 | 0x11000 | 0x1ebdb5 | 0x1ebdb5 | R E |
| pr | 0x1fd000 | 0x1fd000 | 0x56c80 | 0x56c80 | R |
| pr | 0x254c10 | 0x254c10 | 0xce98 | 0xcf48 | RW |

For a file-backed symbol byte at ELF virtual address v within one of those
segments, file offset is p_offset + (v - p_vaddr). Every span below was
checked to fit its file-backed segment. Both objects happen to have equal
virtual/file offsets for these spans; that equality was verified, not
assumed from a process address. Host page size is 4096 bytes. Page indices
below are **file** indices, with half-open file-byte and symbol intervals.
ASLR addresses are not divided by 4096 to infer file pages.

#### All three C++ policy rows

| C++ row / tier | exact raw policy identity | symbol VA span | file-byte span | file pages | placement |
| --- | --- | --- | --- | --- | --- |
| ProcessFull / init | `_ZN12_GLOBAL__N_19Processor11ProcessFullESt17basic_string_viewIcSt11char_traitsIcEES4_` | 0x49ed0,0x49f75 | 0x49ed0,0x49f75 | 73 | inside init span |
| finish / init | `_ZN12_GLOBAL__N_1L6finishEN5tkrzw6StatusE.isra.0` | 0x49f80,0x4a2f1 | 0x49f80,0x4a2f1 | 73,74 | inside init span |
| ProcessEmpty / steady | `_ZN12_GLOBAL__N_19Processor12ProcessEmptyESt17basic_string_viewIcSt11char_traitsIcEE` | 0x7ae80,0x7af25 | 0x7ae80,0x7af25 | 122 | outside init span; inside ordered steady span |

The measured init span ends at 0x7ac1f, **within file page 122**
[0x7a000,0x7b000). ProcessEmpty's body [0x7ae80,0x7af25) is outside that
span but on the **same boundary page**. The page intersects init-tier
functions and steady-tier functions. The other two C++ rows are inside
the init span on pages 73 and 74. Thus none of the three specified C++
rows occupies a file page wholly outside the init span's page coverage.
This is static coverage, not evidence those pages became resident in
init, nor a claim that page 122 is PR-only or caused a major fault.

The retained proto-pinyin-resolved.txt includes the exact .text section
for **each of these three rows**. The as-run ordered-build record says
`0 of 3 C++ policy paths are not in this build; skipped`, meaning **zero
were skipped**, not three. The measured linked-object verification also
passed with 274 policy paths. Base reports 0 of 0 C++ rows. Placement in a
steady tier is not a missing/skipped row.

#### Other emitted shim C++ bodies

These are additional static symbols, not inferred newly resident pages.
No separate policy row names the cold clones, fail, or destructors below;
that is different from a specified policy row being skipped as missing.
Aliases at the same address are retained in the symbol-table interpretation.

| symbol | VA span | file pages | inside init | inside whole ordered span |
| --- | --- | --- | --- | --- |
| (anonymous namespace)::Processor::ProcessFull(std::basic_string_view<char, std::char_traits<char> >, std::basic_string_view<char, std::char_traits<char> >) [clone .cold] | 0x8b88c,0x8b8ff | 139 | False | False |
| (anonymous namespace)::Processor::ProcessEmpty(std::basic_string_view<char, std::char_traits<char> >) [clone .cold] | 0x8b900,0x8b973 | 139 | False | False |
| (anonymous namespace)::fail(char const*) [clone .cold] | 0x8b973,0x8b985 | 139 | False | False |
| (anonymous namespace)::Processor::~Processor() | 0x1fc3a0,0x1fc3a5 | 508 | False | False |
| (anonymous namespace)::Processor::~Processor() | 0x1fc3b0,0x1fc3be | 508 | False | False |
| (anonymous namespace)::fail(char const*) | 0x1fc420,0x1fc50e | 508 | False | False |

Cold/error bodies are on file page 139; the out-of-prefix fail/destructor
bodies are on page 508. Static symbol presence proves neither execution
nor cache population. There is no retained evidence here that those
pages became resident during cold init or incurred either extra fault.
C-linkage shim wrappers also have separate normal/cold bodies; a C linkage
name does not mean the implementation is C or that a C++ policy row for
it was skipped. Nothing about their residency is inferred from their names.

#### Functions intersecting the pages of the three specified C++ rows

The following is a complete sized-text-symbol intersection listing for
pages 73, 74 and 122, **not a list of PR-only resident pages**. Rust crate
disambiguator hashes are suppressed for readability; address spans retain
which body is meant. The exact nm names remain in the local analysis.
Only symbol-covered bytes can be attributed: padding or bytes without a
sized text symbol are not silently assigned to the nearest function.

File page 73 [0x49000,0x4a000):

- `0x48c20..0x492c2`: <pinyin::aux_matrix::SharedMatrix>::of_keys
- `0x492d0..0x4947d`: <std::fs::OpenOptions>::_open
- `0x49480..0x49606`: <std::hash::random::RandomState as core::hash::BuildHasher>::hash_one::<&alloc[24b8200b0946867]::string::String>
- `0x49610..0x49782`: <std::hash::random::RandomState as core::hash::BuildHasher>::hash_one::<&u32>
- `0x49790..0x49902`: <std::path::Path>::_join
- `0x49910..0x499d8`: <std::path::Path>::is_file
- `0x499e0..0x49a2c`: <std::path::Path>::join::<alloc[24b8200b0946867]::string::String>
- `0x49a30..0x49a73`: <std::sync::once::Once>::call_once_force::<<std::sync::once_lock::OnceLock<alloc[24b8200b0946867]::sync::Arc<oxpinyin_core::graph::SegmentGraph>>>::initialize<<std::sync::once_lock::OnceLock<alloc[24b8200b0946867]::sync::Arc<oxpinyin_core::graph::SegmentGraph>>>::get_or_init<<std::sync::once_lock::OnceLock<alloc[24b8200b0946867]::sync::Arc<oxpinyin_core::graph::SegmentGraph>>>::try_insert::{closure#0}>::{closure#0}, !>::{closure#0}>::{closure#0}
- `0x49a73..0x49ab1`: <std::sync::once_lock::OnceLock<alloc[24b8200b0946867]::sync::Arc<oxpinyin_core::graph::SegmentGraph>>>::initialize::<<std::sync::once_lock::OnceLock<alloc[24b8200b0946867]::sync::Arc<oxpinyin_core::graph::SegmentGraph>>>::get_or_init<<std::sync::once_lock::OnceLock<alloc[24b8200b0946867]::sync::Arc<oxpinyin_core::graph::SegmentGraph>>>::try_insert::{closure#0}>::{closure#0}, !>
- `0x49ac0..0x49c5c`: <std::sys::fs::unix::File>::open_c
- `0x49c60..0x49dc9`: <std::sys::sync::once::futex::Once>::call
- `0x49dd0..0x49eca`: <u32 as core::fmt::Display>::fmt
- `0x49ed0..0x49f75`: (anonymous namespace)::Processor::ProcessFull(std::basic_string_view<char, std::char_traits<char> >, std::basic_string_view<char, std::char_traits<char> >)
- `0x49f80..0x4a2f1`: (anonymous namespace)::finish(tkrzw::Status) [clone .isra.0]

File page 74 [0x4a000,0x4b000):

- `0x49f80..0x4a2f1`: (anonymous namespace)::finish(tkrzw::Status) [clone .isra.0]
- `0x4a300..0x4a341`: __rustc::__rust_alloc
- `0x4a350..0x4a3d9`: __rustc::__rust_realloc
- `0x4a3e0..0x4a49e`: __udivti3
- `0x4a4a0..0x4a565`: __umodti3
- `0x4a570..0x4a58e`: alloc[24b8200b0946867]::boxed::box_new_uninit
- `0x4a590..0x4a5bb`: alloc[24b8200b0946867]::boxed::box_new_uninit
- `0x4a5c0..0x4a740`: alloc[24b8200b0946867]::fmt::format::format_inner
- `0x4a740..0x4a900`: core::fmt::write
- `0x4a900..0x4a9bb`: core::ptr::drop_glue::<alloc[24b8200b0946867]::collections::btree::dedup_sorted_iter::DedupSortedIter<alloc[24b8200b0946867]::string::String, oxpinyin_engine::config::ConfigValue, alloc[24b8200b0946867]::vec::into_iter::IntoIter<(alloc[24b8200b0946867]::string::String, oxpinyin_engine::config::ConfigValue)>>>
- `0x4a9c0..0x4ad58`: core::ptr::drop_glue::<alloc[24b8200b0946867]::collections::btree::map::BTreeMap<u32, alloc[24b8200b0946867]::vec::Vec<oxpinyin_user::phrase::UserPronunciation>>>
- `0x4ad60..0x4b128`: core::ptr::drop_glue::<alloc[24b8200b0946867]::collections::btree::map::BTreeMap<u32, core::option::Option<oxpinyin_data::chunk_write::ChunkItem>>>

File page 122 [0x7a000,0x7b000):

- `0x79dd0..0x7a393`: std::fs::read::inner
- `0x7a3a0..0x7a499`: std::io::default_read_to_end::small_probe_read::<std::fs::File>
- `0x7a4a0..0x7a4a1`: std::sys::args::unix::imp::ARGV_INIT_ARRAY::init_wrapper
- `0x7a4b0..0x7a6cd`: std::sys::fs::metadata
- `0x7a6d0..0x7a998`: std::sys::fs::unix::try_statx
- `0x7a9a0..0x7ac1f`: std::sys::random::linux::hashmap_random_keys
- `0x7ac20..0x7ac70`: <alloc[24b8200b0946867]::sync::Arc<oxpinyin_core::graph::SegmentGraph>>::drop_slow
- `0x7ac70..0x7adde`: <oxpinyin_engine::session::buffer::InputBuffer>::clear
- `0x7ade0..0x7ae7b`: <oxpinyin_user::lookup::UserLookup>::phrase_prefix_exists
- `0x7ae80..0x7af25`: (anonymous namespace)::Processor::ProcessEmpty(std::basic_string_view<char, std::char_traits<char> >)
- `0x7af30..0x7b00d`: core::slice::sort::shared::pivot::median3_rec::<oxpinyin_engine::candidate::Candidate, <[oxpinyin_engine::candidate::Candidate]>::sort_by_key<u32, oxpinyin_engine::session::order_window_tail::{closure#0}>::{closure#0}>

### Result and supported proposal

**Demonstrated:** no retained cold per-page set is available for the
requested set subtraction. Both init C++ rows are correctly inside the
init span. ProcessEmpty is a live, resolved steady row outside the init
span but shares its final page. All three specified C++ rows resolved;
none was skipped. Some other emitted cold/error/destructor bodies are
outside the whole ordered span, as the static tables show.

**Not demonstrated:** which facade pages were PR-only M0-to-init-end
resident additions, whether either extra major fault belonged to the
facade at all, which function incurred one, or whether any emitted
out-of-prefix C++ body was read/executed during this cold init. A mincore
bitmap alone would still not make a fault-to-page/function attribution.
No missing-row or C++ placement explanation for the two events is proved.

**Supported proposal:** extend the existing diagnostic controller, only
after explicit authorization, with a one-pass path that reuses the exact
retained worker/fixtures/artifacts, applies the same private-facade-only
cold preparation, and preserves the existing mincore page-index runs at
M0 and the worker's init-only I1 checkpoint. That would enable the page
set and symbol-intersection question without inventing a new worker
capability. If M1 is also retained, label its later first-candidate scope
separately. Fault IP/address evidence would be a further, separately
authorized requirement to identify the two major-fault events themselves.
No such controller extension or fault capture was implemented here.

There is **no supported shim/policy fix yet**. In particular, adding
already-resolved C++ rows again, relabeling ProcessEmpty as init, or moving
cold/error functions on the assumption they caused two faults is not
justified by these records. The missing page evidence and unavailable
same-setup single-pass entry point are the blocker. STOP; no PR or review.

### Read-only tools and commands

```sh
export R=/home/sheng/Documents/Codex/2026-10-24/task-3
export PY=/home/sheng/.local/share/uv/python/cpython-3.15.0-linux-x86_64-gnu/bin/python3.15
cd "$R/worktree"
python3 "$R/page-diagnosis-1125/records.py"
$PY -X lazy_imports=normal "$R/page-diagnosis-1125/static.py"
readelf -lW "$R/campaign-finish/base-stage/lib/libpinyin.so.15.0.0"
readelf -lW "$R/campaign-finish/proto-stage/lib/libpinyin.so.15.0.0"
readelf -SW "$R/campaign-finish/proto-stage/lib/libpinyin.so.15.0.0"
nm -S -n --defined-only -C "$R/campaign-finish/proto-stage/lib/libpinyin.so.15.0.0"
# Static verifier used the same policies and sized nm records as the as-run resolver.
# Retained inputs: campaign-finish/records/{proto,base}-pinyin-resolved.txt,
# ordered-build.txt, measurement-campaign.txt and measurement-originals/.
git diff --check
```

The local scripts only inspect existing records/ELFs and existing policy
verification code. They do not launch subject workers, modify policies,
regenerate order, or constitute a new measurement driver. Derived JSON
and all captures remain local. Only this findings update is committed.


## Retained cold-init diagnosis; STOP remains

Recorded 2026-10-24T10:51:52.840413+00:00. Diagnosis only, on findings
checkpoint 41816464a3d360df023c343a87074d05d40d1871. Base remains
`d7788072d3b8078aaa500cb5fcb98e0542fe7278`; the exact measured prototype
and binaries remain those of the completed campaign at `c4cc6d0c`, with
shipping source identical to `6e50b77e`. No source, policy, build, tuning,
regeneration, rebase, new subject execution or PR was performed.

### What survived and how pairs were formed

All requested fields survived in the original worker stdout. For each
pinyin profile there are 20 measured cold-library rounds plus two warmup
rounds, each with base, prototype, same-bytes prototype control and pin.
The data below include the warmups explicitly, while summary intervals use
only original measured rounds 0-19. No sample is silently replaced.

Pairs use matching profile and original round number. Their execution
order comes from the retained command serials, not sorted timing values.
The four-arm rotation is B/P/C/N, P/C/N/B, C/N/B/P, N/B/P/C for rounds
0,1,2,3 and repeats. B=base, P=prototype, C=byte-identical prototype
control, N=pin. Warmups -2 and -1 retain their own actual order. The pin's
per-sample values are not part of this requested base/prototype diagnosis.
Every selected stdout contains exactly one teardown sample with all eight
fields in both `load_ns` and `init_ns`; nothing below substitutes zero for
an absent field. The original cold setup was a fresh ext4 facade copy,
fsync, POSIX_FADV_DONTNEED and zero-resident mincore verification, CPU 2,
with the certified tkrzw resolution checked before each launch.

`td_now` in tools/bisection/teardown.h reads wall with CLOCK_MONOTONIC_RAW,
CPU with CLOCK_PROCESS_CPUTIME_ID, and user/sys/faults/switches with
getrusage(RUSAGE_SELF). User/sys are microsecond-valued kernel accounting
converted to ns; short-interval splits need not equal the process CPU
clock difference. `load_ns` includes RTLD_NOW dlopen **and** td_resolve's
symbol lookups. Init brackets only td_init. Load-plus-init is the sum of
the two recorded scopes **within each worker sample before any pairing or
summary**, for every field. It is not the sum of independently summarized
medians or an additional contiguous-scope timer.

### Paired summaries from the original 20 rounds

Times are ns; faults and switches are event counts. Each interval is the
existing teardown.py paired-difference bootstrap 95% interval. These are
component diagnostics, not newly gated performance verdicts. C-P columns
show the same-byte control's component variability; CPU/user/sys and the
wall-minus-CPU residual must not be treated as independently validated
causal partitions. The residual is computed within each sample; process
CPU includes all its threads, so the residual is not an exact blocked-time
measurement or an I/O-latency measurement.

| profile / scope / field | B median | P median | C median | paired P-B [95%] | paired C-P [95%] |
| --- | ---: | ---: | ---: | ---: | ---: |
| phrases/init_ns/wall_ns | 4702780 | 4990178 | 5134082.5 | 330290.5 [235780.5, 414841] | 114776.5 [-60454.5, 225615] |
| phrases/init_ns/cpu_ns | 3386059.5 | 3450473 | 3488388 | 53296.5 [-116817.5, 213499] | 108986 [-119926, 204199.5] |
| phrases/init_ns/user_ns | 1868500 | 1855000 | 2086500 | -63500 [-880000, 599500] | 491500 [47000, 1073500] |
| phrases/init_ns/sys_ns | 1868000 | 1725500 | 1216500 | 359000 [-130000, 543500] | -460500 [-824500, -58500] |
| phrases/init_ns/minflt | 521 | 516 | 516 | -4 [-5, -4] | 0 [0, 0] |
| phrases/init_ns/majflt | 6 | 8 | 8 | 2 [2, 2] | 0 [0, 0] |
| phrases/init_ns/nvcsw | 6 | 8 | 8 | 2 [2, 2] | 0 [0, 0] |
| phrases/init_ns/nivcsw | 0 | 0 | 0 | 0 [0, 0] | 0 [0, 0] |
| phrases/init_ns/wall_minus_cpu_ns | 1289488.5 | 1587924 | 1578486 | 270563 [253036.5, 336467] | -18410 [-48365.5, 13545.5] |
| phrases/load_ns/wall_ns | 3357840 | 3273030.5 | 3217742 | -54054.5 [-138838, 242828.5] | -114574 [-383163.5, 99317] |
| phrases/load_ns/cpu_ns | 2459866.5 | 2324110.5 | 2365352.5 | -122001 [-192024.5, 71783] | 1335 [-180806, 117002] |
| phrases/load_ns/user_ns | 1640500 | 1613000 | 1514500 | -207500 [-428500, 382500] | 158000 [-284287.5, 305000] |
| phrases/load_ns/sys_ns | 886000 | 953000 | 917000 | 126500 [-600000, 423500] | 59500 [-495500, 298000] |
| phrases/load_ns/minflt | 146 | 146 | 147 | 0 [-1, 1] | 0 [0, 1] |
| phrases/load_ns/majflt | 3 | 3 | 3 | 0 [0, 0] | 0 [0, 0] |
| phrases/load_ns/nvcsw | 4 | 4 | 4 | 0 [0, 0] | 0 [0, 0] |
| phrases/load_ns/nivcsw | 0 | 0 | 0 | 0 [0, 0] | 0 [0, 0] |
| phrases/load_ns/wall_minus_cpu_ns | 904882 | 880124 | 850184 | 42762.5 [-52410.5, 131240] | -51378 [-122187.5, 2499.5] |
| phrases/load_plus_init/wall_ns | 8126379 | 8411914.5 | 8321985 | 321373 [105431, 998989.5] | -9016.5 [-594795, 327154.5] |
| phrases/load_plus_init/cpu_ns | 5882719.5 | 5782042.5 | 5844818 | -110337.5 [-426630, 306967] | 60764.5 [-296438, 366478] |
| phrases/load_plus_init/user_ns | 3527000 | 3348000 | 3651000 | -193500 [-909500, 1170500] | 465500 [-585500, 929000] |
| phrases/load_plus_init/sys_ns | 2636500 | 2704500 | 2226000 | 276000 [-577500, 879000] | -545000 [-1236000, 557500] |
| phrases/load_plus_init/minflt | 667.5 | 663 | 663 | -5 [-5, -4] | 0 [0, 1] |
| phrases/load_plus_init/majflt | 9 | 11 | 11 | 2 [2, 2] | 0 [0, 0] |
| phrases/load_plus_init/nvcsw | 10 | 12 | 12 | 2 [2, 2] | 0 [0, 0] |
| phrases/load_plus_init/nivcsw | 0 | 0 | 0 | 0 [0, 0] | 0 [0, 0] |
| phrases/load_plus_init/wall_minus_cpu_ns | 2178002 | 2506311 | 2406852.5 | 316705 [249854.5, 410262.5] | -65467.5 [-133856, 26060] |
| journals/init_ns/wall_ns | 4717971.5 | 5104584 | 5105308.5 | 371269 [56364, 533411.5] | -54843 [-213687, 77870.5] |
| journals/init_ns/cpu_ns | 3462819.5 | 3609255 | 3470530 | 161503 [-132117, 275774] | -128484.5 [-225866.7625, 25973] |
| journals/init_ns/user_ns | 1160500 | 1906000 | 1551000 | 363500 [-407500, 1486500] | -371000 [-1308500, -1000] |
| journals/init_ns/sys_ns | 2181000 | 1720000 | 1932500 | -19000 [-1228000, 380500] | 213500 [-37000, 856000] |
| journals/init_ns/minflt | 528.5 | 524 | 524 | -5 [-6, -3.5] | 0 [0, 1] |
| journals/init_ns/majflt | 6 | 8 | 8 | 2 [2, 2] | 0 [0, 0] |
| journals/init_ns/nvcsw | 6 | 8 | 8 | 2 [2, 2] | 0 [0, 0] |
| journals/init_ns/nivcsw | 0 | 0 | 0 | 0 [0, 0] | 0 [0, 0] |
| journals/init_ns/wall_minus_cpu_ns | 1287749.5 | 1535977.5 | 1607990.5 | 198411 [147318.5, 297146] | 56525.5 [5960, 132123] |
| journals/load_ns/wall_ns | 3394700 | 3251392 | 3195124.5 | -157718 [-366455, -38673] | -37725 [-191740.5, 140843] |
| journals/load_ns/cpu_ns | 2457862.5 | 2376893 | 2347842 | -139148.5 [-216151.5, -82862.825] | 9280 [-236487.5, 74861.5] |
| journals/load_ns/user_ns | 1382000 | 1341500 | 1293500 | -171000 [-271500, 154500] | -243500 [-483000, 416500] |
| journals/load_ns/sys_ns | 1048000 | 1006000 | 970500 | 16000 [-420500, 424500] | 74500 [-401500, 270000] |
| journals/load_ns/minflt | 146 | 147 | 147 | 0 [-1, 1.5] | 0 [-0.5, 0.5] |
| journals/load_ns/majflt | 3 | 3 | 3 | 0 [0, 0] | 0 [0, 0] |
| journals/load_ns/nvcsw | 4 | 4 | 4 | 0 [0, 0] | 0 [0, 0] |
| journals/load_ns/nivcsw | 0 | 0 | 0 | 0 [0, 0] | 0 [0, 0] |
| journals/load_ns/wall_minus_cpu_ns | 896580 | 880599 | 889987.5 | -17104.5 [-85611, 53492] | 7716.5 [-52793.5, 71264.5] |
| journals/load_plus_init/wall_ns | 8463188 | 8443236 | 8397354 | 67311.5 [-256304.5, 449887.5] | 47087 [-493602.5, 215207.5] |
| journals/load_plus_init/cpu_ns | 5929994 | 5965539 | 5848581 | 19600.5 [-267196.5, 271317] | -120433 [-468448, 197325] |
| journals/load_plus_init/user_ns | 3070500 | 3680000 | 3026000 | -181000 [-649500, 1877000] | -571500 [-991000, 154500] |
| journals/load_plus_init/sys_ns | 3151500 | 2698000 | 3092500 | -439000 [-1299500, 597000] | 524500 [-530000, 1100000] |
| journals/load_plus_init/minflt | 675 | 670 | 671 | -5 [-6, -3] | 0 [-1, 1.5] |
| journals/load_plus_init/majflt | 9 | 11 | 11 | 2 [2, 2] | 0 [0, 0] |
| journals/load_plus_init/nvcsw | 10 | 12 | 12 | 2 [2, 2] | 0 [0, 0] |
| journals/load_plus_init/nivcsw | 0 | 0 | 0 | 0 [0, 0] | 0 [0, 0] |
| journals/load_plus_init/wall_minus_cpu_ns | 2228723.5 | 2388359.5 | 2468459 | 138433 [54577.5, 270059.5] | 116228 [-10488, 205040.5] |

For phrases, init wall increases by 330,290.5 ns [235,780.5, 414,841],
while init CPU changes by 53,296.5 ns [-116,817.5, 213,499]. Its
wall-minus-CPU residual increases by 270,563 ns [253,036.5, 336,467].
For journals those paired differences are respectively 371,269 ns
[56,364, 533,411.5], 161,503 ns [-132,117, 275,774], and 198,411 ns
[147,318.5, 297,146]. Neither CPU interval establishes a CPU increase;
user/sys splits are also inconclusive. The two extra init major faults
and two extra voluntary switches occur in **all 20 pairs in each cell**;
C/P major-fault and voluntary-switch counts match. Minor faults decrease rather than
increase. Involuntary-switch paired median differences are zero.

The load boundary changes the interpretation: phrases load wall P-B is
-54,054.5 ns [-138,838, 242,828.5], and its within-sample load-plus-init
wall remains +321,373 ns [105,431, 998,989.5]. Journals load wall is
-157,718 ns [-366,455, -38,673]; its combined wall difference is
+67,311.5 ns [-256,304.5, 449,887.5], which is inconclusive. Combined
CPU differences are inconclusive in both cells. The two additional major
faults and voluntary switches remain in both combined scopes; they do not
disappear by moving the boundary. These observations allow some boundary
redistribution, especially for journals, but do not demonstrate that all
of the init slowdown was displaced from dlopen. The original init STOP
is not revoked by the wider journal combined interval.

### Full resolved loaded graphs, with provenance limits

The complete disk-backed ELF node sets and NEEDED edges below were
reconstructed from the retained **memory-worker** M0/M1 maps and readelf
on the fixed files. There are 88 matching snapshots per arm: 22 rounds
(including warmups), two profiles, two checkpoints. Every NEEDED name
resolves to an observed mapped SONAME. The set is unchanged between M0
(after dlopen/resolve, before init) and M1 (after first candidate) across
all those snapshots. Worker PT_INTERP `/lib64/ld-linux-x86-64.so.2`
resolves to the listed loader, and the worker's explicit dlopen edge goes
to its arm's facade. No ldd launch or new subject execution was used.

These memory workers use warmed facade copies. The retention wrapper's
`cold=true` metadata is the last timing-loop variable, **not** the memory
worker's cache state: measure-function-order.py calls prepare_cache with
False immediately before memory_child. Cold **timing** workers' own maps
and loader-event traces were not retained. Thus this is the full observed
resolved graph of the companion workers on the same fixed artifacts,
plus their ELF dependency closure, not a claimed cold-time PID link-map
capture. Temporary facade mappings are identified by the recorded inode
and mapped back to the byte-verified arm artifact. Other mapped file
inodes match the current resolved files. Hashes below are diagnosis-time
hashes of those files; they were not all recorded during timing. The
facades and certified runtime additionally match the completed campaign's
recorded hashes. Transient loads/unloads wholly between checkpoints
cannot be excluded from map snapshots alone.

There are eleven disk-backed ELF nodes per arm: worker, facade, and nine
DSOs. This table lists every resolved file and every outgoing NEEDED edge,
including transitive dependencies. Edge labels resolve to the SONAME/file
rows below. The only difference in these edges is the facade's additional
direct libstdc++.so.6 edge in the prototype.

| node / arm | resolved file | outgoing NEEDED SONAMEs | SHA256 at diagnosis |
| --- | --- | --- | --- |
| facade/base | /home/sheng/Documents/Codex/2026-10-24/task-3/campaign-finish/base-stage/lib/libpinyin.so.15.0.0 | libglib-2.0.so.0, libtkrzw.so.1, libc.so.6, ld-linux-x86-64.so.2, libgcc_s.so.1 | 92a769b8d4c57a61c6190e7e50699deb44f022d87228cb5f96ac00dd16b0031e |
| facade/pr | /home/sheng/Documents/Codex/2026-10-24/task-3/campaign-finish/proto-stage/lib/libpinyin.so.15.0.0 | libglib-2.0.so.0, libtkrzw.so.1, libstdc++.so.6, libc.so.6, ld-linux-x86-64.so.2, libgcc_s.so.1 | d040918c1da19056ef4e8646cb82be7e7151884766025610e83bad08ba5d2837 |
| libtkrzw.so.1 / both | /home/sheng/.cache/oxpinyin/tkrzw/lib/libtkrzw.so.1.77.0 | libstdc++.so.6, libm.so.6, libc.so.6, ld-linux-x86-64.so.2, libgcc_s.so.1 | 36c6aa4d77edb3bd6a628f02fc456f0a67096bdd451c04999141d7144f144d0e |
| worker / both | /home/sheng/Documents/Codex/2026-10-24/task-3/campaign-finish/bisect | libc.so.6 | c9d4061aff85fc8d220e6bec187f0c4ecffd636edfc56c5444057cb0c36c21d4 |
| ld-linux-x86-64.so.2 / both | /usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2 | none | 3b56a07e51593e29b85e2fa8057f9e4bf8bd3022ff9b0f75e3c448fa69877016 |
| libatomic.so.1 / both | /usr/lib/x86_64-linux-gnu/libatomic.so.1.2.0 | libc.so.6 | 59f4b817f28044cdf6463136988b092b1291bff9e74b7859cb5604fe325f519b |
| libc.so.6 / both | /usr/lib/x86_64-linux-gnu/libc.so.6 | ld-linux-x86-64.so.2 | 4f94bc931ad2a03c82ad610dd734eed17f627197068898d04a71862d5ad45207 |
| libgcc_s.so.1 / both | /usr/lib/x86_64-linux-gnu/libgcc_s.so.1 | libc.so.6, ld-linux-x86-64.so.2 | a1e90d180188096c5de561b8d1358ed345f096872f3a8411024997b80b5e75a0 |
| libglib-2.0.so.0 / both | /usr/lib/x86_64-linux-gnu/libglib-2.0.so.0.9000.0 | libatomic.so.1, libm.so.6, libpcre2-8.so.0, libc.so.6 | 9990987ffc88baf5bf5a24c0e1d39fe47834556c3a9c058917362e378f2078a3 |
| libm.so.6 / both | /usr/lib/x86_64-linux-gnu/libm.so.6 | libc.so.6, ld-linux-x86-64.so.2 | 366e3e5cff8674aef178a08cf8385b734ed8e7d35d8fc0fa66dbef8332bf3746 |
| libpcre2-8.so.0 / both | /usr/lib/x86_64-linux-gnu/libpcre2-8.so.0.14.0 | libc.so.6 | 4a6b6b2078685b074869d75cb068664d7da6b3036df7bca0442b1bbbd4ac7e65 |
| libstdc++.so.6 / both | /usr/lib/x86_64-linux-gnu/libstdc++.so.6.0.36 | libm.so.6, libc.so.6, ld-linux-x86-64.so.2, libgcc_s.so.1 | d6abcc37530c3555131349ba47f6deda7fc4318ff5c236ef7f0211b91217aac0 |

The recorded special mappings also include `[vdso]`, `[vvar]`,
`[vvar_vclock]`, `[vsyscall]`, `[heap]` and `[stack]`. They are not
filesystem NEEDED resolutions. The kernel-provided vDSO's ELF bytes and
auxiliary-vector metadata were not retained; its mapped presence is
recorded, not assigned a fabricated file/hash or dependency edge.
`/etc/ld.so.cache` is a mapped resolver cache, not a DSO. M1's system/user
DB files are mappings of data, not loaded ELF dependency nodes.

Only after establishing that graph: libstdc++ is already present at M0
for **both** arms, transitively via certified libtkrzw in base and both
transitively and directly in prototype. A newly introduced loaded
libstdc++ object is therefore not supported as the explanation. Equal DSO
sets do not imply equal bindings, executed pages, page-fault locations,
constructor paths, or resident-page histories. No claim about any such
libstdc++ mechanism is made from the graph.

### Every retained paired sample

All time values below are integer ns; event values are counts. Each
metric cell is `B→P (P-B; C-P)`. The control value is recoverable as
P+(C-P). Actual zeros are printed as zeros. Round labels W-2/W-1 are the
original warmups and do not enter the 20-round summaries. `records B/P/C`
are serials of the original command/worker stdout records, not new runs.
The order column includes N only to retain the original four-arm order.
All load-plus-init entries are computed from each arm's own recorded
load and init fields before these comparisons.

#### phrases / init_ns

| round; order; records B/P/C | wall_ns | cpu_ns | user_ns | sys_ns | minflt | majflt | nvcsw | nivcsw |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| W-2; C>N>B>P; 15/21/3 | 4514208→4937760 (+423552; -99197) | 3237330→3278905 (+41575; -6466) | 2315000→2550000 (+235000; +11000) | 922000→731000 (-191000; -17000) | 521→517 (-4; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| W-1; N>B>P>C; 33/39/45 | 4683883→4908229 (+224346; +190909) | 3308826→3402471 (+93645; +78882) | 2064000→451000 (-1613000; +864000) | 1245000→2952000 (+1707000; -784000) | 522→516 (-6; +1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 0; B>P>C>N; 51/57/63 | 5525711→5804362 (+278651; -29972) | 3335256→3518596 (+183340; -267024) | 1449000→27000 (-1422000; +1354000) | 1888000→3494000 (+1606000; -1623000) | 522→516 (-6; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 1; P>C>N>B; 93/75/81 | 4350987→4915401 (+564414; +103977) | 3141475→3444648 (+303173; -1586) | 1193000→1185000 (-8000; +618000) | 1950000→2261000 (+311000; -620000) | 520→517 (-3; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 2; C>N>B>P; 111/117/99 | 4866419→4674955 (-191464; +225615) | 3478324→3074280 (-404044; +175723) | 70000→3075000 (+3005000; -1004000) | 3408000→0 (-3408000; +1179000) | 520→516 (-4; +1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 3; N>B>P>C; 129/135/141 | 4576129→4995319 (+419190; -284169) | 3236490→3369030 (+132540; -145150) | 2131000→1818000 (-313000; -708000) | 1108000→1553000 (+445000; +563000) | 521→516 (-5; +1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 4; B>P>C>N; 147/153/159 | 5532717→4863638 (-669079; +456387) | 4239507→3326877 (-912630; +500735) | 3123000→2184000 (-939000; +1042000) | 1118000→1144000 (+26000; -542000) | 520→516 (-4; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 5; P>C>N>B; 189/171/177 | 4530459→4931555 (+401096; +828579) | 3244692→3384530 (+139838; +866412) | 2135000→2016000 (-119000; +1443000) | 1110000→1371000 (+261000; -578000) | 520→516 (-4; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 6; C>N>B>P; 207/213/195 | 4874243→5096161 (+221918; -90937) | 3613162→3474906 (-138256; -94702) | 2000→2277000 (+2275000; +1105000) | 3612000→1198000 (-2414000; -1198000) | 520→516 (-4; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 7; N>B>P>C; 225/231/237 | 4802412→5207065 (+404653; +79930) | 3473054→3503888 (+30834; +95379) | 2226000→1686000 (-540000; +201000) | 1248000→1818000 (+570000; -105000) | 521→516 (-5; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 8; B>P>C>N; 243/249/255 | 4595265→5772781 (+1177516; -1007540) | 3331427→4241743 (+910316; -986169) | 823000→1217000 (+394000; +1626000) | 2510000→3027000 (+517000; -2614000) | 520→516 (-4; +1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→1 (+1; -1) |
| 9; P>C>N>B; 285/267/273 | 5476254→4833973 (-642281; +381228) | 4216973→3582272 (-634701; +94697) | 2110000→1234000 (-876000; +2266000) | 2110000→2350000 (+240000; -2171000) | 520→516 (-4; +1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→2 (+2; -2) |
| 10; C>N>B>P; 303/309/291 | 4418866→4764569 (+345703; +125576) | 3108068→3183827 (+75759; +135267) | 1088000→2090000 (+1002000; -965000) | 2022000→1095000 (-927000; +1102000) | 521→517 (-4; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 11; N>B>P>C; 321/327/333 | 4510697→6061794 (+1551097; -957909) | 3202421→4382003 (+1179582; -877739) | 1111000→1525000 (+414000; -13000) | 2093000→2860000 (+767000; -866000) | 521→517 (-4; -1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 12; B>P>C>N; 339/345/351 | 5090734→5085023 (-5711; +197812) | 3789030→3456298 (-332732; +241647) | 1895000→80000 (-1815000; +2694000) | 1895000→3378000 (+1483000; -2454000) | 522→516 (-6; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 13; P>C>N>B; 381/363/369 | 4730496→5594266 (+863770; -807130) | 3474736→4093674 (+618938; -717192) | 2086000→2991000 (+905000; -638000) | 1390000→1104000 (-286000; -79000) | 521→517 (-4; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→1 (+1; -1) |
| 14; C>N>B>P; 399/405/387 | 4653213→4902856 (+249643; +150312) | 3371938→3333115 (-38823; +139397) | 1151000→2193000 (+1042000; +481000) | 2223000→1142000 (-1081000; -343000) | 521→516 (-5; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 1→0 (-1; +0) |
| 15; N>B>P>C; 417/423/429 | 4300942→5103477 (+802535; +60803) | 3066935→3523557 (+456622; +122593) | 1842000→1892000 (+50000; +502000) | 1226000→1633000 (+407000; -379000) | 521→517 (-4; -1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 16; B>P>C>N; 435/441/447 | 5022146→4911138 (-111008; +468773) | 3688330→3306140 (-382190; +527671) | 1841000→957000 (-884000; +509000) | 1848000→2352000 (+504000; +17000) | 520→516 (-4; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +1) |
| 17; P>C>N>B; 477/459/465 | 4982300→5392792 (+410492; -521903) | 3627611→3871269 (+243658; -677499) | 3148000→1995000 (-1153000; +107000) | 482000→1877000 (+1395000; -783000) | 522→516 (-6; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 18; C>N>B>P; 495/501/483 | 4618368→4933246 (+314878; +274944) | 3371662→3338140 (-33522; +258768) | 2210000→2995000 (+785000; -1795000) | 1164000→344000 (-820000; +2055000) | 521→517 (-4; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 19; N>B>P>C; 513/519/525 | 4675064→4985037 (+309973; +180731) | 3400181→3304802 (-95379; +232676) | 3284000→1084000 (-2200000; +271000) | 118000→2222000 (+2104000; -38000) | 521→516 (-5; +1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |

#### phrases / load_ns

| round; order; records B/P/C | wall_ns | cpu_ns | user_ns | sys_ns | minflt | majflt | nvcsw | nivcsw |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| W-2; C>N>B>P; 15/21/3 | 12044994→3357756 (-8687238; +53574) | 2569309→2265002 (-304307; +101684) | 1391000→739000 (-652000; +1315000) | 1177000→1526000 (+349000; -1214000) | 147→147 (+0; +0) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +1) |
| W-1; N>B>P>C; 33/39/45 | 3379120→3290387 (-88733; -22063) | 2408692→2447353 (+38661; +22554) | 795000→2048000 (+1253000; +45000) | 1613000→399000 (-1214000; -23000) | 147→148 (+1; +0) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 0; B>P>C>N; 51/57/63 | 3294301→3231521 (-62780; -176421) | 2303039→2262425 (-40614; -50742) | 1007000→819000 (-188000; +474000) | 1296000→1443000 (+147000; -525000) | 146→147 (+1; +0) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 1; P>C>N>B; 93/75/81 | 3019948→3119874 (+99926; +168166) | 2265569→2313340 (+47771; +53412) | 1511000→1157000 (-354000; -136000) | 754000→1157000 (+403000; +189000) | 146→147 (+1; +1) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 2; C>N>B>P; 111/117/99 | 3266185→2958748 (-307437; +168907) | 2283352→2121389 (-161963; +113245) | 994000→2121000 (+1127000; +114000) | 1289000→0 (-1289000; +0) | 147→145 (-2; +2) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 3; N>B>P>C; 129/135/141 | 3170869→3034130 (-136739; +228353) | 2378941→2161575 (-217366; +179402) | 1961000→1303000 (-658000; +300000) | 418000→858000 (+440000; -120000) | 146→146 (+0; +0) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 4; B>P>C>N; 147/153/159 | 3287541→3171149 (-116392; +19432) | 2554962→2286956 (-268006; +93187) | 1721000→1973000 (+252000; -783000) | 835000→314000 (-521000; +876000) | 146→147 (+1; -2) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 5; P>C>N>B; 189/171/177 | 3242093→3943194 (+701101; -330199) | 2381854→2972089 (+590235; -181204) | 1961000→1530000 (-431000; +284000) | 421000→1442000 (+1021000; -466000) | 149→147 (-2; -2) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 6; C>N>B>P; 207/213/195 | 3712041→3280817 (-431224; -140622) | 2779358→2388294 (-391064; -180408) | 1841000→1614000 (-227000; -380000) | 940000→776000 (-164000; +198000) | 146→146 (+0; +0) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 7; N>B>P>C; 225/231/237 | 3380386→3265244 (-115142; +71451) | 2464232→2297549 (-166683; +190147) | 201000→2155000 (+1954000; -1081000) | 2264000→141000 (-2123000; +1273000) | 147→147 (+0; -1) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 8; B>P>C>N; 243/249/255 | 3454449→3840180 (+385731; -624733) | 2634252→2985999 (+351747; -654673) | 885000→1982000 (+1097000; -1174000) | 1749000→1004000 (-745000; +519000) | 146→145 (-1; +2) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 9; P>C>N>B; 285/267/273 | 3392283→12157166 (+8764883; -9169860) | 2561217→2657012 (+95795; -480810) | 2101000→1756000 (-345000; +276000) | 459000→902000 (+443000; -758000) | 148→145 (-3; +1) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→1 (+1; -1) |
| 10; C>N>B>P; 303/309/291 | 3183843→2940296 (-243547; +295899) | 2313522→2155134 (-158388; +320840) | 2313000→1887000 (-426000; +202000) | 0→267000 (+267000; +119000) | 147→148 (+1; +0) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 11; N>B>P>C; 321/327/333 | 3335294→3811429 (+476135; -497836) | 2405157→3024009 (+618852; -631411) | 1398000→1911000 (+513000; -884000) | 1006000→1112000 (+106000; +253000) | 147→146 (-1; +1) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 12; B>P>C>N; 339/345/351 | 3689281→6502669 (+2813388; -3282632) | 2465011→2195310 (-269701; +120759) | 2056000→965000 (-1091000; +1215000) | 408000→1230000 (+822000; -1094000) | 146→148 (+2; -3) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 13; P>C>N>B; 381/363/369 | 3451077→9194131 (+5743054; -6078283) | 2479033→2950676 (+471643; -707795) | 1670000→1016000 (-654000; +310000) | 809000→1935000 (+1126000; -1019000) | 146→146 (+0; +1) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 14; C>N>B>P; 399/405/387 | 3466941→3326004 (-140937; -137778) | 2573331→2449949 (-123382; -85996) | 512000→1067000 (+555000; +945000) | 2062000→1383000 (-679000; -1031000) | 147→147 (+0; +0) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 15; N>B>P>C; 417/423/429 | 3250127→3343374 (+93247; -91370) | 2326427→2569440 (+243013; -54465) | 789000→2566000 (+1777000; -1309000) | 1538000→3000 (-1535000; +1254000) | 146→146 (+0; +1) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 16; B>P>C>N; 435/441/447 | 3565944→3070431 (-495513; +481770) | 2749405→2075958 (-673447; +624874) | 1812000→1831000 (+19000; -30000) | 937000→244000 (-693000; +655000) | 147→146 (-1; +0) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 17; P>C>N>B; 477/459/465 | 3585224→3539895 (-45329; -436128) | 2655224→2674556 (+19332; -385154) | 887000→1086000 (+199000; +841000) | 1768000→1588000 (-180000; -1227000) | 146→147 (+1; -1) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 18; C>N>B>P; 495/501/483 | 3216325→3254682 (+38357; +80834) | 2455501→2334881 (-120620; +111782) | 2422000→1612000 (-810000; -186000) | 33000→722000 (+689000; +298000) | 147→146 (-1; +2) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 19; N>B>P>C; 513/519/525 | 3393653→3052984 (-340669; +117800) | 2427972→2199411 (-228561; +199422) | 1611000→975000 (-636000; +639000) | 817000→1224000 (+407000; -439000) | 145→146 (+1; +1) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |

#### phrases / load_plus_init

| round; order; records B/P/C | wall_ns | cpu_ns | user_ns | sys_ns | minflt | majflt | nvcsw | nivcsw |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| W-2; C>N>B>P; 15/21/3 | 16559202→8295516 (-8263686; -45623) | 5806639→5543907 (-262732; +95218) | 3706000→3289000 (-417000; +1326000) | 2099000→2257000 (+158000; -1231000) | 668→664 (-4; +0) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +1) |
| W-1; N>B>P>C; 33/39/45 | 8063003→8198616 (+135613; +168846) | 5717518→5849824 (+132306; +101436) | 2859000→2499000 (-360000; +909000) | 2858000→3351000 (+493000; -807000) | 669→664 (-5; +1) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 0; B>P>C>N; 51/57/63 | 8820012→9035883 (+215871; -206393) | 5638295→5781021 (+142726; -317766) | 2456000→846000 (-1610000; +1828000) | 3184000→4937000 (+1753000; -2148000) | 668→663 (-5; +0) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 1; P>C>N>B; 93/75/81 | 7370935→8035275 (+664340; +272143) | 5407044→5757988 (+350944; +51826) | 2704000→2342000 (-362000; +482000) | 2704000→3418000 (+714000; -431000) | 666→664 (-2; +1) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 2; C>N>B>P; 111/117/99 | 8132604→7633703 (-498901; +394522) | 5761676→5195669 (-566007; +288968) | 1064000→5196000 (+4132000; -890000) | 4697000→0 (-4697000; +1179000) | 667→661 (-6; +3) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 3; N>B>P>C; 129/135/141 | 7746998→8029449 (+282451; -55816) | 5615431→5530605 (-84826; +34252) | 4092000→3121000 (-971000; -408000) | 1526000→2411000 (+885000; +443000) | 667→662 (-5; +1) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 4; B>P>C>N; 147/153/159 | 8820258→8034787 (-785471; +475819) | 6794469→5613833 (-1180636; +593922) | 4844000→4157000 (-687000; +259000) | 1953000→1458000 (-495000; +334000) | 666→663 (-3; -2) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 5; P>C>N>B; 189/171/177 | 7772552→8874749 (+1102197; +498380) | 5626546→6356619 (+730073; +685208) | 4096000→3546000 (-550000; +1727000) | 1531000→2813000 (+1282000; -1044000) | 669→663 (-6; -2) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 6; C>N>B>P; 207/213/195 | 8586284→8376978 (-209306; -231559) | 6392520→5863200 (-529320; -275110) | 1843000→3891000 (+2048000; +725000) | 4552000→1974000 (-2578000; -1000000) | 666→662 (-4; +0) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 7; N>B>P>C; 225/231/237 | 8182798→8472309 (+289511; +151381) | 5937286→5801437 (-135849; +285526) | 2427000→3841000 (+1414000; -880000) | 3512000→1959000 (-1553000; +1168000) | 668→663 (-5; -1) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 8; B>P>C>N; 243/249/255 | 8049714→9612961 (+1563247; -1632273) | 5965679→7227742 (+1262063; -1640842) | 1708000→3199000 (+1491000; +452000) | 4259000→4031000 (-228000; -2095000) | 666→661 (-5; +3) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→1 (+1; -1) |
| 9; P>C>N>B; 285/267/273 | 8868537→16991139 (+8122602; -8788632) | 6778190→6239284 (-538906; -386113) | 4211000→2990000 (-1221000; +2542000) | 2569000→3252000 (+683000; -2929000) | 668→661 (-7; +2) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→3 (+3; -3) |
| 10; C>N>B>P; 303/309/291 | 7602709→7704865 (+102156; +421475) | 5421590→5338961 (-82629; +456107) | 3401000→3977000 (+576000; -763000) | 2022000→1362000 (-660000; +1221000) | 668→665 (-3; +0) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 11; N>B>P>C; 321/327/333 | 7845991→9873223 (+2027232; -1455745) | 5607578→7406012 (+1798434; -1509150) | 2509000→3436000 (+927000; -897000) | 3099000→3972000 (+873000; -613000) | 668→663 (-5; +0) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 12; B>P>C>N; 339/345/351 | 8780015→11587692 (+2807677; -3084820) | 6254041→5651608 (-602433; +362406) | 3951000→1045000 (-2906000; +3909000) | 2303000→4608000 (+2305000; -3548000) | 668→664 (-4; -3) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 13; P>C>N>B; 381/363/369 | 8181573→14788397 (+6606824; -6885413) | 5953769→7044350 (+1090581; -1424987) | 3756000→4007000 (+251000; -328000) | 2199000→3039000 (+840000; -1098000) | 667→663 (-4; +1) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→1 (+1; -1) |
| 14; C>N>B>P; 399/405/387 | 8120154→8228860 (+108706; +12534) | 5945269→5783064 (-162205; +53401) | 1663000→3260000 (+1597000; +1426000) | 4285000→2525000 (-1760000; -1374000) | 668→663 (-5; +0) | 9→11 (+2; +0) | 10→12 (+2; +0) | 1→0 (-1; +0) |
| 15; N>B>P>C; 417/423/429 | 7551069→8446851 (+895782; -30567) | 5393362→6092997 (+699635; +68128) | 2631000→4458000 (+1827000; -807000) | 2764000→1636000 (-1128000; +875000) | 667→663 (-4; +0) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 16; B>P>C>N; 435/441/447 | 8588090→7981569 (-606521; +950543) | 6437735→5382098 (-1055637; +1152545) | 3653000→2788000 (-865000; +479000) | 2785000→2596000 (-189000; +672000) | 667→662 (-5; +0) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +1) |
| 17; P>C>N>B; 477/459/465 | 8567524→8932687 (+365163; -958031) | 6282835→6545825 (+262990; -1062653) | 4035000→3081000 (-954000; +948000) | 2250000→3465000 (+1215000; -2010000) | 668→663 (-5; -1) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 18; C>N>B>P; 495/501/483 | 7834693→8187928 (+353235; +355778) | 5827163→5673021 (-154142; +370550) | 4632000→4607000 (-25000; -1981000) | 1197000→1066000 (-131000; +2353000) | 668→663 (-5; +2) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 19; N>B>P>C; 513/519/525 | 8068717→8038021 (-30696; +298531) | 5828153→5504213 (-323940; +432098) | 4895000→2059000 (-2836000; +910000) | 935000→3446000 (+2511000; -477000) | 666→662 (-4; +2) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |

#### journals / init_ns

| round; order; records B/P/C | wall_ns | cpu_ns | user_ns | sys_ns | minflt | majflt | nvcsw | nivcsw |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| W-2; C>N>B>P; 610/616/598 | 4802051→4905366 (+103315; -93158) | 3407524→3383881 (-23643; -48443) | 3169000→2139000 (-1030000; -1648000) | 239000→1246000 (+1007000; +1600000) | 528→523 (-5; +1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| W-1; N>B>P>C; 628/634/640 | 4622226→4819048 (+196822; +26028) | 3310998→3279141 (-31857; -44426) | 2355000→1721000 (-634000; -102000) | 958000→1559000 (+601000; +60000) | 528→525 (-3; -1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 0; B>P>C>N; 646/652/658 | 4494832→4962325 (+467493; +190802) | 3230641→3520269 (+289628; +159649) | 215000→1712000 (+1497000; -1398000) | 3018000→1809000 (-1209000; +1558000) | 527→524 (-3; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 1; P>C>N>B; 688/670/676 | 5115921→4819959 (-295962; -82906) | 3731083→3277600 (-453483; -95075) | 2208000→1140000 (-1068000; -343000) | 1524000→2139000 (+615000; +252000) | 528→525 (-3; -1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 2; C>N>B>P; 706/712/694 | 4515206→5277364 (+762158; +82652) | 3174351→3831167 (+656816; -160232) | 734000→2573000 (+1839000; -2426000) | 2442000→1259000 (-1183000; +2267000) | 530→524 (-6; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 3; N>B>P>C; 724/730/736 | 4693613→5124103 (+430490; -320483) | 3505955→3767875 (+261920; -609481) | 2916000→2893000 (-23000; -1664000) | 591000→877000 (+286000; +1054000) | 528→523 (-5; +1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 4; B>P>C>N; 742/748/754 | 4647398→5246728 (+599330; -223079) | 3397838→3624046 (+226208; -222830) | 1173000→2100000 (+927000; -399000) | 2227000→1526000 (-701000; +175000) | 529→524 (-5; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 5; P>C>N>B; 784/766/772 | 5197565→4831187 (-366378; +19094) | 3516180→3259156 (-257024; -38627) | 1980000→1631000 (-349000; -20000) | 1537000→1631000 (+94000; -20000) | 529→523 (-6; +1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 6; C>N>B>P; 802/808/790 | 4617024→5016827 (+399803; -130863) | 3330025→3467595 (+137570; -96737) | 2870000→823000 (-2047000; +863000) | 462000→2646000 (+2184000; -960000) | 528→524 (-4; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 7; N>B>P>C; 820/826/832 | 5412803→4932066 (-480737; +161810) | 3936854→3409404 (-527450; +90573) | 982000→127000 (-855000; +749000) | 2956000→3283000 (+327000; -657000) | 527→525 (-2; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 8; B>P>C>N; 838/844/850 | 5080772→5041612 (-39160; +282079) | 3703714→3411244 (-292470; +479144) | 1930000→2886000 (+956000; -1971000) | 1775000→528000 (-1247000; +2448000) | 527→525 (-2; -1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +1) |
| 9; P>C>N>B; 880/862/868 | 5370923→5241025 (-129898; -285426) | 4133028→3616881 (-516147; -228866) | 827000→3565000 (+2738000; -176000) | 3308000→54000 (-3254000; -54000) | 528→526 (-2; -1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 10; C>N>B>P; 898/904/886 | 4516184→4775645 (+259461; +793300) | 3253259→3246049 (-7210; +700352) | 1163000→746000 (-417000; +18000) | 2093000→2527000 (+434000; +658000) | 530→523 (-7; +2) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 11; N>B>P>C; 916/922/928 | 4742330→5085065 (+342735; -204295) | 3467092→3482206 (+15114; -59653) | 195000→1671000 (+1476000; -180000) | 3273000→1814000 (-1459000; +120000) | 529→523 (-6; +1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 12; B>P>C>N; 934/940/946 | 4762806→5073917 (+311111; -26780) | 3458547→3643983 (+185436; -218928) | 2689000→2291000 (-398000; -578000) | 771000→1354000 (+583000; +359000) | 528→524 (-4; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 13; P>C>N>B; 976/958/964 | 4669973→5363104 (+693131; -246363) | 3316182→3934100 (+617918; -301693) | 765000→1515000 (+750000; +2119000) | 2553000→2421000 (-132000; -2421000) | 529→523 (-6; +1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→1 (+1; -1) |
| 14; C>N>B>P; 994/1000/982 | 4477915→5549367 (+1071452; -532130) | 3203639→3930563 (+726924; -555361) | 10000→2562000 (+2552000; -2562000) | 3196000→1370000 (-1826000; +2007000) | 528→524 (-4; -1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 15; N>B>P>C; 1012/1018/1024 | 4447955→5655511 (+1207556; +73089) | 3159455→3717977 (+558522; +468305) | 828000→3719000 (+2891000; -2087000) | 2333000→0 (-2333000; +2556000) | 529→523 (-6; +2) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 16; B>P>C>N; 1030/1036/1042 | 5083357→5492922 (+409565; -121667) | 3782528→4021954 (+239426; -292965) | 3200000→3131000 (-69000; -665000) | 584000→893000 (+309000; +372000) | 529→524 (-5; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 17; P>C>N>B; 1072/1054/1060 | 5230286→5157520 (-72766; +63854) | 4026065→3601629 (-424436; -160546) | 2666000→484000 (-2182000; +1237000) | 1361000→3119000 (+1758000; -1398000) | 529→523 (-6; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 18; C>N>B>P; 1090/1096/1078 | 4649604→4801492 (+151888; +757547) | 3290687→3305459 (+14772; +665682) | 1158000→113000 (-1045000; +2680000) | 2135000→3194000 (+1059000; -2015000) | 529→524 (-5; +1) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |
| 19; N>B>P>C; 1108/1114/1120 | 5177912→6062019 (+884107; -870771) | 3953008→4618158 (+665150; -1062080) | 129000→2429000 (+2300000; -1219000) | 3826000→2190000 (-1636000; +159000) | 527→524 (-3; +0) | 6→8 (+2; +0) | 6→8 (+2; +0) | 0→0 (+0; +0) |

#### journals / load_ns

| round; order; records B/P/C | wall_ns | cpu_ns | user_ns | sys_ns | minflt | majflt | nvcsw | nivcsw |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| W-2; C>N>B>P; 610/616/598 | 2962173→2959184 (-2989; +194361) | 2163624→2153829 (-9795; +26379) | 949000→963000 (+14000; +900000) | 1214000→1191000 (-23000; -873000) | 145→149 (+4; -4) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| W-1; N>B>P>C; 628/634/640 | 3242464→3072831 (-169633; +101623) | 2360303→2285149 (-75154; +27445) | 817000→981000 (+164000; +976000) | 1544000→1304000 (-240000; -950000) | 145→146 (+1; +1) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 0; B>P>C>N; 646/652/658 | 3210421→3059839 (-150582; +292960) | 2319263→2242179 (-77084; +158159) | 1580000→2097000 (+517000; -1301000) | 739000→145000 (-594000; +1459000) | 145→147 (+2; -2) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 1; P>C>N>B; 688/670/676 | 3764515→3065795 (-698720; +150670) | 2858656→2130571 (-728085; +171917) | 923000→1438000 (+515000; -489000) | 1935000→693000 (-1242000; +660000) | 146→148 (+2; -2) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 2; C>N>B>P; 706/712/694 | 3003380→3424067 (+420687; +159006) | 2153586→2574082 (+420496; +88779) | 1215000→1116000 (-99000; +1324000) | 938000→1457000 (+519000; -1235000) | 146→147 (+1; +0) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 3; N>B>P>C; 724/730/736 | 3512356→3420386 (-91970; -362801) | 2733263→2489277 (-243986; -346278) | 2733000→1245000 (-1488000; -485000) | 0→1245000 (+1245000; +138000) | 145→147 (+2; +0) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 4; B>P>C>N; 742/748/754 | 3308351→3326622 (+18271; -153698) | 2380994→2484876 (+103882; -226000) | 1630000→844000 (-786000; -481000) | 750000→1640000 (+890000; +255000) | 146→148 (+2; -1) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 5; P>C>N>B; 784/766/772 | 3277567→3632311 (+354744; -530153) | 2375565→2682412 (+306847; -408254) | 2375000→2179000 (-196000; -245000) | 0→503000 (+503000; -163000) | 149→147 (-2; +0) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 6; C>N>B>P; 802/808/790 | 11128191→2895588 (-8232603; +366408) | 2436072→2008934 (-427138; +459131) | 1350000→1174000 (-176000; +901000) | 1086000→835000 (-251000; -442000) | 147→145 (-2; +2) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 7; N>B>P>C; 820/826/832 | 3935457→3295353 (-640104; -111309) | 3082971→2337704 (-745267; -28174) | 962000→796000 (-166000; +199000) | 2120000→1542000 (-578000; -229000) | 146→146 (+0; +0) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 8; B>P>C>N; 838/844/850 | 3551841→3197039 (-354802; -109591) | 2630961→2309683 (-321278; +107957) | 1080000→785000 (-295000; +1388000) | 1551000→1525000 (-26000; -1280000) | 147→147 (+0; +1) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +1) |
| 9; P>C>N>B; 880/862/868 | 3258814→12321116 (+9062302; -9251464) | 2375890→2544932 (+169042; -259200) | 866000→1694000 (+828000; -384000) | 1510000→850000 (-660000; +125000) | 145→147 (+2; -1) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 10; C>N>B>P; 898/904/886 | 3380112→3002004 (-378108; +82561) | 2392168→2203851 (-188317; +42074) | 2172000→1924000 (-248000; -242000) | 221000→279000 (+58000; +285000) | 147→146 (-1; +1) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 11; N>B>P>C; 916/922/928 | 10043794→3235673 (-6808121; +1527547) | 2668198→2408259 (-259939; +27427) | 886000→2217000 (+1331000; -1172000) | 1781000→191000 (-1590000; +1199000) | 146→146 (+0; +3) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 12; B>P>C>N; 934/940/946 | 3430833→3265979 (-164854; +70453) | 2509696→2345527 (-164169; +42070) | 1255000→1019000 (-236000; +961000) | 1255000→1326000 (+71000; -918000) | 146→145 (-1; +2) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 13; P>C>N>B; 976/958/964 | 3337037→3205161 (-131876; -229783) | 2414635→2325697 (-88938; -246975) | 2415000→1162000 (-1253000; +115000) | 0→1162000 (+1162000; -361000) | 148→145 (-3; +3) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 14; C>N>B>P; 994/1000/982 | 3409288→3237330 (-171958; -36745) | 2542103→2429801 (-112302; -112535) | 1482000→1634000 (+152000; -282000) | 1059000→796000 (-263000; +170000) | 147→146 (-1; +0) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 15; N>B>P>C; 1012/1018/1024 | 13529347→3130516 (-10398831; -38705) | 2328889→2190526 (-138363; +127483) | 0→0 (+0; +2064000) | 2328000→2190000 (-138000; -1937000) | 146→147 (+1; +0) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 16; B>P>C>N; 1030/1036/1042 | 3852621→3770627 (-81994; -580963) | 3110116→2936186 (-173930; -558511) | 1306000→2461000 (+1155000; -1424000) | 1803000→476000 (-1327000; +863000) | 148→146 (-2; +0) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 17; P>C>N>B; 1072/1054/1060 | 3260806→3265454 (+4648; +131016) | 2587035→2494986 (-92049; -8867) | 2587000→2148000 (-439000; -905000) | 0→346000 (+346000; +897000) | 146→146 (+0; +0) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 18; C>N>B>P; 1090/1096/1078 | 3338174→3081087 (-257087; +251200) | 2465040→2325106 (-139934; +60944) | 2465000→800000 (-1665000; +39000) | 0→1522000 (+1522000; +24000) | 146→148 (+2; -2) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |
| 19; N>B>P>C; 1108/1114/1120 | 3273332→3801057 (+527725; -362513) | 2450685→2919323 (+468638; -360754) | 1414000→1571000 (+157000; +634000) | 1037000→1347000 (+310000; -994000) | 147→147 (+0; -1) | 3→3 (+0; +0) | 4→4 (+0; +0) | 0→0 (+0; +0) |

#### journals / load_plus_init

| round; order; records B/P/C | wall_ns | cpu_ns | user_ns | sys_ns | minflt | majflt | nvcsw | nivcsw |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| W-2; C>N>B>P; 610/616/598 | 7764224→7864550 (+100326; +101203) | 5571148→5537710 (-33438; -22064) | 4118000→3102000 (-1016000; -748000) | 1453000→2437000 (+984000; +727000) | 673→672 (-1; -3) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| W-1; N>B>P>C; 628/634/640 | 7864690→7891879 (+27189; +127651) | 5671301→5564290 (-107011; -16981) | 3172000→2702000 (-470000; +874000) | 2502000→2863000 (+361000; -890000) | 673→671 (-2; +0) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 0; B>P>C>N; 646/652/658 | 7705253→8022164 (+316911; +483762) | 5549904→5762448 (+212544; +317808) | 1795000→3809000 (+2014000; -2699000) | 3757000→1954000 (-1803000; +3017000) | 672→671 (-1; -2) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 1; P>C>N>B; 688/670/676 | 8880436→7885754 (-994682; +67764) | 6589739→5408171 (-1181568; +76842) | 3131000→2578000 (-553000; -832000) | 3459000→2832000 (-627000; +912000) | 674→673 (-1; -3) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 2; C>N>B>P; 706/712/694 | 7518586→8701431 (+1182845; +241658) | 5327937→6405249 (+1077312; -71453) | 1949000→3689000 (+1740000; -1102000) | 3380000→2716000 (-664000; +1032000) | 676→671 (-5; +0) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 3; N>B>P>C; 724/730/736 | 8205969→8544489 (+338520; -683284) | 6239218→6257152 (+17934; -955759) | 5649000→4138000 (-1511000; -2149000) | 591000→2122000 (+1531000; +1192000) | 673→670 (-3; +1) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 4; B>P>C>N; 742/748/754 | 7955749→8573350 (+617601; -376777) | 5778832→6108922 (+330090; -448830) | 2803000→2944000 (+141000; -880000) | 2977000→3166000 (+189000; +430000) | 675→672 (-3; -1) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 5; P>C>N>B; 784/766/772 | 8475132→8463498 (-11634; -511059) | 5891745→5941568 (+49823; -446881) | 4355000→3810000 (-545000; -265000) | 1537000→2134000 (+597000; -183000) | 678→670 (-8; +1) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 6; C>N>B>P; 802/808/790 | 15745215→7912415 (-7832800; +235545) | 5766097→5476529 (-289568; +362394) | 4220000→1997000 (-2223000; +1764000) | 1548000→3481000 (+1933000; -1402000) | 675→669 (-6; +2) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 7; N>B>P>C; 820/826/832 | 9348260→8227419 (-1120841; +50501) | 7019825→5747108 (-1272717; +62399) | 1944000→923000 (-1021000; +948000) | 5076000→4825000 (-251000; -886000) | 673→671 (-2; +0) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 8; B>P>C>N; 838/844/850 | 8632613→8238651 (-393962; +172488) | 6334675→5720927 (-613748; +587101) | 3010000→3671000 (+661000; -583000) | 3326000→2053000 (-1273000; +1168000) | 674→672 (-2; +0) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +2) |
| 9; P>C>N>B; 880/862/868 | 8629737→17562141 (+8932404; -9536890) | 6508918→6161813 (-347105; -488066) | 1693000→5259000 (+3566000; -560000) | 4818000→904000 (-3914000; +71000) | 673→673 (+0; -2) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 10; C>N>B>P; 898/904/886 | 7896296→7777649 (-118647; +875861) | 5645427→5449900 (-195527; +742426) | 3335000→2670000 (-665000; -224000) | 2314000→2806000 (+492000; +943000) | 677→669 (-8; +3) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 11; N>B>P>C; 916/922/928 | 14786124→8320738 (-6465386; +1323252) | 6135290→5890465 (-244825; -32226) | 1081000→3888000 (+2807000; -1352000) | 5054000→2005000 (-3049000; +1319000) | 675→669 (-6; +4) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 12; B>P>C>N; 934/940/946 | 8193639→8339896 (+146257; +43673) | 5968243→5989510 (+21267; -176858) | 3944000→3310000 (-634000; +383000) | 2026000→2680000 (+654000; -559000) | 674→669 (-5; +2) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 13; P>C>N>B; 976/958/964 | 8007010→8568265 (+561255; -476146) | 5730817→6259797 (+528980; -548668) | 3180000→2677000 (-503000; +2234000) | 2553000→3583000 (+1030000; -2782000) | 677→668 (-9; +4) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→1 (+1; -1) |
| 14; C>N>B>P; 994/1000/982 | 7887203→8786697 (+899494; -568875) | 5745742→6360364 (+614622; -667896) | 1492000→4196000 (+2704000; -2844000) | 4255000→2166000 (-2089000; +2177000) | 675→670 (-5; -1) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 15; N>B>P>C; 1012/1018/1024 | 17977302→8786027 (-9191275; +34384) | 5488344→5908503 (+420159; +595788) | 828000→3719000 (+2891000; -23000) | 4661000→2190000 (-2471000; +619000) | 675→670 (-5; +2) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 16; B>P>C>N; 1030/1036/1042 | 8935978→9263549 (+327571; -702630) | 6892644→6958140 (+65496; -851476) | 4506000→5592000 (+1086000; -2089000) | 2387000→1369000 (-1018000; +1235000) | 677→670 (-7; +0) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 17; P>C>N>B; 1072/1054/1060 | 8491092→8422974 (-68118; +194870) | 6613100→6096615 (-516485; -169413) | 5253000→2632000 (-2621000; +332000) | 1361000→3465000 (+2104000; -501000) | 675→669 (-6; +0) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 18; C>N>B>P; 1090/1096/1078 | 7987778→7882579 (-105199; +1008747) | 5755727→5630565 (-125162; +726626) | 3623000→913000 (-2710000; +2719000) | 2135000→4716000 (+2581000; -1991000) | 675→672 (-3; -1) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |
| 19; N>B>P>C; 1108/1114/1120 | 8451244→9863076 (+1411832; -1233284) | 6403693→7537481 (+1133788; -1422834) | 1543000→4000000 (+2457000; -585000) | 4863000→3537000 (-1326000; -835000) | 674→671 (-3; -1) | 9→11 (+2; +0) | 10→12 (+2; +0) | 0→0 (+0; +0) |

### Cause, correlation, unknowns, and conditional probe decision

**Demonstrated:** the retained cold timing scopes contain every requested
field. Init wall P-B is positive in both original cells. The
wall-minus-process-CPU residual increases, and the prototype records two
additional major faults and voluntary switches in every measured pair,
including after summing load+init. A CPU increase is not established by
these component intervals. Load+init wall is still higher for phrases;
journals' combined wall comparison is inconclusive. The resolved
companion-worker dependency sets are identical as qualified above.

**Correlation, not demonstrated causation:** extra major faults and
voluntary switches accompany the larger residual. They are consistent
with additional fault/I/O waiting or other scheduling delay, but they do
not identify the blocked operation or prove that those two faults account
for the entire wall difference. The CPU/residual accounting is not a
complete causal partition, and neither medians nor bootstrap endpoints
may be added to fabricate one. The observed DSO graph does not identify
which library or data mapping incurred those faults.

**Unknown:** fault IPs/addresses, the backing file/pages involved, time
spent servicing each fault, kernel wait reasons and call paths, hardware
cycles/instructions, and whether specific relocations, bindings or code
placement contributed. Cold timing-worker maps and transient loader
activity were not recorded. No specific libstdc++ cause is demonstrated.

The conditional OXPINYIN_TEARDOWN_INIT_PROBE pass was **not run**. Step 1
already supplies the broad cost-category evidence: a larger non-CPU
residual with extra fault/switch events, while CPU change is not
established. The remaining attribution gap is chiefly fault/wait location
and mechanism. A single count pass would repeat event categories without
supplying those addresses or call paths; this diagnosis stops at that
uncertainty rather than expanding the run. Hardware cycles/instructions
are **unrecorded**, not zero; their availability, kernel/user scope and
multiplexing cannot be reported as measured. Existing Callgrind Ir is a
separate metric and is not substituted for hardware instructions.
No perf permission was changed and no event-open failure was bypassed.

No concrete code or policy fix is supported by these records. In
particular, neither removing a purported new libstdc++ load nor changing
layout/pretouch behavior is justified by the established graph and
unlocated faults. No fix was implemented. STOP remains; no additional
measurement, build, policy generation, source edit, rebase or PR follows.

### Tools and commands

The local scripts below read existing files only; their derived JSON and
all original worker records remain local, uncommitted. Only this findings
update is pushed. The pairing script checks completeness, forms sums
within each sample, joins by original round/profile and reports original
serial order. The graph script validates all 88 snapshot sets per arm,
resolves every NEEDED edge against mapped SONAMEs, and checks retained
inodes/current files. No artifact, capture, archive or log is published.

```sh
export R=/home/sheng/Documents/Codex/2026-10-24/task-3
export PY=/home/sheng/.local/share/uv/python/cpython-3.15.0-linux-x86_64-gnu/bin/python3.15
cd "$R/worktree"
$PY -X lazy_imports=normal "$R/diagnosis-1125/analyze.py"
python3 "$R/diagnosis-1125/graphs.py"
readelf -l "$R/campaign-finish/bisect"
# For each resolved ELF file listed in the graph:
readelf -d "$ELF"
sha256sum "$ELF"
git diff --check
```


## Completed fixed-base full-finish campaign; STOP for maintainer decision

Recorded 2026-10-24T10:09:47.898417+00:00. **STOP: a controlled
cold-library pinyin init slowdown trades time for residency.** The paired
prototype/base ratios are 1.073206 [1.047978, 1.090068] for phrases and
1.076421 [1.012480, 1.116484] for journals. Those two init timing controls
pass; no time improvement is claimed. All other init/first-candidate wall
cells are INCONCLUSIVE. The coordinator's separate steady maximum-spread
guard did not fire; that does not cancel these controlled init regressions.

All 30 store timing controls and all 60 store profiler controls pass.
Seven store timing intervals are wholly above 1; all rows, including
regressions, are below. Three store VmHWM controls are INCONCLUSIVE.
M1 total-Pss controls pass in all four cells, with paired prototype-base
medians -260, -256, -317.5 and -316 KiB. Resident libtkrzw executable text
falls by 312 KiB in every cell; facade text is unchanged. Exact init Ir
falls 1.37%-1.96%; first/steady Ir controls fail and are withheld.
Stripped shipping size increases by 12,456 B (pinyin) and 4,264 B (zhuyin).
No adoption recommendation or PR follows from this run.

This section reports the new single Ubuntu campaign from checkpoint
c4cc6d0ca1063704ca2905966a481647b8bf164b (shipping source identical to
6e50b77e), against fixed base d7788072 and the pin. The empty-message
fast path remains removed. Fresh regenerated policies match the existing
code-commit policy bytes; both independent passes matched. Ordered builds
resolved all C++ paths and the linked-object verifier rejected no subject.
No rebase onto main was performed. Original campaign results below retain
their own provenance and are not substituted into these tables.

All 30 operations from the existing store harness ran in this campaign.
Store timing uses 30 rounds plus three warmups, rotating base / same-bytes
base control / prototype, one fresh process per sample. Checksums and
units must match. Ir and DHAT figures require exact controls; invalid
figures are withheld. The existing runner has no pin implementation of
these store-operation entry points: pin store cells are unavailable, not
zero or a substitute from a different workload. Pin is included in the
same-campaign facade init/first-candidate/memory and size measurements.
No new copy/alignment experiment or further memcmp investigation occurred.

### Every store operation: wall time and instructions

Wall medians are ns/store call. The ratio and whole 95% bootstrap interval
are shown only if the same-bytes timing control passes [0.95, 1.05].
INCONCLUSIVE rows support no timing claim. Ir/call is shown only when
base/control whole counts match exactly. No range qualification is used
to rescue an unexplained mismatch.

| operation | wall base | wall prototype | wall control | timing status | prototype/base [95%] | base Ir/call | prototype Ir/call | Ir control |
| --- | ---: | ---: | ---: | --- | ---: | ---: | ---: | --- |
| contains_raw.tree | 486.9420425 | 482.34269 | 487.820645 | STABLE | 0.984333429761 [0.976431499065, 0.992387590274] | 2578.17745 | 2539.1663 | VALID |
| get_raw.tree.empty | 482.8851025 | 482.70931 | 483.4542425 | STABLE | 1.00035988454 [0.991258567741, 1.01106918783] | 2566.1371 | 2568.72165 | VALID |
| get_raw.tree.small | 502.113505 | 496.413185 | 500.42715 | STABLE | 0.990375872356 [0.985935255648, 0.999758184918] | 2621.41835 | 2623.10775 | VALID |
| get_raw.tree.medium | 519.745135 | 523.28208 | 525.300985 | STABLE | 1.0021562783 [0.997483772166, 1.01673764976] | 2743.2518 | 2744.4776 | VALID |
| get_raw.tree.large | 1602.230075 | 1599.47083 | 1596.45666 | STABLE | 0.99885960974 [0.99401401971, 1.00098686548] | 7821.2533 | 7825.0239 | VALID |
| get_raw.tree.miss | 488.157285 | 488.606135 | 488.0711125 | STABLE | 0.998425035781 [0.993993129305, 1.00473926017] | 4211.72685 | 4215.3682 | VALID |
| get_raw.tree.retain | 547.130567187 | 543.694284375 | 551.451617188 | STABLE | 0.999558466185 [0.979651417658, 1.01080101517] | 2885.77559375 | 2891.7811875 | VALID |
| get_raw.hash | 444.2603475 | 445.5053625 | 443.226715 | STABLE | 1.00353403721 [0.999276752855, 1.00790448646] | 2555.8313 | 2556.0516 | VALID |
| get_raw.hash.small | 220.1956 | 221.0675675 | 218.8631375 | STABLE | 1.00523636533 [1.00014571689, 1.01023667509] | 1591.44375 | 1590.713 | VALID |
| get_raw.hash.medium | 632.0536825 | 632.6762775 | 633.0595125 | STABLE | 1.00067585593 [0.997835649438, 1.00520052189] | 3440.87835 | 3439.87275 | VALID |
| get_raw.hash.large | 12500.95274 | 12479.30396 | 12506.09896 | STABLE | 0.998943353366 [0.996540292351, 1.0005605415] | 67543.6014 | 67557.7532 | VALID |
| get_raw.hash.miss | 123.2226925 | 125.3353425 | 122.862555 | STABLE | 1.01890849755 [1.0124125517, 1.02619365191] | 817.05385 | 818.02655 | VALID |
| scan_raw.suggestion | 9657.775 | 9820.45 | 9875.15 | STABLE | 1.01217213849 [0.992975393124, 1.03555364559] | 137656 | 137901.5 | VALID |
| scan_raw.full.tree | 92871754.8 | 92729299.2 | 92970908 | STABLE | 0.99672112423 [0.991828736619, 1.00564813428] | 621719025 | 623792408 | VALID |
| scan_raw.full.hash | 113723993.75 | 114116874.5 | 113975563.25 | STABLE | 1.00194021275 [0.997687833331, 1.00552738529] | 553027451 | 553646119 | VALID |
| framed.get | 298.91284 | 298.1472775 | 296.158585 | STABLE | 0.988755539263 [0.980073885994, 1.00404140642] | 1884.88355 | 1874.9424 | VALID |
| framed.scan | 6794.91197 | 6917.83712 | 6820.49071 | STABLE | 1.01655034457 [1.01286929854, 1.02194742071] | 48058.6604 | 47766.5742 | VALID |
| framed.txn_scan | 2943.74325 | 3004.787625 | 2928.13125 | STABLE | 1.01835243745 [1.01135861969, 1.02785771051] | 24994.105 | 24801.6575 | VALID |
| framed.is_empty | 345.505 | 329.8350775 | 342.097725 | STABLE | 0.957287123027 [0.949625566821, 0.964925928971] | 2925.07385 | 2782.06065 | VALID |
| user_bigram.get | 301.51268 | 302.605445 | 300.21247 | STABLE | 1.0063899985 [1.00084141574, 1.01141637013] | 1432.1947 | 1432.77625 | VALID |
| user_bigram.get.small | 108.0541025 | 110.44466 | 108.0045775 | STABLE | 1.02293302906 [1.01897075866, 1.02765523843] | 677.6992 | 678.4709 | VALID |
| user_bigram.get.large | 11721.61485 | 11766.74933 | 11734.8552 | STABLE | 1.00320353524 [1.00176100767, 1.00604236791] | 56578.954 | 56573.0212 | VALID |
| user_bigram.miss | 70.29714 | 69.9054475 | 70.4830025 | STABLE | 0.993409041327 [0.989249589492, 0.997252832339] | 420.4697 | 421.4697 | VALID |
| user_bigram.store | 323.677385 | 319.694835 | 322.96787 | STABLE | 0.986963568435 [0.98444786592, 0.989064366375] | 2371.6074 | 2347.20215 | VALID |
| user_bigram.remove_store | 314.5012375 | 310.7064125 | 314.7648125 | STABLE | 0.989284401243 [0.986316644591, 0.991372925274] | 2296.7516 | 2271.13805 | VALID |
| user_bigram.keys | 32040975.125 | 32050589.365 | 32041804.46 | STABLE | 1.00020456191 [0.998454874067, 1.00240297524] | 66185953.9 | 66215685.4 | VALID |
| lifecycle.open_system | 59171.405 | 51656.9808333 | 60702.4516667 | STABLE | 0.886876498831 [0.860570018397, 0.934084940637] | 188071.716667 | 166285.225 | VALID |
| lifecycle.user_load | 46522029.68 | 46684877.785 | 46785478.825 | STABLE | 1.00143036866 [0.994415724466, 1.0085627754] | 176837405.8 | 176805659.5 | VALID |
| lifecycle.user_save | 28159634.92 | 28234879.63 | 28182354.51 | STABLE | 0.996007056094 [0.985794123466, 1.01019002741] | 51742465.7 | 51689074.2 | VALID |
| session.typing_training | 722910.658986 | 712434.624424 | 699678.391705 | STABLE | 0.992818830813 [0.982361349521, 1.00360808533] | 1963296.16129 | 1962357.34101 | VALID |

### Every store operation: heap accounting

DHAT blocks and bytes are full-minus-setup per store call; peak is the
whole process in bytes. A small negative full-minus-setup value is the
runner's subtraction result, not negative allocation or a clamped zero. All three counters require exact base/control
values. INVALID rows are retained without a comparison.

| operation | blocks/call base / prototype | bytes/call base / prototype | peak B base / prototype | control |
| --- | ---: | ---: | ---: | --- |
| contains_raw.tree | 0.03235 / 0.03235 | 2.3229 / 2.3229 | 21877916 / 21877828 | VALID |
| get_raw.tree.empty | 0.03235 / 0.03235 | 2.3229 / 2.3229 | 25825965 / 25825893 | VALID |
| get_raw.tree.small | 0.0324 / 0.0324 | 2.3231 / 2.3231 | 29635149 / 29635077 | VALID |
| get_raw.tree.medium | 0.04995 / 0.04995 | 3.02605 / 3.02605 | 25611086 / 25611014 | VALID |
| get_raw.tree.large | 0.1001 / 0.1001 | 9.7681 / 9.7681 | 24719143 / 24719071 | VALID |
| get_raw.tree.miss | 4.9971 / 4.9971 | 321.9979 / 321.9979 | 3090228 / 3090156 | VALID |
| get_raw.tree.retain | 1.0624375 / 1.0624375 | 65.692375 / 65.692375 | 7902975 / 7902903 | VALID |
| get_raw.hash | 0.49645 / 0.49645 | 216.2225 / 216.2225 | 18472648 / 18472560 | VALID |
| get_raw.hash.small | 0.18455 / 0.18455 | 9.7977 / 9.7977 | 19393478 / 19393406 | VALID |
| get_raw.hash.medium | 1.0002 / 1.0002 | 397.4545 / 397.4545 | 19358039 / 19357967 | VALID |
| get_raw.hash.large | 1.0004 / 1.0004 | 12097.0258 / 12097.0258 | 18491261 / 18491189 | VALID |
| get_raw.hash.miss | -5e-05 / -5e-05 | -0.0013 / -0.0013 | 20397525 / 20397453 | VALID |
| scan_raw.suggestion | 93 / 92 | 6380.5 / 6364.5 | 21877914 / 21877826 | VALID |
| scan_raw.full.tree | 217188 / 217187 | 14324506 / 14324490 | 6493605 / 6493517 | VALID |
| scan_raw.full.hash | 87469 / 87468 | 18247082 / 18247066 | 475018 / 474930 | VALID |
| framed.get | 2 / 2 | 31.9991 / 31.9991 | 1886038 / 1885966 | VALID |
| framed.scan | 5.9998 / 4.9998 | 247.9946 / 231.9946 | 1886038 / 1885966 | VALID |
| framed.txn_scan | 4 / 3.5 | 135.95 / 127.95 | 1886041 / 1885969 | VALID |
| framed.is_empty | 5.99995 / 4.99995 | 240.9987 / 224.9987 | 1886043 / 1885971 | VALID |
| user_bigram.get | 0.00025 / 0.00025 | 0.5571 / 0.5571 | 18472651 / 18472563 | VALID |
| user_bigram.get.small | 0.00015 / 0.00015 | 0.0055 / 0.0055 | 18472657 / 18472569 | VALID |
| user_bigram.get.large | 0 / 0 | 19.0082 / 19.0082 | 18472656 / 18472568 | VALID |
| user_bigram.miss | -5e-05 / -5e-05 | -0.0013 / -0.0013 | 18472652 / 18472564 | VALID |
| user_bigram.store | 10.99995 / 10.99995 | 422.0947 / 422.0947 | 18472653 / 18472565 | VALID |
| user_bigram.remove_store | 10.99995 / 10.99995 | 429.23365 / 429.23365 | 18472660 / 18472572 | VALID |
| user_bigram.keys | 7.9 / 6.9 | 20797.1 / 20781.1 | 18472649 / 18472561 | VALID |
| lifecycle.open_system | 330.675 / 304.333333333 | 161703.458333 / 159870.525 | 1010146 / 1009778 | VALID |
| lifecycle.user_load | 6490.5 / 6458.5 | 5590298.7 / 5587977.7 | 27522423 / 27522311 | VALID |
| lifecycle.user_save | 30553.4 / 30533.9 | 2180531 / 2179321 | 27555359 / 27555247 | VALID |
| session.typing_training | 230.35483871 / 228.98156682 | 60406.235023 / 60325.4976959 | 28932748 / 28932660 | VALID |

### Every store operation: process VmHWM

KiB; paired differences are shown only with a valid residency control.

| operation | base | prototype | control | prototype-base [95%] | control |
| --- | ---: | ---: | ---: | ---: | --- |
| contains_raw.tree | 40962 | 40720 | 40982 | -248 [-286, -200] | VALID |
| get_raw.tree.empty | 44802 | 44530 | 44784 | -252 [-318, -224] | VALID |
| get_raw.tree.small | 48536 | 48276 | 48484 | -270 [-298, -236] | VALID |
| get_raw.tree.medium | 45962 | 45710 | 45930 | -238 [-286, -160] | VALID |
| get_raw.tree.large | 45068 | 44824 | 45048 | -220 [-294, -172] | VALID |
| get_raw.tree.miss | 15384 | 15096 | 15372 | -290 [-308, -208] | VALID |
| get_raw.tree.retain | 23884 | 23626 | 23890 | -238 [-296, -184] | VALID |
| get_raw.hash | 46676 | 46490 | 46670 | -222 [-254, -166] | VALID |
| get_raw.hash.small | 48492 | 48274 | 48478 | -242 [-300, -198] | VALID |
| get_raw.hash.medium | 48104 | 47866 | 48074 | -208 [-274, -170] | VALID |
| get_raw.hash.large | 46688 | 46454 | 46672 | -214 [-270, -170] | VALID |
| get_raw.hash.miss | 49034 | 48848 | 49020 | -210 [-248, -182] | VALID |
| scan_raw.suggestion | 40964 | 40712 | 40944 | -236 [-280, -214] | VALID |
| scan_raw.full.tree | 20310 | 20106 | 20286 | -192 [-238, -150] | VALID |
| scan_raw.full.hash | 27002 | 26768 | 26984 | -232 [-278, -192] | VALID |
| framed.get | 9494 | 8992 | 9410 | withheld | INCONCLUSIVE |
| framed.scan | 9506 | 8966 | 9410 | withheld | INCONCLUSIVE |
| framed.txn_scan | 9488 | 8980 | 9456 | -514 [-544, -472] | VALID |
| framed.is_empty | 9466 | 8990 | 9460 | -496 [-526, -432] | VALID |
| user_bigram.get | 46676 | 46458 | 46692 | -222 [-264, -192] | VALID |
| user_bigram.get.small | 46700 | 46474 | 46652 | -228 [-258, -208] | VALID |
| user_bigram.get.large | 46688 | 46498 | 46654 | -200 [-228, -162] | VALID |
| user_bigram.miss | 46682 | 46444 | 46664 | -228 [-242, -174] | VALID |
| user_bigram.store | 46708 | 46426 | 46676 | -254 [-292, -226] | VALID |
| user_bigram.remove_store | 46678 | 46434 | 46658 | -224 [-254, -182] | VALID |
| user_bigram.keys | 46682 | 46450 | 46676 | -220 [-290, -166] | VALID |
| lifecycle.open_system | 8362 | 8088 | 8316 | withheld | INCONCLUSIVE |
| lifecycle.user_load | 48940 | 48714 | 48898 | -192 [-284, -164] | VALID |
| lifecycle.user_save | 47306 | 47096 | 47286 | -212 [-268, -150] | VALID |
| session.typing_training | 55316 | 55080 | 55294 | -246 [-308, -192] | VALID |

### Same-run facade M1 memory

Twenty paired rounds after two warmups. KiB; paired deltas have 95%
bootstrap intervals. Total Pss uses the usual residency control bound
±max(64 KiB, 1% of the prototype median).

| facade/profile | total Pss base / prototype / control / pin | paired prototype-base [95%] | paired prototype-pin [95%] | control |
| --- | ---: | ---: | ---: | --- |
| pinyin/phrases | 9396 / 9132.5 / 9135 / 9258.5 | -260 [-262, -260] | -124.5 [-134, -59.5] | VALID |
| pinyin/journals | 9003 / 8748 / 8744 / 8754 | -256 [-257, -253] | -8.5 [-14, -1.5] | VALID |
| zhuyin/phrases | 8484.5 / 8168 / 8168 / 8159 | -317.5 [-320, -315.5] | 6 [6, 10] | VALID |
| zhuyin/journals | 8548 / 8232 / 8232 / 8162 | -316 [-317, -314.5] | 70 [67, 72.5] | VALID |

Resident executable text Rss at M1, KiB:

| facade/profile | facade base / prototype / pin | libtkrzw base / prototype / pin |
| --- | ---: | ---: |
| pinyin/phrases | 508 / 508 / 464 | 1080 / 768 / 896 |
| pinyin/journals | 508 / 508 / 464 | 1080 / 768 / 896 |
| zhuyin/phrases | 520 / 520 / 448 | 1080 / 768 / 896 |
| zhuyin/journals | 520 / 520 / 448 | 1080 / 768 / 896 |

### Same-run facade instructions

Exact prototype/control equality is required separately for each scope. INVALID scopes have no comparative figures.

| facade/profile/scope | base Ir | prototype Ir | control Ir | pin Ir | control |
| --- | ---: | ---: | ---: | ---: | --- |
| pinyin/phrases/init | 10450483 | 10246177 | 10246177 | 338885707 | VALID |
| pinyin/phrases/first | withheld | withheld | withheld | withheld | INVALID |
| pinyin/phrases/steady | withheld | withheld | withheld | withheld | INVALID |
| pinyin/journals/init | 10441064 | 10236215 | 10236215 | 338945747 | VALID |
| pinyin/journals/first | withheld | withheld | withheld | withheld | INVALID |
| pinyin/journals/steady | withheld | withheld | withheld | withheld | INVALID |
| zhuyin/phrases/init | 10061080 | 9923053 | 9923053 | 338543130 | VALID |
| zhuyin/phrases/first | withheld | withheld | withheld | withheld | INVALID |
| zhuyin/phrases/steady | withheld | withheld | withheld | withheld | INVALID |
| zhuyin/journals/init | 10054711 | 9916995 | 9916995 | 338613850 | VALID |
| zhuyin/journals/first | withheld | withheld | withheld | withheld | INVALID |
| zhuyin/journals/steady | withheld | withheld | withheld | withheld | INVALID |

### Init and first-candidate wall time

Medians ns; warm/cold refers to the facade-library cache only. No wall comparison is claimed when the same-bytes control fails.

| facade/profile/cache/scope | base | prototype | control | pin | status | prototype/base [95%] |
| --- | ---: | ---: | ---: | ---: | --- | ---: |
| pinyin/journals/warm/first_candidate_ns | 181529.5 | 182855 | 195585 | 232676 | INCONCLUSIVE | withheld |
| pinyin/journals/warm/init_ns | 3151563 | 3179906.5 | 3200376 | 100788830 | INCONCLUSIVE | withheld |
| pinyin/journals/cold/first_candidate_ns | 185987 | 193888 | 186888.5 | 690787.5 | INCONCLUSIVE | withheld |
| pinyin/journals/cold/init_ns | 4717971.5 | 5104584 | 5105308.5 | 97656006.5 | VALID | 1.07642061251 [1.01247968893, 1.11648355685] |
| pinyin/phrases/warm/first_candidate_ns | 206603 | 191393.5 | 193191.5 | 239654 | INCONCLUSIVE | withheld |
| pinyin/phrases/warm/init_ns | 3181335 | 2984598 | 3062570.5 | 99162290 | INCONCLUSIVE | withheld |
| pinyin/phrases/cold/first_candidate_ns | 194951.5 | 200957 | 207222 | 643260 | INCONCLUSIVE | withheld |
| pinyin/phrases/cold/init_ns | 4702780 | 4990178 | 5134082.5 | 98610162.5 | VALID | 1.07320645775 [1.04797839574, 1.09006840835] |
| zhuyin/journals/warm/first_candidate_ns | 157664.5 | 150365.5 | 156755 | 155377.5 | INCONCLUSIVE | withheld |
| zhuyin/journals/warm/init_ns | 2851748.5 | 2661742.5 | 2813724.5 | 96380274.5 | INCONCLUSIVE | withheld |
| zhuyin/journals/cold/first_candidate_ns | 160757.5 | 162627 | 166229 | 521332.5 | INCONCLUSIVE | withheld |
| zhuyin/journals/cold/init_ns | 4658114 | 4279491.5 | 4479559 | 99377720.5 | INCONCLUSIVE | withheld |
| zhuyin/phrases/warm/first_candidate_ns | 157811 | 154652 | 157722 | 155510.5 | INCONCLUSIVE | withheld |
| zhuyin/phrases/warm/init_ns | 2780328.5 | 2723135 | 2622975.5 | 97934758 | INCONCLUSIVE | withheld |
| zhuyin/phrases/cold/first_candidate_ns | 170481 | 164866 | 172011.5 | 4222286.5 | INCONCLUSIVE | withheld |
| zhuyin/phrases/cold/init_ns | 4678914 | 4355857.5 | 4348793.5 | 104213583 | INCONCLUSIVE | withheld |

### Steady wall samples and facade peak heap

Steady wall medians are ns/cycle; the coordinator applies its existing same-bytes paired maximum-spread guard. These medians alone imply no timing claim.

| facade | base median | prototype median | control median | pin median |
| --- | ---: | ---: | ---: | ---: |
| pinyin | 28065255.5 | 28364684.5 | 28550312.5 | 29164286 |
| zhuyin | 26801839 | 26553382.5 | 26877426 | 26124780 |

Facade whole-process DHAT peak bytes (exact prototype/control required):

| facade/profile | base | prototype | control | pin | control |
| --- | ---: | ---: | ---: | ---: | --- |
| pinyin/phrases | 9554551 | 9554355 | 9554355 | 9492130 | VALID |
| pinyin/journals | 9549521 | 9549325 | 9549325 | 9492130 | VALID |
| zhuyin/phrases | 8995382 | 8995306 | 8995306 | 8915329 | VALID |
| zhuyin/journals | 8990352 | 8990276 | 8990276 | 8915329 | VALID |

### Timed-artifact provenance

SHA256, file size, and ELF NEEDED were read from each actual timed source
artifact after the successful unchanged-hash campaign check. Base is
`d7788072d3b8078aaa500cb5fcb98e0542fe7278`; prototype shipping source is
`6e50b77ef9b380f36ca0cd6d3fb76732c094baaf`, built at docs-only checkpoint
`c4cc6d0ca1063704ca2905966a481647b8bf164b`. The runtime SHA256 remains
`36c6aa4d77edb3bd6a628f02fc456f0a67096bdd451c04999141d7144f144d0e`.

| artifact | SHA256 | bytes | ELF NEEDED |
| --- | --- | ---: | --- |
| base/pinyin | 92a769b8d4c57a61c6190e7e50699deb44f022d87228cb5f96ac00dd16b0031e | 5199952 | libglib-2.0.so.0, libtkrzw.so.1, libc.so.6, ld-linux-x86-64.so.2, libgcc_s.so.1 |
| base/zhuyin | 2eb6d48a03f91587aecad9fc7268dc9a7efc9a1debd29d60f11c782bc4236aee | 4926152 | libglib-2.0.so.0, libtkrzw.so.1, libc.so.6, ld-linux-x86-64.so.2, libgcc_s.so.1 |
| prototype/pinyin | d040918c1da19056ef4e8646cb82be7e7151884766025610e83bad08ba5d2837 | 5218192 | libglib-2.0.so.0, libtkrzw.so.1, libstdc++.so.6, libc.so.6, ld-linux-x86-64.so.2, libgcc_s.so.1 |
| prototype/zhuyin | 267f459a5932f0ca71ee39daca18dfcfd01fb9c9bc4db02f449df192ffe2dbab | 4935752 | libglib-2.0.so.0, libtkrzw.so.1, libstdc++.so.6, libc.so.6, ld-linux-x86-64.so.2, libgcc_s.so.1 |
| control/pinyin | d040918c1da19056ef4e8646cb82be7e7151884766025610e83bad08ba5d2837 | 5218192 | libglib-2.0.so.0, libtkrzw.so.1, libstdc++.so.6, libc.so.6, ld-linux-x86-64.so.2, libgcc_s.so.1 |
| control/zhuyin | 267f459a5932f0ca71ee39daca18dfcfd01fb9c9bc4db02f449df192ffe2dbab | 4935752 | libglib-2.0.so.0, libtkrzw.so.1, libstdc++.so.6, libc.so.6, ld-linux-x86-64.so.2, libgcc_s.so.1 |
| pin/pinyin | d78a91676af43ad5075c20ab4ae014a8bc6e03b7a5f5ab656d014087f18ff7b8 | 5773096 | libglib-2.0.so.0, libtkrzw.so.1, libstdc++.so.6, libm.so.6, libc.so.6, libgcc_s.so.1 |
| pin/zhuyin | adbcc8c8c3dec00a488ea89413a525f57fdccf280a8ca44e6c2ad6765fd24c3d | 5643440 | libglib-2.0.so.0, libtkrzw.so.1, libstdc++.so.6, libm.so.6, libc.so.6, libgcc_s.so.1 |

Store benchmark subjects (the control is a byte-identical base copy):

- base: `/home/sheng/Documents/Codex/2026-10-24/task-3/campaign-finish/base-build/release/deps/backend_matrix_tkrzw-6df5c85cf36edf5d`, SHA256 `b31e10856b2428aca9235886fcb70796a42a171ea86ff436907dfdacebee6d28`, 3071968 B; NEEDED libtkrzw.so.1, libm.so.6, libc.so.6, libgcc_s.so.1.
- prototype: `/home/sheng/Documents/Codex/2026-10-24/task-3/campaign-finish/proto-build/release/deps/backend_matrix_tkrzw-6d7c8325b0fda4f3`, SHA256 `f36d8b52777fb13b096f2897e51f0f258a6589b8d41d3002b42e610f7cd53e2a`, 3089144 B; NEEDED libtkrzw.so.1, libstdc++.so.6, libm.so.6, libc.so.6, ld-linux-x86-64.so.2, libgcc_s.so.1.

Canonical driver arguments used by the local retention wrappers (the
wrappers retain original outputs without changing driver arguments):

```sh
export R=/home/sheng/Documents/Codex/2026-10-24/task-3
cd "$R/worktree"
source "$R/setup-followup.env"
export TMPDIR="$R/tmp-finish"
PIN="$OXPINYIN_ORACLE_PREFIX_TKRZW"
DATA="$OXPINYIN_ORACLE_DATA_TKRZW"
BASE="$R/campaign-finish/base-stage"
PR="$R/campaign-finish/proto-stage"
CONTROL="$R/campaign-finish/control-stage"
PROFILES="$R/campaign-finish/teardown/profiles"
BASE_BENCH="/home/sheng/Documents/Codex/2026-10-24/task-3/campaign-finish/base-build/release/deps/backend_matrix_tkrzw-6df5c85cf36edf5d"
PR_BENCH="/home/sheng/Documents/Codex/2026-10-24/task-3/campaign-finish/proto-build/release/deps/backend_matrix_tkrzw-6d7c8325b0fda4f3"
DATA_ID=$(tools/generate-function-order.py --print-identity "$DATA")
# Each pass uses the freshly installed unordered inputs and their static archives.
for pass in a b; do
tools/generate-function-order.py --library "pinyin=$R/campaign-finish/proto-unordered/lib/libpinyin.so.15.0.0" --library "zhuyin=$R/campaign-finish/proto-unordered/lib/libzhuyin.so.15.0.0" --staticlib "pinyin=$R/campaign-finish/proto-build/x86_64-unknown-linux-gnu/release/libpinyin.a" --staticlib "zhuyin=$R/campaign-finish/proto-build/x86_64-unknown-linux-gnu/release/libzhuyin.a" --bisect "$R/campaign-finish/bisect" --system-dir "$DATA" --system-identity "$DATA_ID" --profiles-dir "$PROFILES" --work-dir "$R/campaign-finish/generation-work" --cpu 2 --output-dir "$R/campaign-finish/policy-$pass"
done
# Store driver (base/prototype paths identify the hashed benchmarks above):
tools/bisection/store-ops.py --base "$BASE_BENCH" --candidate "$PR_BENCH" --data "$DATA" --work "$R/campaign-finish/store-work" --profile --keep-profiles --cpu 2 --out "$R/campaign-finish/records/store-operation.jsonl"
# Four-arm facade driver:
tools/measure-function-order.py --subject "base=$BASE" --subject "pr=$PR" --subject "control=$CONTROL" --subject "pin=$PIN" --function-order "base/pinyin=$R/base/tools/packaging/function-order/tkrzw/libpinyin.order" --function-order "base/zhuyin=$R/base/tools/packaging/function-order/tkrzw/libzhuyin.order" --bisect "$R/campaign-finish/bisect" --system-dir "$DATA" --system-identity "$DATA_ID" --profiles-dir "$PROFILES" --work-dir "$R/campaign-finish/measure-work" --cpu 2 --rounds 20
```

The as-run store profiler divisor is 10 (minimum one iteration); the
runner uses its published per-operation iteration defaults. The facade
native phase uses 20 rounds plus two warmups with rotating four-arm order.

All three phases ran sequentially through the normal heavy admission
wrapper, not concurrently. `$DATA_ID` is the existing generator's
`--print-identity "$DATA"` result. The shell wrappers above provide the
exact uv-managed interpreter invocation and the ordered install commands;
they remain local. No new workload is run by producing this report.

### Stripped sizes and exact reproduction commands

- stripped bytes base pinyin 2487016
- stripped bytes base zhuyin 2245352
- stripped bytes pr pinyin 2499472
- stripped bytes pr zhuyin 2249616
- stripped bytes control pinyin 2499472
- stripped bytes control zhuyin 2249616
- stripped bytes pin pinyin 867216
- stripped bytes pin zhuyin 838520

All libraries, profiles, system data, work paths and TMPDIR were verified
ext4. The certified runtime was checked before each launch. Separate
base/prototype targets and stages prevented stale installed artifacts;
prototype imports concrete DBM constructors, no PolyDBM/C API, while base
imports the #1114 tkrzw_dbm_set path. Normal heavy admission covered each
sequential phase. Raw profiler outputs, individual timings and exact
commands remain local under $R/campaign-finish/records; none is committed,
attached, archived or uploaded. Only findings are published.

```sh
just heavy -- bash "$R/prepare-policies-finish.sh"
just heavy -- bash "$R/build-ordered-finish.sh"
just heavy -- bash "$R/run-measurements-finish.sh"
$PY -X lazy_imports=normal "$R/summarize-measurement-finish.py"
$PY -X lazy_imports=normal "$R/summarize-counters-finish.py"
```

STOP for the maintainer's decision. No PR or review. Before any future
approved PR, rebase onto then-current main and regenerate tkrzw policies
on the combined tree, including #1140; that step was not performed here.


## Full-finish campaign: restored-source policies regenerated

The maintainer released the full evaluation from source checkpoint
6e50b77ef9b380f36ca0cd6d3fb76732c094baaf, with the fast path removed and
base fixed at d7788072. Fresh outputs are isolated in $R/campaign-finish;
prior campaign and campaign-followup outputs remain intact. Normal heavy
admission was used. All checked paths, including TMPDIR, are ext4; the
probe-certified tkrzw runtime is used for both facades and the pin.

Both complete policy-generation passes succeeded and are byte-identical.
They also equal the policy bytes already folded into the code commit;
there is no changed policy blob to fold and no history was rewritten.
libpinyin.order SHA256: 419b2c2b130d2013267ed953deb01142370c5ef220dbaddb680bd2c069f92bec.
libzhuyin.order SHA256: 4396b5ea496754d12c0440920d5a7ab74f736ea9a8f2b8401277cc4faedc9c5d.
This regeneration supersedes the stale-policy warning below for this source.
Ordered builds and the full measurement campaign are next; no result is
claimed for them here. GNU ld 2.47 supports section ordering on this host.

```sh
just heavy -- bash "$R/prepare-policies-finish.sh"
cmp "$R/campaign-finish/policy-a/libpinyin.order" "$R/campaign-finish/policy-b/libpinyin.order"
cmp "$R/campaign-finish/policy-a/libzhuyin.order" "$R/campaign-finish/policy-b/libzhuyin.order"
```


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
