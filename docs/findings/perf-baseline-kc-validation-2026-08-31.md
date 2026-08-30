# KC Baseline Validation — 2026-08-31

Validation pass on the KC performance baseline (`15e1b47`). The baseline
document (`perf-baseline-kc-2026-08-31.md`) is preserved; this document
records corrections, additional evidence, and the acceptance gate.

## Methodology

All measurements were taken inside the same `oxpinyin-validate` Docker
container used for the baseline, extended with strace/ltrace. Containers
run on Docker Desktop 4.88.1 on Apple Silicon (linux/arm64 VM). CPU-pinned
runs use `taskset -c 0`.

## Correction 1 — Initialization bottleneck attribution

### Baseline claim

> "interpolation2.text (79.59 MiB raw text) must be parsed on every
> pinyin_init. The oracle pre-compiles this into binary indexes during
> make install; its data directory does not contain the file at all."

### Validated finding

The 80 MiB file size is misleading. **Init reads only the 1-gram section
(2.0 MiB, 63,907 items = 2.5% of the file).** The 77.62 MiB 2-gram section
(1,849,609 bigram entries) is never touched at runtime — it was already
compiled into the KC tables by `oxpinyin-datagen`.

File structure of `interpolation2.text`:

| Section | Offset | Size | Items |
|---|---:|---:|---:|
| `\data model interpolation` | 0 | header | 0 |
| `\1-gram` | 26 | 1.97 MiB | 63,907 |
| `\2-gram` | 2,065,398 | 77.62 MiB | 1,849,609 |
| `\end` | 83,457,176 | marker | 0 |

strace (`-f -y -T`) confirmed: 253 read() calls of 8,192 bytes each =
2,072,576 bytes, stopping 7,178 bytes into the `\2-gram` section (BufReader
overshoot; the parser closes the file after the 1-gram section header).

### Corrected init attribution

| Configuration | Median init (ms) | n |
|---|---:|---:|
| Full data (control) | 116.3 | 10 |
| Empty 1-gram, KC tables present | 103.5 | 10 |
| 1-gram only (63,907 items), KC present | 108.9 | 10 |
| Oracle | 0.9 | 10 |

(CPU-pinned, `taskset -c 0`)

| Component | Estimated cost | Share |
|---|---:|---:|
| KC database initialization | ~104 ms | 89% |
| interpolation2.text 1-gram parsing | ~5–13 ms | 5–11% |
| Other (table.conf, framework) | <5 ms | <5% |

**The bottleneck is Kyoto Cabinet B+ tree initialization, not text parsing.**
KC opens 3 `.kct` files (pinyin_index, phrase_index, bigram) and loads their
B-tree indexes into memory eagerly. The oracle achieves 0.9 ms init because
it uses mmap-backed binary files — data is loaded lazily by the kernel via
page faults.

### I/O vs CPU breakdown (strace -c, all syscalls)

| Metric | Value |
|---|---:|
| Total kernel syscall time | 8.1 ms |
| read() wall-clock for interpolation2.text | 5.3 ms |
| read() kernel CPU for interpolation2.text | 0.25 ms |
| Userspace CPU time (by subtraction) | ~108 ms |

The init is overwhelmingly CPU-bound in userspace (KC tree construction,
hash table building, data structure allocation), not I/O-bound.

## Correction 2 — Shared-object size comparison

### Baseline claim

> "Shared object is smaller than the oracle (0.454×, same direction as
> W8's 0.40×)."

### Validated finding

The oracle `.so` contains debug symbols (`.debug_aranges`, `.debug_info`,
`.debug_abbrev`, `.debug_line`, `.debug_str`). The oxpinyin `.so` does not
(release build, no debug info). This is an apples-to-oranges comparison.

| Metric | Oracle | oxpinyin | Ratio |
|---|---:|---:|---:|
| As-installed (oracle has debug) | 5,505,928 | 2,498,472 | 0.454× |
| Stripped (fair comparison) | 789,512 | 1,708,768 | **2.164×** |

When both are stripped, **oxpinyin's shared object is 2.16× larger than the
oracle's**, not smaller. The baseline's 0.454× figure compared a debug-build
oracle against a release-build oxpinyin.

The oxpinyin `.so` is still well within the Constitution's install-size budget
(pinned reference stack +10%), because the `.so` is a small fraction of total
install size and runtime data is the dominant component for both sides.

## Strace file-access proof

### Oracle init opens (strace -f -e trace=openat,read,mmap)

| File | Access method | Size |
|---|---|---:|
| `pinyin_index.bin` | read()+mmap | 5.70 MiB |
| `phrase_index.bin` | read()+mmap | 3.90 MiB |
| `bigram.db` | read()+mmap | 19.09 MiB |
| `gb_char.bin` | mmap | 2.83 MiB |
| `gbk_char.bin` | mmap | 0.33 MiB |
| `merged.bin`, `opengram.bin` | mmap | 0.81 MiB |
| Various `addon_*.bin` | mmap | 2.12 MiB |
| `punct.bin` | mmap | 0.51 MiB |
| `table.conf` | read() | 1.2 KiB |

**Does NOT open `interpolation2.text`.** Confirmed: the oracle's data directory
does not contain it at all. It is consumed at build time by
`import_interpolation` (writes bigram counts into `bigram.db`) and
`gen_unigram` (computes unigram frequencies).

### oxpinyin init opens

| File | Access method | Bytes read | Wall-clock |
|---|---|---:|---:|
| `interpolation2.text` | read() (253 × 8 KiB) | 2,072,576 | 5.3 ms |
| `pinyin_index.kct` | KC B-tree (mmap) | — | — |
| `phrase_index.kct` | KC B-tree (mmap) | — | — |
| `bigram.kct` | KC B-tree (mmap) | — | — |
| Various `.kct.wal` | KC WAL files | — | — |

## Oracle build-artifact investigation

Source: `libpinyin-2.11.91/data/Makefile.am` and
`utils/storage/gen_binary_files.cpp`.

The build pipeline that eliminates `interpolation2.text` from runtime:

1. **`gen_binary_files`** — reads `.table` text files (gb_char.table,
   opengram.table, merged.table, etc.), builds ChewingLargeTable2 and
   PhraseLargeTable3 indexes, writes `pinyin_index.bin`,
   `phrase_index.bin`, and per-category `.bin` files.

2. **`import_interpolation < interpolation2.text`** — reads the full 80 MiB
   file (both 1-gram and 2-gram sections), imports bigram counts into
   `bigram.db` (Tkrzw database).

3. **`gen_unigram`** — computes unigram frequency from the phrase index,
   adjusting by a constant to avoid zero frequencies.

The installed data (`$(libdir)/libpinyin/data/`) contains only the binary
outputs. `interpolation2.text` is listed in `EXTRA_DIST` (shipped in the
source tarball) but NOT in `libpinyin_db_DATA` (not installed).

## Exact ratios from raw JSONL

Computed from `/tmp/perf-out/speed.jsonl` and `/tmp/perf-out/ram-*.jsonl`
(n=20 speed, n=10 RAM per side).

| Metric | Oracle median | oxpinyin median | Ratio | Baseline said |
|---|---:|---:|---:|---|
| init | 0.866 ms | 102.248 ms | **118.1×** | ~110× |
| cold cycle | 9.661 ms | 9.524 ms | **0.986×** | 0.99× |
| steady cycle | 8.721 ms | 9.407 ms | **1.079×** | 1.08× |
| post-init RSS | 13,324 KiB | 72,652 KiB | **5.45×** | 5.45× |
| peak HWM | 19,108 KiB | 85,152 KiB | **4.46×** | 4.46× |

The baseline's "~110×" for init was imprecise; the median is 118.1×. The
mean ratio is 96.6× (pulled down by a single oracle cold-cache outlier at
4.028 ms). All other ratios are confirmed accurate.

## Build reproducibility

### oxpinyin-capi shared object

Two consecutive `cargo cinstall --locked --release` builds in the same
container produce **bit-identical** `.so` files:

```
ddaf4cb4ee8c63223bd5ef59b43778ca19ef66864e4ccde313dd2950dd2e9486
```

### KC data files

The three `.kct` files in the staged data directory are byte-identical to
the `oxpinyin-datagen` export directory. The datagen step is deterministic
for a given `interpolation2.text` input and the same KC library version.

### Oracle build reproducibility

Not tested. The oracle is built from upstream source using autotools; the
Tkrzw `bigram.db` may contain nondeterministic elements (hash seeds,
allocation order). This is acceptable — the oracle is a fixed pin, not a
reproducibility target.

## Architecture portability

The benchmark infrastructure contains **no architecture-specific code**:

- `run-perf-baseline.sh`: uses `find` for dynamic pkgconfig path discovery
  (handles any `lib/<triplet>/` layout). No hardcoded `lib64/` or
  `aarch64-linux-gnu/`.
- `perf-baseline.py`: size classification uses `".so" in path.name` — no
  architecture-specific SO naming.
- `bisect.c`: pure C with dlopen/dlsym — architecture-independent.
- `Dockerfile.perf-baseline`: base image is `debian:testing` without
  architecture pinning — builds natively on any supported platform.

The fixes from `15e1b47` are general. An x86_64 build would use
`lib/x86_64-linux-gnu/` and the dynamic discovery works unchanged.

## W8 semantic equivalence

The `bisect --perf` harness drives both oracle and oxpinyin through the
identical CAPI sequence:

1. `pinyin_init(systemdir, userdir)` — context creation
2. `pinyin_alloc_instance(ctx)` — instance allocation
3. `pinyin_set_options(ctx, PINYIN_OPTION_MASK)` — enable parsing modes
4. Per-cycle: `pinyin_parse_more_chewing_keys(inst, ...)` →
   `pinyin_guess_candidates(inst, ...)` → `pinyin_get_n_candidate(inst, ...)`
5. `pinyin_free_instance(inst)` → `pinyin_fini(ctx)` — cleanup

The workload is the same keystroke sequence processed the same way. The
only difference is internal: which storage backend (Tkrzw vs KC) and data
format (`.bin`/`.db` vs `.kct`/`.text`) backs the CAPI.

This matches the W8 design: same harness, same API surface, same input
data (model20). The measurement environment differs (Docker/ARM64 vs
bare-metal/x86_64), but ratios are meaningful because both sides run in
the same environment.

## Stage-2 conclusion reassessment

### Initialization bottleneck: PARTIALLY PROVEN, REATTRIBUTED

The baseline correctly identified a ~118× init gap. The attribution was
wrong:

| Baseline attribution | Validated attribution |
|---|---|
| interpolation2.text parsing (80 MiB) | KC B-tree initialization (~89%) |
| Implicit: file I/O is the cost | interpolation2.text 1-gram parsing (~5–11%) |
| | File I/O negligible (~5 ms of ~116 ms) |

The bottleneck is proven to exist (118.1× ratio from raw JSONL) but
the root cause is **KC eager B-tree initialization**, not text file parsing.
The oracle avoids this cost by using mmap-backed binary files with lazy
page-fault loading.

### Implications for Stage-2 optimization

1. **Binary interpolation2.text**: would save only 5–13 ms (5–11% of init).
   Worth doing but not the primary win.
2. **KC initialization mode**: the dominant cost. Possible approaches:
   lazy-open, mmap-backed KC access, or replacing KC with a mmap-friendly
   format.
3. **Runtime data size**: the 80 MiB file ships 77.6 MiB of dead weight
   (the 2-gram section is pre-compiled into KC tables). Trimming to
   1-gram-only would reduce runtime data from 101.8 MiB to 24.2 MiB
   (0.68× oracle) with zero functional impact.

These are optimization directions, not changes made in this validation.

## Acceptance gate

| # | Check | Status | Evidence |
|---|---|---|---|
| 1 | Candidate baseline preserved | PASS | `15e1b47` unchanged; this branch adds validation only |
| 2 | strace file-access comparison | PASS | Oracle: .bin/.db (mmap), no interpolation2.text. oxpinyin: .kct + interpolation2.text (1-gram only, 2 MiB of 80 MiB) |
| 3 | Init attribution with measurement | PASS | KC init ~104 ms (89%), text parsing ~5–13 ms (5–11%), I/O ~5 ms. Measured via data-subset ablation with CPU pinning |
| 4 | File size separated from runtime cost | PASS | 80 MiB file, 2 MiB read, 5 ms I/O. File size ≠ runtime cost |
| 5 | Oracle artifact investigated | PASS | gen_binary_files + import_interpolation + gen_unigram. Source inspected in libpinyin-2.11.91 |
| 6 | Build reproducibility tested | PASS | .so bit-identical across two builds; KC data identical to export. Oracle not tested (pin, not target) |
| 7 | Size comparison semantics validated | PASS | Oracle .so has debug symbols, oxpinyin does not. Stripped: oracle 771 KiB, oxpinyin 1,669 KiB (ratio 2.16×, not 0.454×) |
| 8 | Architecture portability confirmed | PASS | No arch-specific code in scripts. Dynamic pkgconfig discovery. Harness is pure C/dlopen |
| 9 | W8 semantic equivalence verified | PASS | Same bisect.c harness, same CAPI sequence, same model20 data. Environment differs (Docker/ARM64 vs bare-metal/x86_64) |
| 10 | Headline numbers from raw JSONL | PASS | init 118.1×, cold 0.986×, steady 1.079×, RSS 5.45×, HWM 4.46×. Baseline's "~110×" init was imprecise |
| 11 | Stage-2 conclusion classified | PASS | PARTIALLY PROVEN: bottleneck exists (118.1×) but root cause is KC init, not text parsing |
| 12 | No production code optimized | PASS | Only measurement scripts, Dockerfile, and documentation modified |
