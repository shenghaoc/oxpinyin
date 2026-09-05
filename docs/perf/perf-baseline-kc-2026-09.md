# Stage-2 Performance Baseline — KC Backend (2026-09)

Re-baseline of the ARM64/KC scoreboard after the P1–P6 data-layer
inversion, the alloc fix, the interpolation2.text trim, and the
fat-LTO profile change. First measured at `94b38948` (origin/main at
2026-09-04); **amended 2026-09-05 at `b5fdfad8`** (origin/main) in the
same oracle and container, with the earlier tip re-measured in the same
session as a control. Every table below is the `b5fdfad8` measurement
unless a row says otherwise.

The workspace default backend changed from Kyoto Cabinet to Tkrzw at
`05688575` (2026-09-05). This document stays on KC so its columns remain
comparable with the 2026-08-31 baseline and the `94b38948` rows; the
amendment therefore builds with `--no-default-features --features
kyotocabinet` explicitly. A Tkrzw scoreboard is a separate measurement.

## Measurement host

| Property | Value |
|---|---|
| Container | Docker Desktop on Apple Silicon (M-series), linux/arm64 (`oxpinyin-validate` image) |
| Guest kernel | Linux 7.0.12-linuxkit aarch64 |
| Guest OS | Debian testing (forky/sid), snapshot 20260831 |
| Rust | 1.97.1 (8bab26f4f 2026-07-14) |
| cargo-c | 0.10.25+cargo-0.99.0 |
| GCC | 15.3.0 (Debian 15.3.0-2) |
| libkyotocabinet-dev | 1.2.80-2+b2 |
| Harness | `bisect --perf`, CPU-pinned via `taskset -c 0` |
| Tree | `git archive b5fdfad8`, bind-mounted; target dir on a fresh Docker volume |
| Tables | `oxpinyin-datagen --no-default-features --features kyotocabinet -- compile --backend kyotocabinet`, regenerated from the pinned model20 |
| Build | `cargo cinstall --locked --release -p oxpinyin-capi --no-default-features --features kyotocabinet` (`ldd`: libkyotocabinet.so.16, no tkrzw) |

## Oracle pin

```
libpinyin 2.11.91 (0c5e80e1) + model20 (59c68e89) + dbm=Tkrzw
```

## Results

### Axis 1 — Execution speed (PERF_RUNS=20, PERF_CYCLES=8)

| Backend | init | alloc (first) | cycle 0 (cold) | cycles 1..N (steady) |
|---|---:|---:|---:|---:|
| oracle (n=20) | 0.659 ms [0.538, 4.265] | 0.000 ms [0.000, 0.000] | 8.595 ms [8.320, 28.411] | 8.034 ms [7.667, 8.964] |
| oxpinyin (n=20) | 3.216 ms [3.026, 3.516] | 17.643 ms [17.223, 18.432] | 9.509 ms [9.382, 10.925] | 9.168 ms [8.897, 11.015] |
| oxpinyin @`94b38948` (n=20, same-session control) | 21.039 ms [20.320, 22.984] | 0.001 ms [0.001, 0.001] | 9.074 ms [8.798, 9.957] | 8.699 ms [8.364, 9.938] |

| Metric | oxpinyin/oracle | `94b38948` ratio | Aug 31 ratio | Change vs `94b38948` |
|---|---:|---:|---:|---|
| init | 4.9× | 30.6× | 118× | −84% (21.1 → 3.2 ms) |
| alloc (first) | 52,983× | 2.9× | 7,985× | 0.001 → 17.6 ms (the deferred walk) |
| init + first alloc | 31.6× | 30.6× | — | 21.1 → 20.9 ms (cost moved, not removed) |
| cold cycle | 1.11× | 1.04× | 0.99× | +5.8% (8.98 → 9.51 ms) |
| steady cycle | 1.14× | 1.08× | 1.08× | +6.5% (8.61 → 9.17 ms) |

The harness times one `pinyin_alloc_instance` per process, so its
`alloc` column is the **first** alloc. A scratch copy of `bisect.c`
that additionally times a second and a third `pinyin_alloc_instance`
on the same context (not committed; run n=20, same pinning) gives:

| Backend | 1st alloc | 2nd alloc | 3rd alloc |
|---|---:|---:|---:|
| oracle | 0.33 µs [0.25, 0.46] | 0.13 µs [0.08, 0.21] | 0.13 µs [0.08, 0.17] |
| oxpinyin | 17.73 ms [17.19, 20.13] | 0.48 µs [0.42, 1.50] | 0.25 µs [0.21, 0.42] |

**init.** The key-cost walk (`key_cost_table`: 440 KC B-tree point
reads into `pinyin_index.bin`) no longer runs in `pinyin_init`.
`87f9a49e` (merged 2026-09-04) moved it behind a lazily filled cache on
`Runtime` that the first `new_session` fills; `706918cf`, `48db10fa` and
`8147b7d7` then made that cache track library visibility (rebuild on a
mask change, shared-lock fast path, epoch-validated rebuild). Init is
now 3.2 ms. The deferral's own log predicted ~1 ms; that was not met on
ARM64/KC — every post-deferral tree in the series below measures 3.3–3.9
ms. `strace -c` over the whole harness process at the tip (dlopen, init,
first alloc, and the harness's own `system("rm -rf")`, whose `wait4`
alone is ~0.9 ms) totals ~2.5 ms of kernel time across 55 `openat`, 93
`mmap`, 41 `mprotect` and 16 `statx`, so init's kernel share is at most
~1.5 ms. At least half of the 3.2 ms is therefore userspace — six KC
handle opens, the user-store creation, `table.conf`, the chunk
mappings — and is not profiled here.

**alloc.** The first `pinyin_alloc_instance` on a context now pays the
walk (17.6 ms); the second and third cost 0.5 µs and 0.25 µs. The
52,983× ratio is against the oracle's 0.33 µs and says nothing new: the
comparable number is init + first alloc, 20.9 ms vs 0.66 ms (31.6×),
which is where `94b38948` already stood (30.6×). The deferral changes
who pays, not how much: a consumer that opens a context without decoding
(libpinyin's `pinyin_init` callers that never allocate an instance) now
pays 3 ms instead of 21; every decoding consumer pays the same total on
its first instance.

**cycles.** Both cycle columns regressed by ~6% against `94b38948`
while the oracle control held (steady 7.985 → 8.034 ms across the two
sessions, 8.02 ms within this one). The same-session series below
localises the step to `a41605ea` (`panic = "abort"`, 2026-09-05):
steady 8.70 → 9.19 ms, cold 9.06 → 9.54 ms, with the oracle flat on
both sides and no overlap between the pre- and post-abort minima. The
abort commit's report measured size only. See "Where the changes
landed" for the numbers and the open item.

### Axis 2 — Installed size

| Side | shared object | runtime data | runtime footprint | total install |
|---|---:|---:|---:|---:|
| oracle | 5,505,928 bytes (5.25 MiB) | 37,266,687 bytes (35.54 MiB) | 42,772,615 bytes (40.79 MiB) | 49,325,304 bytes (47.04 MiB) |
| oxpinyin | 1,632,464 bytes (1.56 MiB) | 38,671,030 bytes (36.88 MiB) | 40,303,494 bytes (38.44 MiB) | 50,252,470 bytes (47.92 MiB) |

| Metric | oxpinyin/oracle | `94b38948` ratio | Aug 31 ratio |
|---|---:|---:|---:|
| shared object (as installed by cargo-c, unstripped) | 0.30× | 0.31× | 0.45× |
| shared object (`strip --strip-all`) | 1.75× | 1.83× | — |
| runtime data | 1.04× | 1.04× | 2.86× |
| runtime footprint | 0.94× | 0.95× | 2.56× |
| total install | 1.02× | 1.03× | 2.78× |

Shared object, both forms (the harness's size table counts the file
cargo-c installs, which is unstripped; the shipped comparison is
stripped):

| Form | oracle | oxpinyin @`94b38948` (same session) | oxpinyin @`b5fdfad8` | delta |
|---|---:|---:|---:|---:|
| as installed | 5,505,928 B | 1,731,312 B | 1,632,464 B | −98,848 B (−5.7%) |
| `strip --strip-all` | 789,512 B | 1,446,528 B | 1,380,992 B | −65,536 B (−4.5%) |

The stripped delta is the `panic = "abort"` step (`a41605ea`), the same
−65,536 B the change's own back-to-back measurement reported
(docs/perf/perf-so-size-2026-09.md, "Further reduction options"); the
series below shows every other tree in the range at 1,446,528 B, so the
facade extraction added no stripped bytes. On aarch64 the linker's
64 KiB max-page-size quantises file layout, which is why both this
delta and the size doc's are exact multiples of 65,536. Sections at
`b5fdfad8` (`readelf -S`, unstripped): `.text` 627,316 B, `.rodata`
43,328 B, `.eh_frame` + `.eh_frame_hdr` + `.gcc_except_table` 69,272 B,
`.data.rel.ro` 262,176 B, `.rela.dyn` 341,328 B. `libpinyin.a`
(9,908,808 B) is also installed by cargo-c and is the bulk of the
"installed code" figure; it is not shipped.

The runtime data directory is byte-identical to the `94b38948`
measurement: 23 files, 38,671,030 bytes, regenerated by the tip's
datagen from the same pinned model20 — none of the changes in the range
touched the compiled tables.

The Aug 31 data dir contained only 4 files (3 KC tables +
`interpolation2.text` at 79.59 MiB) totaling 101.80 MiB. This
measurement ships the full datagen output (23 files: 3 system DBMs,
2 addon DBMs, 16 chunk files, punct, table.conf) — the same set the
oracle ships — with no `interpolation2.text`. The file was trimmed to
1.97 MiB (2026-09-03, 17ae4bf) and removed entirely at `3f0f0f36` after
strace confirmed the runtime never opens it (re-confirmed at
`b5fdfad8`: not in the `openat` trace).

The remaining delta to the oracle (36.88 vs 35.54 MiB) is KC vs Tkrzw
container overhead on the DBMs (+1.34 MiB).

oxpinyin runtime data breakdown (unchanged):

| File | Size |
|---|---:|
| bigram.db | 21.52 MiB |
| pinyin_index.bin | 5.04 MiB |
| phrase_index.bin | 3.63 MiB |
| gb_char.bin | 2.83 MiB |
| addon_pinyin_index.bin | 1.04 MiB |
| opengram.bin | 0.78 MiB |
| addon_phrase_index.bin | 0.78 MiB |
| punct.bin | 0.39 MiB |
| (15 remaining chunk + conf) | 0.87 MiB |

### Axis 3 — RAM (PERF_RAM_RUNS=10)

| Backend | post-init RSS | post-init HWM | after-first HWM | after-last HWM (peak) |
|---|---:|---:|---:|---:|
| oracle (n=10) | 12,308 KiB [11,592, 13,564] | 12,308 KiB | 15,252 KiB [14,720, 16,548] | 15,252 KiB [14,720, 16,548] |
| oxpinyin (n=10) | 27,502 KiB [27,004, 28,580] | 27,502 KiB | 29,838 KiB [28,420, 30,212] | 30,062 KiB [28,552, 30,380] |
| oxpinyin @`94b38948` (n=10, same-session control) | 27,538 KiB [26,772, 28,572] | 27,956 KiB | 29,976 KiB [28,536, 30,388] | 30,120 KiB [28,644, 30,456] |

| Metric | oxpinyin/oracle | `94b38948` ratio | Aug 31 ratio |
|---|---:|---:|---:|
| post-init RSS | 2.23× | 2.45× | 5.45× |
| peak HWM | 1.97× | 1.86× | 4.46× |

oxpinyin post-init: RssAnon 11,878 KiB [11,356, 12,988] (heap), RssFile
15,624 KiB [15,592, 15,648] (mmap). The harness reads `/proc/self/status`
after the first `pinyin_alloc_instance`, so "post-init" includes the
key-cost table on both sides of the deferral. Post-init HWM equals
post-init RSS: no transient allocation at init, as before.

RAM did not move in the range. The medians shifted by under 1 MiB in
both directions between the `94b38948` document and this one, and the
oracle — the same binary in both sessions — moved by +716 KiB, which
bounds the session-to-session noise; the same-session control row
matches the tip within that noise. The ratio changes are the oracle
denominator moving, not oxpinyin.

## Initialization cost — after the deferral

strace (`-e trace=openat`) of the oxpinyin init + first-alloc path at
`b5fdfad8` confirms:

- `interpolation2.text` does **not** appear in the trace. Post-P6,
  `set_unigrams_from_interpolation2` was removed; unigram counts
  come from the mmap'd chunk files.
- The ~18 ms walk is `key_cost_table`: 440 `dictionary.lookup(&[key])`
  calls, each a KC B-tree point read into `pinyin_index.bin`.
  `model.score` for each entry reads from mmap'd chunk files
  (in-memory), not from KC. Since `87f9a49e` it runs in the first
  `new_session`, i.e. the first `pinyin_alloc_instance`, and is cached
  on the `Runtime` for every later session under the same library
  visibility mask.

At `94b38948` the same walk ran inside `pinyin_init`, and the document
measured total kernel syscall time there at ~1.2 ms (48 openat, 76 mmap,
18 read, 25 fstat, 30 close) with the remaining ~20 ms as userspace CPU
in KC B-tree traversal. The `b5fdfad8` trace (init through first alloc)
holds 55 `openat`, 20 of them ENOENT: 13 are the dynamic loader probing
the harness's `LD_LIBRARY_PATH` for system libraries before the system
path, 6 are KC's `.wal` sidecar probes on the six DBM handles (three
system, two addon, punct), 1 is the user store's. The successful opens
are the six DBMs, four chunk files, `table.conf`, and the user store's
`.kct` and `.wal`, which the harness's fresh user dir makes the runtime
create.

### Sequential-walk experiment (reverted)

A sequential walk of `pinyin_index.bin` (replacing 440 point reads
with one forward cursor scan, filtering to 2-byte keys) measured at
**39 ms** vs **20 ms** for point reads — 2× slower. The TreeDB cursor
visits all 201,658 rows, and KC's per-row cursor overhead (~200 ns ×
201K rows ≈ 40 ms) exceeds the cost of 440 individual B-tree seeks
(~45 µs × 440 ≈ 20 ms).

This characterises the KC TreeDB cursor as unsuitable for sparse
sequential scans where < 1% of visited rows are consumed. The init fix
that landed was the deferral (`87f9a49e`, measured above), not a
data-structure traversal change; the walk itself still costs ~18 ms
wherever it runs.

## Where the changes landed — same-session series

Eight trees from `git log 94b38948..b5fdfad8`, each exported with `git
archive`, built cold (every workspace crate recompiled) into its **own**
target dir with the flags in the host table, its tables regenerated by
its own datagen, and driven by the `b5fdfad8` harness on the same image
and host across three container runs on 2026-09-05 (n=20 speed, n=10
RAM, `taskset -c 0`, oracle alternating). Medians; the oracle's steady
cycle at every point was 7.97–8.07 ms.

| Tree | Change | init | first alloc | cold cycle | steady cycle | steady ratio | stripped `.so` |
|---|---|---:|---:|---:|---:|---:|---:|
| `94b38948` | previous baseline tip | 21.039 ms | 0.001 ms | 9.074 ms | 8.699 ms | 1.084× | 1,446,528 B |
| `87f9a49e` | defer `key_cost_table` to first `new_session` | 3.331 ms | 17.438 ms | 9.028 ms | 8.687 ms | 1.088× | 1,446,528 B |
| `8147b7d7` | key-cost cache: visibility rebuild, RwLock fast path, epoch validation | 3.843 ms | 17.831 ms | 9.088 ms | 8.626 ms | 1.070× | 1,446,528 B |
| `b40e3542` | `oxpinyin-facade` extraction, slices 1–2 | 3.907 ms | 18.221 ms | 9.060 ms | 8.697 ms | 1.078× | 1,446,528 B |
| `a41605ea` | release `panic = "abort"` | 3.821 ms | 18.138 ms | **9.543 ms** | **9.194 ms** | 1.146× | **1,380,992 B** |
| `828e2033` | drop `ffi_catch` | 3.758 ms | 18.155 ms | 9.528 ms | 9.160 ms | 1.145× | 1,380,992 B |
| `77c3fb78` | python/CI work (no engine change) | 3.873 ms | 18.059 ms | 9.534 ms | 9.207 ms | 1.156× | 1,380,992 B |
| `b5fdfad8` | this amendment | 3.216 ms | 17.643 ms | 9.509 ms | 9.168 ms | 1.141× | 1,380,992 B |

Three readings:

- The init → first-alloc move is exactly `87f9a49e`, as its log says.
  Nothing after it changes init, first alloc, or RAM outside noise.
- The facade extraction is free on every axis: cycle, RAM, and stripped
  size all match the tree before it. Its as-installed size grew by
  8 KiB, all of it in sections that `strip --strip-all` removes.
- **`panic = "abort"` trades 64 KiB of stripped `.so` for +5.7% on the
  steady cycle and +5.3% on the cold cycle** (0.5 ms per 20-input
  cycle), a clean step: the four trees before it sit at 8.63–8.70 ms
  and the four after at 9.16–9.21 ms, the post-abort minima (≥ 8.90 ms)
  clear the pre-abort medians, and the oracle is flat on both sides.
  `828e2033` (the `catch_unwind` removal that accompanied the abort
  policy) changes nothing measurable on its own. The
  mechanism is not investigated here; the abort commit's report and the
  size doc measured size and a criterion micro-bench, not this harness.
  **Open Stage-2 item:** the trade needs a decision with both numbers
  on the table, since AGENTS.md accepts a regression on one axis only
  when it is traded against, minimised, and justified in the change's
  report — and this one was not measured there.
  **Resolved 2026-09-05:** `panic = "abort"` reverted from
  `[profile.release]` (`perf/revert-release-panic-abort`). A back-to-back
  rebuild of `b5fdfad8` with only that line removed recovers the cycle
  (steady 9.196 → 8.686 ms, cold 9.623 → 9.008 ms, oracle flat) at
  +65,536 B stripped; the `ffi_catch` removal stands.

Method note. A first pass of this series shared one target dir across
the eight trees and was discarded: cargo's freshness check is
mtime-based, `git archive` stamps sources with the commit time, and the
trees share crate names and paths, so only the crates whose manifests
changed were recompiled and the rest were silently reused from whichever
tree had built them last. The symptom was four trees with byte-identical
`.so` files and the init/alloc move appearing at the abort commit
(whose profile change forced a full rebuild). Per-tree target dirs fix
it (79–80 crates compiled at every point).

## Changes since 2026-08-31 baseline

| Date | Change | Effect |
|---|---|---|
| 2026-09-03 | interpolation2.text trimmed to 1-gram (17ae4bf) | runtime data −77.6 MiB |
| 2026-09-03 | alloc fix: key_cost_table moved to Runtime::open (0f6c8a4) | alloc 7,985× → 2.9× |
| 2026-09-03 | fat LTO + single codegen unit (49a5dc0) | .so −196,704 B (−12%) |
| 2026-09-04 | §12 sentence-surface re-freeze (94b3894) | parity pin 491/396/390 |
| 2026-09-04 | interpolation2.text removed from datagen output (3f0f0f3) | runtime data −1.97 MiB |
| 2026-09-04 | P1–P6 data-layer inversion (cumulative) | init 102 → 21 ms; RSS 72,652 → 28,388 KiB |
| 2026-09-04 | key_cost_table deferred to first new_session (87f9a49) | init 21.0 → 3.3 ms; first alloc 0.001 → 17.4 ms; later allocs 0.5 µs; total unchanged |
| 2026-09-04 | key-cost cache tracks library visibility; RwLock fast path; epoch validation (706918c, 48db10f, 8147b7d) | no measurable change (init 3.8 ms, steady 8.63 ms) |
| 2026-09-04 | oxpinyin-facade extraction (cbe8435, c519e1a; tip b40e354) | no measurable change; stripped .so unchanged |
| 2026-09-05 | release `panic = "abort"` (a41605e) | stripped .so −65,536 B (−4.5%); **steady cycle +5.7%, cold +5.3%** — reverted the same day (resolution above) |
| 2026-09-05 | `ffi_catch` removed (828e203) | no measurable change |
| 2026-09-05 | default backend → Tkrzw (0568857) | none on this KC measurement; KC builds now need `--no-default-features --features kyotocabinet` |

Not re-run for this amendment: the parity pin (491/396/390) — no code
changed. The Tkrzw default is not measured here; a Tkrzw scoreboard
would be a new document, not a column in this one.
