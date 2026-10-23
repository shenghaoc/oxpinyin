# KC and tkrzw Pss attribution (#1098)

Date: 2026-10-23 UTC (`date -u` at run time). Cells: Kyoto Cabinet (`kc`)
and tkrzw, each with the same backend on both sides. Shipping source is
`origin/main` 294f14eb. The tools are `wip/1098-kc-tkrzw-pss` at da6e4e9b,
before its rebase onto `wip/1091-cold-cloud` f9e21f0; the parent's only
functional addition, f8aa567, changes cold samples only. No library code
changed. Every command is given where its figure is used. The raw
record streams were not kept: they lived only in this disposable cloud
container.

## Summary

- **Total Pss at M1, ox − pin (ox/pin, paired median):**

  | cell | pinyin `phrases` | pinyin `journals` | zhuyin `phrases` | zhuyin `journals` |
  | --- | ---: | ---: | ---: | ---: |
  | KC | +221 KiB (1.013) | +245 KiB (1.016) | +302 KiB (1.025) | +353 KiB (1.028) |
  | tkrzw | −172 KiB (0.985) | −132 KiB (0.989) | +140 KiB (1.013) | +204 KiB (1.020) |

  Every Pss and Private_Dirty control is inside its bound, at M0 and M1, in
  all eight cells. These come from the teardown tool's M1 checkpoint, not
  from `tools/measure-function-order.py`, so they are not the same figures
  as #1096's.
- **The KC pinyin total-Pss control is valid in this run.** At M1 the paired
  control − ox range is [−78, +52] KiB (`phrases`) and [−66, +74] KiB
  (`journals`) against a bound of ±163 KiB. The spread follows the
  randomized address layout. With ASLR disabled for the whole run
  (`setarch -R`), control − ox is 0 [0, 0] for every metric at M0 and M1,
  and every mapping group has zero range.
- **Above the pin at M1, by Pss:**
  1. KC: libkyotocabinet text, **+580 KiB** in all four cells. The library is
     the same on both sides. The pin runs KC's `HashDB`/`PlantDB` code
     compiled into its own text from the KC headers. oxpinyin calls the KC C
     API, so it runs the library's compiled copies. Counting the facade's
     own text (−208 pinyin, −160 / −156 zhuyin), the net code difference is +372 KiB (pinyin) and
     +420 / +424 KiB (zhuyin).
  2. Duplicate replay views of system phrase files (#985): +172 KiB
     (`phrases`) and +212 KiB (`journals`), in both cells and both facades.
  3. tkrzw: libtkrzw text, **+152 KiB** in all four cells. The library is the
     same on both sides. The pin calls the tkrzw C++ classes; oxpinyin calls
     `tkrzw_dbm_open` in the C API.
  4. tkrzw zhuyin facade text: +72 KiB. tkrzw pinyin's is +44, inside the
     bound.
- **Below the pin at M1:** tkrzw pinyin `addon_pinyin_index.bin` −332 KiB;
  KC facade text −208 (pinyin) and −160 / −156 (zhuyin); facade `r--` −88 to −144; GLib text −64 to
  −128; KC pinyin `addon_pinyin_index.bin` −96 / −108.
- **Massif heap at M1, ox/pin:** KC 1.009–1.030, tkrzw 1.006–1.009. The
  whole-lifecycle peak is 1.009–1.045 (KC) and 1.006–1.009 (tkrzw). Every
  Massif control is exact.
- **Where the extra text comes from**
  ([function-level attribution](#function-level-attribution-of-the-extra-text)):
  - libkyotocabinet: KC's `PolyDB` dispatch and `kc*` wrappers (+77 KiB),
    and code of KC classes that neither side opens (+261 KiB pinyin, +270
    zhuyin). ox executes
    none of those classes; their pages are resident because they are
    interleaved with the code it runs. The classes both sides open cost about
    the same: 199 KiB for ox in the library, 226–244 KiB for the pin, mostly
    in its own text.
  - libtkrzw: the C API's `key=value` parameter parsing and
    `PolyDBM`/`PolyFile` construction. ox executes 50 functions (39,497 B)
    that the pin does not, and no function of an unopened class.
  - tkrzw zhuyin facade text: oxpinyin's own startup code. Its init-to-M1 path
    runs 26,033 distinct instructions in 406 KiB of functions, against the
    pin's 5,724 in 37 KiB. It is not a C-API path.

## Setup

| item | value |
| --- | --- |
| host | claude.ai cloud session; Intel Xeon Processor @ 2.10 GHz, 4 CPUs, 15 GiB; kernel 6.18.44-fc-v153; Ubuntu 24.04.5; measured CPU 2 |
| filesystems | `/dev/vda` ext4 for the checkout, the stages, the work directories, the system data, the setup cache and `TMPDIR` (`findmnt -T`); each `start` record gives `ext4` for the work directory, every measured copy and the model |
| pins | `tools/cloud/setup.sh --cells "kc tkrzw"`: KC libpinyin.so.15.0.0 `ddbb3ce4…5ef6`, libzhuyin.so.15.0.0 `e538c36c…3dc4`; tkrzw `d78a9167…f7b8`, `adbcc8c8…4c3d`; all four equal `oracle-pin.txt`; both data manifests OK |
| KC runtime | host `libkyotocabinet16v5` 1.2.80-1build1, `/usr/lib/x86_64-linux-gnu/libkyotocabinet.so.16.14.0`, on both sides |
| tkrzw runtime | setup's tkrzw 1.0.34 build, `/root/.cache/oxpinyin/tkrzw/lib/libtkrzw.so.1.77.0`, sha256 `ead8c80b3fc155827989e8c9bbcc90d9b02bba22b8848271a90f2248876f38d9` (the host library failed the probe). `ldd` resolves `libtkrzw.so.1` to it for both pin facades and both subjects; the controller's runtime check passed for every copied object before every launch |
| subject KC libpinyin.so.15.0.0 | sha256 `a3a084acc7ee803e5a8564d03f7fd1aeaed842d5783cb0bcb1dbab9d8eb82103`; installed 5,195,616 B; stripped 2,482,912 B |
| subject KC libzhuyin.so.15.0.0 | sha256 `8eb319d49a18ff10ca63cef07c18fe89ee83ae8988a4acc76814c8d94bab12ea`; installed 4,913,152 B; stripped 2,233,056 B |
| subject tkrzw libpinyin.so.15.0.0 | sha256 `776e48a2db31e4979147a65cffa0d5c507a082734a20f901044a703a8308a18b`; installed 5,200,584 B; stripped 2,487,008 B |
| subject tkrzw libzhuyin.so.15.0.0 | sha256 `7a35c0a0ef430e9e85e1f037e7826e04c2ea0cfa57921dcaae40dd76e24b16bb`; installed 4,926,800 B; stripped 2,245,344 B |
| subject build | compatibility image `oxpinyin-compat:ddd2c1a8e182d4d3` (`tools/oracle/Dockerfile.compat`; GNU ld (GNU Binutils for Debian) 2.47, GCC 15.3.0), Rust 1.97.1, cargo-c 0.10.24 |
| function ordering | applied: `install.sh` printed `function ordering: using measured policy …/function-order/kc/libpinyin.order`, `…/kc/libzhuyin.order`, `…/tkrzw/libpinyin.order` and `…/tkrzw/libzhuyin.order`; the teardown controller's arm check reported `ordered` for ox and control in both cells |
| subject dynamic section | no RPATH or RUNPATH. `DT_NEEDED`: libglib-2.0.so.0, the backend library (libkyotocabinet.so.16 or libtkrzw.so.1), libgcc_s.so.1, libc.so.6, ld-linux-x86-64.so.2 |
| system data | teardown model identity KC `215574fa…19ff`, tkrzw `3ed8143e…268a` (data regenerated by this host's setup) |
| profiles | teardown provisioner, 10,000 rows each: KC `phrases` `2af37cd0…dd12`, `journals` `b1b25c69…7ddc`; tkrzw `phrases` `d9d9cd94…871e`, `journals` `65d472a3…83fb` |
| controller | CPython 3.15.0; worker built by the controller with host GCC 13.3.0; Valgrind 3.22.0 |

**Subject build.** The host's GNU ld 2.42 has no `--section-ordering-file`,
so the subjects were built in the compatibility image, as in #1091
(`docs/findings/cold-cache-startup-2026-10-23.md`), from a `git archive`
of da6e4e9b in `$OUT/src` on the host ext4 disk:

```sh
docker run --rm --platform linux/amd64 --network host \
  -e HTTPS_PROXY -e CARGO_HTTP_CAINFO=/ca.crt -e SSL_CERT_FILE=/ca.crt \
  -e CARGO_INSTALL_ROOT=/w/cargo \
  -v /root/.cache/oxpinyin/tkrzw:/root/.cache/oxpinyin/tkrzw:ro \
  -v "$CA:/ca.crt:ro" -v "$OUT:/w" -w /w/src oxpinyin-compat:ddd2c1a8e182d4d3 bash -euc '
    export PATH=/w/cargo/bin:$PATH
    CARGO_TARGET_DIR=/w/cargo-build cargo install cargo-c --version 0.10.24 --locked --features vendored-openssl
    CARGO_TARGET_DIR=/w/target-kc tools/packaging/install.sh libpinyin --prefix=/w/stage-kc -- --locked --release --no-default-features --features kyotocabinet,shipped
    CARGO_TARGET_DIR=/w/target-kc tools/packaging/install.sh libzhuyin --prefix=/w/stage-kc -- --locked --release --no-default-features --features kyotocabinet
    export PKG_CONFIG_PATH=/root/.cache/oxpinyin/tkrzw/lib/pkgconfig
    CARGO_TARGET_DIR=/w/target-tkrzw tools/packaging/install.sh libpinyin --prefix=/w/stage-tkrzw -- --locked --release --no-default-features --features tkrzw,shipped
    CARGO_TARGET_DIR=/w/target-tkrzw tools/packaging/install.sh libzhuyin --prefix=/w/stage-tkrzw -- --locked --release --no-default-features --features tkrzw'
```

`OXPINYIN_FUNCTION_ORDER_POLICY` was unset, and the build ran under
`python -m tools.dev slot run --`. For tkrzw, `PKG_CONFIG_PATH` names setup's
libtkrzw, as `docs/packaging.md` ("Regeneration") requires. A first tkrzw
link without it failed with `rust-lld: error: unable to find library -llz4`
and `-lzstd`. The image's Debian `tkrzw.pc` (1.0.32) lists
`-ltkrzw -llzma -llz4 -lzstd -lz -lstdc++ -lrt -latomic -lpthread -lm -lc`.
The image has only the `liblz4-1` and `libzstd1` runtime packages, so it has
no `liblz4.so` or `libzstd.so` to link. `crates/oxpinyin-store/build.rs`
(`tkrzw::build`) passes every `-l` from `pkg-config --libs tkrzw` to the link.
Setup's `tkrzw.pc` lists `-ltkrzw -lstdc++ -lrt -latomic -lpthread -lm -lc`.
The pin links only `-ltkrzw`: `configure.ac:125-127` at 074a2219 runs
`PKG_CHECK_MODULES(TKRZW, …)` then `AC_SEARCH_LIBS([tkrzw_dbm_open],
[tkrzw])`, and `src/Makefile.am:91` adds only `@GLIB2_LIBS@`.

## Commands

Each cell's campaign ran these stages in order, one invocation each, in one
sitting on one host:

```sh
PINYIN_ORACLE_DBM=$CELL source /root/.oxpinyin-oracle.env
export TMPDIR=/home/user/tmp1098          # ext4
python -m tools.dev slot run -- python tools/bisection/contract-diff.py \
  "$CELL" "$PIN" "$STAGE/lib/libpinyin.so.15.0.0" \
  --zhuyin-so "$STAGE/lib/libzhuyin.so.15.0.0" \
  --teardown --teardown-work-dir "$WORK" --teardown-cpu 2 \
  --teardown-cases phrases,journals --teardown-stage STAGE [ARGS]
```

`python` is `uv run --no-project --managed-python --python cpython@3.15+gil
python -B -X lazy_imports=normal`; `$PIN` is `$OXPINYIN_ORACLE_PREFIX_KC` or
`…_TKRZW`. The stages were `provision`, `confirm`, then `attribute` with
`--teardown-attribution-case phrases` and with `journals`.

| run | stages | UTC |
| --- | --- | --- |
| KC | provision, confirm, attribute ×2 | 12:33:19Z–12:45:06Z |
| KC pinyin, ASLR off | confirm with `--teardown-facades pinyin`, the controller under `setarch x86_64 -R` | 12:46:25Z–12:51:43Z |
| tkrzw | provision, confirm, attribute ×2 | 12:52:09Z–13:09:06Z |

The 1-minute load average was 0.66 before the KC and the tkrzw campaigns.
Nothing else heavy ran; `dockerd` and `containerd` were idle.

All four facade/profile cells were COMPARABLE in every `confirm` run.
`confirm` takes 60 timing rounds plus 3 warm-ups and 30 memory rounds plus
2 warm-ups per cell, in rotating ox/control/pin order, one process per
sample. Every memory figure below is from the `memory` and `memory-deltas`
records of a `confirm` stage. The timing records are not reported here.

Both `attribute` runs per cell printed their Callgrind and Massif records,
then exited 1 with `heap repetition needs an equivalence-verified case`: the
Memcheck repetition after them also needs the `empty` case, which these runs
did not include (the same stop as #1030). Only the Massif figures are used.

Group totals come from the same smaps read as the mapping groups. The later
`smaps_rollup` read differs from that sum by 0 KiB for Rss and Private_Dirty
and by 0 to +6 KiB for Pss, in every sample (`rollup_minus_smaps`).

## KC pinyin total-Pss control

#1096 measured total-Pss control bounds of [−85, 153] and [−105, 151] KiB
for KC pinyin with `tools/measure-function-order.py` on another host. This
run uses the teardown tool on this host. M1 Pss, paired control − ox, median
[min, max] over 30 pairs, KiB:

| profile | ASLR on | ASLR off | bound |
| --- | --- | --- | ---: |
| `phrases` | −4 [−78, +52] | 0 [0, 0] | ±163 |
| `journals` | −2 [−66, +74] | 0 [0, 0] | ±163 |

With ASLR on, both ranges lie inside the bound, so the control is valid.
The heap and anonymous mappings have no spread, and the stack varies by at
most 8 KiB. The spread is in file-backed mappings of shared libraries and
system files. Largest max − min over 30 samples, any of the three metrics,
pinyin `phrases`, M1, ox / control / pin, KiB:

| mapping group | ox | control | pin |
| --- | ---: | ---: | ---: |
| `punct.bin` (KC `TreeDB` file, `punct_table_kyotodb.cpp:62` at 074a2219) | 28 | 52 | 40 |
| libkyotocabinet text | 36 | 36 | 64 |
| GLib text | 32 | 32 | 64 |
| liblzo2 text | 24 | 28 | 28 |
| libm text | 12 | 12 | 48 |
| `[heap]`, `[anonymous]` | 0 | 0 | 0 |

The ASLR-off run is the same `confirm` command for the pinyin facade only,
with the controller started under `setarch x86_64 -R`, which its children
inherit. Every sample then gave the same value, for every metric and every
mapping group, on every side. The spread is therefore set by the address
layout. One likely mechanism, not traced in the kernel source here, is
fault-around: a fault maps nearby page-cache pages in a window aligned in
virtual-address space, and ASLR moves each mapping by whole pages. A fixed
layout is one layout sample, so the ASLR-on medians are the figures of
record. ASLR-off totals:

| cell | point | metric | ox median [p25, p75] | control median | pin median [p25, p75] | control − ox paired median [min, max] | bound | inside | ox − pin | ox/pin [95%] |
| --- | --- | --- | ---: | ---: | ---: | --- | ---: | --- | ---: | --- |
| pinyin `phrases` | M0 | Rss | 5,648 [5,648, 5,648] | 5,648 | 5,812 [5,812, 5,812] | 0 [0, 0] | 64 | yes | -164 | 0.972 [0.972, 0.972] |
| pinyin `phrases` | M0 | Pss | 3,658 [3,658, 3,658] | 3,658 | 3,852 [3,852, 3,852] | 0 [0, 0] | 64 | yes | -194 | 0.950 [0.950, 0.950] |
| pinyin `phrases` | M0 | Private_Dirty | 864 [864, 864] | 864 | 984 [984, 984] | 0 [0, 0] | 64 | yes | -120 | 0.878 [0.878, 0.878] |
| pinyin `phrases` | M1 | Rss | 18,336 [18,336, 18,336] | 18,336 | 18,156 [18,156, 18,156] | 0 [0, 0] | 183 | yes | +180 | 1.010 [1.010, 1.010] |
| pinyin `phrases` | M1 | Pss | 16,262 [16,262, 16,262] | 16,262 | 16,152 [16,152, 16,152] | 0 [0, 0] | 163 | yes | +110 | 1.007 [1.007, 1.007] |
| pinyin `phrases` | M1 | Private_Dirty | 6,920 [6,920, 6,920] | 6,920 | 6,968 [6,968, 6,968] | 0 [0, 0] | 69 | yes | -48 | 0.993 [0.993, 0.993] |
| pinyin `journals` | M0 | Rss | 5,648 [5,648, 5,648] | 5,648 | 5,812 [5,812, 5,812] | 0 [0, 0] | 64 | yes | -164 | 0.972 [0.972, 0.972] |
| pinyin `journals` | M0 | Pss | 3,658 [3,658, 3,658] | 3,658 | 3,852 [3,852, 3,852] | 0 [0, 0] | 64 | yes | -194 | 0.950 [0.950, 0.950] |
| pinyin `journals` | M0 | Private_Dirty | 864 [864, 864] | 864 | 984 [984, 984] | 0 [0, 0] | 64 | yes | -120 | 0.878 [0.878, 0.878] |
| pinyin `journals` | M1 | Rss | 18,484 [18,484, 18,484] | 18,484 | 18,156 [18,156, 18,156] | 0 [0, 0] | 185 | yes | +328 | 1.018 [1.018, 1.018] |
| pinyin `journals` | M1 | Pss | 16,290 [16,290, 16,290] | 16,290 | 16,152 [16,152, 16,152] | 0 [0, 0] | 163 | yes | +138 | 1.009 [1.009, 1.009] |
| pinyin `journals` | M1 | Private_Dirty | 6,956 [6,956, 6,956] | 6,956 | 7,008 [7,008, 7,008] | 0 [0, 0] | 70 | yes | -52 | 0.993 [0.993, 0.993] |

## Memory: totals

`memory` records of each `confirm` stage, KiB. The intervals are the
tool's p25–p75 over 30 samples. The control − ox column is the paired
difference: its median and [min, max] over the 30 pairs. The §7.5
residency bound is ±max(64 KiB, 1% of |median|) of the subject. "inside"
means the whole [min, max] range lies within the bound. The ox/pin column
is the tool's paired-median ratio with its bootstrap 95% interval.

### KC

| cell | point | metric | ox median [p25, p75] | control median | pin median [p25, p75] | control − ox paired median [min, max] | bound | inside | ox − pin | ox/pin [95%] |
| --- | --- | --- | ---: | ---: | ---: | --- | ---: | --- | ---: | --- |
| pinyin `phrases` | M0 | Rss | 5,652 [5,645, 5,659] | 5,652 | 5,830 [5,794, 5,836] | -4 [-24, +28] | 64 | yes | -178 | 0.970 [0.970, 0.972] |
| pinyin `phrases` | M0 | Pss | 3,673 [3,660, 3,677] | 3,670 | 3,847 [3,833.5, 3,851] | -4 [-24, +28] | 64 | yes | -174 | 0.955 [0.953, 0.956] |
| pinyin `phrases` | M0 | Private_Dirty | 868 [864, 871] | 868 | 988 [988, 988] | 0 [-8, +8] | 64 | yes | -120 | 0.879 [0.878, 0.879] |
| pinyin `phrases` | M1 | Rss | 18,334 [18,314, 18,344] | 18,338 | 18,076 [18,054, 18,087] | -4 [-68, +52] | 183 | yes | +258 | 1.014 [1.013, 1.015] |
| pinyin `phrases` | M1 | Pss | 16,268 [16,249.5, 16,281] | 16,269 | 16,047 [16,025, 16,077.5] | -4 [-78, +52] | 163 | yes | +221 | 1.013 [1.012, 1.015] |
| pinyin `phrases` | M1 | Private_Dirty | 6,924 [6,920, 6,924] | 6,920 | 6,972 [6,969, 6,972] | 0 [-8, +8] | 69 | yes | -48 | 0.993 [0.993, 0.993] |
| pinyin `journals` | M0 | Rss | 5,656 [5,645, 5,664] | 5,652 | 5,806 [5,788, 5,835] | -8 [-36, +40] | 64 | yes | -150 | 0.973 [0.971, 0.976] |
| pinyin `journals` | M0 | Pss | 3,677 [3,662.5, 3,681] | 3,668 | 3,841 [3,835, 3,850.5] | -5 [-36, +34] | 64 | yes | -164 | 0.956 [0.955, 0.958] |
| pinyin `journals` | M0 | Private_Dirty | 868 [868, 871] | 868 | 988 [988, 992] | 0 [-8, +4] | 64 | yes | -120 | 0.878 [0.875, 0.879] |
| pinyin `journals` | M1 | Rss | 18,480 [18,453, 18,502] | 18,472 | 18,078 [18,041, 18,099] | -6 [-72, +80] | 185 | yes | +402 | 1.023 [1.022, 1.024] |
| pinyin `journals` | M1 | Pss | 16,294 [16,269, 16,310.5] | 16,284 | 16,049 [16,015, 16,096] | -2 [-66, +74] | 163 | yes | +245 | 1.016 [1.012, 1.017] |
| pinyin `journals` | M1 | Private_Dirty | 6,960 [6,957, 6,960] | 6,960 | 7,012 [7,009, 7,012] | 0 [-8, +4] | 70 | yes | -52 | 0.993 [0.993, 0.993] |
| zhuyin `phrases` | M0 | Rss | 5,562 [5,556, 5,579] | 5,570 | 5,808 [5,800, 5,836] | +4 [-28, +52] | 64 | yes | -246 | 0.958 [0.956, 0.959] |
| zhuyin `phrases` | M0 | Pss | 3,582 [3,577, 3,601] | 3,590 | 3,842 [3,834.5, 3,854.5] | +4 [-34, +52] | 64 | yes | -260 | 0.934 [0.931, 0.938] |
| zhuyin `phrases` | M0 | Private_Dirty | 856 [856, 860] | 856 | 984 [984, 988] | 0 [-4, +4] | 64 | yes | -128 | 0.870 [0.870, 0.872] |
| zhuyin `phrases` | M1 | Rss | 14,324 [14,321, 14,343] | 14,338 | 13,988 [13,980, 14,036] | +8 [-28, +52] | 143 | yes | +336 | 1.024 [1.022, 1.025] |
| zhuyin `phrases` | M1 | Pss | 12,261 [12,258, 12,281] | 12,270 | 11,959 [11,953.5, 12,033] | +4 [-34, +52] | 123 | yes | +302 | 1.025 [1.021, 1.027] |
| zhuyin `phrases` | M1 | Private_Dirty | 3,560 [3,556, 3,560] | 3,560 | 3,580 [3,580, 3,584] | 0 [-4, +4] | 64 | yes | -20 | 0.993 [0.993, 0.994] |
| zhuyin `journals` | M0 | Rss | 5,580 [5,568, 5,584] | 5,568 | 5,832 [5,801, 5,840] | -8 [-40, +24] | 64 | yes | -252 | 0.957 [0.955, 0.959] |
| zhuyin `journals` | M0 | Pss | 3,600 [3,583, 3,605] | 3,583 | 3,849 [3,841, 3,858] | -10 [-40, +24] | 64 | yes | -249 | 0.934 [0.931, 0.936] |
| zhuyin `journals` | M0 | Private_Dirty | 856 [856, 860] | 856 | 984 [984, 988] | 0 [-4, +4] | 64 | yes | -128 | 0.870 [0.870, 0.870] |
| zhuyin `journals` | M1 | Rss | 14,492 [14,478, 14,500] | 14,482 | 13,986 [13,984, 14,016] | -10 [-44, +36] | 145 | yes | +506 | 1.036 [1.034, 1.036] |
| zhuyin `journals` | M1 | Pss | 12,310 [12,293.5, 12,317] | 12,295 | 11,957 [11,955, 12,013] | -6 [-50, +42] | 123 | yes | +353 | 1.028 [1.025, 1.030] |
| zhuyin `journals` | M1 | Private_Dirty | 3,592 [3,588, 3,592] | 3,592 | 3,620 [3,620, 3,623] | 0 [-4, +4] | 64 | yes | -28 | 0.991 [0.991, 0.992] |

### tkrzw

| cell | point | metric | ox median [p25, p75] | control median | pin median [p25, p75] | control − ox paired median [min, max] | bound | inside | ox − pin | ox/pin [95%] |
| --- | --- | --- | ---: | ---: | ---: | --- | ---: | --- | ---: | --- |
| pinyin `phrases` | M0 | Rss | 6,072 [6,068, 6,091] | 6,072 | 6,214 [6,212, 6,224] | 0 [-32, +36] | 64 | yes | -142 | 0.977 [0.976, 0.978] |
| pinyin `phrases` | M0 | Pss | 4,196 [4,194, 4,199.5] | 4,197 | 4,338 [4,334, 4,338] | +0.5 [-14, +36] | 64 | yes | -142 | 0.968 [0.966, 0.969] |
| pinyin `phrases` | M0 | Private_Dirty | 856 [852, 856] | 856 | 968 [965, 968] | 0 [-4, +8] | 64 | yes | -112 | 0.884 [0.881, 0.884] |
| pinyin `phrases` | M1 | Rss | 13,460 [13,456, 13,480] | 13,460 | 13,592 [13,588, 13,622] | 0 [-36, +36] | 135 | yes | -132 | 0.990 [0.990, 0.990] |
| pinyin `phrases` | M1 | Pss | 11,445 [11,441, 11,447] | 11,445 | 11,617 [11,613, 11,630.8] | +0.5 [-18, +36] | 114 | yes | -172 | 0.985 [0.985, 0.985] |
| pinyin `phrases` | M1 | Private_Dirty | 2,120 [2,120, 2,123] | 2,120 | 2,160 [2,157, 2,160] | 0 [-8, +4] | 64 | yes | -40 | 0.981 [0.981, 0.982] |
| pinyin `journals` | M0 | Rss | 6,072 [6,068, 6,088] | 6,070 | 6,214 [6,208, 6,224] | -2 [-32, +36] | 64 | yes | -142 | 0.977 [0.976, 0.979] |
| pinyin `journals` | M0 | Pss | 4,198 [4,194, 4,200] | 4,196 | 4,335.5 [4,332, 4,338] | 0 [-22, +36] | 64 | yes | -137.5 | 0.968 [0.967, 0.969] |
| pinyin `journals` | M0 | Private_Dirty | 856 [853, 856] | 856 | 968 [964, 968] | 0 [-8, +8] | 64 | yes | -112 | 0.884 [0.884, 0.885] |
| pinyin `journals` | M1 | Rss | 13,620 [13,616, 13,636] | 13,620 | 13,592 [13,588, 13,616] | -4 [-32, +36] | 136 | yes | +28 | 1.002 [1.001, 1.003] |
| pinyin `journals` | M1 | Pss | 11,485 [11,481, 11,487] | 11,484.5 | 11,617 [11,613, 11,635] | 0 [-26, +36] | 115 | yes | -132 | 0.989 [0.988, 0.989] |
| pinyin `journals` | M1 | Private_Dirty | 2,160 [2,160, 2,164] | 2,160 | 2,200 [2,196, 2,200] | 0 [-8, +8] | 64 | yes | -40 | 0.982 [0.982, 0.984] |
| zhuyin `phrases` | M0 | Rss | 5,996 [5,992, 6,011] | 6,006 | 6,222 [6,204, 6,224] | +2 [-24, +52] | 64 | yes | -226 | 0.966 [0.964, 0.967] |
| zhuyin `phrases` | M0 | Pss | 4,120.5 [4,118, 4,123.8] | 4,123 | 4,335.5 [4,332, 4,349] | +2.5 [-26, +28] | 64 | yes | -215 | 0.950 [0.948, 0.951] |
| zhuyin `phrases` | M0 | Private_Dirty | 844 [844, 847] | 844 | 964 [964, 968] | 0 [-4, +4] | 64 | yes | -120 | 0.876 [0.876, 0.876] |
| zhuyin `phrases` | M1 | Rss | 12,808 [12,808, 12,824] | 12,822 | 12,646 [12,577, 12,652] | 0 [-28, +52] | 128 | yes | +162 | 1.014 [1.013, 1.018] |
| zhuyin `phrases` | M1 | Pss | 10,793 [10,793, 10,798.8] | 10,798 | 10,653 [10,621, 10,659] | 0 [-30, +26] | 108 | yes | +140 | 1.013 [1.013, 1.016] |
| zhuyin `phrases` | M1 | Private_Dirty | 1,652 [1,652, 1,652] | 1,652 | 1,712 [1,712, 1,716] | 0 [-4, +4] | 64 | yes | -60 | 0.965 [0.964, 0.965] |
| zhuyin `journals` | M0 | Rss | 5,996 [5,992, 6,012] | 6,014 | 6,204 [6,195, 6,224] | +4 [-32, +84] | 64 | NO | -208 | 0.966 [0.966, 0.969] |
| zhuyin `journals` | M0 | Pss | 4,122 [4,118, 4,124] | 4,126 | 4,334 [4,322, 4,349] | +3.5 [-19, +47] | 64 | yes | -212 | 0.951 [0.949, 0.953] |
| zhuyin `journals` | M0 | Private_Dirty | 844 [844, 847] | 844 | 964 [964, 968] | 0 [-4, +4] | 64 | yes | -120 | 0.876 [0.876, 0.876] |
| zhuyin `journals` | M1 | Rss | 12,992 [12,992, 13,012] | 13,012 | 12,636 [12,577, 12,651] | +4 [-28, +84] | 130 | yes | +356 | 1.029 [1.028, 1.032] |
| zhuyin `journals` | M1 | Pss | 10,857 [10,857, 10,863] | 10,863 | 10,653 [10,621, 10,658] | +2 [-19, +47] | 109 | yes | +204 | 1.020 [1.019, 1.021] |
| zhuyin `journals` | M1 | Private_Dirty | 1,716 [1,716, 1,716] | 1,716 | 1,752 [1,752, 1,756] | 0 [0, +4] | 64 | yes | -36 | 0.979 [0.977, 0.979] |

Pss and Private_Dirty are inside the bound in every row of both cells. Rss
is not inside it in one row: tkrzw zhuyin `journals` at M0, +84 KiB against
64.

## Memory: per mapping at M0 and M1

`memory-deltas` records of the same `confirm` runs: the medians over 30
samples of each mapping group, summed into the classes below, in KiB.
"other shared libraries" is libc, libm, libgcc_s, the dynamic loader,
pcre2 and, for KC, libkyotocabinet's own dependencies libz, liblzma and
liblzo2 (`ldd`; the same set on both sides). Both sides map libstdc++ in
both cells: the pin links it, and the backend library pulls it in for the
subject. A data file's row adds up all mappings of that file. For the subject that is the
read-only view and, for replayed files, the private replay view (`rw-p`).
For the pin a replayed file is one `rw-p` mapping, and a backend container
(`*_index.bin`, `bigram.db`, `punct.bin`) is the backend library's own
mapping. The last column is the largest control − ox difference of the
group medians over the three metrics.

The group summaries are per side, not paired, so no paired interval
exists per group. M1 tables are given for every cell. At M0 no data file is
mapped. The M0 table uses `phrases`; each `journals` class median differs
from it by at most the amount stated under the table.

### KC pinyin `phrases`, M1

| mapping | Rss ox / pin | Pss ox / pin | Private_Dirty ox / pin | Pss ox − pin | control − ox (max abs, any metric) |
| --- | ---: | ---: | ---: | ---: | ---: |
| anonymous | 6,516 / 6,524 | 6,516 / 6,524 | 6,516 / 6,524 | -8 | 4 |
| facade lib text (r-x) | 508 / 716 | 508 / 716 | 0 / 0 | -208 | 0 |
| facade lib read-only (r--, incl. RELRO) | 272 / 400 | 272 / 400 | 52 / 92 | -128 | 0 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | 0 | 0 |
| libkyotocabinet (all) | 852 / 272 | 852 / 272 | 20 / 20 | +580 | 0 |
| libstdc++ (all) | 1,384 / 1,384 | 1,384 / 1,384 | 56 / 56 | 0 | 0 |
| GLib (all) | 560 / 624 | 560 / 624 | 8 / 8 | -64 | 0 |
| other shared libraries | 2,500 / 2,584 | 525 / 559 | 88 / 88 | -34 | 8 |
| other | 64 / 64 | 60 / 60 | 8 / 8 | 0 | 0 |
| data addon_phrase_index.bin | 232 / 224 | 232 / 224 | 0 / 0 | +8 | 0 |
| data addon_pinyin_index.bin | 224 / 320 | 224 / 320 | 0 / 0 | -96 | 0 |
| data bigram.db | 64 / 64 | 64 / 64 | 0 / 0 | 0 | 0 |
| data gb_char.bin | 3,160 / 2,904 | 3,076 / 2,904 | 172 / 172 | +172 | 0 |
| data gbk_char.bin | 340 / 340 | 340 / 340 | 0 / 0 | 0 | 0 |
| data merged.bin | 32 / 32 | 32 / 32 | 0 / 0 | 0 | 0 |
| data opengram.bin | 804 / 804 | 804 / 804 | 0 / 0 | 0 | 0 |
| data phrase_index.bin | 192 / 192 | 192 / 192 | 0 / 0 | 0 | 0 |
| data pinyin_index.bin | 436 / 436 | 436 / 436 | 0 / 0 | 0 | 0 |
| data punct.bin | 192 / 192 | 192 / 192 | 0 / 0 | 0 | 0 |
| sum of group medians | 18,336 / 18,080 | 16,273 / 16,051 | 6,924 / 6,972 | +222 | 12 |

### KC pinyin `journals`, M1

| mapping | Rss ox / pin | Pss ox / pin | Private_Dirty ox / pin | Pss ox − pin | control − ox (max abs, any metric) |
| --- | ---: | ---: | ---: | ---: | ---: |
| anonymous | 6,512 / 6,524 | 6,512 / 6,524 | 6,512 / 6,524 | -12 | 0 |
| facade lib text (r-x) | 508 / 716 | 508 / 716 | 0 / 0 | -208 | 0 |
| facade lib read-only (r--, incl. RELRO) | 272 / 400 | 272 / 400 | 52 / 92 | -128 | 0 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | 0 | 0 |
| libkyotocabinet (all) | 852 / 272 | 852 / 272 | 20 / 20 | +580 | 0 |
| libstdc++ (all) | 1,384 / 1,384 | 1,384 / 1,384 | 56 / 56 | 0 | 0 |
| GLib (all) | 560 / 624 | 560 / 624 | 8 / 8 | -64 | 0 |
| other shared libraries | 2,520 / 2,544 | 539 / 545 | 88 / 88 | -6 | 6 |
| other | 64 / 64 | 60 / 60 | 8 / 8 | 0 | 0 |
| data addon_phrase_index.bin | 224 / 192 | 224 / 192 | 0 / 0 | +32 | 0 |
| data addon_pinyin_index.bin | 224 / 332 | 224 / 332 | 0 / 0 | -108 | 0 |
| data bigram.db | 64 / 64 | 64 / 64 | 0 / 0 | 0 | 0 |
| data gb_char.bin | 3,032 / 2,904 | 2,960 / 2,904 | 56 / 56 | +56 | 0 |
| data gbk_char.bin | 468 / 340 | 400 / 340 | 60 / 60 | +60 | 0 |
| data merged.bin | 64 / 32 | 64 / 32 | 32 / 32 | +32 | 0 |
| data opengram.bin | 932 / 804 | 868 / 804 | 64 / 64 | +64 | 0 |
| data phrase_index.bin | 192 / 192 | 192 / 192 | 0 / 0 | 0 | 0 |
| data pinyin_index.bin | 436 / 436 | 436 / 436 | 0 / 0 | 0 | 0 |
| data punct.bin | 182 / 192 | 182 / 192 | 0 / 0 | -10 | 10 |
| sum of group medians | 18,494 / 18,020 | 16,305 / 16,017 | 6,960 / 7,012 | +288 | 4 |

### KC zhuyin `phrases`, M1

| mapping | Rss ox / pin | Pss ox / pin | Private_Dirty ox / pin | Pss ox − pin | control − ox (max abs, any metric) |
| --- | ---: | ---: | ---: | ---: | ---: |
| anonymous | 3,160 / 3,132 | 3,160 / 3,132 | 3,160 / 3,132 | +28 | 0 |
| facade lib text (r-x) | 520 / 680 | 520 / 680 | 0 / 0 | -160 | 0 |
| facade lib read-only (r--, incl. RELRO) | 268 / 412 | 268 / 412 | 44 / 92 | -144 | 0 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | 0 | 0 |
| libkyotocabinet (all) | 852 / 272 | 852 / 272 | 20 / 20 | +580 | 0 |
| libstdc++ (all) | 1,384 / 1,384 | 1,384 / 1,384 | 56 / 56 | 0 | 0 |
| GLib (all) | 560 / 686 | 560 / 686 | 8 / 8 | -126 | 0 |
| other shared libraries | 2,486 / 2,580 | 511 / 557 | 88 / 88 | -46 | 6 |
| other | 64 / 64 | 60 / 60 | 8 / 8 | 0 | 0 |
| data bigram.db | 64 / 64 | 64 / 64 | 0 / 0 | 0 | 0 |
| data gb_char.bin | 3,160 / 2,904 | 3,076 / 2,904 | 172 / 172 | +172 | 0 |
| data gbk_char.bin | 340 / 340 | 340 / 340 | 0 / 0 | 0 | 0 |
| data merged.bin | 32 / 32 | 32 / 32 | 0 / 0 | 0 | 0 |
| data opengram.bin | 804 / 804 | 804 / 804 | 0 / 0 | 0 | 0 |
| data phrase_index.bin | 192 / 192 | 192 / 192 | 0 / 0 | 0 | 0 |
| data pinyin_index.bin | 436 / 436 | 436 / 436 | 0 / 0 | 0 | 0 |
| sum of group medians | 14,326 / 13,986 | 12,263 / 11,959 | 3,560 / 3,580 | +304 | 6 |

### KC zhuyin `journals`, M1

| mapping | Rss ox / pin | Pss ox / pin | Private_Dirty ox / pin | Pss ox − pin | control − ox (max abs, any metric) |
| --- | ---: | ---: | ---: | ---: | ---: |
| anonymous | 3,152 / 3,132 | 3,152 / 3,132 | 3,152 / 3,132 | +20 | 0 |
| facade lib text (r-x) | 520 / 676 | 520 / 676 | 0 / 0 | -156 | 0 |
| facade lib read-only (r--, incl. RELRO) | 268 / 412 | 268 / 412 | 44 / 92 | -144 | 0 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | 0 | 0 |
| libkyotocabinet (all) | 852 / 272 | 852 / 272 | 20 / 20 | +580 | 0 |
| libstdc++ (all) | 1,384 / 1,384 | 1,384 / 1,384 | 56 / 56 | 0 | 0 |
| GLib (all) | 560 / 684 | 560 / 684 | 8 / 8 | -124 | 0 |
| other shared libraries | 2,500 / 2,580 | 525 / 557 | 88 / 88 | -32 | 16 |
| other | 64 / 64 | 60 / 60 | 8 / 8 | 0 | 0 |
| data bigram.db | 64 / 64 | 64 / 64 | 0 / 0 | 0 | 0 |
| data gb_char.bin | 3,032 / 2,904 | 2,960 / 2,904 | 56 / 56 | +56 | 0 |
| data gbk_char.bin | 468 / 340 | 400 / 340 | 60 / 60 | +60 | 0 |
| data merged.bin | 64 / 32 | 64 / 32 | 32 / 32 | +32 | 0 |
| data opengram.bin | 932 / 804 | 868 / 804 | 64 / 64 | +64 | 0 |
| data phrase_index.bin | 192 / 192 | 192 / 192 | 0 / 0 | 0 | 0 |
| data pinyin_index.bin | 436 / 436 | 436 / 436 | 0 / 0 | 0 | 0 |
| sum of group medians | 14,492 / 13,980 | 12,309 / 11,953 | 3,592 / 3,620 | +356 | 16 |

### KC M0, `phrases`

| mapping | pinyin Rss ox / pin | pinyin Pss ox / pin | pinyin Private_Dirty ox / pin | zhuyin Rss ox / pin | zhuyin Pss ox / pin | zhuyin Private_Dirty ox / pin |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| anonymous | 632 / 712 | 632 / 712 | 632 / 712 | 628 / 708 | 628 / 708 | 628 / 708 |
| facade lib text (r-x) | 252 / 192 | 252 / 192 | 0 / 0 | 200 / 192 | 200 / 192 | 0 / 0 |
| facade lib read-only (r--, incl. RELRO) | 120 / 284 | 120 / 284 | 52 / 92 | 100 / 284 | 100 / 284 | 44 / 92 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | 4 / 4 | 4 / 4 | 4 / 4 |
| libkyotocabinet (all) | 180 / 180 | 180 / 180 | 20 / 20 | 180 / 180 | 180 / 180 | 20 / 20 |
| libstdc++ (all) | 1,336 / 1,336 | 1,336 / 1,336 | 56 / 56 | 1,336 / 1,336 | 1,336 / 1,336 | 56 / 56 |
| GLib (all) | 560 / 544 | 560 / 544 | 8 / 8 | 560 / 554 | 560 / 554 | 8 / 8 |
| other shared libraries | 2,500 / 2,520 | 525 / 539 | 88 / 88 | 2,486 / 2,516 | 511 / 537 | 88 / 88 |
| other | 64 / 64 | 60 / 60 | 8 / 8 | 64 / 64 | 60 / 60 | 8 / 8 |

largest |phrases − journals| of any M0 class median above, any side or metric: 40 KiB

### tkrzw pinyin `phrases`, M1

| mapping | Rss ox / pin | Pss ox / pin | Private_Dirty ox / pin | Pss ox − pin | control − ox (max abs, any metric) |
| --- | ---: | ---: | ---: | ---: | ---: |
| anonymous | 1,720 / 1,724 | 1,720 / 1,724 | 1,720 / 1,724 | -4 | 0 |
| facade lib text (r-x) | 508 / 464 | 508 / 464 | 0 / 0 | +44 | 0 |
| facade lib read-only (r--, incl. RELRO) | 268 / 356 | 268 / 356 | 52 / 88 | -88 | 0 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | 0 | 0 |
| libtkrzw (all) | 1,636 / 1,484 | 1,636 / 1,484 | 36 / 36 | +152 | 0 |
| libstdc++ (all) | 1,384 / 1,384 | 1,384 / 1,384 | 56 / 56 | 0 | 0 |
| GLib (all) | 560 / 656 | 560 / 656 | 8 / 8 | -96 | 0 |
| other shared libraries | 2,272 / 2,336 | 345 / 365 | 64 / 64 | -20 | 0 |
| other | 64 / 64 | 60 / 60 | 8 / 8 | 0 | 0 |
| data addon_phrase_index.bin | 64 / 64 | 64 / 64 | 0 / 0 | 0 | 0 |
| data addon_pinyin_index.bin | 64 / 396 | 64 / 396 | 0 / 0 | -332 | 0 |
| data bigram.db | 64 / 64 | 64 / 64 | 0 / 0 | 0 | 0 |
| data gb_char.bin | 3,160 / 2,904 | 3,076 / 2,904 | 172 / 172 | +172 | 0 |
| data gbk_char.bin | 340 / 340 | 340 / 340 | 0 / 0 | 0 | 0 |
| data merged.bin | 32 / 32 | 32 / 32 | 0 / 0 | 0 | 0 |
| data opengram.bin | 804 / 804 | 804 / 804 | 0 / 0 | 0 | 0 |
| data phrase_index.bin | 64 / 64 | 64 / 64 | 0 / 0 | 0 | 0 |
| data pinyin_index.bin | 384 / 384 | 384 / 384 | 0 / 0 | 0 | 0 |
| data punct.bin | 64 / 64 | 64 / 64 | 0 / 0 | 0 | 0 |
| sum of group medians | 13,456 / 13,588 | 11,441 / 11,613 | 2,120 / 2,160 | -172 | 0 |

### tkrzw pinyin `journals`, M1

| mapping | Rss ox / pin | Pss ox / pin | Private_Dirty ox / pin | Pss ox − pin | control − ox (max abs, any metric) |
| --- | ---: | ---: | ---: | ---: | ---: |
| anonymous | 1,720 / 1,724 | 1,720 / 1,724 | 1,720 / 1,724 | -4 | 0 |
| facade lib text (r-x) | 508 / 464 | 508 / 464 | 0 / 0 | +44 | 0 |
| facade lib read-only (r--, incl. RELRO) | 268 / 356 | 268 / 356 | 52 / 88 | -88 | 0 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | 0 | 0 |
| libtkrzw (all) | 1,636 / 1,484 | 1,636 / 1,484 | 36 / 36 | +152 | 0 |
| libstdc++ (all) | 1,384 / 1,384 | 1,384 / 1,384 | 56 / 56 | 0 | 0 |
| GLib (all) | 560 / 656 | 560 / 656 | 8 / 8 | -96 | 0 |
| other shared libraries | 2,272 / 2,340 | 345 / 366.5 | 64 / 64 | -21.5 | 0 |
| other | 64 / 64 | 60 / 60 | 8 / 8 | 0 | 0 |
| data addon_phrase_index.bin | 64 / 64 | 64 / 64 | 0 / 0 | 0 | 0 |
| data addon_pinyin_index.bin | 64 / 396 | 64 / 396 | 0 / 0 | -332 | 0 |
| data bigram.db | 64 / 64 | 64 / 64 | 0 / 0 | 0 | 0 |
| data gb_char.bin | 3,032 / 2,904 | 2,960 / 2,904 | 56 / 56 | +56 | 0 |
| data gbk_char.bin | 468 / 340 | 400 / 340 | 60 / 60 | +60 | 0 |
| data merged.bin | 64 / 32 | 64 / 32 | 32 / 32 | +32 | 0 |
| data opengram.bin | 932 / 804 | 868 / 804 | 64 / 64 | +64 | 0 |
| data phrase_index.bin | 64 / 64 | 64 / 64 | 0 / 0 | 0 | 0 |
| data pinyin_index.bin | 384 / 384 | 384 / 384 | 0 / 0 | 0 | 0 |
| data punct.bin | 64 / 64 | 64 / 64 | 0 / 0 | 0 | 0 |
| sum of group medians | 13,616 / 13,592 | 11,481 / 11,614.5 | 2,160 / 2,200 | -133.5 | 0 |

### tkrzw zhuyin `phrases`, M1

| mapping | Rss ox / pin | Pss ox / pin | Private_Dirty ox / pin | Pss ox − pin | control − ox (max abs, any metric) |
| --- | ---: | ---: | ---: | ---: | ---: |
| anonymous | 1,260 / 1,276 | 1,260 / 1,276 | 1,260 / 1,276 | -16 | 0 |
| facade lib text (r-x) | 520 / 448 | 520 / 448 | 0 / 0 | +72 | 0 |
| facade lib read-only (r--, incl. RELRO) | 260 / 348 | 260 / 348 | 44 / 88 | -88 | 0 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | 0 | 0 |
| libtkrzw (all) | 1,636 / 1,484 | 1,636 / 1,484 | 36 / 36 | +152 | 0 |
| libstdc++ (all) | 1,384 / 1,384 | 1,384 / 1,384 | 56 / 56 | 0 | 0 |
| GLib (all) | 560 / 688 | 560 / 688 | 8 / 8 | -128 | 0 |
| other shared libraries | 2,272 / 2,364 | 345 / 375 | 64 / 64 | -30 | 20 |
| other | 64 / 64 | 60 / 60 | 8 / 8 | 0 | 0 |
| data bigram.db | 64 / 64 | 64 / 64 | 0 / 0 | 0 | 0 |
| data gb_char.bin | 3,160 / 2,904 | 3,076 / 2,904 | 172 / 172 | +172 | 0 |
| data gbk_char.bin | 340 / 340 | 340 / 340 | 0 / 0 | 0 | 0 |
| data merged.bin | 32 / 32 | 32 / 32 | 0 / 0 | 0 | 0 |
| data opengram.bin | 804 / 804 | 804 / 804 | 0 / 0 | 0 | 0 |
| data phrase_index.bin | 64 / 64 | 64 / 64 | 0 / 0 | 0 | 0 |
| data pinyin_index.bin | 384 / 384 | 384 / 384 | 0 / 0 | 0 | 0 |
| sum of group medians | 12,808 / 12,652 | 10,793 / 10,659 | 1,652 / 1,712 | +134 | 20 |

### tkrzw zhuyin `journals`, M1

| mapping | Rss ox / pin | Pss ox / pin | Private_Dirty ox / pin | Pss ox − pin | control − ox (max abs, any metric) |
| --- | ---: | ---: | ---: | ---: | ---: |
| anonymous | 1,284 / 1,276 | 1,284 / 1,276 | 1,284 / 1,276 | +8 | 0 |
| facade lib text (r-x) | 520 / 448 | 520 / 448 | 0 / 0 | +72 | 0 |
| facade lib read-only (r--, incl. RELRO) | 260 / 348 | 260 / 348 | 44 / 88 | -88 | 0 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | 0 | 0 |
| libtkrzw (all) | 1,636 / 1,484 | 1,636 / 1,484 | 36 / 36 | +152 | 0 |
| libstdc++ (all) | 1,384 / 1,384 | 1,384 / 1,384 | 56 / 56 | 0 | 0 |
| GLib (all) | 560 / 688 | 560 / 688 | 8 / 8 | -128 | 0 |
| other shared libraries | 2,274 / 2,354 | 345.5 / 371 | 64 / 64 | -25.5 | 18 |
| other | 64 / 64 | 60 / 60 | 8 / 8 | 0 | 0 |
| data bigram.db | 64 / 64 | 64 / 64 | 0 / 0 | 0 | 0 |
| data gb_char.bin | 3,032 / 2,904 | 2,960 / 2,904 | 56 / 56 | +56 | 0 |
| data gbk_char.bin | 468 / 340 | 400 / 340 | 60 / 60 | +60 | 0 |
| data merged.bin | 64 / 32 | 64 / 32 | 32 / 32 | +32 | 0 |
| data opengram.bin | 932 / 804 | 868 / 804 | 64 / 64 | +64 | 0 |
| data phrase_index.bin | 64 / 64 | 64 / 64 | 0 / 0 | 0 | 0 |
| data pinyin_index.bin | 384 / 384 | 384 / 384 | 0 / 0 | 0 | 0 |
| sum of group medians | 12,994 / 12,642 | 10,857.5 / 10,655 | 1,716 / 1,752 | +202.5 | 18 |

### tkrzw M0, `phrases`

| mapping | pinyin Rss ox / pin | pinyin Pss ox / pin | pinyin Private_Dirty ox / pin | zhuyin Rss ox / pin | zhuyin Pss ox / pin | zhuyin Private_Dirty ox / pin |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| anonymous | 628 / 704 | 628 / 704 | 628 / 704 | 624 / 700 | 624 / 700 | 624 / 700 |
| facade lib text (r-x) | 252 / 216 | 252 / 216 | 0 / 0 | 200 / 220 | 200 / 220 | 0 / 0 |
| facade lib read-only (r--, incl. RELRO) | 120 / 256 | 120 / 256 | 52 / 88 | 100 / 252 | 100 / 252 | 44 / 88 |
| facade lib writable (rw-) | 4 / 4 | 4 / 4 | 4 / 4 | 4 / 4 | 4 / 4 | 4 / 4 |
| libtkrzw (all) | 896 / 896 | 896 / 896 | 36 / 36 | 896 / 852 | 896 / 852 | 36 / 36 |
| libstdc++ (all) | 1,336 / 1,336 | 1,336 / 1,336 | 56 / 56 | 1,336 / 1,336 | 1,336 / 1,336 | 56 / 56 |
| GLib (all) | 560 / 528 | 560 / 528 | 8 / 8 | 560 / 560 | 560 / 560 | 8 / 8 |
| other shared libraries | 2,208 / 2,208 | 338 / 338 | 64 / 64 | 2,208 / 2,236 | 338 / 348 | 64 / 64 |
| other | 64 / 64 | 60 / 60 | 8 / 8 | 64 / 64 | 60 / 60 | 8 / 8 |

largest |phrases − journals| of any M0 class median above, any side or metric: 56 KiB

## Massif heap

From the `attribute` stages (`--time-unit=i --detailed-freq=1
--peak-inaccuracy=0`), in bytes. `mem_heap_B` is shown at M0 and M1. The
peak is the tool's `whole_lifecycle_peak`, the largest snapshot over the
whole lifecycle. Every `massif-control` record (M0–M3 for each facade and
profile) is VALID: the control equals the subject exactly.

| cell | ox M0 | ox M1 | ox peak | control peak | pin M0 | pin M1 | pin peak | ox/pin M1 | ox/pin peak |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| KC pinyin `phrases` | 116,638 | 6,077,954 | 6,084,069 | 6,084,069 | 116,686 | 6,018,097 | 6,024,120 | 1.010 | 1.010 |
| KC pinyin `journals` | 116,638 | 6,072,924 | 6,079,039 | 6,079,039 | 116,686 | 6,018,097 | 6,024,120 | 1.009 | 1.009 |
| KC zhuyin `phrases` | 116,638 | 2,642,974 | 3,008,859 | 3,008,859 | 116,686 | 2,565,311 | 2,879,129 | 1.030 | 1.045 |
| KC zhuyin `journals` | 116,638 | 2,637,944 | 2,672,019 | 2,672,019 | 116,686 | 2,565,311 | 2,571,350 | 1.028 | 1.039 |
| tkrzw pinyin `phrases` | 111,803 | 9,545,496 | 9,551,611 | 9,551,611 | 111,755 | 9,485,754 | 9,489,283 | 1.006 | 1.007 |
| tkrzw pinyin `journals` | 111,803 | 9,540,466 | 9,546,581 | 9,546,581 | 111,755 | 9,485,754 | 9,489,283 | 1.006 | 1.006 |
| tkrzw zhuyin `phrases` | 111,803 | 8,986,461 | 8,992,496 | 8,992,496 | 111,755 | 8,909,007 | 8,912,536 | 1.009 | 1.009 |
| tkrzw zhuyin `journals` | 111,803 | 8,981,431 | 8,987,466 | 8,987,466 | 111,755 | 8,909,007 | 8,912,536 | 1.008 | 1.008 |

At M1, ox − pin is +54,827 to +77,663 B in KC and +54,712 to +77,454 B in
tkrzw. For the same facade and profile the two backends differ by 115 B
(pinyin) or 209 B (zhuyin). The
largest KC allocation is the backend's own and is the same size on both
sides: 2,623,360 B (pinyin) and 2 × 1,049,344 B (zhuyin), allocated in
the subject under `kcdbopen` in libkyotocabinet, and in the pin under
`kyotocabinet::PlantDB<kyotocabinet::HashDB, 49>::create_leaf_cache()`
(`kcplantdb.h:2405`) in its own libpinyin.so. A tkrzw M1 heap of about
9 MB against an anonymous Pss of 1,260–1,720 KiB means that most of the
allocated bytes are never touched.

## Contributors above the pin

M1, Pss, ox − pin, KiB. The bound is ±max(64 KiB, 1% of the subject's
median) for the mapping, so 64 KiB in every row:

| rank | contributor | KC pinyin | KC zhuyin | tkrzw pinyin | tkrzw zhuyin | owner |
| --- | --- | ---: | ---: | ---: | ---: | --- |
| 1 | backend library text, KC | +580 / +580 | +580 / +580 | — | — | oxpinyin's KC adapter (`crates/oxpinyin-store`, `kyotocabinet`), through the same libkyotocabinet |
| 2 | replay views of `gb_char.bin`, `gbk_char.bin`, `merged.bin`, `opengram.bin` | +172 / +212 | +172 / +212 | +172 / +212 | +172 / +212 | #985 (open) |
| 3 | backend library text, tkrzw | — | — | +152 / +152 | +152 / +152 | oxpinyin's tkrzw adapter (`crates/oxpinyin-store`, `tkrzw`), through the same libtkrzw |
| 4 | facade library text (`r-x`) | −208 / −208 | −160 / −156 | +44 / +44 | +72 / +72 | no open issue: facade text residency was #1032 (closed by #1067); the tkrzw policy is #1076's |

Each pair is `phrases` / `journals`. Nothing else is above the bound in any
cell. Anonymous memory is −16 to +28.

**1. libkyotocabinet text.** The same `libkyotocabinet.so.16.14.0` is mapped
on both sides. Its `r-x` segment is 666,229 B (`readelf -lW`). At M1 the
subject has 652 KiB of it resident and the pin 72 KiB. At M0 both have 180
KiB of the library resident. The use differs:

- The pin instantiates KC's C++ classes from the KC headers in its own text.
  `nm -C --defined-only "$OXPINYIN_ORACLE_PREFIX_KC/lib/libpinyin.so"` lists
  602 `kyotocabinet::` symbols, among them `HashDB::open`, `HashDB::accept`,
  `PlantDB<HashDB, 49>::open` and `PlantDB<HashDB, 49>::accept` as local `t`
  symbols. The pin imports 78 `kyotocabinet::` symbols from the library
  (`nm -C -D --undefined-only`). The tables are attached as `TreeDB`
  (`chewing_large_table2_kyotodb.cpp:87`, `phrase_large_table3_kyotodb.cpp:102`,
  `punct_table_kyotodb.cpp:62`) and the system bigram as `HashDB`
  (`ngram_kyotodb.cpp:117`), at 074a2219.
- The subject imports 24 `kc*` C-API functions
  (`nm -D --undefined-only "$STAGE/lib/libpinyin.so.15.0.0"`). It runs the
  library's own compiled copies of the same classes.

The facade's own text is 208 KiB (pinyin) and 160 / 156 KiB (zhuyin) below
the pin's, so facade text plus libkyotocabinet text is +372 KiB (pinyin) and
+420 / +424 KiB (zhuyin). The function-level attribution is under
[Function-level attribution of the extra text](#function-level-attribution-of-the-extra-text).

**2. Replay views.** The same figures as on BDB: +172 (`phrases`) and +56,
+60, +32 and +64 (`journals`, `gb_char.bin`, `gbk_char.bin`, `merged.bin`,
`opengram.bin`). The excess is the read-only view's pages; the replay
view's Private_Dirty equals the pin's. Next step: #985.

**3. libtkrzw text.** The same setup-built `libtkrzw.so.1.77.0` is mapped on
both sides. Its `r-x` segment is 1,255,725 B. At M1 the subject has 1,100
KiB of it resident and the pin 948 KiB. The pin calls the C++ classes:
it imports 14 `tkrzw::` symbols, among them the `HashDBM`, `TreeDBM`,
`TinyDBM` and `BabyDBM` constructors and `HashDBM::OpenAdvanced` and
`TreeDBM::OpenAdvanced`. Its system bigram is a `HashDBM`
(`ngram_tkrzwdb.cpp:96-98` at 074a2219); the tables are `TreeDBM`
(`chewing_large_table2_tkrzwdb.cpp:87`, `phrase_large_table3_tkrzwdb.cpp:77`,
`punct_table_tkrzwdb.cpp:70`). The subject imports 16 `tkrzw_*`
C-API functions. It opens the file containers through `tkrzw_dbm_open` with
`dbm=tree` or `dbm=hash`, and its in-memory ones with `dbm=baby` or
`dbm=tiny` (`crates/oxpinyin-store/src/tkrzw/mod.rs`).
The functions behind the difference are under
[Function-level attribution of the extra text](#function-level-attribution-of-the-extra-text).

**4. tkrzw facade text.** +72 KiB for zhuyin, as #1076 measured with its
own tool; pinyin +44 is inside the bound. Attributed under
[Function-level attribution of the extra text](#function-level-attribution-of-the-extra-text).

### Below the pin

M1, Pss, ox − pin, KiB, `phrases` / `journals`:

| mapping | KC pinyin | KC zhuyin | tkrzw pinyin | tkrzw zhuyin |
| --- | ---: | ---: | ---: | ---: |
| `addon_pinyin_index.bin` | −96 / −108 | n/a | −332 / −332 | n/a |
| facade library text | −208 / −208 | −160 / −156 | — | — |
| facade library `r--` (incl. RELRO) | −128 / −128 | −144 / −144 | −88 / −88 | −88 / −88 |
| GLib (all mappings) | −64 / −64 | −126 / −124 | −96 / −96 | −128 / −128 |
| other shared libraries | −34 / −6 | −46 / −32 | −20 / −21.5 | −30 / −25.5 |

At M0 the subject's total Pss is 138–260 KiB below the pin's in every cell
(the totals tables). Anonymous memory (−76 / −80) and the facade `r--` group
(−136 to −184) account for most of it.

## Function-level attribution of the extra text

Date: 2026-10-23 UTC, 13:35:49Z–13:37:44Z, same host, cells and libraries as
above. Questions: which functions the extra resident text pages hold on each
side, and whether they come from the C APIs' polymorphic dispatch (KC
`PolyDB`, tkrzw `PolyDBM`) and from database types that oxpinyin never uses.

**Types each side opens.** Both sides open the same classes. KC: `TreeDB`
and `HashDB` files, an in-memory `ProtoTreeDB` and a `StashDB`. The pin
names the classes (`chewing_large_table2_kyotodb.cpp:87`,
`phrase_large_table3_kyotodb.cpp:102`, `punct_table_kyotodb.cpp:62`,
`ngram_kyotodb.cpp:117`; `chewing_large_table2_kyotodb.cpp:100`,
`phrase_large_table3_kyotodb.cpp:115` and `ngram_kyotodb.cpp:58` for the
in-memory ones, at 074a2219). oxpinyin passes
`#type=kct|kch|kcpt|kcs` (`crates/oxpinyin-store/src/kyotocabinet/ffi.rs`,
`DbType`). tkrzw: `TreeDBM`, `HashDBM`, `TinyDBM` and `BabyDBM`, which are the
pin's imports and the subject's `dbm=` values. Every other class of either
library (KC `ForestDB`, `GrassDB`, `DirDB`, `CacheDB`, `TextDB`,
`ProtoHashDB`; tkrzw `SkipDBM`, `CacheDBM`, `StdHashDBM`, `StdTreeDBM`,
`ShardDBM`, `AsyncDBM`) is opened by neither side. The C APIs offer no other
entry: `kcdbnew` creates "a polymorphic database object"
(`/usr/include/kclangc.h:294-299`, libkyotocabinet-dev 1.2.80), and
`tkrzw_dbm_open` takes `PolyDBM::OpenAdvanced` parameters (`tkrzw_langc.h:484-495`,
tkrzw 1.0.34).

### Method

- **Symbols.** libkyotocabinet is stripped. Its debug file comes from
  `https://ddebs.ubuntu.com/pool/universe/k/kyotocabinet/libkyotocabinet16v5-dbgsym_1.2.80-1build1_amd64.ddeb`
  (sha256 `5f05ecd96b3d8c67aba6a51737365d9d77d90608724125918268f69c4bf160f1`),
  extracted with `dpkg-deb -x`. Its build-id `81ea75a60143f39d6ec5fabaa5a3266994ce7750`
  equals the host library's (`readelf -n`). libtkrzw and all four facades
  carry their own symbol tables. Functions are the sized `t`/`T`/`w`/`W`
  symbols of `nm -S --defined-only`, demangled with `nm -C`.
- **Page cache.** Each measured object was read once (`cat FILE >/dev/null`):
  both backend libraries and the ox, control and pin copies in both work
  directories. `mincore` over each object's `r-x` segment then reported every
  page cached, and did so again after the run.
- **Resident pages.** The teardown worker ran through `teardown.Campaign.run`
  in memory mode: CPU 2, the `phrases` profile, 10 rounds per side in rotating
  ox/control/pin order, for each facade. At the M1 pause, the controller's
  `read_memory` also read `/proc/PID/pagemap` (bit 63, present) for every page
  of the facade copy's and the backend library's `r-x` mappings. In all 120
  samples the present-page count equals that mapping's smaps Rss.
- **Executed code.** One Callgrind run per side and facade, from process start
  to the first free, which is just after M1:

  ```sh
  taskset -c 2 valgrind --tool=callgrind --cache-sim=no --branch-sim=no \
    --demangle=no --dump-instr=yes --dump-line=no --compress-strings=no \
    --compress-pos=no --collect-atstart=yes \
    --dump-before=bisect_teardown_free_unit --callgrind-out-file="$OUT" \
    "$WORK/bisect-teardown" --teardown "$FACADE" "$LIB" "$DATA" "$USER" \
    time 0 1 1 first-candidate -
  ```

  This runs with `OXPINYIN_TEARDOWN_EXPORTS` set as the controller sets it,
  and reads the first dump, `$OUT.1`. An instruction with a cost, or a call,
  counts as executed. The addresses are object-relative: for 95–100% of
  functions, the lowest executed address minus the symbol address is 0. In
  every object, 98.4–100% of executed instructions fall inside a sized
  symbol. ox and control executed the same pages and functions.
- **Attribution.** A resident page is "executed" when it holds at least one
  executed instruction. Its 4 KiB are split over the symbols it overlaps, by
  bytes, and classed by the demangled class name. The figures are medians
  over the 10 samples, in KiB.

The collector and analysis were scratch scripts and are not committed. The
collector calls `teardown.Campaign` with a `read_memory` wrapper and runs the
command above; the analysis reads `nm` and the Callgrind dump. The record
streams were not kept.

With every page cached, the libkyotocabinet figures equal the `confirm`
run's: 652 KiB ox against 72 KiB pin (104 for pinyin here, range 72–136).
libtkrzw comes out at 1,000 KiB ox against 856 / 896 KiB pin, against 1,100 /
948 in `confirm` and 952 / 832 in an earlier run of the same collector with a
partially cached library (13:32:52Z–13:34:04Z, 287 of 307 text pages cached).
Text residency therefore also depends on page-cache history. The
executed-code figures do not: they are the same in all runs.

| object | resident ox / pin | pages with executed code, ox / pin | executed instructions, ox / pin |
| --- | ---: | ---: | ---: |
| libkyotocabinet (KC pinyin, zhuyin) | 652 / 104, 652 / 72 | 236 / 28 | 5,986 / 774 |
| libtkrzw (tkrzw pinyin, zhuyin) | 1,000 / 856, 1,000 / 896 | 336 / 260 | 12,542 / 8,598, 12,542 / 8,589 |
| tkrzw libzhuyin facade text | 520 / 448 | 400 / 256 | 26,045 / 24,747 |

The instruction totals include the executed instructions that fall outside
any sized symbol: ox's 12 in libkyotocabinet and 28 in libtkrzw, and 12 on
each side of the facade. The breakdowns below count only instructions inside
a symbol, so ox's sum to 5,974, 12,514 and 26,033, and the pin's facade to
24,735.

### libkyotocabinet: +580 KiB

The heading is the `confirm` run's text difference, 652 against 72 KiB; the
other libkyotocabinet mappings are equal there. In this cached run the text
difference is +548 KiB for pinyin, where the pin's text is 104 KiB, and +580
for zhuyin.

KC pinyin, KiB. ox's resident libkyotocabinet text is split into pages that
hold executed code and pages that do not. The pin's KC code is mostly in its
own facade text, so the pin's rows count both. The last three columns are
distinct executed instructions.

| code in the resident pages | ox, executed pages | ox, other pages | ox total | pin, libkyotocabinet | pin, own text | ox instructions | pin instructions, library | pin instructions, own text |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| dispatch: `PolyDB`, `kc*` C API | 44 | 35 | 79 | 2 | 0 | 1,188 | 0 | 0 |
| classes neither side opens | 44 | 238 | 282 | 21 | 0 | 47 | 0 | 0 |
| classes both open (`PlantDB<HashDB>`, `HashDB`, `ProtoTreeDB`, `StashDB`, `BasicDB`) | 91 | 108 | 199 | 30 | 214 | 3,166 | 0 | 2,927 |
| shared infrastructure (`File`, locks, `std::`, other `kyotocabinet::`, no symbol) | 57 | 35 | 92 | 51 | — | 1,573 | 774 | — |

KC zhuyin gives the same subject figures. The pin has 2 / 12 / 12 / 46 KiB in
the library rows, the same 214 KiB in its own text, and 2,869 instructions
there.
Per class, ox's 282 KiB of unopened classes are `ForestDB` 74,
`GrassDB` 71, `DirDB` 48, `CacheDB` 29, `TextDB` 27, `ProtoHashDB` 24 and
`IndexDB`/`MapReduce` 8. Of those, 238 KiB sit in pages with no executed
instruction.

- **Dispatch, +77 KiB.** ox executes five `PolyDB` functions. The largest,
  `kyotocabinet::PolyDB::open(std::string const&, unsigned int)`, is 18,491 B
  and parses the open string to construct any class. The others are `close`,
  `Cursor::Cursor`, `accept` and `error`. ox also executes 11 `kc*` wrappers
  (`kcdbopen`, `kcdbloadsnap`, `kcdbaccept`, `kcdbcheck`, `kcdbdel`,
  `kccurjump`, `kcdbnew`, `kccurdel`, `kccurecode`, `kcdbcursor`, `kcdbclose`).
  The pin executes none of them.
- **Classes neither side opens, +261 KiB** (zhuyin: 282 − 12 = +270). ox
  executes one function of these
  classes, `PlantDB<CacheDB, 33>::write_key [clone .isra.0]` (181 B, 47
  instructions). It is a clone that `PlantDB<HashDB, 49>::load_inner_node` and
  `::search_tree` call, i.e. `TreeDB` code. The rest of
  their 282 KiB is resident without being executed. It lies between the
  `HashDB`, `PlantDB<HashDB>`, `PolyDB` and `kc*` code that ox does execute,
  in the library's text order.
- **Classes both open: about equal.** ox runs 3,166 instructions of them in
  the library, and the pin runs 2,927 of its own instantiations. Their
  resident code is 199 KiB for ox against 30 + 214 = 244 KiB for the pin.
- **Shared infrastructure, +41 KiB.** `File::open`, `File::read`,
  `walpath`, the lock classes and `TSDKey` run on both sides. ox adds
  `strsplit`, `hashmurmur`, `readvarnum` and the `LinkedHashMap` leaf cache
  that the pin compiles into its own text.

So the +580 KiB is the C API's `PolyDB` dispatch (+77) and the code of KC
classes that neither side opens (+261), with smaller shared parts. oxpinyin
does not execute those classes; their pages are resident because the code it
does execute in libkyotocabinet is interleaved with them. The opened classes
cost about the same on both sides. The pin has its copies in its own text,
which is why its facade text is 208 KiB above ox's. ox's own KC adapter,
the `oxpinyin_store` crate, runs 534 instructions in 4 KiB of its text.

### libtkrzw: +152 KiB

The heading is the `confirm` run's text difference, 1,100 against 948 KiB.
In this run, with its different page-cache history (above), the text
difference is +144 KiB for pinyin and +104 for zhuyin, the table below.

tkrzw zhuyin, KiB (pinyin: the same subject figures, pin within 20 KiB per
row):

| code in the resident pages | ox, executed pages | ox, other pages | ox total | pin total | ox − pin | ox instructions | pin instructions |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| dispatch: `PolyDBM`, `tkrzw_*` C API | 41 | 48 | 88 | 3 | +86 | 1,625 | 0 |
| classes neither side opens | 1 | 93 | 93 | 121 | −28 | 0 | 0 |
| classes both open (`HashDBM`, `TreeDBM`, `TinyDBM`, `BabyDBM`, `DBM` base) | 119 | 111 | 230 | 276 | −46 | 5,311 | 5,933 |
| utilities, parameter parsing, `std::`, `File` classes, no symbol | 175 | 412 | 587 | 496 | +91 | 5,578 | 2,656 |

ox executes 50 libtkrzw functions (39,497 B) that the pin does not:

- `tkrzw::PolyDBM::OpenAdvanced` (9,608 B) and nine other `PolyDBM` members;
- 11 parameter and string functions, among them `tkrzw::SetHashTuningParams`
  (6,336 B), `tkrzw::SetTreeTuningParams` (3,109 B), `GetKeyComparatorByName`,
  `StrSplit`, `StrSplitIntoMap`, `StrToIntMetric` and `StrToBool`, plus 14
  `std::map<std::string, std::string>` members;
- `tkrzw::PolyFile::MakeFileInstance` (4,108 B) and the `TreeDBM` and
  `TinyDBM` constructors that take a `File`;
- nine `tkrzw_*` wrappers, of which `tkrzw_dbm_open` is 3,506 B, and the C
  API's `RecordProcessorWrapper::ProcessFull`.

These are the C API's parameter parsing, its `PolyDBM`/`PolyFile`
construction and its callback adapter; the other two are `__tls_init` and a
`std::string` comparison. ox executes no function of a class that neither
side opens. Those classes are resident on both sides (`SkipDBM` 73 KiB on each). The classes
both open cost the same on both sides (`HashDBM` 89 KiB, `TreeDBM` 86 KiB on
each), except that the pin runs `BabyDBM` before M1 and ox does not
(−38 KiB). The pin executes 25 functions that ox does not, mostly `BabyDBM`
members. So the libtkrzw difference is the C API's parameter and
`PolyDBM` path, offset by the pin's `BabyDBM`.

### tkrzw libzhuyin facade text: +72 KiB

This is oxpinyin's own code, not a C-API path. Before M1:

| | ox | pin |
| --- | ---: | ---: |
| resident text | 520 KiB | 448 KiB |
| pages with executed code | 400 KiB | 256 KiB |
| executed instructions, load-time static initializers | 0 | 19,011 (5 functions, 46,463 B) |
| executed instructions, everything else | 26,033 (227 functions, 406 KiB) | 5,724 (86 functions, 37 KiB) |

ox has no load-time initializer. Its init-to-M1 path runs 4.5× the pin's
distinct instructions, in functions 11× the size. The largest are
`<oxpinyin_user::lookup::UserLookup>::from_store` (45,948 B),
`oxpinyin_user::persistence::load_logs_with_mask` (32,319 B) and
`GenericUserStore<TkrzwStore>::open_session` (31,083 B). The tkrzw policy's
ordered block is the first 421 KiB of `.text` (0xe060–0x7737a, 259
functions). It holds 392 KiB of the 400 KiB of pages with executed code, and
all of it is resident (420 KiB). The other 100 KiB are the 4 KiB page before
the block and one run of 96 KiB after it, 0x78000–0x90000; each holds 4 KiB
of executed code.
That run ends on a 64 KiB file-offset boundary in every sample, whatever the
address layout. The pinyin facade has the same structure (block 429 KiB,
76 KiB after it). The pin runs fewer, smaller functions, spread over its
text. The +72 KiB is the size of oxpinyin's startup code, after ordering.

## What this does not show

- Which code runs on each executed page: the attribution above is at function
  granularity, and a page with one executed instruction counts as executed.
- The mechanism that maps pages without executed code, which is likely
  fault-around and page-cache folios; no kernel source was traced.
- Whether #1096's KC pinyin bounds have the same cause: that tool was not
  run here. This run does not reproduce bounds outside the band.
- Time: the `confirm` stages' timing records are not reported.
- The Memcheck heap repetition, which these `attribute` runs stop before.
- BDB, which was not run here (#1030 has the BDB attribution).

## Comparison-stage extension

At source `747b00cd`, only KC/tkrzw profile provisioning was supported.
Confirmation and attribution were refused because final-profile comparison
handled BDB containers only. This extension resolves that source limitation
before measurements: each peer backend's user indexes and user bigram are
compared by all logical records, including empty values. KC loads snapshots;
tkrzw opens TreeDBM indexes and HashDBM bigrams read-only. Unknown files
remain byte comparisons. Existing pinyin-index padding and chunk-header
timestamp rules remain the only normalization. BDB behavior is preserved.

Peer readers run as a mode of the existing isolated contract worker, so
backend native libraries do not remain mapped in the controller between
samples. Runtime checks cover each actual copied object before native,
fixture-population and comparison launches. The tkrzw reader loads the exact
certified runtime path; errors refuse a comparison rather than producing
an empty or partial database result. No new driver or dependency is added.

The source basis was read with
`git -C /home/sheng/repos/libpinyin show 074a2219:PATH`:
`src/storage/ngram_kyotodb.cpp:54-64,82-91` loads/saves the user bigram as
a snapshot; `src/storage/ngram_tkrzwdb.cpp:48-81` uses a HashDBM temporary;
`src/storage/phrase_large_table3_tkrzwdb.cpp:83-118` uses TreeDBM;
`src/storage/chewing_large_table2_tkrzwdb.cpp:92-106` loads TreeDBM records.
These are source-derived storage requirements, not performance results.

The existing worker and readers already provide the relevant primitives:
`teardown.py::mapping_group` and `read_memory` read mapping classes and
checkpoint totals from the same smaps observation; the teardown worker
pauses at M0 and M1. `tools/measure-function-order.py::teardown_memory`
reports total Pss from a later smaps_rollup read. Its text-Pss measurement
uses the earlier smaps read. These are different sampling definitions;
report any rollup-versus-mapping-sum difference explicitly rather than
silently substituting one for the other.

## References

- [Issue #1098](https://github.com/shenghaoc/oxpinyin/issues/1098).
- [BDB attribution method](init-attribution-2026-10-22.md).
- [Backend shipping policies and historical controls](backend-function-order-1076.md).
- [Section 7.5](tooling-consolidation-python-just.md#75-metrics-and-statistics).
- [Cold-cache startup and the image build route (#1091)](cold-cache-startup-2026-10-23.md).
