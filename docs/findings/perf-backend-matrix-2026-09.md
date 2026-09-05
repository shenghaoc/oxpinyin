# Performance Backend Matrix — 2026-09-05

## Executive summary

The 4-cell backend matrix re-measured after the P1–P6 data-layer
inversion, on x86_64, with **one shared data directory per backend**
read by both implementations. The headline change against the
[2026-08-31 matrix](perf-backend-matrix-2026-08-31.md): the data
directory is no longer a per-implementation variable. oxpinyin reads
and writes libpinyin's own compiled file formats, so
`oxpinyin-datagen --backend tkrzw` output is opened unchanged by a
libpinyin built `--with-dbm=Tkrzw` (and likewise for Kyoto Cabinet) —
verified in-session before any timing was taken.

Post-P6, on this host:

- **init is near parity**: oxpinyin inits at 1.16× (Tkrzw) / 1.12×
  (KC) of libpinyin, down from 93.7×/124.5× on 2026-08-31.
- **The deferred key-cost walk moved, not disappeared**: oxpinyin's
  first `pinyin_alloc_instance` costs 56.9 ms (Tkrzw) / 42.1 ms (KC).
  init + first alloc totals 30.9× / 10.6× of libpinyin.
- **Steady-state cycles are at parity** — 0.94×/0.95×, i.e. oxpinyin
  marginally faster on this host (on ARM64 at the same pin it measured
  1.14× slower; the sign of this small gap is host-dependent). Caveat:
  the pin builds oxpinyin with release `panic = "abort"`, which on
  ARM64 cost +5.7% steady cycle (`a41605ea`, reverted at `1b0c84a0`) —
  these steady ratios are slightly pessimistic for oxpinyin;
  post-revert production is marginally faster.
- **Cold-cycle cost swapped sides**: libpinyin-Tkrzw pays ~20 ms of
  lazy paging in its first keystroke cycle (48.4 vs 28.6 ms steady;
  peak HWM jumps 12.6 → 32.7 MiB), because oxpinyin's first-alloc walk
  has already touched its pages. The fair per-keystroke totals
  (init + first alloc + cold cycle) are 1.71× (Tkrzw) / 2.25× (KC).
- **RAM**: post-init RSS 2.40×/1.62×; oxpinyin is now mmap-dominated
  (RssFile > RssAnon), where pre-P6 it was ~90% heap.

These are x86_64 numbers from a single self-contained session. The
ARM64 documents (`perf-baseline-kc-2026-09.md`, the 2026-08-31 matrix)
are different hosts and are not comparable in absolute value; the cell
ratios are the result of this measurement.

## Experimental design

### Matrix

| Cell | Implementation | Backend | Build |
|---|---|---|---|
| A | libpinyin 2.11.91 (0c5e80e1) | Tkrzw 1.0.32 | `./configure --with-dbm=Tkrzw` |
| B | libpinyin 2.11.91 (0c5e80e1) | Kyoto Cabinet 1.2.80 | `./configure --with-dbm=KyotoCabinet` |
| C | oxpinyin (`b5fdfad8`) | Tkrzw 1.0.32 | `cargo cinstall --no-default-features --features tkrzw` |
| D | oxpinyin (`b5fdfad8`) | Kyoto Cabinet 1.2.80 | `cargo cinstall --no-default-features --features kyotocabinet` |

oxpinyin was built from `git archive b5fdfad8` — the same pin the KC
baseline document was amended at. `b5fdfad8` still carries release
`panic = "abort"` (reverted later at `1b0c84a0`, after this pin).

### Structural notes — same data directory per backend

- **Sharing (the S1 gate).** Both cells of a backend pair opened the
  *same* directory in every measured process:
  `/session/data-tkrzw` (compiled by `oxpinyin-datagen compile
  --backend tkrzw`) and `/session/data-kc` (`--backend
  kyotocabinet`). Each directory holds the six DBM files under
  libpinyin's own names (`pinyin_index.bin`, `phrase_index.bin`,
  `bigram.db`, `punct.bin`, `addon_pinyin_index.bin`,
  `addon_phrase_index.bin`), the sixteen `MemoryChunk` chunk files,
  `table.conf` in libpinyin's format, and the datagen manifest — 24
  files. The store layer writes those DBMs through the *system*
  `libtkrzw`/`libkyotocabinet` (bindgen-bound), so the bytes are the
  libraries' own container formats. Before measurement, every cell was
  smoke-run to a full keystroke cycle on its shared directory (exit 0);
  after all 160 measurement processes the directories still held
  exactly those 24 files — no sidecars, no lock files.
- **Data format after P6.** The 2026-08-31 matrix's structural table
  ("oxpinyin routes every table through the backend store") is dead:
  post-P6 oxpinyin keeps libpinyin's split — backend containers only
  for the six DBMs, shared `MemoryChunk` chunk files for the libraries
  — and no `interpolation2.text` ships at all (removed at `3f0f0f36`).
  The 2026-08-31 confound of four separately compiled data directories
  is therefore gone: one compilation per backend, one reader set.
- **Chunk files are backend-independent.** All sixteen `.bin` chunk
  files are byte-identical in size across the two directories (same
  `MemoryChunk` serialization); only the six DBMs differ
  (Tkrzw 38.34 MiB vs KC 36.88 MiB total, 4% apart).

### Controls

| Property | Value |
|---|---|
| Host | Intel Core i7-9750H (6C/12T), x86_64, kernel 6.12.0-211.22.1.el10_2 |
| Runtime | podman 5.8.2, rootless, **one** container for builds and measurement |
| Base image | `debian:testing@sha256:dab11cdb…` (manifest digest, amd64) |
| APT snapshot | `snapshot.debian.org/20260831T000000Z` |
| GCC | 15.3.0 (Debian 15.3.0-2) |
| Rust | 1.97.1 (8bab26f4f 2026-07-14), from `rust-toolchain.toml` |
| cargo-c | 0.10.25+cargo-0.99.0 |
| libtkrzw / libkyotocabinet | 1.0.32-1+b2 (`libtkrzw1t64`) / 1.2.80-2+b2 (`libkyotocabinet16v5`) |
| libpinyin source | 2.11.91 tarball, SHA-256 `eb25890d…`, built twice (fresh trees) |
| oxpinyin source | `git archive b5fdfad8`, fresh per-backend target dirs |
| Model data | model20 (`59c68e89…`), fresh `fetch-model.sh` download |
| Data directories | 2 shared, `oxpinyin-datagen`-compiled (see above) |
| CPU affinity | `taskset -c 0` on every run |
| Speed | 20 interleaved rounds × 4 cells, `PERF_CYCLES=8` |
| RAM | 10 interleaved rounds × 4 cells × {ram-init, ram-cycle} |
| Round-robin order | libpinyin-tkrzw → oxpinyin-tkrzw → libpinyin-kc → oxpinyin-kc |
| Harness | `bisect --perf`, built fresh (`gcc -O2`, `bisect.c` at `b5fdfad8`) |

Every build in the session started from source; no artifact from any
earlier (ARM64) session was reused. The host is a shared dev box —
medians and interleaving absorb background load; upper-tail outliers
remain visible in the ranges.

## Matrix

Medians; brackets are 95% percentile-bootstrap CIs of the median
(10,000 resamples, whole-run resampling for cycle metrics); ranges in
parentheses. n = 20 processes per cell (speed), 10 per mode (RAM);
steady pools cycles 1..7 of every run.

| Cell | init | first alloc | cold cycle | steady cycle | post-init RSS | stripped `.so` |
|---|---:|---:|---:|---:|---:|---:|
| libpinyin-tkrzw | 1.915 [1.791, 2.004] ms | 0.001 ms | 48.388 [44.585, 69.010] ms | 28.615 [26.732, 40.162] ms | 12,648 KiB | 867,248 B |
| oxpinyin-tkrzw | 2.218 [2.121, 2.398] ms | 56.932 [54.958, 59.893] ms | 27.003 [25.187, 38.880] ms | 26.841 [25.451, 31.376] ms | 30,294 KiB | 1,523,576 B |
| libpinyin-kc | 4.431 [4.292, 4.604] ms | 0.001 ms | 28.947 [26.483, 46.708] ms | 28.610 [27.205, 34.493] ms | 17,280 KiB | 1,209,640 B |
| oxpinyin-kc | 4.964 [4.847, 5.103] ms | 42.128 [41.502, 42.900] ms | 28.082 [25.437, 40.428] ms | 27.216 [25.432, 39.762] ms | 27,944 KiB | 1,499,984 B |
| **oxpinyin ÷ libpinyin (Tkrzw)** | **1.16×** | 79,182× (vs 0.7 µs) | 0.56× | **0.94×** | **2.40×** | **1.76×** |
| **oxpinyin ÷ libpinyin (KC)** | **1.12×** | 38,091× (vs 1.1 µs) | 0.97× | **0.95×** | **1.62×** | **1.24×** |

The first-alloc ratios against sub-microsecond denominators say
nothing (the ARM64 baseline made the same observation at 52,983×); the
load-bearing totals:

| Metric | libpinyin-tkrzw | oxpinyin-tkrzw | libpinyin-kc | oxpinyin-kc |
|---|---:|---:|---:|---:|
| init + first alloc | 1.916 ms | 59.150 ms (**30.9×**) | 4.432 ms | 47.092 ms (**10.6×**) |
| init + first alloc + cold (time to first result) | 50.304 ms | 86.153 ms (**1.71×**) | 33.379 ms | 75.174 ms (**2.25×**) |

## RAM

| Cell | post-init RSS | post-init HWM | cycle-peak HWM |
|---|---:|---:|---:|
| libpinyin-tkrzw | 12,648 [12,572, 12,694] KiB | 12,648 KiB | 32,676 KiB |
| oxpinyin-tkrzw | 30,294 [30,240, 30,348] KiB | 30,294 KiB | 32,104 KiB |
| libpinyin-kc | 17,280 [17,204, 17,296] KiB | 17,280 KiB | 22,730 KiB |
| oxpinyin-kc | 27,944 [27,908, 28,028] KiB | 27,944 KiB | 29,358 KiB |

Post-init composition (medians):

| Cell | RssAnon (heap) | RssFile (mmap) | RSS |
|---|---:|---:|---:|
| libpinyin-tkrzw | 1,464 KiB | 11,186 KiB | 12,648 KiB |
| oxpinyin-tkrzw | 13,340 KiB | 16,956 KiB | 30,294 KiB |
| libpinyin-kc | 6,220 KiB | 11,060 KiB | 17,280 KiB |
| oxpinyin-kc | 11,160 KiB | 16,784 KiB | 27,944 KiB |

## Installed size

| Cell | `.so` unstripped | `.so` stripped | data directory |
|---|---:|---:|---:|
| libpinyin-tkrzw | 5,734,384 B | 867,248 B | shared, 40,198,858 B (38.34 MiB) |
| libpinyin-kc | 12,915,960 B | 1,209,640 B | shared, 38,672,513 B (36.88 MiB) |
| oxpinyin-tkrzw | 1,679,344 B | 1,523,576 B | shared (same dir as libpinyin-tkrzw) |
| oxpinyin-kc | 1,648,376 B | 1,499,984 B | shared (same dir as libpinyin-kc) |

Both cells of a pair count the *same* data directory — the drop-in
configuration a distribution would ship.

## Backend effects (KC ÷ Tkrzw, implementation fixed)

| Metric | within libpinyin | within oxpinyin |
|---|---:|---:|
| Init | 2.31× | 2.24× |
| First alloc | — (µs) | 0.74× |
| Cold cycle | 0.60× | 1.04× |
| Steady cycle | 1.00× | 1.01× |
| Post-init RSS | 1.37× | 0.92× |
| Cycle-peak HWM | 0.70× | 0.91× |
| Stripped `.so` | 1.39× | 0.98× |
| Data | 0.96× | 0.96× |

KC's open cost dominates its profile on this host: +2.5 ms init for
libpinyin and +2.7 ms for oxpinyin (six DBM opens either way), plus
~4.6 MiB heap for libpinyin (KC B-tree working set). Within oxpinyin,
KC *repays* at first alloc: the 440-read key-cost walk is 26% cheaper
through KC's B-tree than tkrzw's tree on this host (42.1 vs 56.9 ms).
Steady cycles and the `.so` are backend-neutral.

## Implementation effects (oxpinyin ÷ libpinyin, backend fixed)

| Metric | Tkrzw | KC |
|---|---:|---:|
| Init | 1.16× | 1.12× |
| init + first alloc | 30.9× | 10.6× |
| Time to first result | 1.71× | 2.25× |
| Cold cycle | 0.56× | 0.97× |
| Steady cycle | 0.94× | 0.95× |
| Post-init RSS | 2.40× | 1.62× |
| Cycle-peak HWM | 0.98× | 1.29× |
| Stripped `.so` | 1.76× | 1.24× |

Interaction ((ox KC/Tk) ÷ (lp KC/Tk)): init 0.97, steady 1.01, RSS
0.67, `.so` 0.71, cold 1.74 — the cold-cycle "interaction" is an
artefact of libpinyin-Tkrzw's first-cycle paging (below), not a real
backend sensitivity of either implementation.

## Per-axis attribution

- **init (1.12–1.16×).** After the `87f9a49e` deferral, oxpinyin's
  `pinyin_init` is six DBM opens, `table.conf`, the chunk mappings and
  the user store — structurally the same work libpinyin does, and now
  priced within ~15% of it. The absolute gap (0.3–0.5 ms) is mostly
  the KC/Tkrzw open cost both sides share on this host.
- **first alloc (the walk).** The 56.9/42.1 ms is `key_cost_table`:
  440 point reads into `pinyin_index.bin` filling the
  visibility-masked cache on `Runtime` (the ARM64 KC baseline measured
  the same walk at 17.6 ms — this host is ~2.4× slower per operation,
  consistent with the steady-cycle ratio 28.6 vs 8.0 ms). Second and
  later allocs are sub-microsecond on both sides.
- **cold cycle.** libpinyin-Tkrzw faults ~20 MiB of lazily mapped
  tables during its first keystroke cycle (48.4 vs 28.6 ms steady;
  peak HWM 12.6 → 32.7 MiB). oxpinyin's cells show no cold penalty
  because the first-alloc walk has already touched those pages — the
  eager cost shows up in the alloc column instead. Comparing "time to
  first result" (1.71×/2.25×) puts the two on one footing.
- **steady cycle (0.94–0.95×).** Parity with a slight edge to oxpinyin
  on this host. At the same pin on ARM64 the sign was opposite
  (1.14×); the gap is inside host sensitivity either way and no
  cross-ISA claim is made. The pin's release `panic = "abort"` must be
  weighed with these numbers: on ARM64 that policy measured +5.7%
  steady / +5.3% cold and was reverted at `1b0c84a0` (steady recovered
  9.196 → 8.686 ms there, oracle flat). The steady columns here are
  therefore a mild upper bound for oxpinyin — production after the
  revert is marginally faster — and the cold/first-alloc columns carry
  the same bias. The effect was not re-measured on this host.
- **RSS (1.62–2.40×).** oxpinyin is now mmap-dominated (RssFile 16.8–
  17.0 MiB vs libpinyin's 11.1 MiB; the delta is the six DBMs mapped
  through the backend plus the Rust runtime's anon floor of ~11–13
  MiB against libpinyin's 1.5 MiB). Cycle-peak HWM converges the
  Tkrzw pair (32.1 vs 32.7 MiB) because libpinyin pages in the same
  tables it lazily skipped at init.
- **`.so` (1.24–1.76×).** Tkrzw's libpinyin strips to 867 KB against
  KC's 1,209 KB (backend library boundary, not implementation);
  oxpinyin's two builds land within 24 KB of each other.

## Relation to the 2026-08-31 matrix

Not comparable cell-for-cell: different ISA (x86_64 vs ARM64), host,
and oxpinyin generation (pre-P6 `cd14708` vs post-P6 `b5fdfad8`), and
this session removes the four-data-directory confound by sharing one
datagen-compiled directory per backend. What carries over: the
architecture, not the backend, sets the init/alloc profile; the
backend is a steady-state non-factor (<1% here, <5% there); KC's
memory overhead shows up inside libpinyin and is absorbed by
oxpinyin's larger footprint. What has changed qualitatively since
then: init went from ~100× to ~1.1×, and oxpinyin's memory profile
flipped from 90% heap to majority file-backed — both P1–P6 effects
already visible in the ARM64 KC baseline and here confirmed on a
second architecture with shared data.

## Acceptance gate

| # | Check | Status |
|---|---|---|
| 1 | libpinyin + Tkrzw measured | PASS |
| 2 | libpinyin + KC measured | PASS |
| 3 | oxpinyin + Tkrzw measured | PASS |
| 4 | oxpinyin + KC measured | PASS |
| 5 | one controlled environment, one session | PASS |
| 6 | same data directory per backend pair (S1 gate) | PASS |
| 7 | round-robin interleaving | PASS |
| 8 | `taskset -c 0` on every run | PASS |
| 9 | init / first alloc / cold / steady measured | PASS |
| 10 | RSS + HWM + composition measured | PASS |
| 11 | stripped `.so` sizes recorded | PASS |
| 12 | 95% CIs on speed axes | PASS |
| 13 | ratios per backend pair | PASS |
| 14 | no production code changes | PASS |
