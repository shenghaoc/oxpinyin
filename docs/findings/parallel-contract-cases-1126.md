# Parallel contract cases and cells (#1126)

The comparison campaign uses the full ordinary contract selection at
`06195c1a6ceb6e2ea97d0679ca2b8fe0f26fb1c2`: 2,897 cases (pinyin 1,738,
zhuyin 1,159; `--plan-only`). The host is a Linux x86_64 cloud session,
kernel 6.18.44, Intel Xeon Processor @ 2.80GHz, glibc 2.39 (Ubuntu), with
4 CPUs (`nproc` 4, affinity 0-3) and MemTotal 16,091 MiB (MemAvailable
about 15,600 MiB at start). The cgroup is v1. `cpu.cfs_quota_us` is -1
(no quota). The session's memory cgroup `memory.limit_in_bytes` is
14,341,881,856 B (13,677 MiB), and the root limit is unlimited. There is
no cgroup v2 `cpu.max` or `memory.max`. Admission reads cgroup v2 only, so
every admission line below reports `cgroup / memory room=unknown; cgroup /
CPU quota=unknown` and bases its decisions on MemAvailable and CPU
affinity. Python is managed CPython 3.15.0 with the GIL, uv 0.13.1, and
Rust 1.97.1. Differential artifacts are dev opt-level 1, one retained
target per cell, built from this branch (the branch changes `tools/` and
docs only, so main's subject libraries are the same code). Scratch uses the
default `TMPDIR` (`/tmp`), which is ext4 on this host. No startup or
residency measurement recipe ran. Figures from earlier hosts do not apply.

Provisioning used `tools/cloud/setup.sh --cells 'bdb kc tkrzw'` (exit 0,
about 6 min 48 s, published oracle image pulled). All three cells reported
`pulled libraries match oracle-pin.txt` and `data manifest OK`, with these
sha256 values:

| cell | libpinyin.so | libzhuyin.so |
|---|---|---|
| bdb | `754499651c302efd91fe7f72dd6a791978400b268cb6206d2ebe644015721f30` | `cf4ccbce9f70f6c7e412b92a52a243ba64fdea1ff197cbc44019c6b85650d53b` |
| kc | `ddbb3ce4b625bf3b6ab0f32b2e28e49144595a953490a2221bee6a5642c55ef6` | `e538c36cc0d9e6c43c595c296db539cd2b768bce427252358e6f825387e13dc4` |
| tkrzw | `d78a91676af43ad5075c20ab4ae014a8bc6e03b7a5f5ab656d014087f18ff7b8` | `adbcc8c8c3dec00a488ea89413a525f57fdccf280a8ca44e6c2ad6765fd24c3d` |

The tkrzw probe reported the host libtkrzw broken or missing
(`~/.cache/oxpinyin/tkrzw-host-probe.log`), so setup built upstream
tkrzw 9ee8416; the oracle `libpinyin.so` resolves `libtkrzw.so.1` to
`~/.cache/oxpinyin/tkrzw/lib/`. The tkrzw results below therefore ran
against the sound library. A first `setup.sh` attempt failed before
provisioning because Rust 1.97.1 was not installed; it was rerun after the
toolchain was installed.

## Validation

The first run of the candidate harness tests on this host had 104
`contract-diff.test.py` tests with 5 failures (`run-all.test.py` 12, all
passing). All five were subtests of
`test_parallel_signals_reap_every_group_before_releasing_scratch` and
`test_parallel_sigterm_through_run_all` (the `jobs=2` cancellation tests),
each `worker and descendant did not start`.

The cause was the tests' process observer, not the driver. Its
`CancellationTests.children` read `/proc/<pid>/task/<pid>/children`, which
lists only the children of the main thread. The parallel driver starts
workers from pool threads, so on a kernel that has the `children` file the
observer never saw them, and its stat-scan fallback was not used. The
observer now also reads `/proc/<pid>/task/*/children` for every other
thread, and skips a thread that exits while it reads. No assertion and no
deadline changed. The failures were expected on any host with
`CONFIG_PROC_CHILDREN`.

The branch was then rebased onto main `a12a47b`, which added the #1137
arm64 platform skips and the #1114 handoff cases to the serial loop this
change replaces. They are merged into the case pool: a platform-skipped
case yields its `SKIP` row with the reason from the ordered result stream,
and the #1114 reload workers record their side through the per-thread
state instead of a slice of the shared timing list.

On the rebased head, on Linux 6.18.44-fc-v153 x86_64 (the host above) under
managed CPython 3.15.0 with the GIL, `contract-diff.test.py` ran 118 tests,
all passing, in 10.8 s. The 14 added over the earlier 104 come from main.
After review, `--jobs` defaults to unset and `OXPINYIN_CONTRACT_JOBS` is read
and validated only by an ordinary contract run, so a bad value cannot break
`--list-tags`, `--teardown` or `--per-call`. Three tests for that bring the
file to 121 tests, all passing on the final head.
`run-all.test.py` (12), `host.test.py` (39, one platform skip) and
`slot.test.py` (13) passed on the rebased head before the observer change,
which touches none of them.

## Comparative measurement (at `06195c1`, before the rebase)

This section was measured with main at `06195c1`, before the rebase onto
`a12a47b` and before the observer fix. The timing campaign was not
repeated. All runs are in one job on this host, one arm at a time, after an excluded
warm-up (`--family zero-key`, 109 output lines per cell). Arms: main's
driver serial; the branch with `--jobs 1`; the branch with `--jobs 4` twice.
Each is `tools/bisection/run-contract-diff.sh <cell> <oracle prefix>
<target>/debug/libpinyin_capi.so <target>/debug/libzhuyin_capi.so -- [--jobs
N]` under `run-all.py --heavy-slot --`, from the matching worktree. Wall
time is the whole command. The requested count was 4 (the CPU count); for
all six `--jobs 4` runs admission reported `requested 4, admitted 4`.

| cell | main serial | branch `--jobs 1` | `--jobs 4` run 1 | `--jobs 4` run 2 |
|---|---|---|---|---|
| bdb | 1,420.9 s | 1,398.0 s (0.98× main) | 398.8 s | 368.5 s |
| kc | 1,471.8 s | 1,546.9 s (1.05× main) | 388.3 s | 397.9 s |
| tkrzw | 1,721.8 s | 1,738.3 s (1.01× main) | 444.2 s | 453.9 s |

Speed-up of `--jobs 4` over main serial: bdb 3.56× and 3.86×, kc 3.79× and
3.70×, tkrzw 3.88× and 3.79×. Over branch `--jobs 1`: bdb 3.51× and 3.79×,
kc 3.98× and 3.89×, tkrzw 3.91× and 3.83×. The branch's serial path is
within about 5% of main in both directions; each cell ran each serial arm
once, so differences of that size are not resolved by this data. Every arm
exited 0. Verdicts per cell: bdb 2,893 MATCH and 4 HOST-DEPENDENT; kc and
tkrzw 2,888 MATCH, 5 SKIP and 4 HOST-DEPENDENT.

### Three cells at once

```sh
OXPINYIN_HEAVY_SLOTS=3 tools/bisection/run-all.py --runners contract-diff \
    --cells bdb,kc,tkrzw --no-build --jobs 3 --contract-jobs 4 \
    --runner-timeout contract-diff=7200 --target-root /tmp/tgt \
    --oracle-root ~/.cache/oxpinyin/oracle
```

Wall time 1,211.7 s; all three rows PASS (bdb 1,149.4 s, kc 1,143.1 s,
tkrzw 1,211.0 s). The three cells ran together on 4 CPUs, so this is about
3.9× faster than the three `--jobs 1` runs back to back (4,683 s), and it
costs about 3× the single-cell `--jobs 4` time. Sampling
`MemTotal - MemAvailable` once a second (1,186 samples), peak use was
1,610 MiB of 16,091 MiB. The cgroup root `memory.usage_in_bytes` peaked at
6,457 MiB, which includes page cache.

Admission made no refusal, no reduction and no wait. Every runner row
reports `memory_wait_seconds` 0.0 and `slot_wait_seconds` at most 0.001.
Each cell requested 4 case workers and was admitted 4
(`contract case workers: requested 4, admitted 4`). The reservations of
cells already running reduced the room the next cell saw, as designed:
15,387 MiB, then 11,309 MiB (after a 4,078 MiB charge for slot 0), then
7,238 MiB available, each less 1,609 MiB of headroom and at 1,024 MiB per
case, so 4 cases still fit. The command set the slot count to 3; the
default on this host (at least 15 GiB) is 2.

Raw output, the memory samples and the logs are local and ephemeral; they
are not published.

## Equivalence at `06195c1`, before the rebase

For each cell, the ordered verdict output of each of two `--jobs 4` runs
was compared with the branch `--jobs 1` run, with no normalization:

```sh
cmp <(head -n -1 <cell>/branch-j1.out) <(head -n -1 <cell>/branch-par1.out)
```

`head -n -1` drops the final timing line. Each file has 2,897 verdict
lines.

| cell | run 1 | run 2 |
|---|---|---|
| bdb | differs, line 507 | differs, line 507 |
| kc | identical | identical |
| tkrzw | identical | differs, line 507 |

Line 507 is `fini-twice`, and the only difference is the pin's
`stderr_lines` count (1 against 2; `exit` `[-11, 0]` and `MATCH` are the
same). The case is registered with `stderr_varies=True` (issue 573). It
varies without parallelism: six repeats of `--cases fini-twice --jobs 1`
on bdb gave `[1, 1]` five times and `[2, 1]` once, and on bdb main's serial
output and the branch `--jobs 1` output differ on this line too. With that
case removed from both sides (`grep -v '"case": "fini-twice"'`), all six
comparisons are identical. So the byte-for-byte comparison of the whole
output does not hold on bdb and tkrzw, and does hold on kc; every other
line is identical.

Main's serial output against the branch `--jobs 1` output differs on two
cases only: `fini-twice` as above (bdb), and KC's
`user-gram-unsorted-export`, where in this run main's pin exits -11 and the
branch run's exits 0 (`MATCH` in both). That case is the existing
`strjoinv_over_read` / `strjoinv_fault` rule, not a new exception. At
074a2219,
[`src/pinyin.cpp:844-863`](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.cpp#L844-L863)
passes a non-terminated array to `g_strjoinv`, and the fault depends on
heap layout. The source blob was read and verified as
`f27f7cf776724ead8d55e14b4af390e790054a91`. Case definitions, registered
expectations and verdict functions are unchanged by the branch.

## Equivalence at the rebased head

After the rebase the bdb and tkrzw subject libraries were rebuilt from the
rebased head (`tools/bisection/cell-artifacts.sh`, dev opt-level 1) and
the following ran once each on the same host. No timing is claimed from
them.

bdb, full selection: 2,899 cases (main added the two tkrzw-only #1114
cases, which are `SKIP` on bdb). Branch `--jobs 1` and `--jobs 4`
(`contract case workers: requested 4, admitted 4`):

```sh
tools/bisection/run-contract-diff.sh bdb "$PINYIN_ORACLE_PREFIX" \
    <target>/debug/libpinyin_capi.so <target>/debug/libzhuyin_capi.so -- --jobs N
cmp <(head -n -1 bdb-full-j1.out) <(head -n -1 bdb-full-j4.out)
```

The 2,899 ordered verdict lines (2,893 MATCH, 4 HOST-DEPENDENT, 2 SKIP) are
byte-identical, so no exclusion was needed. The `fini-twice` pin
`stderr_lines` (registered `stderr_varies`, issue 573) did not vary in this
pair. Excluding it from the comparison, as agreed, gives the same result:
identical.

tkrzw, `--cases issue1114-collision-load-save-pinyin,issue1114-collision-load-save-zhuyin`
at `--jobs 1` and `--jobs 4` with `--observations`: both cases are `MATCH`
(`exit` `[0, 0]`) and the two verdict outputs are byte-identical. The
observation files differ only in the random `mkdtemp` suffix of the
`handoff-pin-*` and `handoff-subject-*` directories inside the
`open SCRATCH/handoff-…/user.conf failed.` stderr text; every other field,
including the saved bigram bytes and `cross_reload`, is equal. Those
directory names are random per run in main's handoff code, so two serial
runs differ in the same way.
