# Mach-O function order on the M5: order file, Processor Trace check, measurement (2026-10-27 UTC)

Closes the two remaining items of #1171: a Mach-O counterpart to the ELF
function-order policies, and the Processor Trace investigation, which here
checks it. Everything ran natively on one MacBook Pro (Apple M5, 10 CPUs,
16 GiB, macOS 27.0.1 build 26A434, APFS data volume) from this branch on
main `399b813e`. The Rust tree is main's; the branch changes tooling and
documentation only.

Tools, all already on the Mac: Xcode 27.1 (27A9275) with xctrace and
Instruments 27.1, Apple clang 21.0.0, ld 27037.1, Xcode's `nm` (llvm-nm,
Apple LLVM 21.0.0), `otool` (cctools-1040) and `strip`; rustc and cargo
1.97.1 (the pinned toolchain), cargo-c 0.10.24, uv 0.13.1, just 1.59.0 and
the managed CPython 3.15; Homebrew berkeley-db@5 5.3.28_1, glib 2.90.1 and
gettext's libintl 8. No dependency was added.

## Stage 1: the order file

The mechanism is documented in `docs/packaging.md`, "Mach-O order file
(macOS)": `just packaging install` on macOS relinks cargo-c's dylib from the
staticlib with Apple's `-order_file`, derived at build time from the
backend's ELF policy by `tools/packaging/function_order.py`, and refuses the
install unless the ordered symbols form the `__TEXT,__text` prefix in policy
order. Fixture tests cover the resolver and the relink
(`tools/packaging/function-order.test.py`, `tools/dev/packaging.test.py`).

Resolution of the committed BDB policies against this build's archives
(`function ordering:` lines of the install):

| Facade | Policy paths | Resolved | Symbols ordered | Unresolved |
| --- | ---: | ---: | ---: | ---: |
| libpinyin | 265 | 246 | 253 | 19 |
| libzhuyin | 243 | 227 | 228 | 16 |

Seven libpinyin symbols and one libzhuyin symbol are extra clone bodies
(for example six `.N` clones of `<String as core::fmt::Write>::write_str`)
that join their family, as the ELF resolver orders every body of a family.
Both linked dylibs passed the prefix check with no dead-stripped entry.

The unresolved paths fall into three groups. Linux-only std paths (both
facades): `std::sys::fs::unix::try_statx`,
`std::sys::random::linux::hashmap_random_keys`,
`<std::sys::sync::once::futex::Once>::call` and
`std::sys::args::unix::imp::ARGV_INIT_ARRAY::init_wrapper`. Generic bodies
whose instantiating crate differs: the macOS LTO kept the `oxpinyin_runtime`
copy where the Linux policy names the `oxpinyin_facade` or `oxpinyin_user`
one (`BTreeMap IntoIter::dying_next` for five key types, `VacantEntry<u8,
u32>::insert_entry`, `GenericUserStore<BdbStore>::clone`, two `drop_glue`
instantiations; libpinyin also `SipHasher13::write` and `hash_one::<&u32>`
named under `oxpinyin_engine`, and `<&str as Display>::fmt` under
`oxpinyin_store`). Functions the macOS LTO inlined away:
`<SyllableKey>::from_option_text_with_distance`,
`oxpinyin_engine::session::dynamic_adjust_bigram_possibility` and, on
libpinyin, `drop_glue::<BTreeMap<u32, u32>>`.

The relinked objects were compared with cargo-c's own (the base arm): the
same `__TEXT,__text` size and address, the same function set (1,959 and
1,783 text functions) and the same symbol count; the unstripped files differ
by 48 and 32 bytes of symbol string table. Stripped sizes are identical.

| Dylib (`strip -x`) | Base bytes | PR bytes |
| --- | ---: | ---: |
| libpinyin.15.0.0.dylib | 2,029,000 | 2,029,000 |
| libzhuyin.15.0.0.dylib | 1,828,104 | 1,828,104 |

Two linker facts shaped the relink. `-force_load` of the staticlib kept 100
outline-atomics helpers from compiler-builtins objects that have no
subsections, plus every member's local labels (11,708 symbols instead of
2,899); loading the facade's object through `-u` roots and the archive on
demand, as rustc links it, gives the base's function set. The link also
passes `-S`: rustc's output has no debug map because Cargo's release
profile strips debuginfo after linking, and the objects carry std's DWARF.

Both arms come from one archive in one `CARGO_TARGET_DIR` and one prefix:
the base install opts out (`OXPINYIN_FUNCTION_ORDER_POLICY=`), which keeps
cargo-c's object, as main's install leaves it on macOS; the PR install
relinks the same archive with the order file.

```sh
export OXPINYIN_BDB_INCLUDE_DIR=/opt/homebrew/opt/berkeley-db@5/include
export OXPINYIN_BDB_LIB_DIR=/opt/homebrew/opt/berkeley-db@5/lib
export CARGO_TARGET_DIR="$W/target"
for arm in base pr; do
  [ "$arm" = base ] && export OXPINYIN_FUNCTION_ORDER_POLICY= || unset OXPINYIN_FUNCTION_ORDER_POLICY
  just packaging install libpinyin --prefix="$W/stage" -- --locked --release --no-default-features --features bdb,shipped
  just packaging install libzhuyin --prefix="$W/stage" -- --locked --release --no-default-features --features bdb
  cp -R "$W/stage" "$W/$arm"
done
```

## Stage 2: Processor Trace check

xctrace 27.1 lists the Processor Trace instrument on this M5 and recorded
without any prompt or setting change. Two recordings of the in-tree startup
worker (`tools/workers/startup.c`, built as the controller builds it) on the
ordered libpinyin dylib, the empty profile and the model20 BDB system
directory: `stop ... init`, the init asked for, and `stop ...
first_candidate`, the policy's startup scope (options, one instance and the
first candidate). `just measure processor-trace` (new, documented in
`docs/runbooks/benches.md`, "Processor Trace check (macOS)") reads each
export and compares it with the order file. A Processor Trace holds the
whole process: 49 binaries appear, and only the facade's calls are compared.

```sh
cc -std=gnu11 -Wall -Wextra -Werror -O2 -g -o "$W/startup" tools/workers/startup.c
EXPORTS=$(grep -o 'pinyin_[A-Za-z0-9_]*[[:space:]]*;' crates/oxpinyin-capi/libpinyin.ver | sed 's/[[:space:]]*;//' | paste -sd, -)
for stop in init first_candidate; do
  cp -R "$PROFILES/empty/user" "$W/user-$stop"
  xcrun xctrace record --template 'Processor Trace' --output "$W/$stop.trace" --no-prompt \
    --env "OXPINYIN_STARTUP_EXPORTS=$EXPORTS" --launch -- "$W/startup" stop pinyin \
    "$W/pr/lib/libpinyin.15.0.0.dylib" "$DATA" "$W/user-$stop" now "$stop"
  just measure processor-trace --trace "$W/$stop.trace" \
    --library "$W/pr/lib/libpinyin.15.0.0.dylib" --policy tools/packaging/function-order/bdb/libpinyin.order
done
```

Both recordings ran at 2026-10-27 12:37 UTC on the final ordered dylib;
each export holds about two million call intervals across 49 binaries, and
every libpinyin offset mapped to a `__TEXT,__text` symbol.

| Recording | Executed functions | Calls | Instructions in libpinyin | Executed and ordered | Their instructions | Executed, not ordered | Ordered init tier, not executed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| stop at `init` | 120 | 3,241 | 598,432 | 101 (all init tier) | 592,913 (99.1 %) | 19 | 140 |
| stop at `first_candidate` | 239 | 9,289 | 856,603 | 212 (all init tier) | 808,889 (94.4 %) | 27 | 29 |

No steady-tier function executed in either recording, as the tiers intend.
Of the 140 init-tier functions the init recording did not execute, 111 are
the options, instance and first-candidate path the policy's startup scope
includes and the first-candidate recording covers.

Executed but not ordered, through the first candidate (27 functions, 47,714
instructions, 5.6 percent of the library's): 11 are policy paths whose
instantiating crate differs (21,353 instructions), led by
`<SipHasher13 as Hasher>::write` (11,084 instructions, 378 calls) and
`<RandomState as BuildHasher>::hash_one::<&u32>` (8,695, 126 calls), both
kept under the `pinyin` crate here and named under `oxpinyin_engine` in the
policy, then the `BTreeMap IntoIter::dying_next`, `VacantEntry::insert_entry`,
`GenericUserStore::clone` and `drop_glue` bodies under `oxpinyin_runtime`.
The other 16 (26,361 instructions) are absent from the policy: three
functions the macOS LTO outlined (`<PhraseLibraries>::library`, 17,665
instructions in 256 calls; `take_layout_faults`; `<Target>::originals`), the
std paths this target takes (`small_c_string::run_with_cstr_allocating`,
7,423 instructions in 2,474 calls; the pthread `Mutex::init` and
`OnceBox::initialize`; `queue::Once::call`; `hashmap_random_keys`;
`Stderr::lock`), `compiler_builtins::...::u128_div_rem` behind the ordered
`__udivti3`, `oxpinyin_core::syllables::option_alias_canonical`,
`<BigramLanguageModel>::unigram_total`, `<&str as Display>::fmt`, and three
machine-outlined `OUTLINED_FUNCTION_N` stubs of 2 to 7 instructions.

Ordered init tier but not executed through the first candidate (29): the
populated-profile path (`oxpinyin_user::persistence::load_libraries` and
`load_logs_with_mask`, the `replay::Content` and `replay::Merge` functions
and their drop glue, the BDB `Cursor::read`, `SharedDb::cursor` and the
cursor drop, `decode_log_record_at`, the user `CountCache::unigram`), the
allocator shim `__rust_realloc`, `<RawVec<u32>>::grow_one`,
`<BTreeMap<u8, u32>>::insert`, the two `Arc::drop_slow` bodies,
`<PhraseContent>::as_slice`, an `ipnsort` instantiation, and five of the six
`write_str` clone bodies.

This stage changes no policy. What it suggests:

1. **Instantiating-crate identity is target-sensitive under LTO.** Eleven
   executed functions have a policy entry that differs only in the
   instantiating crate, among them the two heaviest executed-but-unordered
   bodies (`SipHasher13::write` and `hash_one::<&u32>`, 19,779 instructions
   together through the first candidate). A Mach-O fallback that accepts
   the one body of a crate-insensitive path when exactly one exists would
   order them without a second policy. The identity rule is the ELF
   resolver's, so this needs a maintainer ruling before it changes.
2. **macOS-only startup paths are outside the policy.** The std paths the
   Mac takes (`small_c_string::run_with_cstr_allocating`, the pthread
   `Mutex::init` and `OnceBox::initialize`, `queue::Once::call`,
   `hashmap_random_keys`) and three functions the macOS LTO outlined
   (`PhraseLibraries::library`, 17,665 instructions in 256 calls;
   `take_layout_faults`; `Target::originals`) execute every startup and
   are not in the ELF-measured policy. Ordering them would need a
   macOS-measured supplement, which Processor Trace can supply; that is a
   separate proposal, not part of this change.
3. **The init tier's unexecuted entries are the populated-profile path.**
   On the empty profile the user replay, the BDB cursor reads and the
   library loading do not run; the policy orders them first by design
   (#1090), and five of the six `write_str` clones did not execute.

## Stage 3: measurement

One controller run per cache mode, `just initbench` from this branch, base
(cargo-c's layout) against PR (the ordered relink) with the byte-identical
control, both facades, the empty profile, the model20 BDB system directory
(controller input digest `5f0f1fe73264d54587a7866c1f32066205a63fbb79d5c3240718710f8d25f56d`)
and the empty profile fixture (`a53eaadd1c3a45fc8f0009e5c0ceb9fffbcfea75268f0b96eeb866464f24cb1e`),
nothing else heavy on the host. Cold: 20 timing rounds plus 3 warm-ups and
5 memory rounds plus 2 warm-ups, as #1323 ran; no sample was rejected. Warm:
the default 60 plus 3 and 30 plus 2.

```sh
TMPDIR="$W/tmp" just initbench --base "$W/base" --pr "$W/pr" --system-dir "$DATA" \
  --profiles-dir "$PROFILES" --profiles empty --work-dir "$W/tmp/cold" --cache cold \
  --rounds 20 --warmups 3 --memory-rounds 5 --memory-warmups 2
TMPDIR="$W/tmp" just initbench --base "$W/base" --pr "$W/pr" --system-dir "$DATA" \
  --profiles-dir "$PROFILES" --profiles empty --work-dir "$W/tmp/warm"
```

Cold init of the ordered libpinyin is 3.2 percent faster and of libzhuyin
2.4 percent faster, with the controls inside their rules; the dylib's newly
resident pages between load and init fall from 65 to 57 (libpinyin) and from
62 to 52 (libzhuyin), all of the difference in pages that hold only
`__TEXT,__text`. The pages the first candidate brings in fall more (24 to 9,
13 to 2), and the pages after the first candidate rise (2 to 8, 3 to 17):
the teardown path's functions, not in the ordered prefix, now sit after it.
Warm init is not worse: the PR medians are below the base medians and the
95 percent intervals of the ratio include 1 (0.956–1.004 and 0.968–1.010).
The warm `first_candidate_phase` ratio (0.78 and 0.80) is a 14 µs interval.
The `lib_load` interval, which maps the dylib and resolves its exports with
`RTLD_NOW`, is unchanged in both modes.

### Cold cache: timing medians (ms)

Rejected cold samples: base 0, PR 0, control 0 (warm-ups 0, 0, 0); limit 2.

| Facade | Metric | Base | PR | Control | PR/base median | PR/base 95% CI | Verdict |
| --- | --- | ---: | ---: | ---: | ---: | --- | --- |
| pinyin | lib_load | 3.9815 | 3.9643 | 3.9826 | 1.004 | 0.983–1.019 | STABLE |
| pinyin | init | 8.5506 | 8.2554 | 8.2396 | 0.968 | 0.960–0.977 | STABLE |
| pinyin | first_candidate_phase | 1.0290 | 0.4477 | 0.4671 | — | — | INCONCLUSIVE |
| pinyin | ttfc | 13.7277 | 12.7194 | 12.6878 | 0.928 | 0.921–0.933 | STABLE |
| pinyin | process_wall | 16.3728 | 15.5067 | 15.4360 | 0.952 | 0.939–0.969 | STABLE |
| zhuyin | lib_load | 3.8112 | 3.8010 | 3.8425 | 1.001 | 0.979–1.010 | STABLE |
| zhuyin | init | 7.9428 | 7.7564 | 7.7796 | 0.976 | 0.973–0.980 | STABLE |
| zhuyin | first_candidate_phase | 0.8294 | 0.2886 | 0.2991 | — | — | INCONCLUSIVE |
| zhuyin | ttfc | 12.8866 | 11.9527 | 12.0359 | 0.924 | 0.922–0.932 | STABLE |
| zhuyin | process_wall | 15.4076 | 14.8252 | 14.9536 | 0.960 | 0.954–0.968 | STABLE |

The remaining metrics (config, alloc, drop_instance, fini, setup_plus_first_candidate) are intervals of microseconds; STABLE among them: pinyin fini (1.10), pinyin setup_plus_first_candidate (0.93), zhuyin alloc (0.05), zhuyin drop_instance (229.35), zhuyin setup_plus_first_candidate (0.92); INCONCLUSIVE: pinyin alloc, pinyin config, pinyin drop_instance, zhuyin config, zhuyin fini.

### Cold cache: newly resident dylib pages (16 KiB) per memory round

| Facade | Checkpoints | Base | PR | Control |
| --- | --- | --- | --- | --- |
| pinyin | start→lib_load | 7 | 7 | 7 |
| pinyin | lib_load→init | 65 | 57 | 57 |
| pinyin | init→config | 3 | 0 | 0 |
| pinyin | config→alloc | 0 | 0 | 0 |
| pinyin | alloc→first_candidate | 24 | 9 | 9 |
| pinyin | first_candidate→end | 2 | 8 | 8 |
| zhuyin | start→lib_load | 6 | 6 | 6 |
| zhuyin | lib_load→init | 62 | 52 | 52 |
| zhuyin | init→config | 6 | 1 | 1 |
| zhuyin | config→alloc | 0 | 0 | 0 |
| zhuyin | alloc→first_candidate | 13 | 2 | 2 |
| zhuyin | first_candidate→end | 3 | 17 | 17 |

Each cell is the count every round reported when the rounds agree. lib_load→init pages by Mach-O section, summed over the five rounds:

| Facade | Arm | Total | `__TEXT,__text` only | `__TEXT,__const` only | Pages spanning two or more sections |
| --- | --- | ---: | ---: | ---: | ---: |
| pinyin | base | 325 | 270 | 35 | 20 |
| pinyin | pr | 285 | 235 | 30 | 20 |
| zhuyin | base | 310 | 265 | 30 | 15 |
| zhuyin | pr | 260 | 215 | 30 | 15 |

### Warm cache: timing medians (ms)

| Facade | Metric | Base | PR | Control | PR/base median | PR/base 95% CI | Verdict |
| --- | --- | ---: | ---: | ---: | ---: | --- | --- |
| pinyin | lib_load | 2.9278 | 2.9524 | 2.9149 | 1.006 | 0.995–1.017 | STABLE |
| pinyin | init | 0.6948 | 0.6717 | 0.6822 | 0.982 | 0.956–1.004 | STABLE |
| pinyin | first_candidate_phase | 0.0602 | 0.0468 | 0.0474 | 0.781 | 0.763–0.805 | STABLE |
| pinyin | ttfc | 3.7041 | 3.6863 | 3.6543 | 0.997 | 0.985–1.010 | STABLE |
| pinyin | process_wall | 6.0545 | 6.0834 | 6.0213 | 1.007 | 0.993–1.014 | STABLE |
| zhuyin | lib_load | 2.9094 | 2.8990 | 2.8918 | 0.997 | 0.983–1.003 | STABLE |
| zhuyin | init | 0.4983 | 0.4958 | 0.4888 | 0.989 | 0.968–1.010 | STABLE |
| zhuyin | first_candidate_phase | 0.0537 | 0.0431 | 0.0426 | 0.803 | 0.790–0.824 | STABLE |
| zhuyin | ttfc | 3.4835 | 3.4514 | 3.4390 | 0.994 | 0.981–1.001 | STABLE |
| zhuyin | process_wall | 5.8049 | 5.7678 | 5.7777 | 0.994 | 0.975–1.004 | STABLE |

The remaining metrics (config, alloc, drop_instance, fini, setup_plus_first_candidate) are intervals of microseconds; STABLE among them: pinyin drop_instance (2.33), pinyin setup_plus_first_candidate (1.00), zhuyin alloc (0.46), zhuyin fini (1.03), zhuyin setup_plus_first_candidate (0.99); INCONCLUSIVE: pinyin alloc, pinyin config, pinyin fini, zhuyin config, zhuyin drop_instance.

## Limits

- The controller does not verify Mach-O ordering; it reports
  `function-order` as `unavailable` on macOS and measures the supplied
  dylibs. The install verified the PR arm's layout; the base arm is
  cargo-c's object by construction.
- A Processor Trace interval is a reconstructed call: a function inlined
  into its caller leaves no interval, and the instruction counts are the
  calls' own. The comparison is by function identity, not by page.
- Newly resident pages include readahead and the pages a checkpoint pause
  lets arrive (`page-residency` limits), and macOS pages are 16 KiB.
- These are native macOS figures, not pin comparisons; the reference
  results stay on linux/amd64 (#1171, "Scope").
