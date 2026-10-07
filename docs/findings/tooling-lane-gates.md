# Tooling lane: final three-cell gate run

Date: 2026-10-07 UTC (captured with `date -u`).

The stack changes only tooling and documentation. Parity artifacts use the dev
profile with `CARGO_PROFILE_DEV_OPT_LEVEL=1`, one retained target per cell. The
eight measurement recipes remain unchanged and outside the gate set. No oracle
output cache, shipped crate, dependency manifest or CI policy was changed.

## Sweep after restack onto `8e76b17d`

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
This findings-count update is a subsequent **docs-only commit** on #675;
no runtime/runner changes or second sweep follow the recorded test.

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
`/home/sheng/work/lane-tools/restack-evidence-20261007`; no captures enter Git.

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
`/home/sheng/work/lane-tools` is mounted at `/lane`, and host
`/home/sheng/work/oracle` is mounted read-only at `/oracle`. Subject data copies
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
  --cell-env bdb:OXPINYIN_METADATA_PREFIX=/home/sheng/work/oracle/bdb \
  --cell-env kc:OXPINYIN_METADATA_PIN_ROOT=/lane/work/pc-pin-kc \
  --cell-env kc:OXPINYIN_METADATA_SUBJECT_ROOT=/lane/work/pc-subject-kc \
  --cell-env kc:OXPINYIN_METADATA_PREFIX=/home/sheng/work/oracle/kc \
  --cell-env tkrzw:OXPINYIN_METADATA_PIN_ROOT=/lane/work/pc-pin-tkrzw \
  --cell-env tkrzw:OXPINYIN_METADATA_SUBJECT_ROOT=/lane/work/pc-subject-tkrzw \
  --cell-env tkrzw:OXPINYIN_METADATA_PREFIX=/home/sheng/work/oracle/tkrzw \
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

Three separate patched prefixes were built under `/home/sheng/work/oracle`:
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
`/home/sheng/work/oracle/work-bdb/src/libpinyin-074a2219c90feaf962d0d24f034514033ece5f99`;
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
