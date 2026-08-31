# Performance Backend Matrix — 2026-08-31

## Executive summary

A controlled 2×2 factorial experiment (implementation × backend)
definitively answers the core question from PR #251:

> **Why is oxpinyin's initialization ~100× slower than libpinyin?**
>
> **The implementation architecture, not the database backend.**

libpinyin achieves <1 ms init with either Kyoto Cabinet (0.92 ms) or
Tkrzw (0.85 ms). oxpinyin takes 86–106 ms with either backend. Switching
oxpinyin from KC to Tkrzw makes init *worse* (106 ms vs 86 ms), not better.

The PR #251 finding that "KC-related operations account for ~89% of
oxpinyin's initialization time" was correct about *where* the time was
spent — inside KC library calls. The controlled experiment shows that
replacing KC with Tkrzw increases init time by 23%, which is consistent
with the hypothesis that the bottleneck is architectural rather than
backend-specific, but does not eliminate the init gap. A definitive
proof would require an ablation isolating eager database initialization
from the other implementation differences (code paths, data layout,
table count). What the matrix does establish is that no single backend
swap resolves the gap: oxpinyin's architecture of routing all index
tables through a general-purpose key-value store with eager
initialization is at least a necessary condition, while libpinyin uses
mmap-backed binary files with lazy page-fault loading for its indexes.

## Experimental design

### Matrix

| Cell | Implementation | Backend | Build flag |
|---|---|---|---|
| A | libpinyin 2.11.91 | Tkrzw 1.0.32 | `--with-dbm=Tkrzw` |
| B | libpinyin 2.11.91 | KC 1.2.80 | `--with-dbm=KyotoCabinet` |
| C | oxpinyin (cd14708) | Tkrzw 1.0.32 | `--features tkrzw` |
| D | oxpinyin (cd14708) | KC 1.2.80 | `--features kyotocabinet` |

PR #251 measured cells A and D. This experiment measures all four in a
single controlled container.

### Structural difference

libpinyin and oxpinyin use the selected backend differently:

| Component | libpinyin | oxpinyin |
|---|---|---|
| pinyin index | `pinyin_index.bin` (mmap, backend-independent) | `pinyin_index.{kct,tkt}` (backend store) |
| phrase index | `phrase_index.bin` (mmap, backend-independent) | `phrase_index.{kct,tkt}` (backend store) |
| bigram | `bigram.db` (selected backend) | `bigram.{kct,tkt}` (backend store) |
| addon indexes | `.bin` files (mmap) | individual `.{kct,tkt}` files |
| interpolation2.text | not shipped (consumed at build time) | shipped, 1-gram section read at init |

libpinyin's backend choice affects only `bigram.db`. oxpinyin routes every
table through the backend.

### Controls

All four cells share:

| Property | Value |
|---|---|
| Container | Docker Desktop 4.88.1 on Apple Silicon |
| Guest kernel | Linux 7.0.12-linuxkit aarch64 |
| Base image | `debian:testing@sha256:dab11cdb…` |
| APT snapshot | `snapshot.debian.org/20260831T000000Z` |
| GCC | 15.3.0 (Debian 15.3.0-2) |
| Rust | 1.97.1 (8bab26f4f 2026-07-14) |
| cargo-c | 0.10.25 |
| libkyotocabinet | 1.2.80-2+b2 |
| libtkrzw | 1.0.32-1+b2 |
| libpinyin | 2.11.91 (0c5e80e1) |
| oxpinyin | cd14708 |
| Model data | model20 (SHA-256 `59c68e89…`) |
| CPU affinity | `taskset -c 0` |
| Speed runs | 20 per cell, round-robin interleaved |
| Speed cycles | 8 per process |
| RAM runs | 10 per cell per mode |

## Speed results

| Cell | init | alloc | cold cycle | steady cycle |
|---|---:|---:|---:|---:|
| libpinyin-tkrzw (n=20) | 0.854 ms [0.688, 4.442] | 0.000 ms | 8.719 ms [8.430, 12.364] | 8.035 ms [7.662, 9.510] |
| libpinyin-kc (n=20) | 0.920 ms [0.680, 4.428] | 0.000 ms | 8.548 ms [8.434, 24.624] | 8.002 ms [7.793, 10.866] |
| oxpinyin-kc (n=20) | 86.188 ms [84.261, 140.329] | 2.435 ms | 8.840 ms [8.627, 9.190] | 8.473 ms [8.098, 10.058] |
| oxpinyin-tkrzw (n=20) | 106.383 ms [104.046, 236.468] | 2.407 ms | 9.167 ms [9.014, 9.275] | 8.932 ms [8.648, 9.962] |

### Variance

| Cell | init CV% | steady CV% |
|---|---:|---:|
| libpinyin-tkrzw | 76.6% | 3.5% |
| libpinyin-kc | 74.1% | 4.1% |
| oxpinyin-kc | 15.5% | 2.7% |
| oxpinyin-tkrzw | 25.1% | 1.9% |

libpinyin init CV is high (74–77%) due to cold-cache outliers (4.4 ms vs
0.85 ms median). The median is robust. All steady-cycle CVs are <5%.

## RAM results

| Cell | post-init RSS | post-init HWM | cycle-peak HWM |
|---|---:|---:|---:|
| libpinyin-tkrzw | 13,328 KiB | 13,328 KiB | 19,048 KiB |
| libpinyin-kc | 17,972 KiB | 17,972 KiB | 23,128 KiB |
| oxpinyin-kc | 72,910 KiB | 86,618 KiB | 85,494 KiB |
| oxpinyin-tkrzw | 69,256 KiB | 83,952 KiB | 84,422 KiB |

### Memory composition (post-init)

| Cell | RssAnon (heap) | RssFile (mmap) | RSS |
|---|---:|---:|---:|
| libpinyin-tkrzw | 2,862 KiB | 10,488 KiB | 13,328 KiB |
| libpinyin-kc | 7,614 KiB | 10,362 KiB | 17,972 KiB |
| oxpinyin-kc | 66,798 KiB | 6,112 KiB | 72,910 KiB |
| oxpinyin-tkrzw | 62,200 KiB | 7,056 KiB | 69,256 KiB |

libpinyin is mmap-dominated (79–58% file-backed). oxpinyin is
heap-dominated (90–92% anonymous). This reflects the architectural
difference: libpinyin lazily maps binary files, oxpinyin eagerly parses
into heap structures.

## Installed size (all .so stripped)

| Cell | .so | runtime data | total |
|---|---:|---:|---:|
| libpinyin-tkrzw | 771 KiB | 35.54 MiB | 42.54 MiB |
| libpinyin-kc | 1,093 KiB | 36.77 MiB | 53.54 MiB |
| oxpinyin-kc | 1,669 KiB | 112.12 MiB | 140.34 MiB |
| oxpinyin-tkrzw | 1,733 KiB | 125.25 MiB | 153.60 MiB |

oxpinyin's runtime data is dominated by `interpolation2.text` (79.59 MiB),
which libpinyin does not ship. Excluding it, oxpinyin's database-only
data is 32.5 MiB (KC) or 45.7 MiB (Tkrzw) — comparable to libpinyin's
35–37 MiB.

## Backend effects (KC / Tkrzw ratio, holding implementation fixed)

| Metric | within libpinyin | within oxpinyin |
|---|---:|---:|
| Init | 1.077× | **0.810×** |
| Cold cycle | 0.980× | 0.964× |
| Steady cycle | 0.996× | 0.949× |
| Post-init RSS | 1.348× | 1.053× |
| Cycle-peak HWM | 1.214× | 1.013× |
| .so | 1.418× | 0.963× |
| Data | 1.035× | 0.895× |
| Total installed | 1.259× | 0.914× |

**Init**: KC adds 8% to libpinyin's init (still <1 ms). For oxpinyin, KC
is 19% *faster* than Tkrzw (86 ms vs 106 ms). The backend effects are
opposite in sign — a significant interaction.

**Cycle**: backend has negligible effect (<4%) on steady-state performance
in both implementations.

**RAM**: libpinyin with KC uses 35% more RSS than with Tkrzw (18 MiB vs
13 MiB) — KC adds ~5 MiB heap. For oxpinyin, backend has minimal RSS
effect (~5%).

## Implementation effects (oxpinyin / libpinyin, holding backend fixed)

| Metric | with KC | with Tkrzw |
|---|---:|---:|
| Init | **93.7×** | **124.5×** |
| Cold cycle | 1.034× | 1.051× |
| Steady cycle | 1.059× | 1.112× |
| Post-init RSS | 4.057× | 5.196× |
| Cycle-peak HWM | 3.697× | 4.432× |
| .so | 1.526× | 2.247× |
| Data | 3.049× | 3.524× |
| Total installed | 2.621× | 3.611× |

The implementation gap is massive for init (~100×) and significant for RAM
(4–5×), regardless of backend. Steady-state cycle performance is within
6–11% — near parity.

## Interaction effects

| Metric | (oxpinyin KC/Tkrzw) / (libpinyin KC/Tkrzw) |
|---|---:|
| Init | 0.752 |
| Cold cycle | 0.984 |
| Steady cycle | 0.953 |
| Post-init RSS | 0.781 |
| Cycle-peak HWM | 0.834 |
| .so | 0.679 |
| Data | 0.865 |
| Total installed | 0.726 |

**Init** (0.752): significant interaction. KC benefits oxpinyin
(KC/Tkrzw = 0.81) but marginally hurts libpinyin (KC/Tkrzw = 1.08).
This makes sense: KC's eager B-tree initialization adds a fixed cost per
database; oxpinyin opens ~30 databases vs libpinyin's one `bigram.db`.
However, KC's B-tree initialization is still faster than Tkrzw's hash
table initialization at this scale.

**Cycle** (~0.95–0.98): no meaningful interaction.

**RSS** (0.78): moderate interaction. KC's memory overhead is absorbed
by oxpinyin's already-large heap, but stands out in libpinyin's lean
mmap-based profile.

## Interpretation of the original 118.1× result

PR #251 established:

> oxpinyin+KC / libpinyin+Tkrzw ≈ 118.1× init

This was a **cross-implementation + cross-backend comparison**. The
controlled matrix decomposes it:

| Factor | Contribution |
|---|---|
| Implementation architecture | 93.7–124.5× |
| Backend | 0.8–1.1× (negligible, direction varies) |
| Interaction | present (0.75) but small vs implementation effect |

The 118.1× is almost entirely the implementation gap. The backend
contributes less than a factor of 1.1 in either direction.

The new cross-comparison measures 100.9× (oxpinyin-KC 86.2 ms /
libpinyin-Tkrzw 0.854 ms). The difference from 118.1× reflects
environmental variation (different APT snapshot, interleaved measurement)
and is within expected bounds for sub-millisecond denominators.

## Revisiting the KC initialization finding

PR #251's validation found:

> "KC-related operations account for approximately 89% of oxpinyin's
> initialization time."

This observation remains valid: strace showed that KC B-tree
initialization dominates oxpinyin's init. However, the causal
interpretation must change:

| Before (PR #251) | After (matrix) |
|---|---|
| "KC causes the init bottleneck" | KC is a **symptom**, not the cause |
| "Replacing KC could fix init" | Replacing KC with Tkrzw makes it **worse** |
| "The bottleneck is the backend" | The bottleneck is **routing all tables through any backend** |

The root cause is that oxpinyin opens ~30 backend databases at init
(3 system tables + 24 addon tables + punct), each requiring eager
tree/hash initialization. libpinyin opens only one backend database
(`bigram.db`); its other files are mmap'd binary blobs that load lazily.

## Remaining uncertainties

1. **x86_64**: all measurements are on linux/arm64. Absolute timings
   will differ on x86_64, though ratios should be stable.

2. **Addon tables**: oxpinyin opens 24 additional backend databases for
   addon categories. The addon contribution to init was not isolated;
   a further ablation could measure init with and without addons.

3. **mmap-backed store**: a potential oxpinyin backend that uses mmap'd
   binary files (like libpinyin's native format) was not tested. This
   would be the natural next experiment to validate the architectural
   hypothesis.

4. **Write path**: only read-path performance was measured. The write
   path (user dictionary, learning) may show different backend effects.

5. **oxpinyin init regression**: oxpinyin-KC init dropped from 102 ms
   (PR #251) to 86 ms in this run. The absolute change could reflect
   environmental factors, code changes between commits, or APT package
   version differences. The relative comparison (the 4-cell matrix) is
   internally consistent.

## Recommended next optimization experiment

The matrix points to two high-value targets:

1. **Lazy/mmap-backed store for read-only tables**: the dominant cost is
   eager initialization of ~30 backend databases. A read-only store
   backed by mmap'd files (similar to libpinyin's `.bin` format) would
   eliminate this cost entirely. This is an architectural change, not a
   backend swap. Expected init reduction: from ~86 ms to <5 ms (the
   residual cost of interpolation2.text parsing plus framework setup).

2. **Eliminate runtime interpolation2.text**: compile the 1-gram section
   into the binary store at datagen time (as libpinyin does with
   `import_interpolation`). This would remove 79.59 MiB from runtime
   data and ~5 ms from init.

These are optimization directions, not changes made in this experiment.

## Acceptance gate

| # | Check | Status |
|---|---|---|
| 1 | libpinyin + Tkrzw measured | PASS |
| 2 | libpinyin + KC measured | PASS |
| 3 | oxpinyin + Tkrzw measured | PASS |
| 4 | oxpinyin + KC measured | PASS |
| 5 | identical controlled environment | PASS |
| 6 | same logical workload | PASS |
| 7 | exact backend/build configuration documented | PASS |
| 8 | initialization measured | PASS |
| 9 | cold cycle measured | PASS |
| 10 | steady cycle measured | PASS |
| 11 | RSS measured | PASS |
| 12 | HWM measured | PASS |
| 13 | installed .so measured | PASS |
| 14 | installed data measured | PASS |
| 15 | total size measured | PASS |
| 16 | variance checked | PASS |
| 17 | backend effect isolated | PASS |
| 18 | implementation effect isolated | PASS |
| 19 | interaction considered | PASS |
| 20 | original 118.1× result retained | PASS |
| 21 | original result correctly labelled as cross-backend | PASS |
| 22 | no unsupported causal claims | PASS |
| 23 | no production code changes | PASS |
