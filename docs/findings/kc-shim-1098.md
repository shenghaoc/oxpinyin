# Kyoto Cabinet through a C++ shim (#1098)

Recorded 2026-10-23 (UTC). The maintainer approved the shim as a PR on
2026-10-23, including `cc` as a build-dependency of `oxpinyin-store` under
the `kyotocabinet` feature.

## Question

`docs/findings/kc-tkrzw-pss-1098.md` attributes about +580 KiB of resident
libkyotocabinet text above the pin to the C API. `PolyDB` dispatch and the
`kc*` wrappers account for +77 KiB. Code of KC classes that neither side opens
accounts for +261 KiB (pinyin) and +270 KiB (zhuyin). The pin constructs
`TreeDB`, `HashDB`, `ProtoTreeDB` and `StashDB` directly
(`chewing_large_table2_kyotodb.cpp:87`, `phrase_large_table3_kyotodb.cpp:102`,
`punct_table_kyotodb.cpp:62`, `ngram_kyotodb.cpp:58,117` at 074a2219). Does a
shim that does the same remove that text, and at what cost?

## Change

- `crates/oxpinyin-store/src/kyotocabinet/shim.{h,cc}` add 27 `extern "C"`
  `noexcept` entry points over `kc::BasicDB`. `oxkc_db_new` constructs one of
  the four classes. Every entry point catches every C++ exception and returns
  its failure value; that always holds. Where a database exists, the shim
  also records a `SYSTEM` error on it, so the error code names the cause.
  That recording is best effort: if it throws in turn, the exception is
  dropped.
- `build.rs` compiles the shim with `cc` at `-O2`, the pin's autoconf default,
  with hidden visibility and `cc`'s default warnings. `cc` is an optional
  build-dependency enabled by `kyotocabinet`; it was already in `Cargo.lock`.
  bindgen now reads only `shim.h`.
- `ffi.rs` calls `oxkc_*` instead of `kc*`. Open-path tuning accepts only
  `#bnum=`, the one parameter the bench-internal entry point sets.
- `relink-versioned.sh` adds `-lstdc++` for `kyotocabinet`.
- The function-order tools admit C++ paths under their raw Itanium names
  (`docs/packaging.md`, "Final-link mechanism and fallback"). A C++ path that a
  build lacks is skipped. Every resolve prints the count:
  `function ordering: N of M C++ policy paths are not in this build; skipped`.
  Rust and C paths stay exact.
- The KC policies were regenerated. Against `main`'s, each gains the shim's
  14 `oxkc_*` entry points and 51 C++ rows (47 init, 4 steady), and four Rust
  rows follow the adapter's changed instantiations. libpinyin has 335 paths
  and libzhuyin 320. `main`'s KC policies do not resolve against the shim's
  staticlib, so the shim and the policies must land together.

The dynamic exports are unchanged: the same names as base in both facades,
and the cargo-built `libpinyin_capi.so` and `libzhuyin_capi.so` export no
shim or Kyoto Cabinet symbol. `DT_NEEDED` gains `libstdc++.so.6` and
`libm.so.6`, the pin's own set; libkyotocabinet already maps both. The facades
import no `kc*` C-API function, against 24 in base, and 68 distinct
`kyotocabinet::` names. 67 of those are among the pin's 77; the other is
`kyotocabinet::VERSION`, which `oxkc_version` reads. No `PolyDB` symbol is
left.

One observable difference is unchanged, and is not registered anywhere. A KC
build refuses a store path that contains `#`, and the pin opens one. The
`kclangc.h` binding needed the refusal, because `PolyDB` reads `#` as the
start of its parameters. The shim does not, but accepting `#` would change
observable behaviour and needs its own differential, so this change leaves
it as it was.

## Correctness

Every differential ran on the dev build of its cell (`CARGO_PROFILE_DEV_OPT_LEVEL=1`,
as `tools/bisection/oracle-cell.sh` sets), against the pin at 074a2219:

| cell | contracts, default suite (2,889 cases) | same-data |
| --- | --- | --- |
| bdb | 2,884 MATCH, 0 DIFFER, 1 SKIP, 4 HOST-DEPENDENT | IDENTICAL on 15 drivers |
| kc | 2,872 MATCH, 0 DIFFER, 13 SKIP, 4 HOST-DEPENDENT | IDENTICAL on 15 drivers |
| tkrzw | 2,872 MATCH, 0 DIFFER, 13 SKIP, 4 HOST-DEPENDENT | IDENTICAL on 15 drivers |

- SKIP: fixtures in another backend's format.
- HOST-DEPENDENT: the four
  `table-conf-lambda{,-hex}-exponent-without-digits{,-zhuyin}` cases. They are
  registered `host_scanf` cases: this host's glibc scanner accepts a spelling
  that the oracle image's rejects.
- tkrzw ran against setup's sound libtkrzw. `ldd` resolved `libtkrzw.so.1` to
  `<cache>/tkrzw/lib/libtkrzw.so.1` for both pin facades and both subject
  libraries.

The bdb and tkrzw builds compile no shim code. The kc build predates only
comment changes, which move the `__LINE__` constants that `_KCCODELINE_`
passes in three shim functions; the code is otherwise byte-identical.

## Measurements

All KC measurements ran in one job on one host: cloud x86-64, Ubuntu 24.04.5,
4 CPUs, load 0.4–1.1, nothing else heavy running. The shipping libraries were
built in the compatibility image (GNU ld 2.47, cargo-c 0.10.24) with their own
policies:

- **base:** `main`'s crates and KC policy (`da6e4e9b`, identical to `main`
  at `cb645df` in every built file);
- **shim:** this change at `5059139`;
- **control:** the same bytes as the shim;
- **pin:** the kc oracle.

`tools/packaging/function_order.py` verified both shim facades as ordered
over all 335 / 320 paths, with no C++ path skipped. The relink printed
"using measured policy" for both. The head differs from `5059139` only in
comments, docs, the skip-count line and a build-script error message; the
shim's object code differs only in those `__LINE__` constants.

The job ran in three parts:

1. `tools/measure-function-order.py` ran all four arms interleaved, 20 rounds
   per cell.
2. The teardown `confirm` stage ran warm, once with ox = base and once with
   ox = shim. Each run had its own same-bytes control.
3. Main's cold mode ran the `verify`, `pilot` and `confirm` stages with
   `--teardown-cache cold`, again for base and for the shim.

Every teardown run was COMPARABLE, with identical final-profile digests on
all three sides.

### Stripped size (bytes)

| facade | base | shim | shim − base | pin |
|---|---:|---:|---:|---:|
| libpinyin | 2,482,912 | 2,792,640 | +309,728 (+12.5%) | 1,209,592 |
| libzhuyin | 2,233,056 | 2,546,880 | +313,824 (+14.1%) | 1,189,080 |

### Total Pss after init, interleaved run (KiB, median of 20)

| cell | base | shim | control | pin | shim − base | shim − pin |
|---|---:|---:|---:|---:|---:|---:|
| pinyin `phrases` | 16,659 | 16,177 | 16,178.5 | 16,625 | −482 | −448 |
| pinyin `journals` | 16,697 | 16,089 | 16,089 | 16,617 | −608 | −528 |
| zhuyin `phrases` | 12,401 | 11,925 | 11,921 | 12,093 | −476 | −168 |
| zhuyin `journals` | 12,416.5 | 11,957 | 11,959 | 12,094 | −459.5 | −137 |

### Total Pss at M1, warm teardown `confirm` (KiB, median)

| cell | base | pin (base run) | base control bound | shim | pin (shim run) | shim control bound | shim − pin | shim/pin [95%] |
|---|---:|---:|---|---:|---:|---|---:|---|
| pinyin `phrases` | 16,635 | 16,734 | inside | 16,165 | 16,659.5 | inside | −494.5 | 0.970 [0.965, 0.973] |
| pinyin `journals` | 16,686.5 | 16,669 | inside | 16,081 | 16,727.5 | **outside** | — | — |
| zhuyin `phrases` | 12,377 | 12,115 | inside | 11,893 | 12,081 | inside | −188 | 0.984 [0.983, 0.986] |
| zhuyin `journals` | 12,420 | 12,087 | inside | 11,938.5 | 12,080 | inside | −141.5 | 0.987 [0.986, 0.989] |

The shim run's pinyin `journals` control reached +164 against ±161 KiB, so
that cell gives no figure. The M0 controls fall outside their ±64 KiB bound
in both runs, as they did in #1105's runs, so no M0 total is claimed.

### M1 by mapping, warm teardown `confirm` (Pss KiB, base / shim / pin)

Rss equals Pss in every row except "other shared libraries".

| mapping | pinyin `phrases` | zhuyin `phrases` | shim − base |
|---|---:|---:|---:|
| facade text (r-x) | 508 / 548 / 716 | 520 / 560 / 676 | +40 / +40 |
| facade read-only (r--, RELRO) | 272 / 304 / 400 | 268 / 300 / 412 | +32 / +32 |
| facade writable (rw-) | 4 / 8 / 4 | 4 / 8 / 4 | +4 / +4 |
| libkyotocabinet (all) | 804 / 244 / 244 | 804 / 244 / 244 | −560 / −560 |
| libstdc++ (all) | 1,384 / 1,384 / 1,384 | 1,384 / 1,384 / 1,384 | 0 / 0 |
| other shared libraries | 555 / 569 / 564 | 557 / 543 / 572.5 | +14 / −14 |
| sum of group medians | 16,639 / 16,169 / 16,672 | 12,385 / 11,883 / 12,076.5 | −470 / −502 |

`journals` gives the same code rows. Its sums are 16,687 / 16,199 / 16,745
(pinyin) and 12,423 / 11,943 / 12,076 (zhuyin). Anonymous memory and the data
files move by at most 4 KiB between base and shim.

### Resident text and executed pages through init (interleaved run)

Facade text Pss and executed facade text pages (Callgrind, object-relative):

| cell | text Pss after load, base / shim / control / pin | after init | executed pages through init | through M1 |
|---|---|---|---|---|
| pinyin `phrases` | 252 / 228 / 228 / 192 | 508 / 548 / 548 / 716 | 76 / 88 / 88 / 83 | 101 / 111 / 111 / 101 |
| pinyin `journals` | 252 / 228 / 228 / 192 | 508 / 548 / 548 / 716 | 76 / 89 / 89 / 83 | 101 / 112 / 112 / 101 |
| zhuyin `phrases` | 200 / 240 / 240 / 192 | 520 / 560 / 560 / 676 | 77 / 86 / 86 / 76 | 100 / 110 / 110 / 94 |
| zhuyin `journals` | 200 / 240 / 240 / 192 | 520 / 560 / 560 / 676 | 77 / 86 / 86 / 76 | 100 / 110 / 110 / 94 |

Before the C++ rows were added, the shim's facade text after init was
776 KiB (pinyin) and 772 KiB (zhuyin), and it executed 104 / 137 and
101 / 134 pages. That earlier prototype build is not part of this job.

### Instructions (Callgrind Ir, base / shim / control / pin)

| cell | init | first candidate | steady |
|---|---|---|---|
| pinyin `phrases` | 15,230,580 / 15,186,233 / 15,186,233 / 15,649,152 | 881,671 / 857,350 / 857,324 / 700,499 | 529,578,000 / 529,088,239 / 529,090,013 / 567,328,302 |
| pinyin `journals` | 15,258,436 / 15,213,824 / 15,213,824 / 15,713,824 | 842,843 / 819,186 / 819,579 / 700,350 | 529,579,761 / 529,081,683 / 529,082,293 / 567,328,302 |
| zhuyin `phrases` | 11,747,369 / 11,720,565 / 11,720,565 / 12,169,560 | 871,006 / 846,714 / 846,727 / 536,511 | 491,223,993 / 490,974,184 / 490,974,659 / 558,941,813 |
| zhuyin `journals` | 11,767,769 / 11,741,054 / 11,741,054 / 12,235,741 | 833,283 / 809,830 / 810,036 / 537,129 | 491,230,651 / 490,971,492 / 490,966,134 / 558,941,813 |

The init control equals the shim exactly in every cell, so the init figure
stands: the shim executes 0.23–0.29% fewer instructions than base. The
first-candidate and steady controls differ from the shim by 13–393 and
475–5,358 instructions, so no gain is claimed for either.

### Heap

Live heap after init, base / shim / control / pin: 6,117,456 / 6,116,352 /
6,116,352 / 6,062,096 B (pinyin `phrases`) and 2,677,072 / 2,676,736 /
2,676,736 / 2,595,264 B (zhuyin `phrases`). `journals` is within 1,100 B of
those. DHAT peak, base / shim / control: 6,084,129 / 6,083,729 / 6,083,729 B
(pinyin) and 3,008,901 / 3,008,853 / 3,008,853 B (zhuyin).

### Warm time (interleaved run, ms, median of 20)

| cell | init base / shim / control / pin | first candidate base / shim / control / pin |
|---|---|---|
| pinyin `phrases` | 6.339 / 5.994 / 5.842 / 5.606 | 0.246 / 0.249 / 0.213 / 0.193 |
| pinyin `journals` | 5.685 / 5.197 / 5.868 / 5.687 | 0.214 / 0.195 / 0.227 / 0.193 |
| zhuyin `phrases` | 3.528 / 3.381 / 3.388 / 3.327 | 0.207 / 0.178 / 0.193 / 0.130 |
| zhuyin `journals` | 3.468 / 3.576 / 3.253 / 3.418 | 0.194 / 0.199 / 0.184 / 0.139 |

The shim and its same-bytes control differ by up to 0.67 ms in init, more
than any shim − base difference, so no warm-time change is resolved.

### Cold, main's cold mode

Before every timing and memory sample, `--teardown-cache cold` evicted the
facade, its whole `DT_NEEDED` closure outside the C runtime, every system
data file and the sample's private profile, and then verified them at zero
resident pages. For every arm the closure was `libglib-2.0.so.0.8000.0`,
`libkyotocabinet.so.16.14.0`, `liblzma.so.5.4.5`, `liblzo2.so.2.0.0`,
`libpcre2-8.so.0.11.2`, `libstdc++.so.6.0.33` and `libz.so.1.3`. All 1,140
`confirm` eviction records and all 120 `pilot` records in each run show
libkyotocabinet and every other file at zero resident pages after eviction and
at verification. That is 190 `confirm` samples and 20 `pilot` samples per arm
(base or shim, control, pin) and facade.

A first attempt was refused, because the idle `dockerd` left by the image
builds mapped liblzma, which DONTNEED cannot evict. After `dockerd` stopped,
a scan of every process's maps found no cold-set library mapped, and the run
was repeated.

Major page faults (`ru_majflt` per phase). In every cell the minimum equals
the maximum, and the control equals ox:

| facade | phase | base | shim | pin |
|---|---|---:|---:|---:|
| pinyin | load | 26 | 25 | 25 |
| pinyin | init | 36 | 33 | 30 |
| pinyin | first candidate | 3 | 4 | 3 |
| pinyin | full lifecycle | 76 | 74 | 59 |
| zhuyin | load | 25 | 25 | 25 |
| zhuyin | init | 24 | 23 | 21 |
| zhuyin | first candidate | 3 | 3 | 2 |
| zhuyin | full lifecycle | 63 | 65 | 50 |

Cold wall time, ms, medians (base run: ox / control / pin; shim run: ox /
control / pin):

| cell | init, base run | init, shim run | first candidate, base run | first candidate, shim run |
|---|---|---|---|---|
| pinyin `phrases` | 13.067 / 13.428 / 11.919 | 12.737 / 13.080 / 12.187 | 0.654 / 0.647 / 0.648 | 0.737 / 0.727 / 0.663 |
| pinyin `journals` | 12.981 / 12.825 / 12.894 | 12.378 / 12.172 / 11.903 | 0.627 / 0.648 / 0.635 | 0.674 / 0.709 / 0.626 |
| zhuyin `phrases` | 9.361 / 9.615 / 8.390 | 9.157 / 9.029 / 8.467 | 0.577 / 0.597 / 0.414 | 0.562 / 0.572 / 0.431 |
| zhuyin `journals` | 9.172 / 8.914 / 8.322 | 8.524 / 8.714 / 8.712 | 0.601 / 0.594 / 0.394 | 0.540 / 0.583 / 0.397 |

The control/ox interval left [0.95, 1.05] in every init and first-candidate
cell except base's pinyin `journals` init (ox/pin 1.022 [0.974, 1.056]) and
zhuyin `phrases` init (1.097 [1.062, 1.147]). Every other ratio is withheld.
No cold-time difference between base and shim is claimed.

### Store operations (`tools/bisection/store-ops.py --profile-only`)

`backend_matrix_kyotocabinet` was built with the bench profile at `41adb60`
(base, the `kclangc.h` binding) and at `e62dfe9` (the shim), on the host. The
same-bytes control (the base binary under another path) equals base exactly
in every count, so every row is VALID.

| operation | Ir/call base / control / shim | shim/base | blocks/call base / control / shim | bytes/call base / control / shim |
| --- | --- | ---: | --- | --- |
| `contains_raw.tree` | 2,001 / 2,001 / 1,976 | 0.987 | 0.04 / 0.04 / 0.04 | 2.35 / 2.35 / 2.35 |
| `get_raw.tree.empty` | 3,981 / 3,981 / 3,965 | 0.996 | 0.04 / 0.04 / 0.04 | 2.35 / 2.35 / 2.35 |
| `get_raw.tree.small` | 4,230 / 4,230 / 4,215 | 0.996 | 0.04 / 0.04 / 0.04 | 2.35 / 2.35 / 2.35 |
| `get_raw.tree.medium` | 3,764 / 3,764 / 3,747 | 0.995 | 0.06 / 0.06 / 0.06 | 3.43 / 3.43 / 3.43 |
| `get_raw.tree.large` | 9,854 / 9,854 / 9,835 | 0.998 | 0.12 / 0.12 / 0.12 | 10.57 / 10.57 / 10.57 |
| `get_raw.tree.miss` | 3,219 / 3,219 / 3,194 | 0.992 | 3.82 / 3.82 / 3.82 | 218 / 218 / 218 |
| `get_raw.tree.retain` | 2,567 / 2,567 / 2,548 | 0.993 | 1.07 / 1.07 / 1.07 | 65.94 / 65.94 / 65.94 |
| `get_raw.hash` | 2,673 / 2,673 / 2,647 | 0.990 | 0.63 / 0.63 / 0.63 | 260 / 260 / 260 |
| `get_raw.hash.small` | 1,300 / 1,300 / 1,275 | 0.981 | 0.3 / 0.3 / 0.3 | 15.29 / 15.29 / 15.29 |
| `get_raw.hash.medium` | 3,151 / 3,151 / 3,125 | 0.992 | 1 / 1 / 1 | 416 / 416 / 416 |
| `get_raw.hash.large` | 45,680 / 45,680 / 45,657 | 0.999 | 1 / 1 / 1 | 8,589 / 8,589 / 8,589 |
| `get_raw.hash.miss` | 631 / 631 / 625 | 0.991 | 0 / 0 / 0 | 0 / 0 / 0 |
| `scan_raw.suggestion` | 1,718,254 / 1,718,254 / 1,717,747 | 1.000 | 116 / 116 / 116 | 6,376 / 6,376 / 6,360 |
| `scan_raw.full.tree` | 445,511,862 / 445,511,862 / 442,458,513 | 0.993 | 219,094 / 219,094 / 219,094 | 12,655,586 / 12,655,586 / 12,655,570 |
| `scan_raw.full.hash` | 185,542,203 / 185,542,203 / 184,620,241 | 0.995 | 90,930 / 90,930 / 90,930 | 31,393,889 / 31,393,889 / 31,393,889 |
| `framed.get` | 1,674 / 1,674 / 1,643 | 0.981 | 2 / 2 / 2 | 41 / 41 / 41 |
| `framed.scan` | 11,322 / 11,322 / 11,250 | 0.994 | 7 / 7 / 7 | 153 / 153 / 137 |
| `framed.txn_scan` | 1,080,525 / 1,080,525 / 1,078,330 | 0.998 | 17 / 17 / 17 | 163,883 / 163,883 / 163,875 |
| `framed.is_empty` | 2,752 / 2,752 / 2,698 | 0.980 | 5 / 5 / 5 | 104 / 104 / 88.5 |
| `user_bigram.get` | 9,251 / 9,251 / 9,251 | 1.000 | 0 / 0 / 0 | 19.93 / 19.93 / 19.93 |
| `user_bigram.get.small` | 710 / 710 / 712 | 1.002 | 0 / 0 / 0 | 0.01 / 0.01 / 0.01 |
| `user_bigram.get.large` | 108,538 / 108,538 / 108,540 | 1.000 | 0 / 0 / 0 | 83.91 / 83.91 / 83.91 |
| `user_bigram.miss` | 456 / 456 / 457 | 1.002 | 0 / 0 / 0 | 0 / 0 / 0 |
| `user_bigram.store` | 2,244 / 2,244 / 2,231 | 0.994 | 0 / 0 / 0 | 0 / 0 / 0 |
| `user_bigram.remove_store` | 1,542 / 1,542 / 1,549 | 1.004 | 0.5 / 0.5 / 0.5 | 898 / 898 / 898 |
| `user_bigram.keys` | 12,046,298 / 12,046,298 / 12,079,058 | 1.003 | 0.9 / 0.9 / 0.9 | 20,477 / 20,477 / 20,477 |
| `lifecycle.open_system` | 1,029,641 / 1,029,641 / 1,024,467 | 0.995 | 141 / 141 / 133 | 965,348 / 965,348 / 964,713 |
| `lifecycle.user_load` | 13,823,766 / 13,823,766 / 13,666,654 | 0.989 | 5,524 / 5,524 / 5,518 | 1,294,607 / 1,294,607 / 1,294,294 |
| `lifecycle.user_save` | 14,431,976 / 14,431,976 / 14,262,427 | 0.988 | 3,462 / 3,462 / 3,459 | 230,844 / 230,844 / 230,687 |
| `session.typing_training` | 203,937 / 203,937 / 203,133 | 0.996 | 159 / 159 / 159 | 41,306 / 41,306 / 41,281 |

The whole-process DHAT peak is lower by 88 to 758 B in every row. Timing
was not run in this campaign.

## Reading

- The shim removes the libkyotocabinet excess: 804 → 244 KiB, the pin's 244,
  in all four cells.
- With its C++ functions in the ordering, the KC code compiled into the
  facade costs +40 KiB of text, +32 of read-only and +4 of writable. Facade
  text after init is 548 / 560 KiB, against the pin's 716 / 676.
- Total Pss after init falls by 459.5–608 KiB and ends 137–528 KiB below the
  pin in all four cells. At M1 it ends 141.5–494.5 KiB below the pin in the
  three cells whose controls hold.
- Instructions: init −0.23 to −0.29% against an exact control. Per store
  operation, 0.980–1.004× base, all with exact controls. No first-candidate
  or steady gain is claimed.
- Cold, with libkyotocabinet evicted for every arm: major faults change by
  −3 to +1 in load, init and first candidate, and by −2 (pinyin) and +2
  (zhuyin) over the full lifecycle. No cold-time difference is resolved.
- The stripped facades grow by 310–314 KB (+12.5%, +14.1%).

## Commands

From the repository root, with all three cells provisioned
(`tools/cloud/setup.sh --cells "bdb kc tkrzw"`), its environment file
sourced, and
`PY='uv run --no-project --managed-python --python cpython@3.15+gil python -B -X lazy_imports=normal'`.
The scratch scripts that chained these steps are not committed; each step is
below.

```sh
# shipping libraries, compatibility image; policies as docs/packaging.md,
# "Regeneration" (ordering off for the unordered build, then generate twice)
tools/packaging/install.sh libpinyin --prefix=$STAGE -- --locked --release \
  --no-default-features --features kyotocabinet,shipped
tools/packaging/install.sh libzhuyin --prefix=$STAGE -- --locked --release \
  --no-default-features --features kyotocabinet
$PY -m tools.dev slot run -- $PY tools/generate-function-order.py \
  --library pinyin=$UNORDERED/lib/libpinyin.so.15.0.0 \
  --library zhuyin=$UNORDERED/lib/libzhuyin.so.15.0.0 \
  --staticlib pinyin=$BUILD/x86_64-unknown-linux-gnu/release/libpinyin.a \
  --staticlib zhuyin=$BUILD/x86_64-unknown-linux-gnu/release/libzhuyin.a \
  --bisect $BISECT --system-dir "$OXPINYIN_ORACLE_DATA_KC" \
  --system-identity eb894fd5ef68e95fe021df2e4cdb1fe0b6b81093cc1da490ba010946a3528da6 \
  --profiles-dir $PROFILES --work-dir $WORK --cpu 2 --output-dir $OUT

# four-arm campaign
$PY -m tools.dev slot run -- $PY tools/measure-function-order.py \
  --subject base=$BASE --subject pr=$SHIM --subject control=$CONTROL \
  --subject pin="$OXPINYIN_ORACLE_PREFIX_KC" \
  --function-order base/pinyin=$BASE_POLICY/libpinyin.order \
  --function-order base/zhuyin=$BASE_POLICY/libzhuyin.order \
  --bisect $BISECT --system-dir "$OXPINYIN_ORACLE_DATA_KC" \
  --system-identity eb894fd5ef68e95fe021df2e4cdb1fe0b6b81093cc1da490ba010946a3528da6 \
  --profiles-dir $PROFILES --work-dir $WORK --cpu 2 --rounds 20

# teardown: STAGE:CACHE = confirm:warm, then verify:cold pilot:cold
# confirm:cold; once with ox = base (plus --function-order ox/... and
# control/... for its policy) and once with ox = the shim
$PY -m tools.dev slot run -- $PY tools/bisection/contract-diff.py kc \
  "$OXPINYIN_ORACLE_PREFIX_KC" $LIB/lib/libpinyin.so.15.0.0 \
  --zhuyin-so $LIB/lib/libzhuyin.so.15.0.0 --teardown --teardown-work-dir $TD \
  --teardown-cpu 2 --teardown-cases phrases,journals \
  --teardown-stage $STAGE --teardown-cache $CACHE

# store operations
cargo bench --locked --no-run -p oxpinyin-store --no-default-features \
  --features kyotocabinet --bench backend_matrix_kyotocabinet
$PY -m tools.dev slot run -- $PY tools/bisection/store-ops.py \
  --base $BASE_BENCH --candidate $SHIM_BENCH --data "$OXPINYIN_ORACLE_DATA_KC" \
  --work $WORK --profile-only --out $RECORDS

# correctness, per cell (dev build)
$PY -m tools.dev slot run -- bash tools/bisection/run-contract-diff.sh $CELL \
  "$PIN" $DEV/libpinyin_capi.so $DEV/libzhuyin_capi.so
$PY -m tools.dev slot run -- bash tools/bisection/run-same-data-dir-diff.sh \
  "$PIN/lib/libpinyin.so" $DEV/libpinyin_capi.so "$PIN/lib/libpinyin/data"
```

The per-mapping tables group the teardown `memory-deltas` records by the
classes of `docs/findings/kc-tkrzw-pss-1098.md` and sum the group medians.
No raw records are retained: the capture host was an ephemeral cloud
container.
