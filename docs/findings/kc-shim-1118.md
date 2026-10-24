# Kyoto Cabinet through a C++ shim (#1118)

Recorded 2026-10-23 (UTC). The maintainer approved the shim as a PR on
2026-10-23, including `cc` as a build-dependency of `oxpinyin-store` under
the `kyotocabinet` feature. Scope: #1118.

## Question

`docs/findings/kc-tkrzw-pss-1098.md` (#1098) attributes about +580 KiB of
resident libkyotocabinet text above the pin to the C API. `PolyDB` dispatch
and the `kc*` wrappers account for +77 KiB. Code of KC classes that neither
side opens accounts for +261 KiB (pinyin) and +270 KiB (zhuyin). The pin
constructs `TreeDB`, `HashDB`, `ProtoTreeDB` and `StashDB` directly
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
  (`docs/packaging.md`, "Final-link mechanism and fallback"). A C++ path
  that a build lacks is skipped. Every resolve prints the count, in every
  relink and in the nightly `function-order-policy` job:
  `function ordering: N of M C++ policy paths are not in this build; skipped`.
  Rust and C paths stay exact.
- The KC policies were regenerated on the rebased code. Against `main`'s,
  each gains the shim's 14 `oxkc_*` entry points and 51 C++ rows (47 init,
  4 steady). A few Rust rows follow the adapter's changed instantiations
  (libpinyin +5 / −4, libzhuyin +4 / −3). libpinyin has 330 paths and
  libzhuyin 310. `main`'s KC policies do not resolve against the shim's
  staticlib, so the shim and its policies are one commit.

The dynamic exports are unchanged: the same names as base in both facades,
and the cargo-built `libpinyin_capi.so` and `libzhuyin_capi.so` export no
shim or Kyoto Cabinet symbol. `DT_NEEDED` gains `libstdc++.so.6` and
`libm.so.6`, the pin's own set; libkyotocabinet already maps both. The facades
import no `kc*` C-API function, against 24 in base, and 68 distinct
`kyotocabinet::` names. 67 of those are among the pin's 77; the other is
`kyotocabinet::VERSION`, which `oxkc_version` reads. No `PolyDB` symbol is
left.

The compiled header code must match the installed libkyotocabinet version
(`docs/packaging.md`, "Kyoto Cabinet: build headers must match the installed
library"). The runtime check accepts any 1.2.x library and does not detect a
mismatch.

One observable difference is unchanged. A KC build refuses a store path that
contains `#`, and the pin opens one. The `kclangc.h` binding needed the
refusal, because `PolyDB` reads `#` as the start of its parameters. The shim
does not, but accepting `#` is a separate issue (#1118).

## Correctness

Every differential ran on the dev build of its cell
(`CARGO_PROFILE_DEV_OPT_LEVEL=1`, as `tools/bisection/oracle-cell.sh` sets),
on `main` at `d1f9d42` plus this change, against the pin at 074a2219:

| cell | contracts, default suite (2,889 cases) | same-data |
| --- | --- | --- |
| bdb | 2,884 MATCH, 0 DIFFER, 1 SKIP, 4 HOST-DEPENDENT | IDENTICAL on 15 drivers |
| kc | 2,872 MATCH, 0 DIFFER, 13 SKIP, 4 HOST-DEPENDENT | IDENTICAL on 15 drivers |
| tkrzw | 2,872 MATCH, 0 DIFFER, 13 SKIP, 4 HOST-DEPENDENT | IDENTICAL on 15 drivers |

- SKIP: backend-scoped coverage exclusions at the recorded revision, including
  behaviours covered only on BDB.
- HOST-DEPENDENT: the four
  `table-conf-lambda{,-hex}-exponent-without-digits{,-zhuyin}` cases. They are
  registered `host_scanf` cases: this host's glibc scanner accepts a spelling
  that the oracle image's rejects.
- tkrzw ran against setup's sound libtkrzw. `ldd` resolved `libtkrzw.so.1` to
  `<cache>/tkrzw/lib/libtkrzw.so.1` for both pin facades and both subject
  libraries.

## Measurements

All KC measurements ran in one job on one host: cloud x86-64, Ubuntu 24.04.5,
4 CPUs, load 0.6–1.1, nothing else heavy running. The shipping libraries were
built in the compatibility image (GNU ld 2.47, cargo-c 0.10.24) with their own
policies:

- **base:** `main` at `d1f9d42`, with its KC policies;
- **shim:** this change on `d1f9d42`;
- **control:** the same bytes as the shim;
- **pin:** the kc oracle.

`tools/packaging/function_order.py` verified both shim facades as ordered
over all 330 / 310 paths. Both relinks printed `function ordering: 0 of 51
C++ policy paths are not in this build; skipped`, then "using measured
policy". The measured shim was built from a checkpoint whose KC-relevant
shipping inputs, policies and measurement tools are byte-identical to the
PR's.

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

These are the four-arm campaign's `stripped bytes` lines (Commands):
`tools/measure-function-order.py` copies each arm's library, runs
`strip --strip-unneeded` on the copy and prints the copy's file size.

### Total Pss after init, interleaved run (KiB, median of 20)

| cell | base | shim | control | pin | shim − base | shim − pin |
|---|---:|---:|---:|---:|---:|---:|
| pinyin `phrases` | 16,533.5 | 16,052 | 16,052 | 16,614 | −481.5 | −562 |
| pinyin `journals` | 16,560 | 15,960 | 15,956 | 16,486 | −600 | −526 |
| zhuyin `phrases` | 12,258 | 11,792 | 11,792 | 11,952 | −466 | −160 |
| zhuyin `journals` | 12,299 | 11,824 | 11,802 | 11,966 | −475 | −142 |

The observed reduction of 466–600 KiB is a difference of per-arm medians.
`tools/measure-function-order.py` does not compute the paired total-Pss
control difference and its 95% interval required by §7.5 of
`docs/findings/tooling-consolidation-python-just.md`, so this table does not
establish that qualification. The warm teardown M1 results below are
separately validated evidence.

### Total Pss at M1, warm teardown `confirm` (KiB, median)

| cell | base | pin (base run) | base control | shim | pin (shim run) | shim control | shim − pin | shim/pin [95%] |
|---|---:|---:|---|---:|---:|---|---:|---|
| pinyin `phrases` | 16,506 | 16,534 | outside | 16,039.5 | 16,489.5 | inside | −450 | 0.972 [0.966, 0.973] |
| pinyin `journals` | 16,535 | 16,482.5 | outside | 15,944 | 16,580 | inside | −636 | 0.966 [0.960, 0.968] |
| zhuyin `phrases` | 12,252 | 11,940 | inside | 11,780 | 11,940 | inside | −160 | 0.986 [0.984, 0.987] |
| zhuyin `journals` | 12,269 | 11,936 | inside | 11,796 | 11,936 | inside | −140 | 0.988 [0.987, 0.990] |

Every shim-run control is inside its bound. In the base run the two pinyin
controls are outside (−182 / +152 and −180 / +124 KiB against ±165), so base's
pinyin ox − pin figures are not claimed. In the base run zhuyin is +312 and
+333 KiB above the pin (1.025, 1.029). The M0 controls fall outside their
±64 KiB bound in both runs, as in #1105's runs, so no M0 total is claimed.

### M1 by mapping, warm teardown `confirm` (Pss KiB, base / shim / pin)

Rss equals Pss in every row except "other shared libraries".

| mapping | pinyin `phrases` | zhuyin `phrases` | shim − base |
|---|---:|---:|---:|
| facade text (r-x) | 508 / 548 / 716 | 520 / 560 / 676 | +40 / +40 |
| facade read-only (r--, RELRO) | 272 / 304 / 400 | 268 / 300 / 412 | +32 / +32 |
| facade writable (rw-) | 4 / 8 / 4 | 4 / 8 / 4 | +4 / +4 |
| libkyotocabinet (all) | 804 / 244 / 244 | 804 / 244 / 244 | −560 / −560 |
| libstdc++ (all) | 1,384 / 1,384 / 1,384 | 1,384 / 1,384 / 1,384 | 0 / 0 |
| other shared libraries | 608 / 624 / 632 | 608 / 628 / 632 | +16 / +20 |
| sum of group medians | 16,504 / 16,036 / 16,488 | 12,244 / 11,780 / 11,940 | −468 / −464 |

`journals` gives the same code rows. Its sums are 16,536 / 15,944 / 16,610.5
(pinyin) and 12,262 / 11,794 / 11,933 (zhuyin). Anonymous memory and the data
files move by at most 4 KiB between base and shim.

### Resident text and executed pages through init (interleaved run)

Facade text Pss, and executed facade text pages (Callgrind, object-relative),
base / shim / control / pin:

| cell | text Pss after load | after init | executed pages through init | through M1 |
|---|---|---|---|---|
| pinyin `phrases` | 252 / 228 / 228 / 192 | 508 / 548 / 548 / 716 | 75 / 87 / 87 / 83 | 100 / 112 / 112 / 101 |
| pinyin `journals` | 252 / 228 / 228 / 192 | 508 / 548 / 548 / 716 | 75 / 87 / 87 / 83 | 100 / 112 / 112 / 101 |
| zhuyin `phrases` | 200 / 240 / 240 / 192 | 520 / 560 / 560 / 676 | 76 / 86 / 86 / 76 | 100 / 109 / 109 / 94 |
| zhuyin `journals` | 200 / 240 / 240 / 192 | 520 / 560 / 560 / 676 | 76 / 86 / 86 / 76 | 100 / 109 / 109 / 94 |

Before the C++ functions were ordered, an earlier build of the shim held
776 KiB (pinyin) and 772 KiB (zhuyin) of facade text after init. That build
is not part of this job.

### Instructions (Callgrind Ir, base / shim / control / pin)

| cell | init | first candidate | steady |
|---|---|---|---|
| pinyin `phrases` | 15,216,496 / 15,171,348 / 15,171,348 / 15,649,644 | 880,390 / 857,505 / 857,478 / 700,501 | 529,560,820 / 529,186,664 / 529,186,179 / 567,328,263 |
| pinyin `journals` | 15,243,371 / 15,198,819 / 15,198,819 / 15,729,875 | 842,462 / 819,727 / 819,796 / 700,501 | 529,558,559 / 529,184,769 / 529,194,213 / 567,328,263 |
| zhuyin `phrases` | 11,732,300 / 11,704,777 / 11,704,777 / 12,162,240 | 871,373 / 847,548 / 847,574 / 536,511 | 491,110,656 / 490,961,515 / 490,964,068 / 558,941,766 |
| zhuyin `journals` | 11,752,639 / 11,725,756 / 11,725,756 / 12,229,819 | 833,968 / 810,488 / 810,289 / 537,139 | 491,114,914 / 490,974,144 / 490,965,345 / 558,941,766 |

The init control equals the shim exactly in every cell, so the init figure
stands: the shim executes 0.23–0.30% fewer instructions than base. The
first-candidate and steady controls differ from the shim by 26–199 and
485–9,444 instructions, so no gain is claimed for either.

### Heap

Live heap after init, base / shim / control / pin: 6,118,336 / 6,117,888 /
6,117,888 / 6,062,064 B (pinyin `phrases`) and 2,678,528 / 2,678,320 /
2,678,320 / 2,595,248 B (zhuyin `phrases`); 6,111,248 / 6,110,896 /
6,110,896 / 6,062,064 B and 2,671,552 / 2,671,440 / 2,671,440 / 2,595,648 B
(`journals`). DHAT peak, base / shim / control: 6,084,114 / 6,083,714 / 6,083,714 B
(pinyin) and 3,008,889 / 3,008,841 / 3,008,841 B (zhuyin).

### Warm time (interleaved run, ms, median of 20)

| cell | init base / shim / control / pin | first candidate base / shim / control / pin |
|---|---|---|
| pinyin `phrases` | 5.302 / 5.442 / 5.570 / 5.529 | 0.232 / 0.220 / 0.236 / 0.215 |
| pinyin `journals` | 5.459 / 5.400 / 5.864 / 5.246 | 0.240 / 0.209 / 0.223 / 0.208 |
| zhuyin `phrases` | 3.604 / 3.392 / 3.497 / 3.590 | 0.224 / 0.198 / 0.188 / 0.152 |
| zhuyin `journals` | 3.584 / 3.498 / 3.304 / 3.325 | 0.215 / 0.214 / 0.192 / 0.166 |

The shim and its same-bytes control differ by up to 0.46 ms in init, more
than any shim − base difference, so no warm-time change is resolved.

### Cold, main's cold mode

Before every timing and memory sample, `--teardown-cache cold` evicted the
facade, its whole `DT_NEEDED` closure outside the C runtime, every system
data file and the sample's private profile, and then verified them at zero
resident pages. For every arm the closure was `libglib-2.0.so.0.8000.0`,
`libkyotocabinet.so.16.14.0`, `liblzma.so.5.4.5`, `liblzo2.so.2.0.0`,
`libpcre2-8.so.0.11.2`, `libstdc++.so.6.0.33` and `libz.so.1.3`. In each run,
all 1,140 `confirm` eviction records and all 120 `pilot` records show
libkyotocabinet and every other file at zero resident pages after eviction
and at verification. That is 190 `confirm` samples and 20 `pilot` samples
per arm (base or shim, control, pin) and facade.

The idle `dockerd` left by the image builds maps liblzma, which DONTNEED
cannot evict, and an earlier attempt was refused for that. The job stops
`dockerd` first. A scan of every process's maps then found no cold-set
library mapped.

Major page faults (`ru_majflt` per phase), the same for both profiles. In
every cell the minimum equals the maximum, and the control equals ox:

| facade | phase | base | shim | pin |
|---|---|---:|---:|---:|
| pinyin | load | 26 | 25 | 25 |
| pinyin | init | 35 | 33 | 30 |
| pinyin | first candidate | 3 | 4 | 3 |
| pinyin | full lifecycle | 74 | 75 | 59 |
| zhuyin | load | 25 | 25 | 25 |
| zhuyin | init | 25 | 19 | 21 |
| zhuyin | first candidate | 3 | 2 | 2 |
| zhuyin | full lifecycle | 65 | 61 | 50 |

Cold wall time, ms, medians (base run: ox / control / pin; shim run: ox /
control / pin):

| cell | init, base run | init, shim run | first candidate, base run | first candidate, shim run |
|---|---|---|---|---|
| pinyin `phrases` | 12.228 / 12.435 / 11.856 | 12.010 / 12.022 / 11.798 | 0.632 / 0.622 / 0.666 | 0.708 / 0.711 / 0.636 |
| pinyin `journals` | 12.290 / 12.361 / 11.574 | 12.908 / 13.254 / 12.533 | 0.614 / 0.611 / 0.620 | 0.689 / 0.713 / 0.658 |
| zhuyin `phrases` | 8.493 / 8.932 / 7.726 | 8.958 / 8.608 / 8.683 | 0.473 / 0.507 / 0.394 | 0.510 / 0.483 / 0.450 |
| zhuyin `journals` | 8.709 / 8.959 / 8.270 | 9.012 / 9.177 / 8.982 | 0.552 / 0.570 / 0.399 | 0.560 / 0.573 / 0.453 |

The control/ox interval stayed in [0.95, 1.05] only for base's pinyin
`journals` init (ox/pin 1.069 [1.025, 1.099]), zhuyin `phrases` init (1.116
[1.047, 1.140]) and zhuyin `journals` init (1.055 [1.032, 1.097]), and for the
shim's zhuyin `journals` first candidate (1.256 [1.218, 1.415]). Every other
ratio is withheld. No cold-time difference between base and shim is claimed.
`tools/measure-function-order.py`'s own cold mode evicts only the facade copy,
so its cold figures are not used here.

### Store operations (`tools/bisection/store-ops.py --profile-only`)

`backend_matrix_kyotocabinet` was built with the bench profile on the host,
at `d1f9d42` (base, the `kclangc.h` binding) and with this change. The
same-bytes control (the base binary under another path) equals base exactly
in every count shown, so every row is VALID.

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
| `lifecycle.user_load` | 13,823,766 / 13,823,766 / 13,666,654 | 0.989 | 5,524 / 5,524 / 5,518 | 1,294,607 / 1,294,607 / 1,294,290 |
| `lifecycle.user_save`¹ | 14,432,432 / 14,432,432 / 14,263,409 | 0.988 | 3,462 / 3,462 / 3,459 | 230,816 / 230,816 / 230,659 |
| `session.typing_training` | 203,937 / 203,937 / 203,133 | 0.996 | 159 / 159 / 159 | 41,306 / 41,306 / 41,281 |

¹ In the campaign's `lifecycle.user_save` DHAT run, the control allocated
30 B more than base over 10 calls (2,308,420 against 2,308,390), so the tool
marked it INVALID and printed no figures. One rerun of that operation was
VALID in every count, and its DHAT columns are shown; the Ir columns are the
campaign's, also VALID.

Per call the shim executes 0.980–1.004× base's instructions. The
whole-process DHAT peak is lower by 88 to 758 B in every row. Timing was not
run in this campaign.

## Reading

- The shim removes the libkyotocabinet excess: 804 → 244 KiB, the pin's 244,
  in all four cells.
- With its C++ functions in the ordering, the KC code compiled into the
  facade costs +40 KiB of text, +32 of read-only and +4 of writable. Facade
  text after init is 548 / 560 KiB, against the pin's 716 / 676.
- Total Pss after init has an observed difference of medians of −466 to
  −600 KiB against base and −142 to −562 KiB against the pin in all four
  cells, without paired-control qualification. Separately, warm teardown
  M1 ends 140–636 KiB below the pin, with every shim-run control inside its
  bound.
- Instructions: init −0.23 to −0.30% against an exact control. Per store
  operation, 0.980–1.004× base, all with exact controls. No first-candidate
  or steady gain is claimed.
- Cold, with libkyotocabinet evicted for every arm: per phase, major faults
  change by −6 to +1, and by +1 (pinyin) and −4 (zhuyin) over the full
  lifecycle. No cold-time difference is resolved.
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
cc -std=gnu11 -Wall -Wextra -Werror -O2 -o $BISECT tools/bisection/bisect.c -ldl
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
  --work $WORK --profile-only --out $RECORDS   # rerun: --ops lifecycle.user_save

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
