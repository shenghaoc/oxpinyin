# Current-main init attribution, BDB (#1090, 2026-10-27 UTC)

The historical warm init wall gap is absent in pinyin under the issue's
interval rule. A smaller gap remains in zhuyin: phrases 1.004628×
[1.000103, 1.014989], journals 1.013142× [1.004783, 1.020577]. All four
init wall controls are STABLE. The phrases lower bound is only 0.0103%
above one; no confirmation rerun was made.

Attribution finds extra minor faults and a wider executed facade footprint,
and names journal replay as the main instruction hotspot. It does **not**
establish how many microseconds those contributors cost. Hardware events,
CPU sampling and fault-IP attribution were denied by the kernel. The full
cause of the remaining zhuyin wall gap is unresolved; #1090 remains open.
This is diagnosis only, with no shipping change or time/residency trade.

The [maintainer's static code-layout analysis](https://github.com/shenghaoc/oxpinyin/issues/1090#issuecomment-6433999792)
identifies the generator's omission of the empty-user-directory case used by
the cold run, plus a policy one source commit stale, as the likely cause of
excess cold-init code residency. Missed functions can bring mmap read-around
neighbours into residency. Its contribution to the remaining warm wall gap
is not proven without perf. #1090 stays open for an empty-profile fixture and
regenerated policies, pending maintainer approval.

## Campaign and artifacts

Measured main `7f7bb721f74919e488374703c427f11302bf6fa7`, on one native
Linux amd64 host, Intel Core i7-9750H, kernel 7.3.0-9-generic, CPU 2, ext4.
Rust 1.97.1, cargo-c 0.10.25, GCC 15.3.0, GNU ld 2.47, managed CPython
3.15.0 and Valgrind 3.27.1. Both facades used release `cargo cinstall`
through `just packaging install`, with the checked-in BDB function orders.
The measurement tool verified every copied ox/control arm's order.

The reference is the existing verified BDB build of libpinyin at
`074a2219c90feaf962d0d24f034514033ece5f99`, using its model20 data and the
maintained provisioner's `phrases` and `journals` profiles. Model identity
is `eab71f4c6947d380f1b1e9375cec7a0e9c6d623992ce37a20f3d4c7a4069d593`.
All four cells were COMPARABLE before both stages.

| Artifact | Installed bytes | SHA-256 |
| --- | ---: | --- |
| ox libpinyin | 5,189,720 | `78dee9b8f1950e739edd07b3e868f5a63a83d33e991640199f95317e6514e9ba` |
| ox libzhuyin | 4,906,704 | `56e9143c429ddf8ae42205472139cf20c904080b4622e4789cd06b3604f37270` |
| pin libpinyin | 4,802,600 | `754499651c302efd91fe7f72dd6a791978400b268cb6206d2ebe644015721f30` |
| pin libzhuyin | 4,681,128 | `cf4ccbce9f70f6c7e412b92a52a243ba64fdea1ff197cbc44019c6b85650d53b` |

Control was an exact copy of ox at a second equal-length path. Artifact
hashes still matched after attribution. This campaign compares current
main, its same-bytes control and the pin; it has no implementation PR arm.
No historical absolute measurement is used as a baseline.

The shared heavy admission remained held from both builds through confirm
and attribution, including the decision between stages. The initial host
check found no competing heavy process. Confirm ran 04:35:02–04:35:56 UTC;
attribution ran 04:36:20–04:42:33 UTC. The admitted shell exited at
04:43:10 UTC; the subsequent process check found no measurement, profiler,
compiler or container jobs and both slot records were empty.

Starting evidence is [#1318's M0→I1 comment](https://github.com/shenghaoc/oxpinyin/pull/1318#issuecomment-6433238871):
212 own-library pages for ox versus 52 for the pin, including 162 versus 38
code pages, with identical data-file ranges and similar BDB pages. That
cold, empty-profile residency observation is distinct from this campaign's
warm populated-profile execution measurement. Residency includes readahead;
page/symbol overlap proves neither execution nor a fault cause. #1318 was
not rerun.

## Primary init timing

`confirm` uses 60 retained native timing rounds after three discarded
warmups, serially rotating ox/control/pin. It also performs its prescribed
memory rounds. Only its init records decide the gap. Values are microsecond
medians. Ratios are medians of paired ratios with the maintained 4,000-resample
95% bootstrap, not quotients of the marginal medians. Each shown control
median and whole interval lies within [0.95, 1.05].

| Facade/profile | Clock | Ox µs | Control µs | Pin µs | Control/ox [95%] | Ox/pin [95%] |
| --- | --- | ---: | ---: | ---: | --- | --- |
| pinyin phrases | wall | 2470.075 | 2472.990 | 2482.562 | 1.001515 [0.990413, 1.011555] | 0.987188 [0.978578, 0.994942] |
| pinyin phrases | cpu | 2468.195 | 2473.809 | 2474.157 | 1.001539 [0.990424, 1.011500] | 0.987367 [0.978436, 0.994867] |
| pinyin journals | wall | 2479.231 | 2477.545 | 2528.786 | 1.004717 [0.996597, 1.012257] | 0.989726 [0.979392, 1.003687] |
| pinyin journals | cpu | 2479.697 | 2477.351 | 2529.227 | 1.004725 [0.996720, 1.011482] | 0.989750 [0.980856, 1.003711] |
| zhuyin phrases | wall | 2357.905 | 2465.055 | 2306.213 | 1.006145 [1.000014, 1.019344] | 1.004628 [1.000103, 1.014989] |
| zhuyin phrases | cpu | 2355.268 | 2465.637 | 2306.878 | 1.006172 [1.000017, 1.021267] | 1.004371 [0.999309, 1.014885] |
| zhuyin journals | wall | 2058.080 | 2046.498 | 2004.534 | 1.001238 [0.995701, 1.008628] | 1.013142 [1.004783, 1.020577] |
| zhuyin journals | cpu | 2058.637 | 2047.210 | 2005.030 | 1.000758 [0.995742, 1.010582] | 1.012343 [1.005091, 1.020515] |

Pinyin phrases is below one and journals includes one; no pinyin slowdown
is established. Both zhuyin wall intervals are above one, so the planned
four-cell init-only attribution stage followed in the same campaign. Zhuyin
phrases CPU includes one; journals CPU remains above one. No timing rerun
or additional campaign was used to seek a different verdict.

## Init resources and unavailable hardware attribution

The `count` probe retains 30 rounds after one warmup, again rotating all
three sides. These are diagnostic samples with M0 after load and I1 after
init. Values below are medians; user/sys are microseconds. All major-fault
samples were zero. Independent user/sys medians need not add to CPU time.

| Facade/profile | Side | User µs | Sys µs | Minor faults | Major faults |
| --- | --- | ---: | ---: | ---: | ---: |
| pinyin phrases | ox | 1220.0 | 937.5 | 290.5 | 0 |
| pinyin phrases | control | 1270.5 | 954.5 | 290 | 0 |
| pinyin phrases | pin | 1582.5 | 602.5 | 279 | 0 |
| pinyin journals | ox | 1434.0 | 785.0 | 302 | 0 |
| pinyin journals | control | 1047.5 | 1158.5 | 302 | 0 |
| pinyin journals | pin | 1108.5 | 1087.5 | 287 | 0 |
| zhuyin phrases | ox | 1371.0 | 585.0 | 286 | 0 |
| zhuyin phrases | control | 1327.0 | 629.5 | 286 | 0 |
| zhuyin phrases | pin | 1110.0 | 931.0 | 273 | 0 |
| zhuyin journals | ox | 1096.5 | 868.5 | 297.5 | 0 |
| zhuyin journals | control | 1272.5 | 909.5 | 297 | 0 |
| zhuyin journals | pin | 1038.5 | 985.5 | 282 | 0 |

Paired ox−pin minor-fault differences [95%] are +11 [11, 11] for pinyin
phrases, +15 [14, 15] for pinyin journals, +13 [11, 13.5] for zhuyin
phrases and +16 [15.5, 16.5] for zhuyin journals. These establish extra
faults, not their locations or cost. The user/sys split is quantized by
kernel CPU accounting at this short duration. Every paired user and sys
difference interval includes zero, so it does not isolate an excess kernel
or user-time contribution.

All sides/all four cells reported **NOT RUN, errno 13, Permission denied**
for cycles, instructions, L1i misses, iTLB misses, branch misses and the
software perf events. IPC therefore cannot be measured. Fine/coarse
CPU-clock sampling and fault-IP probes were also NOT RUN with errno 13.
These were the maintained stage's attempts; kernel settings and permissions
were not changed, and no dependency was added.

## Dynamic instructions and simulated caches

Three fresh Callgrind processes per side/cell collect only the init export.
BDB counts are DIAGNOSTIC: libdb seeds `srandom_r` from the process, so the
maintained BDB rule reports ranges rather than an exact control verdict.
Below are the three-process Ir ranges. Cache counts were constant across
those three processes. I1mr and D1mr are simulated L1 instruction/read
misses; ILmr and DLmr are simulated last-level instruction/read misses.
The fixed simulator configuration is I1/D1 32 KiB, 8-way, 64-byte lines;
LL 8 MiB, 16-way, 64-byte lines. These are not hardware counters or IPC.

| Facade/profile | Side | Ir range | I1mr | ILmr | D1mr | DLmr |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| pinyin phrases | ox | 9,560,021–9,560,022 | 12,792 | 3,034 | 100,266 | 75,321 |
| pinyin phrases | control | 9,560,021–9,560,022 | 12,792 | 3,034 | 100,266 | 75,321 |
| pinyin phrases | pin | 10,259,483–10,259,485 | 21,133 | 2,277 | 84,395 | 65,334 |
| pinyin journals | ox | 9,601,716–9,601,717 | 12,891 | 3,029 | 103,404 | 75,999 |
| pinyin journals | control | 9,601,716–9,601,719 | 12,891 | 3,029 | 103,404 | 75,999 |
| pinyin journals | pin | 10,332,584–10,332,585 | 21,187 | 2,276 | 86,517 | 68,276 |
| zhuyin phrases | ox | 9,481,750–9,481,751 | 9,924 | 2,942 | 100,008 | 75,310 |
| zhuyin phrases | control | 9,481,750 | 9,924 | 2,942 | 100,008 | 75,310 |
| zhuyin phrases | pin | 10,101,543–10,101,545 | 14,808 | 2,194 | 84,259 | 65,326 |
| zhuyin journals | ox | 9,523,213 | 10,089 | 2,940 | 103,163 | 75,988 |
| zhuyin journals | control | 9,523,213–9,523,214 | 10,089 | 2,940 | 103,163 | 75,988 |
| zhuyin journals | pin | 10,173,842 | 14,832 | 2,193 | 86,360 | 68,270 |

Ox still executes fewer dynamic instructions. It has more simulated
last-level instruction and data-read misses, but fewer simulated I1 misses.
This mixed result does not establish an instruction-cache explanation for
the native wall gap.

## Executed footprint and named contributors

One init-only Lackey pass per side/cell identifies distinct executed
instruction addresses. The table counts only the measured facade's code,
not its dependencies. Function bytes include whole symbol bodies containing
executed instructions, not only the executed bytes. Code-page counts mean
pages containing executed instructions, not residency or faults.
Ox and its same-bytes control matched every footprint value below.

| Facade/profile | Distinct instructions ox/pin | Functions ox/pin | Function bytes ox/pin | Code pages ox/pin |
| --- | ---: | ---: | ---: | ---: |
| pinyin phrases | 13,697/3,237 | 131/58 | 237,970/20,035 | 76/20 |
| pinyin journals | 13,632/3,221 | 128/58 | 236,659/20,035 | 75/20 |
| zhuyin phrases | 12,711/2,873 | 117/47 | 229,963/18,168 | 74/19 |
| zhuyin journals | 12,646/2,857 | 114/47 | 228,652/18,168 | 74/19 |

Every executed ox page is already inside the verified ordered prefix. In
zhuyin journals, the largest distinct-instruction contributors are
`Runtime::open_with_law` (1,816), `GenericUserStore<BdbStore>::open_session`
(891), `persistence::load_logs_with_mask` (815), and
`store_libpinyin::seed_txn_with_replay` (471). These are executed-address
counts, not dynamic instructions or elapsed-time shares. The larger
footprint is also present in pinyin, whose primary wall gap is absent;
footprint alone cannot explain which cells are slower.

The retained Callgrind summaries name journal replay as the dynamic hotspot.
For zhuyin, `persistence::load_logs_with_mask` has inclusive Ir 8,193,493
(phrases) and 8,235,097 (journals), about 86% of total init Ir. Inclusive
costs overlap and must not be added to their descendants. Its principal
exclusive costs, stable across the three ox and control rounds, are:

| Exclusive function | Phrases Ir | Journals Ir |
| --- | ---: | ---: |
| `persistence::load_logs_with_mask` | 1,814,457 | 1,825,271 |
| `replay::Content::header` | 1,180,000 | 1,180,000 |
| `Vec::retain` closure in `replay::Merge::finish_lazy` | 1,140,000 | 1,140,000 |
| `replay::Content::item` | 940,000 | 940,000 |
| `user_files::decode_log_record_at` | 880,348 | 880,348 |

Source reading at the measured main names the corresponding work:
`crates/oxpinyin-user/src/persistence.rs::load_logs_with_mask` validates
journal framing, replays the records and finalizes retained state;
`crates/oxpinyin-user/src/replay.rs::Merge::finish_lazy` revisits touched
items, checks headers/content and retains lazy views. The pin's journal
loop is `src/storage/phrase_index.cpp:439–517` at `074a2219`; zhuyin opens
its default libraries in `src/zhuyin.cpp:270–358`. Those pin sources were
read directly from the exact pinned Git objects, without changing the
upstream checkout.

**Proposal only:** first examine reducing repeated retained-item header and
content work in `Merge::finish_lazy`, carrying validated descriptors where
mutation and malformed-input rules permit it; separately outline cold error
construction from the large init/replay functions. These target the named
instruction work and executable footprint, respectively. They are candidates,
not measured fixes for the wall gap. Any later implementation needs explicit
maintainer approval and both time and residency evaluation; a trade between
those axes is a STOP. This campaign implements neither proposal.

## Init syscall counts and traced time

Six strace rounds per side/cell bracket init only. Each slash-separated
triple below is **ox/control/pin**. Counts and microseconds are per-call-type
sample medians; totals are medians of each sample's total, so column medians
need not sum to them. All calls had zero reported errors; no unfinished or
unparsed trace records occurred. Ptrace stops perturb these times: they
bound traced syscall duration and are not native kernel-time attribution.

### pinyin

| Call | Phrases count | Phrases µs | Journals count | Journals µs |
| --- | ---: | ---: | ---: | ---: |
| brk | 1/1/2 | 11.0/11.0/19.0 | 2/2/2 | 21.5/23.5/21.0 |
| close | 36/36/45 | 269.5/268.0/353.5 | 36/36/45 | 258.0/259.5/363.0 |
| fcntl | 36/36/36 | 224.5/221.0/234.5 | 36/36/36 | 211.5/213.0/226.0 |
| fstat | 9/9/12 | 56.5/56.0/82.5 | 9/9/12 | 54.0/54.5/91.0 |
| getpid | 10/10/19 | 60.0/58.5/124.5 | 10/10/19 | 58.5/60.5/130.0 |
| getrandom | 1/1/0 | 7.0/6.5/0.0 | 1/1/0 | 7.0/6.5/0.0 |
| lseek | 1/1/22 | 6.0/6.5/137.0 | 1/1/22 | 6.0/7.0/143.5 |
| mmap | 20/20/11 | 186.0/188.5/118.0 | 22/22/13 | 209.0/203.0/142.0 |
| munmap | 9/9/3 | 116.5/100.5/67.5 | 8/8/5 | 99.5/130.5/87.0 |
| newfstatat | 19/19/28 | 141.0/140.5/218.5 | 19/19/28 | 135.5/145.5/230.5 |
| openat | 46/46/51 | 449.5/445.0/544.0 | 46/46/51 | 421.0/435.5/563.0 |
| pread64 | 5/5/5 | 35.5/32.0/43.5 | 5/5/5 | 32.0/34.0/38.0 |
| read | 37/37/61 | 449.0/430.0/628.5 | 37/37/61 | 440.0/462.0/651.0 |
| rt_sigprocmask | 2/2/0 | 12.0/12.0/0.0 | 2/2/0 | 12.0/12.0/0.0 |
| statx | 30/30/0 | 226.0/221.0/0.0 | 30/30/0 | 208.5/217.5/0.0 |
| write | 1/1/1 | 14.5/14.0/17.5 | 1/1/1 | 15.0/15.0/17.0 |
| Total | 263/263/296 | 2262.5/2209.5/2604.0 | 265/265/300 | 2187.5/2255.5/2658.0 |

### zhuyin

| Call | Phrases count | Phrases µs | Journals count | Journals µs |
| --- | ---: | ---: | ---: | ---: |
| brk | 1/1/2 | 13.5/12.0/20.5 | 2/2/2 | 22.0/27.0/22.0 |
| close | 29/29/35 | 236.5/201.5/256.5 | 29/29/35 | 189.0/256.0/262.0 |
| fcntl | 24/24/24 | 176.5/143.0/150.0 | 24/24/24 | 145.5/193.0/154.5 |
| fstat | 6/6/8 | 45.5/36.0/56.5 | 6/6/8 | 36.5/49.5/54.0 |
| getpid | 7/7/13 | 49.5/41.5/85.5 | 7/7/13 | 42.5/58.5/83.5 |
| getrandom | 1/1/0 | 8.0/8.0/0.0 | 1/1/0 | 7.0/9.0/0.0 |
| lseek | 1/1/22 | 7.0/6.0/145.5 | 1/1/22 | 6.0/8.0/145.0 |
| mmap | 17/17/8 | 194.0/175.5/91.0 | 19/19/10 | 193.0/238.5/104.5 |
| munmap | 9/9/3 | 135.5/132.5/75.0 | 8/8/5 | 101.0/120.5/85.0 |
| newfstatat | 13/13/19 | 118.0/98.0/152.0 | 13/13/19 | 96.0/126.5/153.5 |
| openat | 36/36/38 | 406.5/331.5/410.0 | 36/36/38 | 329.0/467.5/428.5 |
| pread64 | 5/5/5 | 42.5/37.5/35.0 | 5/5/5 | 32.5/44.5/38.0 |
| read | 31/31/52 | 459.5/428.5/582.5 | 31/31/52 | 424.0/470.0/582.5 |
| rt_sigprocmask | 2/2/0 | 14.5/14.0/0.0 | 2/2/0 | 12.5/16.0/0.0 |
| statx | 27/27/0 | 230.5/220.0/0.0 | 27/27/0 | 194.5/254.5/0.0 |
| Total | 209/209/229 | 2152.0/1859.0/2067.0 | 211/211/233 | 1840.5/2361.0/2138.0 |

Ox makes fewer calls overall, with nine more `mmap` calls in every cell,
three to six more `munmap` calls and 27–30 `statx` calls absent from the pin.
These differences nominate mapping and metadata work for source review;
they do not quantify the native gap. In particular, traced total time is
lower for ox in three cells and higher in zhuyin phrases, whose same-bytes
control is substantially lower. It cannot support an excess-syscall-time
explanation. Reviewing whether pre-open metadata checks can share work with
file opening is a possible later candidate, subject to preserving the
required failure/diagnostic order.

## Reproduction

Follow [native setup](../runbooks/setup.md) and the
[measurement runbook](../runbooks/benches.md#native-teardown-workload).
Use the existing verified BDB oracle. Choose task-owned ext4 paths for
`CARGO_TARGET_DIR`, `STAGE`, `WORK`, `SUMMARY`, `REPORT` and `TMPDIR`, and
create `WORK`, `REPORT` and `TMPDIR`; `SUMMARY` must be empty and under the
worktree's ignored `target/`. Export those variables and `PIN` (the verified
BDB prefix) before entering the admitted shell. Refresh
main before a new campaign and record the revision actually measured.
Keep the host exclusive and the one admitted shell alive through both
stages; build and measurement commands below execute inside it.

```sh
just heavy -- bash

just packaging install libpinyin --prefix="$STAGE" \
  -- --locked --release --no-default-features --features bdb,shipped \
  >"$REPORT/build-pinyin.out" 2>"$REPORT/build-pinyin.err"
just packaging install libzhuyin --prefix="$STAGE" \
  -- --locked --release --no-default-features --features bdb \
  >"$REPORT/build-zhuyin.out" 2>"$REPORT/build-zhuyin.err"

uv run --no-project --managed-python \
  python -B -X lazy_imports=normal tools/bisection/contract-diff.py \
  bdb "$PIN" "$STAGE/lib/libpinyin.so.15.0.0" \
  --zhuyin-so "$STAGE/lib/libzhuyin.so.15.0.0" \
  --teardown --teardown-work-dir "$WORK" --teardown-cpu 2 \
  --teardown-cases phrases,journals --teardown-cache warm \
  --teardown-stage confirm >"$REPORT/confirm.jsonl" 2>"$REPORT/confirm.err"
```

The `time` records with `metric=init_ns` give the primary timing table.
Apply the issue's interval/STOP rule before the next command. If all gaps
are absent, stop without attribution. If one remains, while retaining the
same admission and artifacts:

```sh
uv run --no-project --managed-python \
  python -B -X lazy_imports=normal tools/bisection/contract-diff.py \
  bdb "$PIN" "$STAGE/lib/libpinyin.so.15.0.0" \
  --zhuyin-so "$STAGE/lib/libzhuyin.so.15.0.0" \
  --teardown --teardown-work-dir "$WORK" --teardown-cpu 2 \
  --teardown-cases phrases,journals --teardown-cache warm \
  --teardown-stage init-attribute --teardown-attribute-window init \
  --teardown-summary-dir "$SUMMARY" \
  >"$REPORT/attribution.jsonl" 2>"$REPORT/attribution.err"
exit
```

`init-resources`, `init-perf`, `init-samples`, `init-faults`, `init-syscalls`
and `init-pages` give their respective tables/statuses. `init-ir` and the
complete retained per-process Callgrind summaries provide dynamic totals,
simulated events, exclusive functions and inclusive call edges. All campaign
figures above come from these two invocations; none require another experiment.
The #1318 page figures cited earlier come from [#1318's own run](https://github.com/shenghaoc/oxpinyin/pull/1318#issuecomment-6433238871),
not these invocations.

Both stages exited 0. All measuring stages and their processes have ended.
All stdout/stderr, 36 raw Callgrind profiles and 40 parsed JSON summaries
remain local. No captures were uploaded. No issue was closed and no shipping
implementation, ranking file, kernel setting or dependency was changed.
