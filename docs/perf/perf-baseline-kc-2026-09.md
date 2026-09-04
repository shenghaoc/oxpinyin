# Stage-2 Performance Baseline — KC Backend (2026-09)

Re-baseline of the ARM64/KC scoreboard after the P1–P6 data-layer
inversion, the alloc fix, the interpolation2.text trim, and the
fat-LTO profile change. Measured at `94b38948` (origin/main at
2026-09-04) with the same oracle and container as the 2026-08-31
baseline.

## Measurement host

| Property | Value |
|---|---|
| Container | Docker Desktop on Apple Silicon (M-series), linux/arm64 |
| Guest kernel | Linux 7.0.12-linuxkit aarch64 |
| Guest OS | Debian testing (forky/sid) |
| Rust | 1.97.1 (8bab26f4f 2026-07-14) |
| cargo-c | 0.10.25+cargo-0.99.0 |
| GCC | 15.3.0 (Debian 15.3.0-2) |
| libkyotocabinet-dev | 1.2.80-2+b2 |
| Harness | `bisect --perf`, CPU-pinned via `taskset -c 0` |

## Oracle pin

```
libpinyin 2.11.91 (0c5e80e1) + model20 (59c68e89) + dbm=Tkrzw
```

## Results

### Axis 1 — Execution speed (PERF_RUNS=20, PERF_CYCLES=8)

| Backend | init | alloc | cycle 0 (cold) | cycles 1..N (steady) |
|---|---:|---:|---:|---:|
| oracle (n=20) | 0.690 ms [0.522, 7.452] | 0.000 ms [0.000, 0.000] | 8.607 ms [8.328, 29.801] | 7.985 ms [7.670, 8.886] |
| oxpinyin (n=20) | 21.138 ms [20.386, 26.216] | 0.001 ms [0.001, 0.005] | 8.984 ms [8.829, 9.501] | 8.607 ms [8.309, 16.895] |

| Metric | oxpinyin/oracle | Aug 31 ratio | Change |
|---|---:|---:|---|
| init | 30.6× | 118× | −74% |
| alloc | 2.9× | 7,985× | −99.96% |
| cold cycle | 1.04× | 0.99× | stable |
| steady cycle | 1.08× | 1.08× | stable |

Init fell from 102 ms to 21 ms: the P1–P6 data-layer inversion
eliminated KC's eager B-tree initialization at open time. The
remaining 21 ms is `key_cost_table` — 440 individual KC B-tree point
reads in `pinyin_index.bin`, one per `SyllableKey`. The `model.score`
calls inside `key_cost_table` read unigram counts from the mmap'd
chunk files (in-memory), not from KC.

`perf/defer-key-cost-table` (pending merge at `87f9a49`) defers these
440 reads from `Runtime::open` to the first `new_session` call,
reducing init to ~1 ms.

### Axis 2 — Installed size

| Side | shared object | runtime data | runtime footprint | total install |
|---|---:|---:|---:|---:|
| oracle | 5,505,928 bytes (5.25 MiB) | 37,266,687 bytes (35.54 MiB) | 42,772,615 bytes (40.79 MiB) | 49,325,304 bytes (47.04 MiB) |
| oxpinyin | 1,730,992 bytes (1.65 MiB) | 38,671,030 bytes (36.88 MiB) | 40,402,022 bytes (38.53 MiB) | 50,544,630 bytes (48.20 MiB) |

| Metric | oxpinyin/oracle | Aug 31 ratio |
|---|---:|---:|
| shared object (unstripped) | 0.31× | 0.45× |
| runtime data | 1.04× | 2.86× |
| runtime footprint | 0.95× | 2.56× |
| total install | 1.03× | 2.78× |

Runtime data fell from 101.80 MiB to 36.88 MiB: the 79.59 MiB
`interpolation2.text` was trimmed to 1.97 MiB (2026-09-03), then
removed entirely (this measurement) — the runtime never opens the
file. The remaining delta to the oracle (36.88 vs 35.54 MiB) is KC vs
Tkrzw container overhead on the three DBMs (+1.34 MiB).

oxpinyin runtime data breakdown:

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
| (10 remaining chunk + conf) | 1.87 MiB |

### Axis 3 — RAM (PERF_RAM_RUNS=10)

| Backend | post-init RSS | post-init HWM | after-first HWM | after-last HWM (peak) |
|---|---:|---:|---:|---:|
| oracle (n=10) | 11,592 KiB | 11,592 KiB | 15,866 KiB | 15,866 KiB |
| oxpinyin (n=10) | 28,388 KiB | 28,388 KiB | 29,102 KiB | 29,470 KiB |

| Metric | oxpinyin/oracle | Aug 31 ratio |
|---|---:|---:|
| post-init RSS | 2.45× | 5.45× |
| peak HWM | 1.86× | 4.46× |

oxpinyin post-init: RssAnon 12,732 KiB (heap), RssFile 15,690 KiB
(mmap). The HWM spike from init (86,364 → 27,772 KiB at Aug 31) is
gone — post-init HWM equals post-init RSS, no transient allocation.

## Initialization bottleneck — corrected attribution

strace (`-e trace=openat`) of the oxpinyin init path confirms:

- `interpolation2.text` does **not** appear in the trace. Post-P6,
  `set_unigrams_from_interpolation2` was removed; unigram counts
  come from the mmap'd chunk files.
- The full ~21 ms is `key_cost_table`: 440 `dictionary.lookup(&[key])`
  calls, each a KC B-tree point read into `pinyin_index.bin`.
  `model.score` for each entry reads from mmap'd chunk files
  (in-memory), not from KC.

Total kernel syscall time is ~1.2 ms (48 openat, 76 mmap, 18 read,
25 fstat, 30 close). The remaining ~20 ms is userspace CPU in KC
B-tree traversal.

### Sequential-walk experiment (reverted)

A sequential walk of `pinyin_index.bin` (replacing 440 point reads
with one forward cursor scan, filtering to 2-byte keys) measured at
**39 ms** vs **20 ms** for point reads — 2× slower. The TreeDB cursor
visits all 201,658 rows, and KC's per-row cursor overhead (~200 ns ×
201K rows ≈ 40 ms) exceeds the cost of 440 individual B-tree seeks
(~45 µs × 440 ≈ 20 ms).

This characterises the KC TreeDB cursor as unsuitable for sparse
sequential scans where < 1% of visited rows are consumed. The correct
init fix is the OnceLock deferral (`perf/defer-key-cost-table`), not a
data-structure traversal change.

## Changes since 2026-08-31 baseline

| Date | Change | Effect |
|---|---|---|
| 2026-09-03 | interpolation2.text trimmed to 1-gram (17ae4bf) | runtime data −77.6 MiB |
| 2026-09-03 | alloc fix: key_cost_table moved to Runtime::open (0f6c8a4) | alloc 7,985× → 2.9× |
| 2026-09-03 | fat LTO + single codegen unit (49a5dc0) | .so −196,704 B (−12%) |
| 2026-09-04 | §12 sentence-surface re-freeze (94b3894) | parity pin 491/396/390 |
| 2026-09-04 | interpolation2.text removed from datagen output (this) | runtime data −1.97 MiB |
| 2026-09-04 | P1–P6 data-layer inversion (cumulative) | init 102 → 21 ms; RSS 72,652 → 28,388 KiB |
