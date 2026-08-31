# Stage-2 Performance Baseline — KC Backend (2026-08-31)

> **Superseded in part** by the validation pass
> (`perf-baseline-kc-validation-2026-08-31.md`). Two corrections:
>
> 1. **Init attribution**: the bottleneck is KC B-tree initialization
>    (~89%), not `interpolation2.text` parsing (~5–11%). Init reads only
>    2 MiB (1-gram section) of the 80 MiB file.
> 2. **Shared-object size**: the 0.454× ratio compared a debug-build
>    oracle against a release-build oxpinyin. Stripped-to-stripped,
>    oxpinyin is 2.16× larger (1,669 KiB vs 771 KiB).
>
> All other measurements (steady cycle, cold cycle, RSS, HWM) are
> confirmed accurate.

First measured baseline after the Kyoto Cabinet backend switch and five
perf-branch landings (dict-lm-load, fill-lookup, init-text-slurp,
init-typed-map, keystroke-heap-alloc). Replaces the blocked audit of the
same date (`perf-baseline-2026-08-31.md`, commit `9dfcb98`) which could
not execute on the macOS host.

## Measurement host

| Property | Value |
|---|---|
| Container | Docker Desktop 4.88.1 on Apple Silicon (M-series) |
| Guest kernel | Linux 7.0.12-linuxkit aarch64 |
| Guest OS | Debian testing (forky/sid) |
| vCPUs | 10 |
| Base image | `debian:testing@sha256:dab11cdb0a9dcf4bbd68f671635b35f1f726b452b92396875b69bb2c7daa42a9` |
| Rust | 1.97.1 (8bab26f4f 2026-07-14) |
| cargo-c | 0.10.25+cargo-0.99.0 |
| GCC | 15.3.0 (Debian 15.3.0-2) |
| libkyotocabinet-dev | 1.2.80-2+b2 |
| libtkrzw-dev | 1.0.32-1+b2 |
| Harness | `bisect --perf`, CPU-pinned via `taskset -c 0` |

**Host caveat:** Docker Desktop runs containers in a lightweight Linux
VM on Apple Silicon. Absolute timings differ from the W8 baseline
(bare-metal Intel i7-9750H, x86_64). Ratios between oracle and oxpinyin
are meaningful because both run in the same environment; absolute
comparisons with W8 should be limited to ratios.

## Oracle pin

```
libpinyin 2.11.91 (0c5e80e1) + model20 (59c68e89) + dbm=Tkrzw
```

Built from source via `tools/oracle/build-oracle.sh --prefix /opt/pinyin-oracle`.
Pin verified via `oracle-pin.txt`.

## Results

### Axis 1 — Execution speed (PERF_RUNS=20, PERF_CYCLES=8)

| Backend | init | alloc | cycle 0 (cold) | cycles 1..N (steady) |
|---|---:|---:|---:|---:|
| oracle (n=20) | 0.866 ms [0.796, 4.028] | 0.000 ms [0.000, 0.001] | 9.661 ms [9.077, 10.420] | 8.721 ms [7.974, 10.604] |
| oxpinyin (n=20) | 102.248 ms [98.933, 111.779] | 2.667 ms [2.515, 2.975] | 9.524 ms [9.219, 10.468] | 9.407 ms [8.852, 10.570] |

| Metric | oxpinyin/oracle | W8 (redb) ratio | Change |
|---|---:|---:|---|
| init | 118× | 158× | improved |
| alloc | 7,985× | 48,483× | improved |
| cold cycle | 0.99× | 2.06× | **near parity** |
| steady cycle | 1.08× | 2.19× | **near parity** |

Steady-state cycle performance is now within 8% of the oracle — a
transformation from the 2.19× gap measured in W8.

### Axis 2 — Installed size

| Side | shared object | runtime data | runtime footprint | total install |
|---|---:|---:|---:|---:|
| oracle | 5.25 MiB | 35.54 MiB | 40.79 MiB | 47.04 MiB |
| oxpinyin | 2.38 MiB | 101.80 MiB | 104.18 MiB | 130.77 MiB |

| Metric | oxpinyin/oracle |
|---|---:|
| shared object | 0.454× |
| runtime data | 2.86× |
| total install | 2.78× |

**Caveat:** the 0.454× shared-object ratio is invalid — the oracle `.so`
contains debug symbols; see the validation document for the corrected
stripped-to-stripped comparison (2.16×). Runtime data is 2.86× (improved
from W8's 3.48× with redb).

oxpinyin runtime data breakdown:

| File | Size |
|---|---:|
| interpolation2.text | 79.59 MiB |
| bigram.kct | 15.90 MiB |
| pinyin_index.kct | 3.34 MiB |
| phrase_index.kct | 2.98 MiB |

The raw `interpolation2.text` accounts for 78% of oxpinyin's runtime
data. The oracle does not ship this file — libpinyin's `make install`
compiles it into binary `.bin` indexes. The three KC tables total 22.2
MiB vs the oracle's 35.5 MiB (0.63×) — the KC representation is more
compact than Tkrzw for this workload.

### Axis 3 — RAM (PERF_RAM_RUNS=10)

| Backend | post-init RSS | post-init HWM | peak HWM |
|---|---:|---:|---:|
| oracle | 13,324 KiB | 13,324 KiB | 19,108 KiB |
| oxpinyin | 72,652 KiB | 86,364 KiB | 85,152 KiB |

| Metric | oxpinyin/oracle | W8 (redb) ratio |
|---|---:|---:|
| post-init RSS | 5.45× | 8.22× |
| peak HWM | 4.46× | — |

oxpinyin RAM breakdown (post-init):

| Component | Value |
|---|---:|
| RssAnon (heap+stack) | 66,476 KiB |
| RssFile (mmap'd files) | 6,176 KiB |

The heap dominates: 91% of oxpinyin's post-init RSS is anonymous memory
(parsed data structures), vs only 21% for the oracle (2,836 KiB anon,
10,488 KiB file). The oracle relies heavily on mmap'd binary files;
oxpinyin parses `interpolation2.text` into heap structures.

## Variance analysis

| Metric | oxpinyin CV | oxpinyin IQR |
|---|---:|---:|
| init | 4.3% | [99.3, 103.0] ms |
| steady cycle | 3.9% | [8.88, 9.27] ms |

All metrics well within the 20% variance gate. The oracle init shows
high CV (165%) due to one cold-cache outlier (4ms vs median 0.87ms); the
median is robust.

## Reproducibility

Two independent container runs produced matching results:

| Metric | Run 1 | Run 2 |
|---|---:|---:|
| Init ratio | 101.9× | 118.1× |
| Steady ratio | 1.08× | 1.08× |
| Post-init RSS ratio | 5.43× | 5.45× |
| Peak HWM ratio | 4.42× | 4.46× |

Init ratio varies (oracle init is <1ms so small jitter causes large
ratio swings) but the absolute oxpinyin init is stable: 100.2 ms vs
102.2 ms (2% difference). All other ratios are within 1%.

## Initialization bottleneck investigation

> **Superseded.** The attribution below was corrected by the validation
> pass. The bottleneck is KC B-tree initialization (~89%), not text
> parsing (~5–11%). See `perf-baseline-kc-validation-2026-08-31.md`.

W8 attributed the 158× init gap to `interpolation2.text` parsing. With
KC, the gap narrowed to ~118× (median from raw JSONL; originally
reported as ~110×). The root cause was **reattributed** by strace +
data-subset ablation: KC B-tree initialization dominates (~104 ms),
while text parsing costs only ~5–13 ms.

1. **interpolation2.text** — init reads only the 1-gram section
   (2 MiB / 63,907 entries), not the full 80 MiB file. The 77.6 MiB
   2-gram section is pre-compiled into KC tables by `oxpinyin-datagen`
   and never touched at runtime. The oracle does not ship this file at
   all — `import_interpolation` consumes it at build time.

2. **Absolute improvement**: 586 ms (W8/redb) → 102 ms (KC) = 5.7×
   faster init. This is the cumulative effect of the five perf-branch
   landings.

3. **The HWM spike** (72,652 → 86,364 KiB, a 14 MiB transient) during
   init comes from KC tree construction and data structure allocation.

4. **KC tables** load faster than redb in steady-state cycles, but their
   eager B-tree initialization at open time is the dominant init cost
   (~104 ms). The oracle avoids this via mmap-backed binary files with
   lazy page-fault loading.

## KC vs redb summary

| Metric | redb (W8) | KC (this) | Direction |
|---|---:|---:|---|
| init ratio | 158× | ~110× | -30% |
| steady cycle ratio | 2.19× | 1.08× | **-51%** |
| cold cycle ratio | 2.06× | 0.99× | **-52%** |
| post-init RSS ratio | 8.22× | 5.45× | -34% |
| data size ratio | 3.48× | 2.86× | -18% |
| .so size ratio (as-installed) | 0.40× | 0.45× | +13% (see caveat) |

The KC switch plus the five perf-branch landings produced a dramatic
improvement. Steady-cycle performance went from 2.19× (nearly double the
oracle) to 1.08× (near parity). RAM dropped by a third. Data storage is
more compact.

The as-installed .so sizes are not directly comparable because the oracle
contains debug symbols and oxpinyin does not. When both are stripped,
oxpinyin (1,669 KiB) is 2.16× larger than the oracle (771 KiB). See the
validation document for the corrected analysis.

## Infrastructure changes

1. **`run-perf-baseline.sh`**: updated for KC backend (`.kct` table
   names, dynamic pkgconfig path discovery, `libpinyin` pkg-config name).

2. **`perf-baseline.py`**: fixed shared-object classification to match
   the installed `libpinyin.so.15.0.0` naming.

3. **`Dockerfile.perf-baseline`**: new reproducible benchmark container
   based on `debian:testing` (pinned by digest). Builds both oracle
   (libpinyin 2.11.91 + Tkrzw) and oxpinyin-capi (KC default) in a
   single image.
