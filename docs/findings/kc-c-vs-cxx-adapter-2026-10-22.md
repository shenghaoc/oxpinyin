# Kyoto Cabinet adapters: C binding against a cxx bridge (2026-10-22)

Three builds of the Kyoto Cabinet backend, compared pairwise:

| build | binding | access pattern |
| --- | --- | --- |
| A | bindgen over `kclangc.h` (main before this stack) | `kcdbget` copies; `scan_raw` copies and sorts the whole file |
| B | bindgen over `kclangc.h` | visitor reads; ordered containers walked with one cursor |
| C | cxx bridge to a C++ shim | B's access pattern, call for call |

A→B measures the access-pattern change. B→C measures the bridge alone:
C uses B's collectors, read-only flags, open modes, paths, tuning and
error mapping, and differs only in how the calls reach Kyoto Cabinet.
C's implementation is recorded in #1043, which is not proposed for merge.

Kyoto Cabinet 1.2.80 (Debian `libkyotocabinet-dev` 1.2.80-2build1),
libpinyin pin 074a2219.

## What each change does

**B1, point reads through a visitor.** `kcdbget` allocates a `new[]`
copy of the value inside the record lock, which the binding then copied
into a `Vec` and released with `kcfree`. B reads with `kcdbaccept`
(read-only) and a visitor that copies the value once, into a `Vec`
reserved exactly (`reserve_exact`; an allocation failure aborts, as the
pin's `new[]` does). A cursor read is `kccuraccept` into a buffer the
cursor reuses. Whole-file iteration is `kcdbiterate` with a read-only
visitor that copies each record, key and value, into one allocation (A
made three per record: `kccurget`'s region, then a `Vec` each for the
key and the value). The
records are consumed by value, so a caller that copies rows out frees
each as it goes and never holds two copies of the file. The visitors
only copy bytes; nothing runs under the record lock that calls back
into the database.

**B3, the ordered-container walk.** `scan_raw` on a `TreeDB` or
`ProtoTreeDB` copied every row of the file, sorted it and filtered it.
B positions one cursor at the lower bound and steps until the first key
past the upper bound: libpinyin's own `search_suggestion` walk,
`cursor->jump(key)` then `step()`
(`src/storage/phrase_large_table3_kyotodb.cpp:211-244` at 074a2219).
`TreeDB` and `ProtoTreeDB` order keys with `LEXICALCOMP`, which is the
order of `[u8]`, and the pin never tunes the comparator (no `tune_`,
`rcomp` or `#type` anywhere in the pin's `src/`), so the walk returns the
same rows in the same order. `HashDB` and `StashDB` keep the copy and
sort. The B3 model test also covers a defect in A's unordered fallback:
an `Excluded` lower bound let keys below it through. No caller passes
`Excluded`, so no output changes.

**C, the bridge.** A cxx 1.0.205 bridge to a shim that constructs the
concrete classes libpinyin constructs — `new TreeDB`, `new HashDB`,
`new StashDB`, `new ProtoTreeDB` (`ngram_kyotodb.cpp:54-120`,
`phrase_large_table3_kyotodb.cpp:92-137`,
`chewing_large_table2_kyotodb.cpp:77-122` at 074a2219) — owned behind a
`UniquePtr`. Records cross as byte slices into Kyoto Cabinet's storage,
valid only inside the visitor. Status is read from `Error::code()`,
never from an exception message. No bridge function returns `Result`,
so an exception that reaches the boundary terminates the process, the
end it met escaping the C API. `Db` is `Send` and `Sync` for the reason
the C binding's was: Kyoto Cabinet's classes lock internally, and the
shim keeps no state of its own outside them.

## Is `PolyDB::open` with only `#type=` a bare concrete open?

Yes. `kclangc.h`'s `kcdbopen` is `PolyDB::open`. Read from the 1.2.80
headers, `kcpolydb.h:438-925`: the path is split on `#`; with only
`type=` present, every tuning local keeps its default (`-1`, `0`,
`false` or `NULL`), so each `tune_*` call, guarded by `> 0` or
non-`NULL`, is skipped; no logger or meta trigger is installed, since
neither `log` nor `mtrigger` was given and none was tuned on the
`PolyDB`; no archive compressor is set. `PolyDB` then constructs the
class with `new` and calls `db->open(fpath, mode)` (`kcpolydb.h:910`)
with `fpath` the path before the `#`, the same mode, and copies the
class's error code and message into its own on failure. A bare
`new TreeDB()` and `open(path, mode)` therefore opens the same file
with the same options, and reports the same code and message. The
`#bnum=` suffix the stash uses is `tune_buckets(bnum)` before the open
in both (`kcpolydb.h:747`).

## Where the engine is compiled

Kyoto Cabinet's database classes are header code (`kchashdb.h`,
`kcplantdb.h`, `kcstashdb.h`, `kcprotodb.h`); `libkyotocabinet.so`
exports the C API, the utilities and a few out-of-line helpers. In A
and B the engine reaches the process inside `libkyotocabinet`, compiled
when the distribution built it. In C, as in the pin, the shim
instantiates the classes from the build-time headers, so the engine is
compiled into the shipping library and the system `libkyotocabinet`
supplies the rest. A header update therefore needs a rebuild of oxpinyin
to reach it, exactly as it needs one of libpinyin.

## Results: A→B, the access patterns

Time and residency from 30 rounds against an in-window same-bytes control
(A at an equal-length path); instructions and allocations per call, with the
exact-count control equal to A in every row. INCONCLUSIVE marks a row whose
control fell outside the rule; no figure is given for it. This campaign
ran on a host whose heavy-slot tool has no exclusive mode, so the in-window
control, not exclusive use of the machine, is what qualifies each timing row.

| operation | base ns | time ratio (cand/base) [95% CI] | Ir/call base → cand | blocks/call base → cand | bytes/call base → cand | peak heap B base → cand | VmHWM KiB base | VmHWM Δ KiB [95% CI] |
| --- | ---: | --- | --- | --- | --- | --- | ---: | --- |
| `get_raw.tree.empty` | 521.1 | 0.927 [0.919, 0.937] | 2,461 → 2,311 | 1.04 → 0.04 | 3.35 → 2.35 | 36,211,477 → 27,578,405 | 57486 | -11974 [-12024, -11926] |
| `get_raw.tree.small` | 554.5 | 0.953 [0.948, 0.955] | 2,510 → 2,411 | 2.04 → 1.04 | 11.3 → 6.35 | 36,211,477 → 31,387,589 | 57454 | -8200 [-8388, -8120] |
| `get_raw.tree.medium` | 582.7 | 0.948 [0.946, 0.951] | 2,565 → 2,464 | 2.06 → 1.06 | 69.7 → 36.0 | 39,015,753 → 27,353,967 | 61224 | -14806 [-14908, -14724] |
| `get_raw.tree.large` | 1,808 | 0.957 [0.952, 0.959] | 7,804 → 7,128 | 2.12 → 1.12 | 2,346 → 1,176 | 39,015,751 → 26,462,023 | 61190 | -15682 [-15750, -15596] |
| `get_raw.tree.miss` | 395.2 | 0.954 [0.950, 0.958] | 3,248 → 3,072 | 3.82 → 3.82 | 217.9 → 217.9 | 3,647,600 → 3,647,584 | 14494 | +24 [-4, +96] |
| `get_raw.hash` | 494.0 | 0.959 [0.956, 0.963] | 2,618 → 2,473 | 2.63 → 1.63 | 786.9 → 522.7 | 21,889,687 → 20,096,559 | 47338 | -1790 [-1866, -1754] |
| `scan_raw.suggestion` | 71,873,611 | 0.000 [0.000, 0.000] | 518,985,801 → 128,184 | 554,940 → 115.5 | 39,195,323 → 6,376 | 36,211,475 → 23,629,187 | 57428 | -15760 [-15828, -15682] |
| `scan_raw.full.tree` | 86,006,042 | 0.686 [0.681, 0.694] | 604,161,343 → 434,962,486 | 774,020 → 219,094 | 51,850,356 → 12,655,586 | 31,628,693 → 8,242,797 | 41600 | -20724 [-20782, -20642] |
| `scan_raw.full.hash` | 63,799,423 | 0.687 [0.675, 0.692] | 273,031,471 → 181,581,496 | 203,667 → 90,930 | 54,398,327 → 31,393,889 | 21,232,223 → 16,736,760 | 44998 | -5360 [-5420, -5324] |
| `framed.get` | 339.5 | 1.016 [1.001, 1.027] | 1,716 → 1,631 | 4.00 → 3.00 | 58.0 → 49.0 | 1,894,083 → 1,894,083 | 8724 | INCONCLUSIVE |
| `framed.scan` | 2,021 | 0.842 [0.829, 0.848] | 13,274 → 10,609 | 22.0 → 7.00 | 605.0 → 153.0 | 1,894,084 → 1,894,084 | 8690 | INCONCLUSIVE |
| `framed.txn_scan` | 390,338 | INCONCLUSIVE | 2,937,402 → 1,049,177 | 6,154 → 17.5 | 278,392 → 163,887 | 1,894,091 → 1,894,091 | 8788 | INCONCLUSIVE |
| `framed.is_empty` | 304.1 | 1.017 [1.002, 1.028] | 2,394 → 2,353 | 5.00 → 5.00 | 114.5 → 104.5 | 1,894,093 → 1,894,093 | 8688 | +32 [+4, +76] |
| `user_bigram.get` | 2,174 | 0.955 [0.951, 0.959] | 10,625 → 8,918 | 2.00 → 1.00 | 3,526 → 1,763 | 21,889,693 → 20,096,565 | 46680 | -3092 [-3194, -3028] |
| `user_bigram.store` | 210.7 | 0.991 [0.981, 1.001] | 2,223 → 2,223 | 0.00 → 0.00 | 0.00 → 0.00 | 21,889,697 → 20,096,569 | 46700 | -3064 [-3174, -3014] |
| `user_bigram.remove_store` | 174.5 | 0.989 [0.985, 0.999] | 1,476 → 1,475 | 0.50 → 0.50 | 898.3 → 898.3 | 21,889,711 → 20,096,583 | 46714 | -3082 [-3206, -3030] |
| `user_bigram.keys` | 2,390,152 | INCONCLUSIVE | 12,999,368 → 13,628,894 | 8,206 → 4,107 | 1,691,503 → 212,893 | 21,889,692 → 20,096,564 | 46732 | -3166 [-3194, -3076] |
| `lifecycle.open_system` | 191,230 | 0.986 [0.965, 1.008] | 1,015,140 → 1,015,143 | 141.5 → 141.5 | 965,449 → 965,449 | 5,859,908 → 5,859,908 | 12860 | +30 [-78, +132] |
| `lifecycle.user_load` | 5,869,796 | 0.996 [0.976, 1.004] | 13,297,286 → 13,339,756 | 5,526 → 5,525 | 1,294,599 → 1,294,590 | 40,827,539 → 28,245,251 | 62330 | -12472 [-12568, -12384] |
| `lifecycle.user_save` | 3,949,672 | 0.993 [0.982, 1.000] | 16,908,831 → 16,913,778 | 3,463 → 3,463 | 972,689 → 972,689 | 40,827,539 → 28,245,251 | 62390 | -12574 [-12616, -12492] |
| `session.typing_training` | 25,134,449 | 0.002 [0.002, 0.002] | 153,861,929 → 206,478 | 164,749 → 159.6 | 11,656,914 → 43,749 | 43,631,859 → 31,071,621 | 67022 | -13752 [-13790, -13664] |

The two ordered-container walks are the large changes: the suggestion walk
executes 0.025% of A's instructions and allocates 115 blocks per call instead
of 554,940, and a typing session with training (`session.typing_training`)
runs in 0.002 of A's time. The whole-file scans copy each record once
instead of three times (−31% time, −20.2 MiB `VmHWM` on the tree). Point
reads that find their record, through `get_raw` and `user_bigram.get`,
drop one allocation each (the `kcdbget` copy), and point reads take 4–7%
less time, misses included. `VmHWM` falls by 1.7 to
20.2 MiB on every operation with a STABLE residency row except
`get_raw.tree.miss`, `framed.is_empty` and `lifecycle.open_system`, which
are within 0.1 MiB. B is slower on two rows: `framed.get` (1.016) and
`framed.is_empty` (1.017), each with fewer instructions and no more
allocations than A. `user_bigram.keys` executes 5% more instructions in B, with its
time INCONCLUSIVE.

## Results: B→C, the bridge alone

Timing for B→C is pending: it is not reported until the host's heavy-slot
tool offers an exclusive mode, so that no other heavy work shares the
machine during a timing campaign. The deterministic measurements below
need no exclusive host; the exact-count control (B at an equal-length path)
equals B in every row.

| operation | Ir/call base → cand | Ir ratio | blocks/call base → cand | bytes/call base → cand | peak heap B base → cand |
| --- | --- | ---: | --- | --- | --- |
| `get_raw.tree.empty` | 2,311 → 2,372 | 1.026 | 0.04 → 0.04 | 2.35 → 2.35 | 27,578,432 → 27,578,336 |
| `get_raw.tree.small` | 2,411 → 2,473 | 1.026 | 1.04 → 1.04 | 6.35 → 6.35 | 31,387,616 → 31,387,520 |
| `get_raw.tree.medium` | 2,464 → 2,517 | 1.021 | 1.06 → 1.06 | 36.0 → 36.0 | 27,353,993 → 27,353,897 |
| `get_raw.tree.large` | 7,128 → 7,174 | 1.006 | 1.12 → 1.12 | 1,176 → 1,176 | 26,462,050 → 26,461,954 |
| `get_raw.tree.miss` | 3,072 → 3,055 | 0.994 | 3.82 → 3.82 | 217.9 → 217.9 | 3,647,612 → 3,647,516 |
| `get_raw.hash` | 2,473 → 2,563 | 1.036 | 1.63 → 1.63 | 522.7 → 522.7 | 20,096,592 → 20,096,496 |
| `scan_raw.suggestion` | 128,170 → 127,416 | 0.994 | 115.5 → 115.5 | 6,376 → 6,360 | 23,629,213 → 23,629,101 |
| `scan_raw.full.tree` | 434,962,500 → 432,405,977 | 0.994 | 219,094 → 219,094 | 12,655,586 → 12,655,570 | 8,242,824 → 8,242,712 |
| `scan_raw.full.hash` | 181,705,113 → 182,922,342 | 1.007 | 90,930 → 90,930 | 31,393,889 → 31,393,889 | 16,736,787 → 16,736,691 |
| `framed.get` | 1,630 → 1,628 | 0.999 | 3.00 → 3.00 | 49.0 → 49.0 | 1,894,118 → 1,894,022 |
| `framed.scan` | 10,609 → 11,070 | 1.043 | 7.00 → 7.00 | 153.0 → 137.0 | 1,894,118 → 1,894,022 |
| `framed.txn_scan` | 1,049,163 → 1,106,586 | 1.055 | 17.5 → 17.5 | 163,887 → 163,879 | 1,894,121 → 1,894,025 |
| `framed.is_empty` | 2,353 → 2,398 | 1.019 | 5.00 → 5.00 | 104.5 → 88.5 | 1,894,123 → 1,894,027 |
| `user_bigram.get` | 8,918 → 9,001 | 1.009 | 1.00 → 1.00 | 1,763 → 1,763 | 20,096,595 → 20,096,499 |
| `user_bigram.store` | 2,223 → 2,267 | 1.020 | 0.00 → 0.00 | 0.00 → 0.00 | 20,096,597 → 20,096,501 |
| `user_bigram.remove_store` | 1,476 → 1,529 | 1.037 | 0.50 → 0.50 | 898.3 → 898.3 | 20,096,604 → 20,096,508 |
| `user_bigram.keys` | 13,628,522 → 11,696,107 | 0.858 | 4,107 → 4,107 | 212,893 → 212,893 | 20,096,593 → 20,096,497 |
| `lifecycle.open_system` | 1,015,292 → 1,011,039 | 0.996 | 141.5 → 131.5 | 965,449 → 964,630 | 5,859,932 → 5,859,034 |
| `lifecycle.user_load` | 13,340,484 → 12,618,417 | 0.946 | 5,525 → 5,516 | 1,294,696 → 1,294,249 | 28,245,277 → 28,245,165 |
| `lifecycle.user_save` | 16,914,010 → 15,778,528 | 0.933 | 3,463 → 3,458 | 972,795 → 972,510 | 28,245,277 → 28,245,165 |
| `session.typing_training` | 206,494 → 191,050 | 0.925 | 159.6 → 159.3 | 43,750 → 43,719 | 31,071,719 → 31,071,607 |

Allocations are equal or fewer in C: blocks per call match on every
operation except `lifecycle.open_system` (−10), `lifecycle.user_load` (−9),
`lifecycle.user_save` (−5) and `session.typing_training` (−0.4), and bytes
per call are equal or lower. Instructions differ by −14% to +5.5%.

### Where B→C's extra instructions are

For each operation where C executes more instructions than B, Callgrind's
self cost inside `ops::measured`, per call, split by symbol: the engine
(`kyotocabinet::` code, the C++ standard-library code it instantiates,
and the unnamed local functions of `libkyotocabinet`), the C API wrapper
(`kcdb*`/`kccur*` in `libkyotocabinet`) together with the bridge (the
`oxpinyin::kc` shim, the `cxxbridge1$` glue, `rust::Slice` accessors and
the Rust side of the binding), and everything else (the bench loop,
Rust code, and in B a small amount in an object Callgrind cannot name).
Allocator and libc cost is equal to within 0.5 instructions per point call
and is left out.

| operation | engine B → C | C API + bridge B → C | other Δ | total Δ |
| --- | --- | --- | ---: | ---: |
| `get_raw.tree.empty` | 1,817 → 1,765 | 58 → 191 | −19 | +61 |
| `get_raw.tree.small` | 1,754 → 1,701 | 74 → 209 | −20 | +62 |
| `get_raw.tree.medium` | 1,689 → 1,627 | 74 → 209 | −20 | +52 |
| `get_raw.tree.large` | 1,478 → 1,409 | 74 → 209 | −21 | +46 |
| `get_raw.hash` | 783 → 764 | 74 → 209 | −27 | +90 |
| `scan_raw.full.hash` | 20,992,895 → 19,687,689 | 39,084,628 → 42,071,628 | −363,718 | +1,217,229 |
| `framed.scan` | 2,269 → 2,100 | 1,744 → 2,478 | −110 | +460 |
| `framed.txn_scan` | 209,633 → 193,231 | 213,282 → 295,304 | −8,235 | +57,423 |
| `framed.is_empty` | 751 → 702 | 179 → 314 | −37 | +45 |
| `user_bigram.get` | 266 → 232 | 74 → 209 | −18 | +83 |
| `user_bigram.store` | 338 → 305 | 26 → 101 | +3 | +44 |
| `user_bigram.remove_store` | 331 → 328 | 28 → 87 | −3 | +54 |

In every such row the engine executes fewer instructions in C than in B
(3 to 69 per point call, 6% on the hash scan), and the whole increase is
in the bridge. A point read crosses the boundary through four functions
(`ffi::Db::get`, `Db::accept_get`, the `Collect` visitor's `visit_full`,
the `visit_point` trampoline) against B's `kcdbaccept` and one visitor
callback, and each `rust::Slice` that C++ builds or reads is an
out-of-line call into the Rust side (`cxxbridge1$slice$new`, `sliceInit`,
`sliceLen`). `Db::set` costs 69 instructions per call to `kcdbset`'s 26.
On the bulk paths where C is ahead (`user_bigram.keys` −14%,
`session.typing_training` −7.5%, `lifecycle.user_save` −6.7%,
`lifecycle.user_load` −5.4%), the same split shows the engine executing
18.8%, 13.4%, 10.1% and 3.6% fewer instructions in C, more than the
bridge adds.

### Compiler flags

The shim is compiled by `cc` 1.4.3 through `cxx-build` 1.0.205 under the
bench profile, which inherits the release profile (opt-level 3, no debug
information): `-O3 -ffunction-sections -fdata-sections -fPIC -m64`, with
no flags from `pkg-config --cflags kyotocabinet` (empty) or the
environment, by GCC 15.3.0 (`.comment` of the shim object). The
distribution's `libkyotocabinet.so.16.14.0` (Debian source `kyotocabinet`
1.2.80-2build1) is stripped and carries no `.comment`; the one flag it
records is `-fcf-protection` (IBT and SHSTK in `.note.gnu.property`). Its
optimisation level is not recoverable from the installed object; it is
recorded in the separate debug symbols, which are not installed. Because
the B→C increase lies in the bridge rather than the engine, no C′ build
with the shim at the distribution's optimisation level was made.

## Capabilities only the C++ interface offers

C uses none: every call the shim makes has a `kclangc` counterpart
(`kcdbaccept`, `kcdbset`, `kcdbremove`, `kcdbiterate`, `kcdbcount`,
`kcdbsync`, `kcdbbegintran`/`kcdbendtran`, `kcdbloadsnap`/`kcdbdumpsnap`,
`kcdbcursor`, `kccurjump`, `kccurjumpkey`, `kccuraccept`), and
`tune_buckets` is the `#bnum=` that `kcdbopen` reads from the path. What
the C API does not offer:

- comparator, logger, meta-trigger and compressor objects
  (`tune_comparator`, `tune_logger`, `tune_meta_trigger`,
  `tune_compressor`, `kcplantdb.h`); through the C API only the named
  ones (`#rcomp=`, `#zcomp=`, `#log=`) are reachable;
- the visitor's `visit_before` and `visit_after` hooks (`kcdb.h:85`,
  `:91`);
- compiling the engine into the calling binary, with that binary's
  compiler and flags, which is what changes B→C's engine cost above.

The pin uses neither of the first two: no `tune_`, `rcomp` or `#` option
in its `src/`, and its Kyoto Cabinet visitors (`kyotodb_utils.h`,
`ngram_kyotodb.cpp`, `phrase_large_table3_kyotodb.cpp`,
`chewing_large_table2_kyotodb.cpp`, `punct_table_kyotodb.cpp`,
`flexible_ngram_kyotodb.h` at 074a2219) override only `visit_full` and
`visit_empty`.

## Library sizes

Stripped `libpinyin.so.15.0.0` and `libzhuyin.so.15.0.0`, Kyoto Cabinet
backend. Each build is `cargo build --locked --release -p oxpinyin-capi
-p oxpinyin-zhuyin-capi --no-default-features` with the `kyotocabinet`
feature (and `shipped` for libpinyin), then
`tools/packaging/relink-versioned.sh` from the staticlib against the
crate's `.ver` file, then `strip --strip-unneeded`: the same path for all
three. Every file is reported stripped by `file`, and `ldd -r` finds no
undefined symbol in any of them.

| file | base b13a1693 | B 3bf3e88a | C b89a57d5 |
| --- | ---: | ---: | ---: |
| `libpinyin.so.15.0.0` | 2,720,584 | 2,728,776 (+8,192) | 3,095,872 (+375,288, +13.8%) |
| `libzhuyin.so.15.0.0` | 2,474,824 | 2,474,824 (0) | 2,846,016 (+371,192, +15.0%) |

C's libraries carry the shim, the engine instantiated from the headers
and the cxx support code, and add `libstdc++.so.6` and `libm.so.6` to
`DT_NEEDED`.

## Verdict

B is proposed; C is not. The deterministic results decide it, so B→C
timing, still pending, does not block the decision.

- C uses no capability the C API lacks.
- On the hot paths (point reads, writes, framed scans) C executes 0.6–5.5%
  more instructions than B, all of it in the cxx glue. Instantiating the
  engine into oxpinyin does make the engine cheaper, by up to 19%, which
  puts C ahead on the bulk paths, but on the hot paths the glue costs
  more than that saving.
- C adds about 370 KB to each stripped library.
- The `abi-exports` gate fails on C: the cargo-built `libpinyin_capi.so`
  and `libzhuyin_capi.so` each export 178 symbols beyond `libpinyin.ver` and
  `libzhuyin.ver`: the cxx crate's runtime (`cxxbridge1$rust_vec$*`,
  `cxxbridge1$string$*`, `cxxbridge1$exception`) and the bridge's generated
  Rust entry points (`oxpinyin$kc$cxxbridge1$205$*`). The `pinyin_*` and
  `zhuyin_*` sets still match the version scripts exactly, and the objects
  relinked by `tools/packaging/relink-versioned.sh` export only those. An
  allowlist would make the leaked symbols part of the export set, where
  they would interpose with any other cxx-built library in the same
  process, so no allowlist or cdylib version script is proposed.

## Method

`tools/bisection/store-ops.py` drives the `backend_matrix_kyotocabinet`
bench binary's `--op` mode
(`crates/oxpinyin-store/benches/support/ops.rs`) against the pin-built
system tables of the Kyoto Cabinet cell. Each comparison runs base, a
same-bytes control (the base binary under another path of equal length)
and the candidate in one window on one machine, one fresh process per
sample. Every sample's checksum must equal the base's.

- Time: ns per store call, 3 warm-ups and 30 rounds, the order rotated
  each round, pinned to one CPU. Candidate/base paired median ratio with
  a 95% bootstrap interval. A row is reported only when the control/base
  ratio and its whole interval lie in [0.95, 1.05]; otherwise
  INCONCLUSIVE.
- Residency: process `VmHWM`. Candidate − base paired difference with a
  95% interval, reported only when control − base lies within
  ±max(64 KiB, 1% of the base median).
- Instructions: Callgrind `Ir` inside `ops::measured` only, per call.
- Heap: DHAT, blocks and bytes allocated per call (a run's totals minus
  a zero-iteration run's), and the whole-process peak live heap. DHAT
  counts every allocation in the process, C and C++ included.
- Callgrind and DHAT run at a tenth of the timing iterations. The control
  must equal the base exactly, or the row is INVALID.

```
store-ops.py --base A --candidate B --data <kc cell data> --work <dir> --cpu 11 --profile
store-ops.py --base B --candidate C --data <kc cell data> --work <dir> --profile-only --keep-profiles --out <file>
```

The symbol split reads the kept Callgrind outputs. The raw records are
not retained; the commands above regenerate them.

## Measured builds

| build | commit | `backend_matrix_kyotocabinet` sha256 |
| --- | --- | --- |
| A | d7c61754 (main) | `7a32349fd2e3eb9a91dd9c7230175f34bc7532bf0adbbef3afb61acbc196541e` |
| B | f74ba19e | `9077d8a34143cf70766deece536b5f4e97d18b3933a9b04b09b5c2ac2fa7c4e6` |
| C | fddeff09 | `9146b22f0938f87fe0b0f9b22dbc126a0ecb7159c535c39b65ef36aefcafbf22` |

Measured at f74ba19e and fddeff09 on base d7c61754. The commits have
since been rebased onto a main that changes no file the
`backend_matrix_kyotocabinet` bench compiles: `oxpinyin-store` has no
in-tree dependency. `git range-diff` from `d7c61754..fddeff09` to the
rebased stack (this branch, then #1043's code commit) shows the two Kyoto
Cabinet adapter commits as `=`. The measured stack's §7.5 controls commit
is dropped, because main carries its own version of those controls
(b1a659c2). Two commits are added, this document and the steering
paragraph in `.kiro/steering/rust-conventions.md`. Two commits differ
from the measured ones, neither in measured code:

- the measurement-harness commit, in `tools/bisection/store-ops.py` only:
  a failed write or flush of the records is now a named error with exit
  status 3, and the `--out`, `--profile-only` and `--keep-profiles`
  options were added. On main's controls, each exact count is checked on
  its own and a row is VALID only when every count is equal, the same rule
  the measured harness applied to all counts at once. The B→C records come
  from this harness on the measured stack's controls; the A→B records come
  from the harness before the `--out` and profile options were added,
  which is otherwise identical;
- the cxx bridge commit (#1043), in `tools/packaging/relink-versioned.sh` only:
  the Kyoto Cabinet link adds `-lstdc++`, so the shipping objects record
  `libstdc++.so.6` in `DT_NEEDED`.

The workers were not rebuilt; each campaign checked their sha256 against
the table above.
