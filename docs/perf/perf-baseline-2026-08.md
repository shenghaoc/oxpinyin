# W8 performance baseline — oracle vs installed oxpinyin (2026-08)

Date: 2026-08-16 · Status: **measurement only — no decode, capi, or core
behavior changed.** This is the last input the W8 report needs and the first
entry on the Stage-2 scoreboard.

Host: Intel(R) Core(TM) i7-9750H @ 2.60 GHz, 12 logical CPUs, one
measurement CPU per run (pinned to CPU 3), Linux 6.12. Rust 1.97.1,
cargo-c 0.10.24. Release build, `cargo cinstall --locked --release`.
Oracle: pin-built libpinyin `2.11.91` @
`0c5e80e1200f84fab185d1c5bde458b770a0636c` + model20
`59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155`.

Both backends are driven by the same dlopen harness (`tools/bisection/bisect.c
--perf`), so both pay their own C-ABI boundary. The oxpinyin side loads the
**installed** `libpinyin_capi.so` from a staged `cargo cinstall` tree; the
tree and flags are located with `pkg-config --cflags --libs oxpinyin` against
`PKG_CONFIG_SYSROOT_DIR`/`PKG_CONFIG_PATH`. No `target/release` artifact is
measured.

Profile both sides: `pinyin_set_options(0x18a)` and `pinyin_guess_candidates(
offset=0, sort=0x1e)`. The sequence is the same 20-input corpus as the
Criterion `keystroke_cycle_20_inputs` bench: `ni wo de nihao zhongguo xian
fangan xi'an bu'tian fan'gan n zh chongke caisho paolen waimenggu lenglan
naoxion liangniejue chuaipengdengzaimiu`. For each input the instance is
reset, then every accumulated ASCII prefix is driven as one keystroke:
`pinyin_parse_more_full_pinyins(prefix)`, `pinyin_guess_candidates`,
`pinyin_get_n_candidate`. That is 123 parse+guess+count steps per cycle. The
harness never selects or trains, so the parity-profile-excludes-training note
does not enter the decode measurement.

## Method per axis

- **Speed.** Alternating oracle/oxpinyin processes, back-to-back, each pinned
  to the same CPU with `taskset`. Each process measures `pinyin_init` on its
  own, allocates one instance, then runs 8 cycles. Cycle 0 is reported as
  cold; cycles 1–7 are steady state. Two identical configurations were run:
  20 processes per backend each, so 40 processes per backend, 40 cold cycles,
  and 280 steady cycles total.
- **Size.** Only installed prefixes: the pin-built oracle prefix and the
  staged `cargo cinstall` tree plus the data packagers must add to
  `share/oxpinyin` (cargo-c ships only the library, header, `.pc`, and `.a`).
  Sizes are logical bytes of regular files plus symlink bytes; the tiny `.so`
  symlink chain is not double-counted. MiB is 1024² bytes; KiB is 1024 bytes.
- **RAM.** The same harness reads `/proc/self/status` in every process.
  `ram-init` processes stop after init+alloc and report post-init RSS/HWM.
  `ram-cycle` processes run the 8-cycle sequence and report after-first and
  after-last RSS/HWM. `VmHWM` is monotonic per process, so after-last HWM is
  the lifetime peak (init plus cycle), not a cycle-only reset; the init-only
  HWM separates the two phases.

## Axis 1 — execution speed

Combined over the two identical configurations. Median [min, max].

| Metric | oracle | oxpinyin | ratio |
|---|---:|---:|---:|
| `pinyin_init` | 3.711 ms [2.529, 4.710] | 586.472 ms [445.292, 729.675] | **158x** |
| `pinyin_alloc_instance` | 0.001 ms [0.001, 0.003] | 48.483 ms [32.804, 84.500] | **48,483x** |
| cycle 0 (cold) | 55.940 ms [42.247, 75.846] | 115.424 ms [76.626, 219.325] | **2.06x** |
| cycles 1–7 (steady) | 51.628 ms [38.156, 77.007] | 113.234 ms [73.474, 319.829] | **2.19x** |

Run-to-run medians for the two identical configurations:

| Configuration | oracle steady | oxpinyin steady | ratio |
|---|---:|---:|---:|
| run 1 | 54.253 ms | 113.333 ms | 2.089x |
| run 2 | 49.805 ms | 112.547 ms | 2.260x |

The oxpinyin medians agree within 0.7%; the oracle median moved 8.9% between
the two configurations on this shared machine. The published ratio band is
therefore **2.09–2.26x**, with 2.19x as the combined median. That band is
meaningful and does not substantially revise the earlier 2.0–2.2× estimate.
The earlier absolute oracle number (38.169 ms/cycle) was not reproduced under
today's host load; the earlier oxpinyin criterion number was engine-level,
while this number includes the C-ABI path for both sides.

Cold start is separated: oxpinyin pays its load cost in `pinyin_init`
(586 ms, dominated by parsing the 83,457,181-byte `interpolation2.text` and
slurping the redb tables), not in cycle 0. Oracle pays ~3.7 ms at init and a
smaller first-cycle warm-up (~56 ms cold vs ~52 ms steady). Conflating init
with the hot loop would hide both: the hot path is **2.19x**, the startup
story is **158x**.

## Axis 2 — size on disk

Installed prefixes, logical bytes. Runtime footprint = real shared object +
data files a deployment must ship. Total install also includes the `.a`,
headers, `.pc`, and (for libpinyin) its tools/docs.

| Side | shared object | runtime data | runtime footprint | total install |
|---|---:|---:|---:|---:|
| oracle | 5,702,848 B (5.44 MiB) | 37,266,687 B (35.54 MiB) | 42,969,535 B (40.98 MiB) | 61,578,505 B (58.73 MiB) |
| oxpinyin | 2,288,040 B (2.18 MiB) | 129,787,037 B (123.77 MiB) | 132,075,077 B (125.96 MiB) | 159,185,747 B (151.81 MiB) |
| ratio | **0.40x** | **3.48x** | **3.07x** | **2.59x** |

Code vs data inside each prefix:

| Side | installed code (.so/.a/bin) | installed data | other (headers/docs/.pc) |
|---|---:|---:|---:|
| oracle | 17,136,504 B (16.34 MiB) | 37,266,687 B (35.54 MiB) | 7,175,276 B (6.84 MiB) |
| oxpinyin | 29,382,810 B (28.02 MiB) | 129,787,037 B (123.77 MiB) | 15,854 B (0.02 MiB) |

The oxpinyin shared object is smaller than libpinyin's, but the runtime data
is 3.48x larger. The three redb tables are 44.18 MiB (runtime data minus
`interpolation2.text`); `interpolation2.text` alone is 83,457,181 bytes
(79.59 MiB) and is mandatory at runtime since #84 made public `pinyin_init`
fail closed. The 27,094,770-byte (25.84 MiB) `.a` is cargo-c install
overhead, not runtime: it makes oxpinyin's total code 1.71x oracle's code
even though the shared object is 0.40x.

## Axis 3 — RAM

Same `/proc/self/status` reads in both harness processes. Median [min, max],
10 runs per backend per mode in each configuration (20 combined).

| Backend | post-init RSS | post-init HWM | after-first HWM | after-last HWM (lifetime peak) |
|---|---:|---:|---:|---:|
| oracle | 12,012 KiB [11,884, 12,120] | 12,012 KiB [11,884, 12,120] | 15,110 KiB [15,016, 15,188] | 15,398 KiB [15,300, 15,472] |
| oxpinyin | 98,708 KiB [98,656, 98,812] | 98,708 KiB [98,656, 98,812] | 98,798 KiB [98,680, 98,908] | 98,804 KiB [98,680, 98,908] |
| ratio | **8.22x** | **8.22x** | **6.54x** | **6.42x** |

RssAnon/RssFile explain the shapes: after init, oracle has
1,420 KiB anonymous / 10,612 KiB file-backed (its mmap'd binary tables are
resident pages), while oxpinyin has 95,542 KiB anonymous / 3,156 KiB
file-backed (redb contents are slurped into anonymous BTreeMaps and the text
model becomes Rust structures).

Mmap-accounting note: `VmHWM`/`VmRSS` count resident pages, so libpinyin's
mmap'd tables are **not** inflating its apparent RSS — if anything the
comparison understates its footprint in address-space terms: oracle `VmSize`
is ~57 MiB while its RSS is 11.7–15.0 MiB (12,012–15,398 KiB). Oxpinyin has
`VmSize` ≈ `VmRSS` ≈ 96.4 MiB (98,708 KiB) because the data lives in
anonymous heap. The peak number above is the monotonic per-process HWM; the
init-only runs show that oxpinyin reaches essentially its final working set
at init, whereas oracle grows from 11.7 MiB to 15.0 MiB during the cycle.

## Stage-2 verdict

- **Smaller binary on disk: mixed.** The shared object is 0.40x (smaller),
  but the runtime footprint is 3.07x and total install is 2.59x. Data, not
  code, is the loss — specifically the mandatory 79.59 MiB text model.
- **Faster: no, currently ~2.19x slower steady state** (2.09–2.26x across
  two configurations), and 2.06x slower on the cold cycle. Startup is a
  separate, much larger loss: `pinyin_init` is ~158x slower because oxpinyin
  parses `interpolation2.text` and materializes the redb tables at init.
- **Much less RAM: no.** Post-init RSS is 8.22x oracle and peak RSS is 6.42x
  oracle; oxpinyin sits at 96.4 MiB (98,708 KiB) before the first keystroke
  and stays there.

## Candidate Stage-2 targets, ranked by these numbers

Observations only — no optimization work in this PR.

1. **Init-time data materialization.** `pinyin_init` 586 ms and 96.4 MiB RSS
   both come from the same design: `interpolation2.text` is parsed into
   per-key structures and all three redb tables are read into BTreeMaps.
   A streaming/lazy/compacted-binary representation attacks both the 158x
   init gap and the 8.2x RAM gap at once.
2. **Hot decode allocation/table reads.** Steady state is 2.19x. The prior
   Callgrind/alloc profile points at `LookupTable::get`, redb key `memcmp`,
   candidate string clones, and `Vec<Candidate>` growth. This is the
   classic first decode target, but it is smaller than the RAM/init problem
   and should not be micro-optimized before representation work changes the
   input shape.
3. **`interpolation2.text` runtime format.** 79.59 MiB of the 123.77 MiB
   runtime data is an ASCII text file that libpinyin ships in binary tables.
   A compact binary/redb unigram representation would directly cut the
   3.48x data ratio and likely help init parse time.
4. **Install-code packaging.** The cargo-c `.a` is 25.84 MiB of the 28.02 MiB
   installed code and is not needed by the shared-library runtime path.
   Decide whether distro policy really wants it shipped.
5. **Cold-cycle warmth.** There is little separate cold-cycle headroom:
   cycle 0 is only 2% slower than steady state on oxpinyin; the cold cost has
   already been paid at init.

## Caveats

- This is a shared, not isolated, machine; absolute medians moved between the
  two identical configurations (oracle steady 54.25 → 49.81 ms). The
  alternating pinned design and the repeat run bound the ratio to 2.09–2.26x.
- `VmHWM` is monotonic and cannot be reset per phase; post-init HWM and
  after-last HWM are therefore reported separately, and the peak row is the
  lifetime peak, not a cycle-only reset.
- Oracle's mmap pages count in RSS only when resident, so the RSS comparison
  is fair and conservative in address-space terms (`VmSize` is reported).
- `cargo cinstall` does not install data files; this measurement stages them
  under `share/oxpinyin`, which is the documented packager step
  (`docs/packaging.md`). The `.a`, header, and `.pc` are included only in the
  total-install row.
- The parity-profile-excludes-training note still applies: the W8 GSettings
  parity profile uses sort option 2, whose sentence-candidate-excluding mask
  never reaches `pinyin_train`. This decode-only harness uses the requested
  ABI sort `0x1e` and does not select or train at all.
- Symlinks in both installs total <100 bytes and are counted but immaterial.

## Harness

- `tools/bisection/bisect.c` `--perf` mode — dlopen either `.so`, identical
  sequence/profile, emits one JSON line per process with init/alloc/cycle
  timings and `/proc/self/status` fields. Default bisection output unchanged.
- `tools/bisection/run-perf-baseline.sh` — builds/stages the installed
  cargo-c tree, resolves it through `pkg-config oxpinyin`, alternates the
  speed runs, runs ram-init/ram-cycle, and measures installed sizes.
- `tools/bisection/perf-baseline.py` — summary/report helper for the JSONL
  captures and the size breakdown.
- Repro: `PINYIN_ORACLE_PREFIX=$HOME/.local/opt/pinyin-oracle \
  PERF_CPU=3 PERF_RUNS=20 PERF_CYCLES=8 PERF_RAM_RUNS=10 \
  tools/bisection/run-perf-baseline.sh`

Continued by the Stage-2 measurement harness in
`docs/perf/perf-stage2-harness-2026-08.md` (Criterion groups over the
C ABI, `[profile.profiling]`, `tools/profile/run-w8-cycle.sh`).
