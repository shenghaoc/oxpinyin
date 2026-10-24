# tkrzw user-bigram record copy (#1114)

Verified 2026-10-24 UTC (`date -u` on Ubuntu). Correctness and shipping
measurement subject: `838dabc2fb86c9b221ba3bde914584596de075da`;
base: `dee083f4ae5728319ca84244b676408ce6f91b78`; pin: `074a2219`.
Parent #1110 merged, including its final `holds_no_record` guard. #1119's
KC `#` handling remains intact. The current landing-tip reconciliation
brings only #1124's KC include-directory precedence and its findings; it
changes no tkrzw build input, load code, measurement worker or fixture.

The per-row copy now uses one synchronous set. Both facades preserve
TinyDBM bucket-chain order and saved bytes in both cross-load directions.
All three correctness cells passed. On the model-derived trained-row
fixture, init uses about 4.5% fewer instructions and 2.8% less time, with
identical peak heap to base. The 100,000-row fixture reduces init time
13.5–14.0%. Detailed scopes and controls follow; these are not claims
about a private profile or facade-driven training.

An earlier Ubuntu build shared a Cargo target directory between source
trees and reused base objects for the PR. Its installed libraries still
called `ProcessMulti`; all its putative PR measurements are invalid and
excluded. The valid build uses separate base/subject Cargo targets and
shipping stages, asserts distinct library bytes, and verifies the actual
load disassembly's call to `tkrzw_dbm_set` in both facades. Layout checks
alone did not establish source identity.

## Source and ordering proof

The pinned libpinyin source was read with `git show 074a2219:PATH` in
`/home/sheng/repos/libpinyin`. The tkrzw source was read from upstream
at `9ee84164cf45e441aa8186418c03a02c9b89f08b`, which is the revision
recorded by the host's cached runtime. These are source conclusions,
not gate results.

At the pin, `src/storage/ngram_tkrzwdb.cpp:48-63` constructs a fresh
default `TinyDBM` and copies a read-only `HashDBM` through
`src/storage/tkrzwdb_utils.h:48-74`: `First`, then `Get`, `Set`, `Next`
for every row. In oxpinyin, `persistence::load_user_bigram_db` delegates
to `TkrzwUserBigramDb::load_db`. The latter retains its source iterator
and #1110's final `holds_no_record` check, including the independent
file-size guard. Only its destination insertion changes.

The previous destination path builds owned key/value vectors and a
single-element `ProcessMulti` batch. The new `db_set_record` lends the
callback's slices to `tkrzw_dbm_set` with validated lengths and overwrite
enabled. The slices stay alive until the synchronous destination call
has copied them. Neither path retains a reference into the source.
Transaction batches, ordinary stores, removals and saves keep their
existing paths.

In tkrzw, `tkrzw_langc.cc:684-703` sends `tkrzw_dbm_set` through
`PolyDBM::Set` (`tkrzw_dbm_poly.cc:604-609`) to `DBM::Set`.
`DBM::RecordProcessorSet` returns the supplied value on both a miss and
an overwrite (`tkrzw_dbm.h:178-205,1123-1132`). `TinyDBMImpl::Process`
and `ProcessMulti` reach the same `ProcessImpl` with the same key,
bucket, value and writable flag (`tkrzw_dbm_tiny.cc:279-311`).

Let S be the source iterator's record sequence. HashDBM yields buckets
in increasing index and uses a `std::set` of byte-string keys within
each bucket (`tkrzw_dbm_hash.cc:228,1836-1886`). Both implementations
consume exactly S; neither sorts, batches across rows or changes the
source bucket count. For any destination bucket b, let S_b be S filtered
by the destination's hash. Each previously absent key prepends to b
(`tkrzw_dbm_tiny.cc:674-686`). Induction over S therefore gives the same
records and the same chain reverse(S_b) after each insertion. This also
covers source files whose bucket count differs from TinyDBM's default:
both paths still consume the same S and filter with the same destination
hash. An overwrite changes the value without changing chain position
(`:638-669`). Native iteration follows bucket then chain (`:730-755`),
so the later save receives the same ordered record sequence, not merely
the same record set.

`loading_preserves_the_pins_colliding_bucket_chain_and_values` uses
three four-byte keys derived from `HashMurmur`'s four-byte case and
`PrimaryHash`'s fold (`tkrzw_hash_util.cc:27-58`,
`tkrzw_dbm_common_impl.h:36-44`). Tokens `0x01000d46`, `0x01004404`,
and `0x01004dd8` hash to default bucket 74121. The expected chain is
`0x01004dd8`, `0x01000d46`, `0x01004404`. The test checks that sequence
and all values against the previous insertion path, including an empty
value, embedded NUL bytes and an 804-byte value. Those are opaque-copy
test inputs, not a claimed realistic trained-profile distribution.
The test passed in the cloud tkrzw store gate at `838dabc2`.

Both paths retain O(n) record storage. The new path removes temporary
owned key/value copies and batch allocations; it introduces no retained
structure. The same-host measurements below establish lower time with
unchanged peak heap. Any time-for-memory trade remains a STOP.

## Saved-byte comparison

The maintainer's clarified ruling authorizes exactly zero-based bytes
48-55: the eight-byte `META_OFFSET_TIMESTAMP` field. It supersedes the
earlier wording about bytes 53-56. Every other byte and bucket-chain
order must match.

The source field is `HashDBMImpl::timestamp_`, located by
`META_OFFSET_TIMESTAMP = 48` (`tkrzw_dbm_hash.cc:46`).
`SaveMetadata` serializes eight bytes there (`:1296`), so the complete
field spans zero-based offsets 48-55. Creation, synchronization and close
assign `GetWallTime() * 1000000` (`:749,1184,1241`).
`META_OFFSET_DB_TYPE = 56` (`:47`), so zero-based offset 56 belongs to
`db_type_`, not the timestamp. It remains exact, as do cyclic-magic bytes,
the rest of the header, bucket pointers, record links and payloads.
This authorization masks no other field or file.

`contract-diff.py::tkrzw_saved_bigram_bytes` masks only the named field;
`tkrzw_saved_bigram_equal` additionally requires exact ordered bucket
chains. Neither changes an existing contract's file-comparison rules.
The chains must be supplied from native iteration, never sorted row
sets. The Rust collision regression separately checks that native walk.

The six `TkrzwSavedBigramTests` pass without builds or oracle calls.
They flip every bit at every outside-field byte in a synthetic 512-byte
buffer (4,032 rejected mutations), explicitly check offset 56 and both
cyclic-magic bytes, require length identity, reject truncated metadata,
accept changes confined to 48-55, and reject chain permutations even
when they contain the same keys and the saved bytes match. Command:

```sh
UV_CACHE_DIR=/home/sheng/Documents/Codex/2026-10-23/task-19/uv-cache \
TMPDIR=/home/sheng/Documents/Codex/2026-10-23/task-19/tmp \
uv run --offline --no-project --managed-python --python cpython@3.15+gil \
  python -B -X lazy_imports=normal tools/bisection/contract-diff.test.py \
  TkrzwSavedBigramTests
```

At `838dabc2`, both facade contract cases passed pin-to-subject and
subject-to-pin collision-profile load/save/reopen checks. The saved files
matched outside exactly this field in both directions. Native iteration
checks the ordered chains independently; logical row sets cannot substitute
for this proof.

## Trained-size fixture

A `SingleGram` consists of a four-byte total and eight bytes per
distinct successor (`src/storage/ngram.cpp:31-40` at 074a2219).
Training creates or loads the user row and adds a successor on its
first occurrence (`src/lookup/phonetic_lookup.h:876-909`). Thus trained
value size is 4 + 8d for successor degree d, rather than always 12.
The prepared trained-size fixture uses the verified public model's first
10,000 predecessor rows in file order, with every distinct successor of
each selected row. Each successor receives the pin's first-event seed 69;
the row total is 69d. This models the row shape after one training event
per public-model pair, derived from source; it is not an observed private
user profile or a claim that the facade trained these rows. The public
`interpolation2.text` SHA-256 is
`e1578307719431e1b8ef1006ae265bbc9a447250bfde4c1b6e15be80afd3a118`.
Its 10,000 values total 2,442,984 bytes: minimum 12, median 52, p90 428,
p99 3,468, maximum 65,108 bytes. The HashDBM file is 6,782,584 bytes.
The 0/10,000/100,000 single-item synthetic fixtures are 4,198,400 /
4,438,400 / 6,598,400 bytes. All were written through certified tkrzw.
No private user profile was accessed. Local orchestration uses the
existing native teardown worker, not a new differential driver.

## Correctness gates

The cloud lane completed these at exact head `838dabc2`, before Ubuntu
measurement. Setup ran unchanged with `tools/cloud/setup.sh --cells
"bdb kc tkrzw"`; its pinned hashes and data manifests passed. No setup
change or dependency was introduced.

| Cell | store unit / trait tests | MATCH | SKIP | HOST-DEPENDENT | DIFFER | Total |
|---|---:|---:|---:|---:|---:|---:|
| BDB | 50 / 7 | 2,893 | 2 | 4 | 0 | 2,899 |
| KC | 43 / 7 | 2,888 | 7 | 4 | 0 | 2,899 |
| tkrzw | 52 / 7 | 2,890 | 5 | 4 | 0 | 2,899 |

Same-data passed 15/15 cases on each cell. The full harness passed 106
tests. Touched-crate clippy passed on all three cells; fmt and diff checks
passed. The store gate is `just heavy -- cargo test --locked -p
oxpinyin-store --no-default-features --features CELL`, with `kyotocabinet`
for KC; clippy uses the same package/features and `--all-targets -- -D
warnings`. The existing differentials are `tools/bisection/run-contract-diff.sh
CELL PREFIX PINYIN_SO ZHUYIN_SO` and
`tools/bisection/run-same-data-dir-diff.sh PIN_SO PINYIN_SO DATA_DIR`
with each cell's matching pin and data.
The Python gate is `tools/bisection/contract-diff.test.py` through the
managed-Python invocation above. No new differential driver was added.

BDB skips only `issue1114-collision-load-save-pinyin` and its `-zhuyin`
peer. KC skips those same tkrzw-only cases; tkrzw passes them. This
accounts for the entire two-case KC/tkrzw MATCH gap, rather than two KC
DIFFERs. KC and tkrzw both skip these five inherited coverage gaps:

- `abort-choose-candidate-predicted-bigram-row`: the chosen predicted row
  and abort check are implemented only for the BDB differential.
- `issue762-token-reuse-stale-bigram-{pinyin,zhuyin}` and
  `issue762-token-reuse-stale-bigram-no-mask-{pinyin,zhuyin}`: the existing
  BDB `db_dump` comparison verifies actual successor rows; the peer
  checker only verifies presence and cannot establish stale-row removal
  or preservation. These remain explicit SKIPs, not passes.

All cells' four HOST-DEPENDENT cases are the existing scanf exceptions
for `1e` and `0x1p`: `table-conf-lambda-exponent-without-digits`,
`table-conf-lambda-hex-exponent-without-digits`, and their `-zhuyin` peers.
No exception or comparison tolerance changed.

AGENTS rule 2's path checks: the store-path check lists
`crates/oxpinyin-store/build.rs` and `crates/oxpinyin-store/src/tkrzw/mod.rs`;
the non-Markdown backend-feature grep lists only
`crates/oxpinyin-store/build.rs`; the
Cargo.toml backend-line check has no output. Backend code therefore
requires all three cells. Final findings-only edits use rule 7's docs
case (fmt and diff check). Measurements retain their exact source head;
post-rebase CI is reported separately in the PR.

## Ubuntu measurement

One Ubuntu x86-64 host, Intel Core i7-9750H, Linux 7.3.0-9-generic;
2026-10-24 UTC. #1031's no-job handoff preceded admission. Builds and
profilers ran through the existing `just heavy --` slot, without overlap.
The canonical compatibility image
`localhost/oxpinyin-compat:1b19799961bb6fbe` (ID
`b545b40c9106b256bf6e4d3ddaf1249d9a31a29cdd058c321e6e66f27afb13f1`)
reported GNU ld 2.47 and Rust 1.97.1. Each source used its own
`CARGO_TARGET_DIR` and install prefix. For each facade, the build was:

```sh
tools/packaging/install.sh libpinyin --prefix="$ARM_PREFIX" -- \
  --locked --release --no-default-features --features tkrzw,shipped
tools/packaging/install.sh libzhuyin --prefix="$ARM_PREFIX" -- \
  --locked --release --no-default-features --features tkrzw
```

The container used the verified tkrzw prefix's pkg-config and runtime,
not the packaged Ubuntu runtime. Every build and launch resolved and
probe-certified `libtkrzw.so.1.77.0` from revision
`9ee84164cf45e441aa8186418c03a02c9b89f08b`. All six base/PR/control copies
passed shipping-order checks (260 pinyin and 240 zhuyin policy paths).
Both source worktrees, isolated build/output stages, every measured
library, resolved runtime, system-data file, fixture-profile file, native
worker, work directory and home-disk TMPDIR passed `findmnt -T` as ext4
(`/dev/nvme0n1p2`). No private profile was accessed.

The local operator reuses `teardown.py::build_worker`, `Campaign.run`,
`massif_profile`, `stable_control`, `paired_ratio`, and the existing
`bisect.c` / `teardown.h` worker; it introduces no repository runner.
Arms are base, PR, a byte-identical PR copy, and pin, in equal-length
seven-character directories. Every sample restores the fixture to one
scratch path; arms rotate each round and use one CPU from the process's
affinity mask. Runtime certification precedes every launch. Commands use
`uv run --offline --no-project --managed-python --python cpython@3.15+gil
python -B -X lazy_imports=normal` with task-owned `UV_CACHE_DIR` and
home-disk `TMPDIR`.

Init Ir uses `generate-function-order.py::callgrind_prefix(worker, cpu,
facade + "_init")` then `--teardown FACADE LIB DATA USER time 0 1 1
first-candidate -`. This is the C init export, excluding ELF constructors,
dlopen and the controller. `contract-diff.py::per_call_profile` reconciles
the exclusive sum. Three rotated repeats are bit-exact for every arm and
fixture, and PR equals its same-bytes control exactly in every repeat.
Massif uses `Campaign.run(..., mode="memory", profiler="massif")` with
the Massif-instrumented worker, three rotated repeats, and exact heap
controls. A first local orchestration attempt mistakenly selected timing
mode for Massif's checkpoint reader and stopped without a heap result;
the corrected continuation retains all eight completed exact Ir controls.
No control was relaxed or result discarded for failing a control.

Timing uses the normal worker and `Campaign.run`'s timing mode: three
warmups and 60 rotated fresh-process samples per arm. The existing
bootstrap 95% control interval must lie entirely within [0.95, 1.05].
The retained summaries report all eight wall-time and eight CPU-time
controls passing; the missing samples prevent independent recalculation. Ratios below are
paired-ratio medians, not quotients of marginal medians. Values include
all timed samples; outliers were not removed. Peak heap means Massif's
whole-lifecycle maximum `mem_heap_B`, not RSS or cumulative allocation.

### Exact as-run commands and retained evidence

The [published sources](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380222481) include the initial controller
recovered verbatim from its retained creation execution record, the corrected
continuation actually run, and its preparation and attribution helpers.
The [evidence index and integrity hashes](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380230973) map commands
to surviving outputs. These concern subject `838dabc2` and base
`dee083f4`; this documentation amendment performs no new measurement.
The local `oracle.env` configuration was not published.

Commands A, B and C below are copied from that execution record. Their
recorded working directory was
`/home/sheng/Documents/Codex/2026-10-23/task-19/oxpinyin-1114`.
Command A uses the initial controller version and exits 1 after completing
all eight init-Ir controls, at the first Massif checkpoint parse.
Command B uses the corrected controller and resumes with
`--memory-time-only`, retaining those completed Ir results.

**Command A — init Ir, per-row slopes and function attribution:**

```sh
/bin/bash -lc 'set -e
source /home/sheng/Documents/Codex/2026-10-23/task-19/oracle.env
export UV_CACHE_DIR=/home/sheng/Documents/Codex/2026-10-23/task-19/uv-cache TMPDIR=/home/sheng/Documents/Codex/2026-10-23/task-19/tmp
export OXPINYIN_TKRZW_LIBRARY=/home/sheng/.cache/oxpinyin/tkrzw/lib/libtkrzw.so.1.77.0
just heavy -- uv run --offline --no-project --managed-python --python cpython@3.15+gil python -B -X lazy_imports=normal /home/sheng/Documents/Codex/2026-10-23/task-19/measure_verified1114.py > /home/sheng/Documents/Codex/2026-10-23/task-19/measurement-838d.local.log 2>&1'
```

Its retained `measurement-838d.local.log` is published in
[part 1](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380223999) and [part 2](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380224517).
The file contains 96 `init_toggle_ir` observations, eight exact-control
records and 194 function-attribution records. These are extracted totals
and reader output; the complete Callgrind streams were consumed in memory
and were not retained.

**Command B — peak heap, wall and CPU timing:**

```sh
/bin/bash -lc 'set -e
source /home/sheng/Documents/Codex/2026-10-23/task-19/oracle.env
export UV_CACHE_DIR=/home/sheng/Documents/Codex/2026-10-23/task-19/uv-cache TMPDIR=/home/sheng/Documents/Codex/2026-10-23/task-19/tmp
export OXPINYIN_TKRZW_LIBRARY=/home/sheng/.cache/oxpinyin/tkrzw/lib/libtkrzw.so.1.77.0
just heavy -- uv run --offline --no-project --managed-python --python cpython@3.15+gil python -B -X lazy_imports=normal /home/sheng/Documents/Codex/2026-10-23/task-19/measure_verified1114.py --memory-time-only > /home/sheng/Documents/Codex/2026-10-23/task-19/memory-time-838d.local.log 2>&1'
```

Its retained `memory-time-838d.local.log` is published in
[part 1](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380225156) and [part 2](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380225715).
It contains 96 extracted heap peaks, eight heap-control records and
sixteen timing summaries. Complete Massif streams were not retained.
Individual timing samples were held only in the controller's `samples`
dictionary: no per-sample timing JSON survives. Thus the timing cells
match retained summaries, but their medians, bootstrap intervals and
timing-control validity cannot be independently recalculated from
surviving samples. No samples or profiler streams have been reconstructed.

**Command C — stripped sizes and unstripped SHA-256 identities:**

```sh
/bin/bash -lc "find tools -iname '*resource*' -o -iname '*heavy*'; python3 - <<'PY'
from pathlib import Path
import subprocess,hashlib
op=Path('/home/sheng/Documents/Codex/2026-10-23/task-19')
for side in ('base','subject'):
 for facade in ('pinyin','zhuyin'):
  p=op/'verified-libraries'/side/'lib'/f'lib{facade}.so'
  src=p.resolve()
  dst=op/'tmp'/f'1114-{side}-{facade}.stripped.so'
  subprocess.run(['objcopy','--strip-unneeded',str(src),str(dst)],check=True)
  print(side,facade,'original',src,'sha256',hashlib.sha256(src.read_bytes()).hexdigest(),'stripped_bytes',dst.stat().st_size)
  subprocess.run(['findmnt','-n','-o','FSTYPE,SOURCE,TARGET','-T',str(dst)],check=True)
PY"
```

The [retained command output](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380230973) records all four exact
input/output expansions, their unstripped SHA-256 identities and stripped
byte counts. The subprocess is exactly
`subprocess.run(['objcopy','--strip-unneeded',str(src),str(dst)],check=True)`.
Only documentation and evidence links are committed; orchestration scripts,
retained outputs and other captures remain outside Git.


The init-Ir table comes from Command A's 96 retained `init_toggle_ir`
observations and eight exact-control records in the linked two-part output.
Complete underlying Callgrind streams are unavailable; the extracted totals
and repeat controls survive.

| Facade / fixture | Init Ir base | PR | Pin | PR/base | PR/pin |
|---|---:|---:|---:|---:|---:|
| pinyin / n0 | 3,708,206 | 3,708,206 | 338,110,000 | 1.000000 | 0.010967 |
| pinyin / n10000 | 382,972,764 | 366,331,757 | 371,062,803 | 0.956548 | 0.987250 |
| pinyin / n100000 | 845,646,813 | 679,244,912 | 670,648,104 | 0.803225 | 1.012819 |
| pinyin / trained | 386,891,539 | 369,480,457 | 374,637,718 | 0.954998 | 0.986234 |
| zhuyin / n0 | 3,319,490 | 3,319,490 | 337,781,896 | 1.000000 | 0.009827 |
| zhuyin / n10000 | 382,610,175 | 365,969,057 | 370,755,416 | 0.956506 | 0.987090 |
| zhuyin / n100000 | 845,279,719 | 678,883,277 | 670,342,123 | 0.803146 | 1.012741 |
| zhuyin / trained | 386,522,022 | 369,110,301 | 374,293,141 | 0.954953 | 0.986153 |

The wall-time table comes from Command B's retained `clock="wall_ns"`
`timing` summaries in [heap/time part 1](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380225156)
and [part 2](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380225715).
The 60 individual samples per arm were not retained; no per-sample JSON
survives. The cells match summaries, but marginal medians, paired bootstrap
intervals and wall-time control validity cannot be independently
recalculated from surviving samples.

| Facade / fixture | Init wall ms base | PR | Pin | PR/base [95% CI] | PR/pin [95% CI] |
|---|---:|---:|---:|---|---|
| pinyin / n0 | 3.423666 | 3.413916 | 71.700154 | 0.994667 [0.991233, 1.001380] | 0.047473 [0.047167, 0.047794] |
| pinyin / n10000 | 83.749556 | 81.814601 | 83.402647 | 0.979590 [0.975039, 0.982624] | 0.982087 [0.977137, 0.985113] |
| pinyin / n100000 | 149.804258 | 129.484053 | 130.125520 | 0.864528 [0.860578, 0.867369] | 0.995309 [0.991316, 0.998494] |
| pinyin / trained | 87.484826 | 85.140506 | 87.113388 | 0.972285 [0.970431, 0.977692] | 0.975889 [0.973658, 0.980754] |
| zhuyin / n0 | 3.112036 | 3.109396 | 71.756384 | 0.997445 [0.993257, 1.002568] | 0.043137 [0.043036, 0.043405] |
| zhuyin / n10000 | 85.683919 | 83.375565 | 85.205516 | 0.973525 [0.969656, 0.975706] | 0.978636 [0.972532, 0.981976] |
| zhuyin / n100000 | 153.160622 | 131.777344 | 132.825768 | 0.859921 [0.856693, 0.862358] | 0.994192 [0.984798, 0.997274] |
| zhuyin / trained | 89.026520 | 86.398949 | 88.400492 | 0.971393 [0.968532, 0.973618] | 0.977078 [0.975079, 0.980892] |

The CPU-time table comes from the same exact Command B invocation with
`--memory-time-only`, using its retained `clock="cpu_ns"` summaries in
[heap/time part 1](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380225156)
and [part 2](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380225715).
Individual CPU-time samples were not retained. These medians, paired
bootstrap intervals and CPU-time control validity cannot be independently
recalculated from surviving samples; only their summary output survives.

| Facade / fixture | Init CPU ms base | PR | Pin | PR/base [95% CI] | PR/pin [95% CI] |
|---|---:|---:|---:|---|---|
| pinyin / n0 | 3.421562 | 3.414579 | 71.698101 | 0.994368 [0.991256, 1.002605] | 0.047483 [0.047242, 0.047809] |
| pinyin / n10000 | 83.602284 | 81.587031 | 83.388030 | 0.976883 [0.973365, 0.980584] | 0.979855 [0.976796, 0.983539] |
| pinyin / n100000 | 149.473247 | 129.402990 | 129.907171 | 0.864605 [0.861338, 0.866795] | 0.996275 [0.994044, 0.997799] |
| pinyin / trained | 87.426800 | 85.094882 | 87.040143 | 0.971844 [0.970822, 0.976030] | 0.977866 [0.974033, 0.980624] |
| zhuyin / n0 | 3.112512 | 3.110018 | 71.705102 | 0.997453 [0.993267, 1.002580] | 0.043189 [0.043095, 0.043460] |
| zhuyin / n10000 | 85.613981 | 83.362206 | 85.156768 | 0.974179 [0.970297, 0.975704] | 0.978997 [0.973435, 0.983299] |
| zhuyin / n100000 | 153.001231 | 131.696666 | 132.461774 | 0.859758 [0.857540, 0.862207] | 0.994490 [0.990154, 0.996877] |
| zhuyin / trained | 88.970315 | 86.365580 | 88.330239 | 0.971239 [0.968548, 0.973328] | 0.977195 [0.975594, 0.980819] |

The peak-heap table comes from Command B's 96 `heap_bytes` observations
and eight `heap_control: VALID` records in
[heap/time part 1](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380225156)
and [part 2](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380225715).
These extracted per-repeat `mem_heap_B` peaks survive; complete Massif
streams do not. The full snapshots behind the maxima cannot be re-read
from retained evidence.

| Facade / fixture | Peak heap bytes base | PR | Pin | PR/base | PR/pin |
|---|---:|---:|---:|---:|---:|
| pinyin / n0 | 14,713,440 | 14,713,440 | 17,462,056 | 1.000000 | 0.842595 |
| pinyin / n10000 | 14,973,440 | 14,973,440 | 17,719,595 | 1.000000 | 0.845022 |
| pinyin / n100000 | 17,313,440 | 17,313,440 | 20,062,020 | 1.000000 | 0.862996 |
| pinyin / trained | 17,299,080 | 17,299,080 | 20,047,660 | 1.000000 | 0.862898 |
| zhuyin / n0 | 14,154,271 | 14,154,271 | 16,885,255 | 1.000000 | 0.838262 |
| zhuyin / n10000 | 14,414,271 | 14,414,271 | 17,142,794 | 1.000000 | 0.840836 |
| zhuyin / n100000 | 16,754,271 | 16,754,271 | 19,485,255 | 1.000000 | 0.859844 |
| zhuyin / trained | 16,739,911 | 16,739,911 | 19,549,493 | 1.000000 | 0.856284 |

Per-row slopes use Command A's retained pinyin `n10000` and `n100000`
`init_toggle_ir` totals in [Ir part 1](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380223999)
and [part 2](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380224517).
For each arm, the calculation is `(Ir100000 - Ir10000) / 90000`;
`PR_slope / pin_slope - 1` gives the residual percentage. These aggregate
calculations are recoverable from the retained totals, although complete
Callgrind streams are unavailable.

There is no measured time-for-memory trade: peak heap equals base in
all eight rows, while nonempty-fixture time improves. The empty shortcut
is unchanged. Synthetic per-record slopes use the 10,000-to-100,000
interval, avoiding #1110's empty shortcut: pinyin base/PR/pin
5,140.823 / 3,476.813 / 3,328.726 Ir per additional row. PR still costs
4.45% more instructions per extra synthetic row than pin; whole-init
ratios do not erase that residual.

The call counts and inclusive/exclusive Ir below come from Command A's
194 retained `attribution` records (first trained repeat, both facades,
all four arms) in [Ir part 1](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380223999)
and [part 2](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380224517).
Those extracted function records survive; the complete underlying
Callgrind streams do not, so attribution cannot be independently re-read
from the full original profiles. Inclusive costs overlap and must not
be summed.

On the trained-shape pinyin fixture, 10,000 `TinyDBM::ProcessMulti` and
`HashMutex::LockMulti` calls disappear. `malloc` calls fall from
138,707 to 28,707 (pin 28,533); zhuyin falls from 138,121 to 28,121
(pin 27,396). Both remove 11 calls per row. Pinyin's source
`HashDBMImpl::ProcessImpl` inclusive Ir falls from 39,326,022 to
21,912,460 (pin 14,914,073); its exclusive Ir stays 1,489,922.
The reduction is in the copy callback beneath it. The destination
`TinyDBMImpl::ProcessImpl` has identical 701,216 exclusive Ir and 10,000
calls across all three, while PR's C `tkrzw_dbm_set` accounts for
520,000 exclusive / 8,075,968 inclusive Ir. The remaining Rust callback,
C wrapper, status handling and PolyDBM dispatch differ from the pin's
direct C++ iterator/Set path. Their source explains a residual, rather
than establishing an allocation regression or claiming pin-equivalent
per-record instruction cost. Inclusive figures overlap and must not be
summed.

Shipping sizes and the SHA-256 table below come from exact Command C
above, including its `objcopy --strip-unneeded` subprocess and concrete
path expansions. The [published retained size/hash output](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380230973)
records all four objects; these figures are tied to that output rather
than inferred from a new strip operation. Pinyin base/PR are both 2,487,008 bytes;
zhuyin grows from 2,241,248 to 2,245,344 bytes (+4,096, 0.183%).
The unstripped measured SHA-256 identities are:

| Facade | Base | PR |
|---|---|---|
| pinyin | `b5c3cea8f5966663b316d1fe377eaa868146765ac0adcf0e66282403cb69db94` | `108437fa47999a2c77970b61e5f154408e4c33b252b43a23379c692b70b7bdf6` |
| zhuyin | `84ef1c5db5946357fb622f7c11ca05c4767f4d7909bbc47d203ac73edbf0df5a` | `4251b9412bff33d60efb589d30abb63c173986b311cf9913826b7cbf3fa6dac8` |

The as-run orchestration sources are now [published on #1130](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380222481),
with [Ir totals and attribution](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380223999),
[heap peaks and timing summaries](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380225156),
and the [full evidence index, second parts, commands and integrity hashes](https://github.com/shenghaoc/oxpinyin/pull/1130#issuecomment-6380230973).
No orchestration script, output log, archive or capture is committed.
Complete profiler streams and individual timing samples were not retained;
the per-figure limitations above remain part of the measurement record. No C ABI, frozen contract, persistent
format, coefficient, dependency or permitted timestamp mask changed.
