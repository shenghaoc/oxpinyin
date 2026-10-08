# Tooling lane: gate evidence and scoped follow-up

Date: 2026-10-08 UTC (captured from the executing host).

The stack changes only tooling and documentation. Parity artifacts use the dev
profile with `CARGO_PROFILE_DEV_OPT_LEVEL=1`, one retained target per cell. The
eight measurement recipes remain unchanged and outside the gate set. No oracle
output cache, shipped crate, dependency manifest or CI policy was changed.

## Cumulative evidence after the selected sweep reruns

Date: 2026-10-08 UTC (captured from the executing host).

This is **not a new full-suite run**. Only `transformed-options-sweep` was
executed, sequentially KC, Tkrzw, then BDB, at retained execution snapshot
`6192bd33bd712f8134281ca1856d1151a8955baa`. The final published findings differ
from that snapshot only in documentation. All other 138 rows retain their
original result, elapsed time and pin ref from the once-only full sweep at
`ccea6eb7b2d700b67d09f5ca942374aa8935e428`, 2026-10-07
14:15:38–14:53:32 UTC. The three earlier 900-second timeout results remain
historical evidence below; only their current table rows are superseded.

| Selected cell | Result | Measured seconds | Exit | UTC start | UTC end |
| --- | --- | ---: | ---: | --- | --- |
| kc | PASS | 1263.426 | 0 | 2026-10-07T17:04:50Z | 2026-10-07T17:25:54Z |
| tkrzw | PASS | 1110.761 | 0 | 2026-10-07T17:25:54Z | 2026-10-08T00:16:19Z |
| bdb | PASS | 1798.929 | 0 | 2026-10-08T00:16:19Z | 2026-10-08T00:49:44Z |

| Cell | Cumulative PASS | FAIL | SKIPPED |
| --- | ---: | ---: | ---: |
| bdb | 47 | 0 | 0 |
| kc | 47 | 0 | 0 |
| tkrzw | 47 | 0 | 0 |

All three selected runs exited zero and reported ordinary/complete mismatch
counts 0/0 for Hanyu, Double-MS, Chewing-standard, Luoma and secondary. No
FAIL remains in the cumulative table; no outcome was changed or waived.

Commands, once for each CELL in the order `kc tkrzw bdb`, from the isolated
runner worktree in retained `oxpinyin-timing-20261007`:

```bash
export LC_ALL=C CARGO_PROFILE_DEV_OPT_LEVEL=1
unset CARGO_TARGET_DIR PINYIN_ORACLE_PREFIX OXPINYIN_SYSTEM_DIR
bash tools/bisection/run-all.sh --runners transformed-options-sweep \
  --cells CELL --jobs 1 --no-build --oracle-root /oracle \
  --target-root /lane/work/cells --json RESULTS_JSON
```

There is no command-line deadline override: the registry supplies 3600 seconds.
The retained per-cell dev-opt1 artifacts were reused because shipped crates,
Cargo manifests/lock and toolchain are unchanged from their verified build.
BDB was explicitly required: the timing run used an external instrumented
copy and separate timing target; the registry uses the uninstrumented
repository driver and ordinary cell target. Neither path was substituted for
the other. The driver cases/expectations remain identical to main `8e76b17`.

Tkrzw's original process survived host suspend from 2026-10-07 17:26:30 to
23:59:32 UTC. Its measured duration/timeout use the runner's monotonic clock;
the calendar interval therefore is not comparable to uninterrupted elapsed.
BDB also survived suspend on 2026-10-08 from 00:43:14 to 00:47:48 UTC.
Neither process was restarted. Monitoring was interrupted, but per-cell
results and logs completed independently; the outer orchestration log became
stale, so terminal status is taken from each cell's result/exit/end files.
Host-load observations have a disconnect gap and resumed afterward. These
selected-runner durations are gate evidence, not controlled benchmarks.

The [updated cumulative table](https://github.com/shenghaoc/oxpinyin/pull/675#issuecomment-6031878566)
labels every row's provenance. The timing archive remains withheld for the
home-directory paths described below. No other runner deadline, gate behavior,
case, expectation, shipped code, host setting or CI policy changed.

## Transformed-options timing and scoped deadline follow-up

Date: 2026-10-07 UTC (captured from the executing host).

The maintainer accepted added successful decoding work, with no demonstrated
scheme-specific penalty. #674 changes only `transformed-options-sweep` from
900 to 3600 seconds and records the approximately 1,766-second isolated BDB
measurement at `8e76b17`, dev opt-level 1. Every other registry deadline is
unchanged. The three historical 900-second timeouts below remain evidence;
this follow-up replaces only those runner rows, not the whole-suite provenance.

### Isolated BDB measurements

Both revisions used the same pinned BDB oracle/data, 470 option words and 11
inputs, plus the driver's targeted checks. Each full sweep had one worker and
no aggregate deadline; the original internal worker timeout was unchanged.

| Revision | Elapsed seconds | Exit | Ordinary/complete mismatches |
| --- | ---: | ---: | --- |
| `044bfb318ee750127bbfdb5edff8f355039b4032` | 510.697807148 | 0 | Luoma 437/439; secondary 437/439; other schemes 0/0 |
| `8e76b17de37d632d99589965aface6c8ac1ac319` | 1765.696517355 | 0 | All five schemes 0/0 |

The newer full run is 3.457419 times the older duration. Its non-overlapping
case-section times include parse/decode, candidate enumeration and instance
cleanup, including targeted checks:

| Scheme | Pin seconds | Subject seconds | Combined seconds |
| --- | ---: | ---: | ---: |
| Hanyu | 1.406682223 | 162.455086304 | 163.861768527 |
| Double-MS | 0.559331334 | 33.257426350 | 33.816757684 |
| Chewing-standard | 0.413442844 | 31.610433647 | 32.023876491 |
| Luoma | 13.975991379 | 1277.419018858 | 1291.395010237 |
| Secondary | 1.715526581 | 162.682269227 | 164.397795808 |

Shared observe setup/teardown adds 3.857895851 seconds; time outside observe
adds 76.343412757 seconds. These categories sum to full elapsed time without
counting overlapping work twice. Luoma plus secondary account for 82.4486%.
Subject nonempty cases rose from 623 to 2808 of 2820 for Luoma, and from 31
to 468 of 470 for secondary. Candidate enumeration counts rose from 340929
to 2729671 and from 23728 to 380124, respectively. Their combined case time
rose from 134.008791256 to 1455.792806045 seconds, while the other scheme
totals decreased. This supports additional now-successful decoding work.

Build commands, run sequentially in each detached revision worktree:

```bash
export LC_ALL=C CARGO_PROFILE_DEV_OPT_LEVEL=1
CARGO_TARGET_DIR=/lane/timing-evidence-20261007/targets/before-bdb \
  cargo build --locked -p oxpinyin-capi --no-default-features --features bdb
# In the newer worktree, use targets/after-bdb instead.
```

External observational driver copies inserted per-case monotonic timers and
recorded setup/teardown separately. Removing only inserted AST nodes restored
the original AST; the original observe function was identical across revisions.
No tracked case, comparison or expected result was changed. The retained
`full_sweeps.py` ran the older command followed by the newer command; after an
external container stop, `full_after_repeat.py` repeated only the interrupted
newer sweep in the dedicated container:

```bash
python3 /lane/timing-evidence-20261007/instrumented/before/transformed-options-sweep.py \
  --oracle /oracle/bdb/lib/libpinyin.so \
  --subject /lane/timing-evidence-20261007/targets/before-bdb/debug/libpinyin_capi.so \
  --data /oracle/bdb/lib/libpinyin/data --expect fixed
python3 /lane/timing-evidence-20261007/instrumented/after/transformed-options-sweep.py \
  --oracle /oracle/bdb/lib/libpinyin.so \
  --subject /lane/timing-evidence-20261007/targets/after-bdb/debug/libpinyin_capi.so \
  --data /oracle/bdb/lib/libpinyin/data --expect fixed --jobs 1
```

### Matched-input sample and limits

The fixed 20-word sample was `0x8, 0xc, 0x18, 0x28, 0x108, 0x18a, 0x408,
0x808, 0x2008, 0x4008, 0x10008, 0x40008, 0x80008, 0x400008, 0x800008,
0x2000008, 0x4000008, 0x10000008, 0x20000008, 0x80000008`: `0x28` plus
19 equally spaced ranks among committed words with INCOMPLETE (`0x8`) and
without FORCE_TONE (`0x40`). It samples active comparable decoding, not the
frequency distribution of all 470 words. Fresh workers/contexts ran
sequentially, with rotated family order and alternating pin/subject order:

```bash
podman exec oxpinyin-timing-20261007 \
  python3 /lane/timing-evidence-20261007/sample_timing.py
# Each worker uses the retained instrumented newer driver:
python3 "$DRIVER" --worker "$LIBRARY" /oracle/bdb/lib/libpinyin/data \
  "$WORD" "$CASES_JSON"
```

Luoma inputs `lishihbakua,chih,rih,sih,zih,shih` match Hanyu
`lishbakua,ch,r,s,z,sh`; secondary `tsz` matches Hanyu `c`, from the pinned
parser indexes. All 160 workers exited zero. Observations matched in 120/120
Luoma/control and 20/20 secondary/control pairs on each side, excluding only
spelling-dependent consumed/parsed byte lengths. Means sum six case sections
per Luoma word and one per secondary word:

| Family | Pin mean seconds | Subject mean seconds | Subject/pin |
| --- | ---: | ---: | ---: |
| Luoma, raw 20 words | 0.027995609 | 5.817082894 | 207.785549 |
| Matched Hanyu for Luoma | 0.026575842 | 2.386331631 | 89.793267 |
| Secondary, raw 20 words | 0.004019771 | 0.534221051 | 132.898374 |
| Matched Hanyu for secondary | 0.003749125 | 0.524279208 | 139.840418 |

The host suspended at 16:27:37 UTC and resumed at 16:47:08 UTC, after the
full newer sweep finished at 16:26:54 UTC. This overlaps the `0x408` Luoma
subject worker: recorded wall 71.641445629 seconds, case total 71.574644062,
and `lishihbakua` alone 69.046344624. Raw results remain unchanged. Monotonic
elapsed is not CPU time and does not equal calendar elapsed across suspend;
no suspend-duration subtraction was applied.

A labeled sensitivity calculation excludes the entire `0x408` word from all
families and both sides, not just the outlier. Remaining 19-word means:

| Family | Pin case seconds | Subject case seconds | Subject/pin |
| --- | ---: | ---: | ---: |
| Luoma | 0.027545095 | 2.356158622 | 85.538230 |
| Matched Hanyu for Luoma | 0.026387237 | 2.374867125 | 90.000597 |
| Secondary | 0.003592542 | 0.477133684 | 132.812265 |
| Matched Hanyu for secondary | 0.003333283 | 0.471599005 | 141.481851 |

Subject transformed/control ratios are 0.992122 (Luoma) and 1.011736
(secondary); pin ratios are 1.043879 and 1.077779. Raw 20-word subject ratios
are 2.437667 and 1.018963; the former is contaminated. The dev-opt1 subject
has a large general pin-relative cost also present in Hanyu controls; this
sample does not demonstrate an additional transformed-scheme penalty, nor
prove its absence for every option or for release builds.

TeX Live installation and ordinary desktop load were present, explicitly
accepted by the maintainer. Retained repeat/sample vmstat intervals had
80/87/94% minimum/median/maximum CPU idle, with swap and I/O traffic. Full
elapsed includes observational overhead: measured bookkeeping 0.046632883
seconds and metric serialization/first writes 0.194739520 seconds; timer,
imports and second-write overhead were not fully calibrated. There was no
concurrent benchmark from this task; this is not a quiet-host performance
claim. The prior aggregate used four runner slots but one worker per sweep:
BDB/KC overlapped about 67.409 seconds and KC/Tkrzw 586.086 seconds, with no
three-way overlap, based on retained launch-log times and result durations.

### Container stop and evidence retention

The interrupted first newer attempt is retained separately (461 completed
pin/subject pairs plus one pin record). Podman events recorded exit 137,
`StoppedByUser=true`, `OOMKilled=false`. Journal command attribution points
to `podman --log-level error system migrate`, UID 1000 from Konsole, during
the same sequence that stopped both containers. `loginctl show-user sheng -p
Linger` returned `Linger=no`; no session-end/user-manager shutdown was found.
The checks were read-only. The shared container stayed stopped. The dedicated
`oxpinyin-timing-20261007` uses the same image ID and mounts; the retained
installed runtime was copied from the stopped source without rebuilding the
subjects. No host settings changed.

All 6907 evidence archive members, including regular-file contents, were
inspected for home-directory paths. Fifteen members contain such paths, so
**the archive is withheld from #675**, neither uploaded nor silently sanitized.
Raw evidence, scripts, partial attempt, targets and containers remain retained
locally. This document records commands and findings, not captures.
Historical host-specific paths below are explicitly represented by local
`WORK_ROOT` and `ORACLE_ROOT` placeholders rather than private home-directory
paths. Set them to the retained work and oracle roots when reproducing those
historical commands; exact original command captures remain local. The archive
was not sanitized or uploaded.

Rule 7: #674 is registry tooling only; JSON/syntax/diff checks and the explicitly
requested selected-runner executions cover it. No Rust crate changed, so no
crate clippy/test or full-suite re-gate applies. #675 is findings/docs only:
fmt/diff checks, no extra build or runtime rerun. Only #675 is restacked above
#674; lower members retain their heads and prior evidence. The explicit request
authorizes KC, Tkrzw and BDB despite the empty backend-change path checks.

## Historical full sweep after restack onto `8e76b17d`

Date: 2026-10-07 UTC (captured with `date -u`). The eight-member stack
was restacked, in order, onto origin/main `8e76b17d`. The existing
`contract-diff` runner was registered on #674; the registry now runs 47 gates
per cell. Main's `transformed-options-sweep.py`, `candidate-assembly-diff.c`
and `contract-diff.py` blobs are unchanged, including every case and expectation.

The [replacement table](https://github.com/shenghaoc/oxpinyin/pull/675#issuecomment-6031878566)
is attached to #675 in place of the previous 132 PASS / 6 FAIL table.
The single invocation started `2026-10-07T14:15:38Z`, finished
`2026-10-07T14:53:32Z`, and tested final runner tip
`ccea6eb7b2d700b67d09f5ca942374aa8935e428`. Aggregate exit: **1**.
That findings-count update was a subsequent **docs-only commit** on #675.
The later selected-runner follow-up above is separate from this historical
full sweep; its other 138 runner rows were not rerun.

| Cell | PASS | SKIPPED | FAIL |
| --- | ---: | ---: | ---: |
| bdb | 46 | 0 | 1 |
| kc | 46 | 0 | 1 |
| tkrzw | 46 | 0 | 1 |

**138 PASS; 3 FAIL; 0 SKIPPED.**

- `transformed-options-sweep` / `bdb`: first failing line
  `Completed transformed-options-sweep / bdb: FAIL`. The unchanged registry deadline expired (timeout after 900s (exit -15)); last runner output: compared 100/470 option words. The aggregate terminated the process group before the sweep printed its final comparison/assertion results, so this is a runtime deadline failure, not evidence of a completed parity mismatch.
- `transformed-options-sweep` / `kc`: first failing line
  `Completed transformed-options-sweep / kc: FAIL`. The unchanged registry deadline expired (timeout after 900s (exit -15)); last runner output: compared 200/470 option words. The aggregate terminated the process group before the sweep printed its final comparison/assertion results, so this is a runtime deadline failure, not evidence of a completed parity mismatch.
- `transformed-options-sweep` / `tkrzw`: first failing line
  `Completed transformed-options-sweep / tkrzw: FAIL`. The unchanged registry deadline expired (timeout after 900s (exit -15)); last runner output: compared 250/470 option words. The aggregate terminated the process group before the sweep printed its final comparison/assertion results, so this is a runtime deadline failure, not evidence of a completed parity mismatch.

The prior bisect failure no longer occurs with main's merged changes.
No gate outcome was fixed, waived or rerun in this restack.
The eight measurement recipes remain excluded.

Execution used the retained `oxpinyin-tools-phase1` (`debian:testing`) container,
Rust 1.97.1, dev opt-level 1 and `/lane/work/cells/{bdb,kc,tkrzw}` targets.
The original oracle prefixes remained mounted read-only and lane-j was untouched.
The exact host command was:

```bash
podman exec oxpinyin-tools-phase1 bash /lane/restack-evidence-20261007/sweep.sh
```

That retained script invokes `bash tools/bisection/run-all.sh --cells
bdb,kc,tkrzw --jobs 4` once from `/lane/restack-20261007`, with the same oracle,
data, IBus, target and staged-metadata arguments in the earlier command below;
its `--json` is `/lane/restack-evidence-20261007/results.json`.
The script refuses a second execution when its start marker exists.
An external Python retention hook prevents only the aggregate scratch directory's
normal cleanup; commands, comparisons, timeouts and result handling are unchanged.
All three build logs, all 141 runner logs, the result JSON, table, timestamps,
exact tested SHA, command script and previous table are retained under host
`$WORK_ROOT/lane-tools/restack-evidence-20261007`; no captures enter Git.

Rule 7: #667, #668, #670, #671, #672 and #673 rebased without conflicts or
upstream/member file overlap, so require no local re-gate. #674 changes the
registry only: fmt, shell/Python/registry validation and the explicitly requested
single sweep cover it; no Rust crate is touched, so crate clippy/test has no
scope. #675 is docs/findings-only: fmt then push, without another build or sweep.
Each member's PR body records its case. Rule 2's three backend path/content
checks are empty; the explicit maintainer request authorizes the all-cell sweep.

## Final sweep (earlier base `e985b581`)

The [final 138-row table](https://github.com/shenghaoc/oxpinyin/pull/675#issuecomment-5986931505) is attached to #675; no captures are committed.
Started `2026-10-05T01:51:13Z`, finished `2026-10-05T02:11:10Z` (both captured with
`date -u`), runner revision `71e090e61df2f510383fa40f623086e2f3155a35`. Aggregate exit: **0**.

| Cell | PASS | SKIPPED | FAIL |
| --- | ---: | ---: | ---: |
| bdb | 46 | 0 | 0 |
| kc | 46 | 0 | 0 |
| tkrzw | 46 | 0 | 0 |

**138 PASS; no remaining FAIL, SKIPPED or timeout.** Exactly six rows use
patch-qualified pin refs: the two authorized runners on three cells. All other
rows use the original unpatched pin. All 135 original-prefix file hashes still
match after the sweep.

The one final invocation runs in the retained `oxpinyin-tools-phase1` container,
from `/lane/phase2`, with Rust 1.97.1 and shipped-code base
`e985b581592c155d63c6dc7e146f206de096c7b2`. Host
`$WORK_ROOT/lane-tools` is mounted at `/lane`, and host
`$ORACLE_ROOT` is mounted read-only at `/oracle`. Subject data copies
use each cell's backend and include pinned `interpolation2.text`. Packaging
metadata inputs are staged independently of the reference installations.

```bash
export LC_ALL=C CARGO_PROFILE_DEV_OPT_LEVEL=1
unset CARGO_TARGET_DIR
bash tools/bisection/run-all.sh --cells bdb,kc,tkrzw --jobs 4 \
  --oracle-root /oracle --target-root /lane/work/cells \
  --data bdb=/lane/work/bdb-data --data kc=/lane/work/kc-data \
  --data tkrzw=/lane/work/tkrzw-data \
  --ibus-build bdb=/oracle/work-bdb --ibus-build kc=/oracle/work-kc \
  --ibus-build tkrzw=/oracle/work-tkrzw \
  --cell-env bdb:OXPINYIN_METADATA_PIN_ROOT=/lane/work/pc-pin-root \
  --cell-env bdb:OXPINYIN_METADATA_SUBJECT_ROOT=/lane/work/pr6-metadata-dev \
  --cell-env bdb:OXPINYIN_METADATA_PREFIX=$ORACLE_ROOT/bdb \
  --cell-env kc:OXPINYIN_METADATA_PIN_ROOT=/lane/work/pc-pin-kc \
  --cell-env kc:OXPINYIN_METADATA_SUBJECT_ROOT=/lane/work/pc-subject-kc \
  --cell-env kc:OXPINYIN_METADATA_PREFIX=$ORACLE_ROOT/kc \
  --cell-env tkrzw:OXPINYIN_METADATA_PIN_ROOT=/lane/work/pc-pin-tkrzw \
  --cell-env tkrzw:OXPINYIN_METADATA_SUBJECT_ROOT=/lane/work/pc-subject-tkrzw \
  --cell-env tkrzw:OXPINYIN_METADATA_PREFIX=$ORACLE_ROOT/tkrzw \
  --json /lane/work/patched-evidence/final-results.json
```

The registry selects patched prefixes only for `bigram-export-diff` and
`residue-a-tail-diff`. Every other runner uses its original unpatched prefix,
including `train-index-diff`, which rejects patched pins. The table's `pin_ref`
column records the selected variant's exact patch-qualified identity.

## Row 1: cause and patched prefixes

The residue-A crash is compatibility-policy register row 1, class **(b)**. The
pin's bigram export iterator passes an unterminated `GPtrArray` to `g_strjoinv`,
which reads past the allocation. The [GDB backtrace](https://github.com/shenghaoc/oxpinyin/pull/675#issuecomment-5986569018)
locates `g_strjoinv` → `pinyin_bigram_iterator_has_next_phrase` at pinned
`pinyin.cpp:862`, called by `dump_bigram` during `X2-train1`. The earlier
unpatched 3/3 PASS was chance under undefined behavior, as ruled by the
maintainer; it is not a defined reference result or a subject regression.

The unpatched invocation audit independently changed SAME_DIR, oracle versus
copied data, LC_ALL, TMPDIR and the backend extension. Every form still
SIGSEGVed. The historical passing run at `1dc42d7e` mounted the bdb prefix at
`/oracle` and used SAME_DIR=1; the failing Phase 1 run at `b04ace57` mounted it
at `/oracle/bdb`, used copied subject data, and set explicit locale/temp/backend
variables. Both used Rust 1.97.1 unoptimized dev; 22 crate files changed between
those revisions but neither residue driver nor runner did. These differences
are recorded context, not competing explanations for the row-1 over-read.

Three separate patched prefixes were built under `$ORACLE_ROOT`:
`bdb-bigram-export-strjoinv`, `kc-bigram-export-strjoinv`, and
`tkrzw-bigram-export-strjoinv`. Original `bdb`, `kc` and `tkrzw` prefixes were
mounted read-only throughout; before/after SHA256 inventories match for all
135 original files/symlink targets. New workspaces and caches remain beside them.

The retained `localhost/oxp-debian-testing` image has the existing oracle-build
prerequisites; no packages were added. In builder container
`oxpinyin-tools-patched-build`, host oracle directories are mounted at `/oracle`,
with only the new prefix/work directories writable. For CELL=bdb,kc,tkrzw, from
`/lane/phase2`:

```bash
bash tools/oracle/build-oracle.sh --dbm CELL --enable-libzhuyin --jobs 4 \
  --apply-patches tools/bisection/patches/bigram-export-strjoinv \
  --prefix /oracle/CELL-bigram-export-strjoinv \
  --work-dir /oracle/work-CELL-bigram-export-strjoinv
```

The three concurrent builds took 135.730 s (bdb), 186.126 s (kc), and 158.602 s
(tkrzw), all exit0, using fresh pinned source checkouts and the retained,
SHA-verified model archives. Installed prefixes occupy about 193 MiB; new
workspaces plus prefixes about 1.2 GiB. The patch manifest digest is
`10313a5bb59ddd1874e7f3f9e42bb3c52a3dcbbca7e35bacd78d27188dd7892c`, included in
all three `pin_ref` values. The original prefixes and their pin identities remain
unchanged. [Build commands, dates and repeated bigram captures](https://github.com/shenghaoc/oxpinyin/pull/675#issuecomment-5986760848).

The pin source was read at exact commit
`074a2219c90feaf962d0d24f034514033ece5f99` from
`$ORACLE_ROOT/work-bdb/src/libpinyin-074a2219c90feaf962d0d24f034514033ece5f99`;
the new builds apply only the named instrumentation patch to that pin. Other
runners keep their original oracle because the iterator patch can change
UB-derived crash/garbage observations wherever that iterator is called.

## Required repeated verification

For each cell, with the retained dev opt1 subject and matching backend:

```bash
bash tools/bisection/run-bigram-export-diff.sh \
  /oracle/CELL-bigram-export-strjoinv/lib/libpinyin.so \
  /lane/work/cells/CELL/debug/libpinyin_capi.so \
  /oracle/CELL-bigram-export-strjoinv/lib/libpinyin/data
```

The unchanged default `full` gate passes **3/3 on bdb, 3/3 on kc, and 3/3 on
tkrzw**. The drivers and expected output were not changed.

For residue-A, set `PINYIN_ORACLE_DBM=bdb`,
`PINYIN_ORACLE_PREFIX=/oracle/bdb-bigram-export-strjoinv`,
`OXPINYIN_CAPI_SO=/lane/work/cells/bdb/debug/libpinyin_capi.so`,
`OXPINYIN_SYSTEM_DIR=/lane/work/bdb-data`, matching `CARGO_TARGET_DIR`,
`CARGO_PROFILE_DEV_OPT_LEVEL=1`, `LC_ALL=C`, and `TMPDIR=/lane/work/tmp`.
Run `bash tools/bisection/run-residue-a-tail-diff.sh` three times with distinct
`RESIDUE_A_OUT` directories. SAME_DIR remains unset. All three pass with
byte-identical logs; no differing line triggers the requested STOP.
The exact captures are attached to #675:
[repeat 1](https://github.com/shenghaoc/oxpinyin/pull/675#issuecomment-5986761060),
[repeat 2](https://github.com/shenghaoc/oxpinyin/pull/675#issuecomment-5986761216),
[repeat 3](https://github.com/shenghaoc/oxpinyin/pull/675#issuecomment-5986761408). A Cargo-rejecting PATH sentinel verifies
supplied-artifact execution in all twelve focused runs.

The missing-marker check runs residue-A against the unpatched `/oracle/bdb`
and expects exit1. The missing-prefix aggregate check selects residue-A and
train-index with `--oracle-root /lane/work/patched-evidence/missing --no-build`:
residue-A is FAIL while train-index is SKIPPED. Explicit skip allowance cannot
turn the patched prerequisite failure into success. These checks precede the
single final sweep and change neither driver nor expected comparison payload.

## Earlier sweep and focused corrections

The [original 138-row capture](https://github.com/shenghaoc/oxpinyin/pull/675#issuecomment-5986553748)
is retained on #675, not in Git. Its historical counts were bdb42/4, kc40/6,
tkrzw41/5 PASS/FAIL, with no skips/timeouts. Failure classes were missing patch
markers (3), bisect assertions (3), bdb residue-A over-read (1), stale dynamic-off
fixtures (3), missing native dump tools (4), and a kc union planter error (1).
These are historical counts, superseded by the final sweep above.

The union planter correction belongs to #667. Dynamic-off’s flat `fixtures/w3`
path failed normal initialization with missing `pinyin_index.bin` (exit1, no
signal or panic); #672 selects the complete backend fixture, and all three cells
pass. That member also checks bisect’s registered class-(c) observation:
`nihao`, right-offset5 computes6 and asserts at pin `pinyin.cpp:3092`, while the
C API returns false with untouched output. Full C API smoke/Valgrind coverage
remains; it is not a full-oracle byte-parity claim. Installing `kchashmgr` and
`tkrzw_dbm_util` resolved all four requested kc/tkrzw state-comparison failures.
The [earlier commands and backtraces](https://github.com/shenghaoc/oxpinyin/pull/675#issuecomment-5986569018)
remain attached for review; their provisional historical-crash explanation is
superseded by the row-1 ruling above.

## Rule 7 and stack placement

1. Prefix builds are explicitly authorized environment provisioning; they do
   not change dependencies or shipped code. Original-prefix hashes verify
   isolation; no unrelated runner re-gate is inferred from provisioning alone.
2. The resolution review fix belongs to #668 (`oracle-cell.sh`), the residue
   prerequisite check to #672, and the registry/aggregate adapter to #674.
   ShellCheck, native warnings-as-errors compilation, Python syntax and diff
   checks pass. Focused changed-runner verification uses the explicit all-cell
   and three-repeat requirements above. No Rust crate is touched, so no crate
   clippy/test or workspace sweep applies.
3. The requested repeated runs are the changed differentials’ proportional
   checks. No revert or expected-output adjustment is made.
4. Findings/runbook changes belong to #675: docs-only, format/diff checks, no
   extra runtime re-gate. Captures remain attached to the PR and outside Git.
5. The final all-cell sweep is explicitly requested and runs once. A subsequent
   conflict-free restack with an unchanged landing tip requires no repeat;
   descendants that change none of these files retain their earlier verification.

Worktree, targets, original and patched oracle prefixes, builder/runtime
containers, lane image and evidence remain available for review. The stack stays
draft, with no merge or review request.
