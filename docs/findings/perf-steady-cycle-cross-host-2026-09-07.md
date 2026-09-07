# Steady-cycle cross-host data record

## Purpose

Data only. No interpretation; see PR 3.

The keystroke-cycle harness has always reported one workload size, so no
record exists of how any cycle timing behaves as the amount of work in the
timed region changes. This document is the raw cross-host capture of that
sweep, taken with `PERF_REPEATS` (added on the harness branch named under
Method). It states what was measured and under what conditions, and stops
there: no ratios, no comparison between the two hosts' sections, and no
conclusion of any kind is drawn here.

Each host fills only its own Environment and Results sections.

## Method

| Property | Value |
|---|---|
| Harness SHA | `50afb7f68c9d7e7fa1f1d7a008eb6d3f43107946` (tip of `perf/steady-cycle-workload-knob`) |
| Script | `tools/bisection/run-perf-same-data.sh` driving `bisect --perf` |
| Workload sizes | `PERF_REPEATS` ∈ {1, 2, 4, 8, 16} — passes over the frozen 20-input corpus inside one timed cycle |
| Unit at size 1 | 20 inputs, one `pinyin_reset` each, 123 keystroke steps total; each step is one `pinyin_parse_more_full_pinyins` + `pinyin_guess_candidates` + `pinyin_get_n_candidate` |
| Rounds | 20 speed processes per cell per size, all four cells round-robin per round; `PERF_CYCLES=8`; `taskset -c 0` on every process |
| RAM | 10 processes per cell per size per mode (`ram-init`, `ram-cycle`) |
| Cold / steady | cold = cycle index 0 of each process; steady = pooled cycle indices 1..7 of every process (n = 140 samples per cell per size) |
| Aggregation | medians with 95% percentile-bootstrap CIs, `tools/bisection/perf-ci.py`, 10,000 resamples, seed 20260907 |
| Resampling unit | whole runs (processes), not individual cycles |
| Held fixed across sizes | database, `.so` binaries, backend, machine, configuration, and input structure — the two `oxpinyin-capi` artifacts are built once and reused for every size |

**Image provenance — required, not optional.** Each host must build the
container image from the harness SHA above, not from a cached image and not
from "the branch tip" as found later. A cached `oxpinyin-matrix` image built
before 2026-09-06 carries libpinyin **2.11.91**; the oracle pin moved to
`074a2219…` / **2.11.92** in `871139a1` on that date. The arm64 pass in this
document was taken on an image rebuilt at the harness SHA after that was
discovered. The amd64 pass must be built from the identical commit, or the two
hosts are running different oracles and the sections below are not describing
the same subject.

**Absolute times are not comparable across measurement sessions.** The
instrument carries a whole-session offset. This sweep's `n = 1` steady cells
run 9–11% faster than the arm64 record in
[perf-keycost-first-alloc-2026-09-07.md](perf-keycost-first-alloc-2026-09-07.md),
taken on the same machine with the same harness schedule:

| cell | that record | this sweep, `n = 1` |
|---|---:|---:|
| libpinyin-tkrzw steady | 8.686 | 7.759 |
| oxpinyin-tkrzw steady | 10.037 | 8.988 |
| libpinyin-kc steady | 8.565 | 7.779 |
| oxpinyin-kc steady | 10.107 | 9.013 |

All four cells moved in the same direction by a similar magnitude, so the
offset is a property of the session — thermal state, host load, or something
else not accounted for — and not of any one implementation. Two consequences
bind everything downstream of this document:

1. Only cells measured **within one session on one host** may be compared to
   each other. Absolute milliseconds from different sessions, even on the same
   machine, are not comparable.
2. The arm64 and amd64 sections below are captured on different machines at
   different times. Their absolute timings must **never** be placed side by
   side. Any cross-host comparison has to be made on within-session quantities
   that divide the offset out.

This is a constraint on method, recorded here so it is in force before any
interpretation begins rather than discovered during it.

## Environment — linux/arm64

| Property | Value |
|---|---|
| Host | Apple silicon, darwin 27 (`arm64`); Docker 29.7.2 |
| Container arch | `aarch64` — equal to the host arch; not emulated |
| Base image | `debian:testing@sha256:dab11cdb0a9dcf4bbd68f671635b35f1f726b452b92396875b69bb2c7daa42a9`, apt from `snapshot.debian.org/archive/debian/20260831T000000Z` |
| Built image | `oxpinyin-matrix:knob`, local build id `sha256:9b3f50e4340b7c5a3874c9f8754e4f0018f8f9da241c488e6bf7d53bd46e3774` (a local id, not a registry digest — reproduce by rebuilding at the harness SHA) |
| Kernel | `Linux 7.0.12-linuxkit #1 SMP PREEMPT Fri Aug 14 16:27:59 UTC 2026 aarch64` |
| CPU | Apple, 10 cores, 1 thread/core, 1 cluster; MemTotal 8,124,516 kB (container VM) |
| Toolchain | `rustc 1.97.1 (8bab26f4f 2026-07-14)`, `cargo 1.97.1 (c980f4866 2026-06-30)`; `rust-toolchain.toml` `channel = "1.97.1"` — match, no override |
| Compiler | `gcc (Debian 15.3.0-2) 15.3.0` |
| Cargo profile | `[profile.release]`: `lto = "fat"`, `codegen-units = 1`; no `debug`; **no `panic` override — `panic = "abort"` is absent** |
| Oracle | libpinyin **2.11.92**, pin `074a2219c90feaf962d0d24f034514033ece5f99`, built in-image from one SHA-verified checkout |
| libpinyin build flags | `./configure --disable-static --with-dbm=Tkrzw` and `--with-dbm=KyotoCabinet` |
| oxpinyin build flags | `cargo build --locked --release -p oxpinyin-capi --no-default-features --features {kyotocabinet,tkrzw}`; `NEEDED` verified per artifact |
| capi artifacts | KC `sha256:6c3c664b1a180afe…`, `NEEDED libkyotocabinet.so.16`; Tkrzw `sha256:8500f4d1be75dfae…`, `NEEDED libtkrzw.so.1`; 1,577,696 bytes each, stripped |
| Dataset | image-baked libpinyin installs, one shared directory per backend pair; the oxpinyin cells open the **same** directories (no oxpinyin-generated data) |
| Capture window | 2026-09-07T13:20:08Z – 2026-09-07T13:24:29Z (UTC, from the measuring container) |

## Results — linux/arm64

Medians [95% CI], milliseconds. `n` is `PERF_REPEATS`. 20 runs per cell per
size; 140 pooled samples for steady, 20 for cold.

### Cold and steady cycle

| cell | n | cold ms [95% CI] | steady ms [95% CI] |
|---|---|---|---|
| libpinyin-kc | 1 | 8.496 [8.231, 8.569] | 7.779 [7.741, 7.813] |
| libpinyin-kc | 2 | 16.800 [16.635, 17.149] | 15.710 [15.660, 15.792] |
| libpinyin-kc | 4 | 33.203 [33.063, 33.761] | 31.465 [31.391, 31.559] |
| libpinyin-kc | 8 | 65.009 [64.805, 65.092] | 63.021 [62.939, 63.052] |
| libpinyin-kc | 16 | 128.209 [128.093, 128.384] | 126.017 [125.936, 126.271] |
| libpinyin-tkrzw | 1 | 8.422 [8.253, 8.494] | 7.759 [7.684, 7.839] |
| libpinyin-tkrzw | 2 | 16.891 [16.621, 16.977] | 15.687 [15.391, 15.881] |
| libpinyin-tkrzw | 4 | 33.453 [33.147, 33.755] | 31.847 [31.687, 31.997] |
| libpinyin-tkrzw | 8 | 65.447 [64.965, 65.875] | 63.192 [62.933, 63.746] |
| libpinyin-tkrzw | 16 | 128.850 [128.445, 129.559] | 126.216 [126.045, 126.696] |
| oxpinyin-kc | 1 | 10.455 [10.177, 10.607] | 9.013 [8.924, 9.065] |
| oxpinyin-kc | 2 | 19.645 [19.402, 19.954] | 17.989 [17.892, 18.165] |
| oxpinyin-kc | 4 | 38.409 [38.057, 38.625] | 36.222 [36.109, 36.416] |
| oxpinyin-kc | 8 | 75.017 [74.616, 75.264] | 72.519 [72.310, 72.908] |
| oxpinyin-kc | 16 | 148.602 [147.888, 149.958] | 145.394 [144.988, 146.424] |
| oxpinyin-tkrzw | 1 | 10.483 [10.399, 10.870] | 8.988 [8.930, 9.131] |
| oxpinyin-tkrzw | 2 | 19.942 [19.755, 20.091] | 18.086 [18.004, 18.219] |
| oxpinyin-tkrzw | 4 | 38.511 [38.447, 39.334] | 36.533 [36.358, 36.768] |
| oxpinyin-tkrzw | 8 | 75.401 [75.125, 75.552] | 73.017 [72.759, 73.314] |
| oxpinyin-tkrzw | 16 | 148.474 [148.190, 150.510] | 146.058 [145.770, 147.256] |

### Init and first allocation

Not workload-dependent by construction — both precede the timed cycles — and
recorded so drift across the five passes is visible.

| cell | n | init ms [95% CI] | first alloc ms [95% CI] |
|---|---|---|---|
| libpinyin-kc | 1 | 0.761 [0.671, 0.968] | 0.000 [0.000, 0.000] |
| libpinyin-kc | 2 | 0.811 [0.762, 0.935] | 0.000 [0.000, 0.000] |
| libpinyin-kc | 4 | 0.947 [0.847, 1.004] | 0.000 [0.000, 0.000] |
| libpinyin-kc | 8 | 0.891 [0.768, 0.968] | 0.000 [0.000, 0.000] |
| libpinyin-kc | 16 | 1.019 [0.935, 1.181] | 0.000 [0.000, 0.000] |
| libpinyin-tkrzw | 1 | 0.766 [0.720, 1.013] | 0.000 [0.000, 0.000] |
| libpinyin-tkrzw | 2 | 0.734 [0.705, 0.752] | 0.000 [0.000, 0.000] |
| libpinyin-tkrzw | 4 | 0.801 [0.732, 0.820] | 0.000 [0.000, 0.000] |
| libpinyin-tkrzw | 8 | 0.761 [0.748, 0.778] | 0.000 [0.000, 0.000] |
| libpinyin-tkrzw | 16 | 0.772 [0.741, 0.831] | 0.000 [0.000, 0.000] |
| oxpinyin-kc | 1 | 0.924 [0.892, 1.049] | 0.001 [0.001, 0.001] |
| oxpinyin-kc | 2 | 0.972 [0.923, 1.037] | 0.001 [0.001, 0.001] |
| oxpinyin-kc | 4 | 1.049 [0.946, 1.223] | 0.001 [0.001, 0.001] |
| oxpinyin-kc | 8 | 1.055 [0.960, 1.145] | 0.001 [0.001, 0.001] |
| oxpinyin-kc | 16 | 1.101 [1.044, 1.294] | 0.001 [0.001, 0.001] |
| oxpinyin-tkrzw | 1 | 1.004 [0.948, 1.231] | 0.001 [0.001, 0.001] |
| oxpinyin-tkrzw | 2 | 1.002 [0.943, 1.041] | 0.001 [0.001, 0.001] |
| oxpinyin-tkrzw | 4 | 1.076 [1.049, 1.128] | 0.001 [0.001, 0.001] |
| oxpinyin-tkrzw | 8 | 1.061 [1.019, 1.158] | 0.001 [0.001, 0.001] |
| oxpinyin-tkrzw | 16 | 1.059 [0.966, 1.332] | 0.001 [0.001, 0.001] |

### Memory, per size

Medians, KiB, from the `ram-init` and `ram-cycle` modes (10 processes per cell
per size per mode). `rss-init`/`hwm-init` are the `after_init` snapshot, which
the harness takes after `pinyin_alloc_instance`; `rss-cycle`/`hwm-cycle` are
the `after_last` snapshot.

| cell | n | rss-init | hwm-init | rss-cycle | hwm-cycle |
|---|---|---:|---:|---:|---:|
| libpinyin-tkrzw | 1 | 12,576 | 12,576 | 19,740 | 19,740 |
| libpinyin-tkrzw | 2 | 12,950 | 12,950 | 19,236 | 19,236 |
| libpinyin-tkrzw | 4 | 12,440 | 12,440 | 18,948 | 18,948 |
| libpinyin-tkrzw | 8 | 11,892 | 11,892 | 19,396 | 19,396 |
| libpinyin-tkrzw | 16 | 13,102 | 13,102 | 19,576 | 19,576 |
| libpinyin-kc | 1 | 17,826 | 17,826 | 23,394 | 23,394 |
| libpinyin-kc | 2 | 17,868 | 17,868 | 23,166 | 23,166 |
| libpinyin-kc | 4 | 17,416 | 17,416 | 23,686 | 23,686 |
| libpinyin-kc | 8 | 17,374 | 17,374 | 23,024 | 23,024 |
| libpinyin-kc | 16 | 17,490 | 17,490 | 23,372 | 23,372 |
| oxpinyin-tkrzw | 1 | 13,696 | 13,696 | 23,596 | 23,596 |
| oxpinyin-tkrzw | 2 | 14,220 | 14,220 | 23,378 | 23,536 |
| oxpinyin-tkrzw | 4 | 13,886 | 13,886 | 23,732 | 23,896 |
| oxpinyin-tkrzw | 8 | 13,620 | 13,620 | 22,840 | 23,674 |
| oxpinyin-tkrzw | 16 | 14,282 | 14,282 | 23,612 | 23,686 |
| oxpinyin-kc | 1 | 19,438 | 19,438 | 27,996 | 28,136 |
| oxpinyin-kc | 2 | 19,192 | 19,192 | 28,086 | 28,298 |
| oxpinyin-kc | 4 | 19,898 | 19,898 | 27,652 | 27,996 |
| oxpinyin-kc | 8 | 19,360 | 19,360 | 28,314 | 28,532 |
| oxpinyin-kc | 16 | 19,546 | 19,546 | 27,642 | 28,244 |

## Environment — linux/amd64

_Not yet collected._

## Results — linux/amd64

_Not yet collected._

## Known caveats

Facts about what was and was not measured. Nothing here is a conclusion.

- **n = 20 runs per cell per size**, `PERF_CYCLES=8`, giving 140 pooled steady
  samples and 20 cold samples per cell per size. Cold is a single cycle per
  process, so its intervals rest on 20 samples, not 140.
- **Cold and steady are kept in separate columns and must not be pooled.** At
  `n > 1` a cold cycle contains one cold corpus pass followed by `n − 1` warm
  ones, so its composition changes with size. Steady cycles are fully warm at
  every size.
- **Backend durability class.** The two Tkrzw cells and the two Kyoto Cabinet
  cells are compared within their own backend pair. The oxpinyin cells'
  NOSYNC-class store behaviour is a standing property of this comparison and
  is not altered by the workload knob.
- **RSS and HWM did not vary with workload size** in any of the four cells
  (table above). The knob repeats the same 123 keystroke steps rather than
  enlarging the working set, which is consistent with that observation.
- **No training and no sentence decode is exercised.** The `--perf` path
  resolves neither `pinyin_train` nor `pinyin_guess_sentence`; the user
  directory is a fresh `mkdtemp` per process and is removed at exit. Any
  per-cycle cost associated with user-database growth or training is therefore
  outside this record entirely.
- **Only the drop-in configuration was measured.** The oxpinyin cells open the
  libpinyin installs' own data directories; no oxpinyin-generated data is
  involved, so this record says nothing about oxpinyin's own datagen output.
- **Single CPU, single container, one pass per size.** Every process ran under
  `taskset -c 0` inside one container invocation, sizes measured in ascending
  order. Sizes were not interleaved with each other, so slow drift over the
  ~4.5-minute capture window is not separated from size effects; the four cells
  *were* interleaved round-robin within each size.
- **The `.so` size, install size and cross-host comparisons of absolute times
  are not part of this document.** Absolute timings from different hosts are
  not comparable directly; each host's section stands on its own.
- **Captures.** Raw `speed.jsonl`, `ram-init.jsonl` and `ram-cycle.jsonl` for
  every size, carrying the per-cycle-index timings and the `repeats` stamp, are
  the auditable source for every table above. They are not committed; each
  host's agent reports where its own captures were written.
