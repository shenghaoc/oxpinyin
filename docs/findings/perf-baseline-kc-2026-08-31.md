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

Post-init values are from init-only processes; cycle-peak values are from
separate cycle processes. The two process groups may show different HWM
medians due to cross-process variance.

| Backend | post-init RSS | post-init HWM | cycle-peak HWM |
|---|---:|---:|---:|
| oracle | 13,324 KiB | 13,324 KiB | 19,108 KiB |
| oxpinyin | 72,652 KiB | 86,364 KiB | 85,152 KiB |

| Metric | oxpinyin/oracle | W8 (redb) ratio |
|---|---:|---:|
| post-init RSS | 5.45× | 8.22× |
| cycle-peak HWM | 4.46× | — |

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

The reported oxpinyin init and steady-cycle CVs are well within the 20%
variance gate. The oracle init shows high CV (165%) due to one cold-cache
outlier (4 ms vs median 0.87 ms); that metric does not satisfy the gate,
but the oracle median is robust and the ratio remains meaningful.

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

4. **KC eager initialization** at open time is the dominant init cost
   (~104 ms). The oracle avoids this via mmap-backed binary files with
   lazy page-fault loading. The steady-state improvement from W8 (2.19×
   → 1.08×) reflects the combined effect of the KC switch and the five
   perf-branch landings; a controlled KC-vs-redb comparison was not
   performed.

## KC vs redb summary

| Metric | redb (W8) | KC (this) | Direction |
|---|---:|---:|---|
| init ratio | 158× | ~118× | -25% |
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

## Amendments

| Date | Commit | Metric | Before | After | Note |
|---|---|---|---:|---:|---|
| 2026-09-03 | 17ae4bf | runtime data (interpolation2.text) | 80.0 MiB | 1.97 MiB (2,065,403 bytes) | \2-gram section omitted; already compiled into KC tables; byte-level verified; runtime parser yields identical UnigramTable (63,907 records, total 50,913,735). |
| 2026-09-03 | 0f6c8a4 | alloc | 6.223 ms | 0.003 ms | `key_cost_table` moved from per-`new_session` to once at `Runtime::open`; 440 dictionary lookups eliminated per alloc. Criterion bench added (`alloc_instance` group in `stage2.rs`). Before/after pair re-measured on the second host described below; the original M-series before-value (2.667 ms) is not comparable across hosts. |
| 2026-09-03 | 49a5dc0 | shared object (stripped), x86_64/redb host | 2,914,304 B (x86_64/redb, no release profile) | 2,694,568 B (−219,736, −7.54%; x86_64/redb) | `lto = "fat"`, `codegen-units = 1` added to `[profile.release]`; measured on x86_64/redb because that host lacks KC headers — not a change against the ARM64/KC baseline below. See `docs/perf/perf-so-size-2026-09.md`. |
| 2026-09-04 | 49a5dc0 | shared object (stripped), ARM64/KC host | 1,643,232 B (ARM64/KC at the rebase tip) | 1,446,528 B (−196,704, −11.97%) | ARM64/KC re-measurement of the same change; the ARM64/KC baseline at Correction 2 was 1,708,768 B stripped, but 30 commits (the P1–P8 data rewrite) landed between measurements, so this before/after pair was measured back-to-back at one tip instead of against Correction 2. `.text` −12.9%, unwind tables −42.3%, `.rodata` −30.0%; `guess_candidates/offset_0` 11.27 → 8.67 ns (−23.1%), no regression. |

### Amendment environment (2026-09-03, alloc row)

The alloc before/after pair was measured on a second host, because the
pinned model20 tables were unavailable where the change was written:
podman 5.8.2 on RHEL 10.2 (x86_64, 12 vCPU), guest Debian testing with
libkyotocabinet-dev 1.2.80-2+b2, libtkrzw-dev 1.0.32-1+b2, gcc 15.3.0,
Rust 1.97.1 (8bab26f4), cargo-c 0.10.25 — the same package set as the
image above, via a live mirror rather than the 20260831 snapshot.
Absolute numbers are not comparable with the M-series table at the top
of this document (host init alone: 244 ms vs 102 ms there); the
before/after ratio is internally consistent because both sides and the
oracle control ran alternating in one session.

- `bisect --perf` speed axis, n=20 processes per side, `taskset -c 0`:
  oracle alloc 0.001 ms [0.001, 0.001]; oxpinyin at `main` (`bf83ffb9`)
  6.223 ms [5.931, 8.001]; oxpinyin with the change 0.003 ms
  [0.003, 0.007]. Alloc ratio to the oracle: 7,872× → 4.2× (median to
  median, ~1,870×).
- The moved work reappears once at open: init 244.271 ms → 250.504 ms
  (+6.2 ms, +2.5%) on the same runs, matching the eliminated per-alloc
  cost.
- Criterion `alloc_instance` (`stage2.rs`, first run on this host):
  1.082 µs [1.080, 1.085] per alloc+free iteration in a warm loop.
- Running that bench needed a companion fix, landed with the change: the
  stage2 support module still staged `.redb` table names from the pre-KC
  era, so `pinyin_init` failed on the staged directory under the KC
  default; the names now derive from
  `oxpinyin_store::default_store_file` (the compiled-in backend).

The x86_64/redb figure in the first `.so` row is not the same build as
this baseline's ARM64/KC artifact (the redb backend compiles the
database engine into the `.so`; KC links it externally), so the ratio is
recorded per host. The second `.so` row is that ARM64/KC
re-measurement, run below.

### Amendment environment (2026-09-04, ARM64/KC `.so` row)

Measured in the ARM64 `oxpinyin-validate` container (Docker Desktop on
Apple Silicon, linux/arm64, Debian testing at the 20260831 snapshot,
libkyotocabinet-dev, Rust 1.97.1 (8bab26f4), cargo-c 0.10.25) — the
same environment family as the baseline above. Before = `53eb5b8`
(`origin/main` at measurement time); after = the same tree plus the
`[profile.release]` change. (The branch was rebased onto `30556ae`
afterwards; the two intervening commits touch bisection scripts and CI
only, and do not change the build artifact.) Both builds ran cold
(`cargo cinstall --locked --release -p oxpinyin-capi --prefix=/usr`,
verified via the `Compiling oxpinyin-capi` provenance lines), stripped
with `strip --strip-all`; the export tables were regenerated from the
pinned model20 by the after tree's datagen and shared by both sides.

- Stripped `.so`: 1,643,232 → 1,446,528 B (−196,704, −11.97%).
  Sections: `.text` −96,064 (−12.9%), `.rodata` −18,488 (−30.0%),
  unwind (`.eh_frame` + `.eh_frame_hdr`) −53,256 (−42.3%),
  `.data.rel.ro` −5,656 (−2.1%), `.rela.dyn` −7,488 (−2.2%).
- `guess_candidates/offset_0` (stage2 criterion, `taskset -c 0`, 4
  alternating rounds × 20 samples): before median 11.27 ns
  [11.209, 11.269], after median 8.67 ns [8.5754, 8.6970] — −23.1%,
  faster on every round. The absolute scale (ns, not the µs the x86_64
  host saw) reflects the P1–P8 data rewrite's candidate-path cost
  change, which affects both sides equally; the before/after comparison
  is internally consistent.
- Steady-state parity gates: `sentence_surface` §12 pin FAILED on both
  trees identically (1-best 491 vs the frozen 488) — a pre-existing
  drift from the P1–P8 rewrites, not an LTO effect (profiles cannot
  change deterministic output); `real_tables` fixture-freshness 2/2
  PASS; clippy `-D warnings` and `cargo fmt --check` clean.
- Measuring needed one companion fix, carried on this branch: the
  stage2 benches staged the pre-P1–P5 `.kct` table names, so
  `pinyin_init` failed under the KC default; they now stage the names
  `system_dbm_names` returns.
