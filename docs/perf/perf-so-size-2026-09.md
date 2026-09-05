# `.so` size — fat LTO + single codegen unit (2026-09)

Status: **profile change adopted.** The workspace had no
`[profile.release]` at all — release builds ran on cargo defaults
(`lto = false`, `codegen-units = 16`, `opt-level = 3`, `panic = "unwind"`),
leaving cross-crate duplication and per-CGU dead code in the `.so`. This
change adds `lto = "fat"` + `codegen-units = 1` and measures the result.

**Host note.** The before/after figures in the sections below are
**x86_64/redb** (Linux EL10, rustc 1.97.1, commit `bf83ffb9`, built
with `--no-default-features --features redb` because that host lacks KC
dev headers). Under redb the entire database engine compiles into the
`.so`; a KC build links the external `libkyotocabinet` instead. The
before/after comparison is internally consistent (same host, commit,
features, and fixtures). The ARM64/KC re-measurement was completed
2026-09-04 and is recorded below under "ARM64/KC re-measurement";
see also the row in `docs/findings/perf-baseline-kc-2026-08-31.md`.

Method: `cargo build --locked --release -p oxpinyin-capi
--no-default-features --features redb`, then `strip --strip-all` on the
`cdylib` (`libpinyin_capi.so`). `strip --strip-all` removes `.symtab`,
so `nm` reports "no symbols" on the stripped file; the symbol-level
analysis below ran on the unstripped copies (saved under
`/tmp/measure-{before,after}/`). Bench: `cargo bench --bench stage2
--no-default-features --features redb -- guess_candidates/offset_0`,
back-to-back on the same host and fixtures (model20 via
`target/model20/extracted`, exported `.redb` tables regenerated once
before the change and shared by both runs).

## ARM64/KC re-measurement (2026-09-04)

The merge notes asked the next perf-container pass to re-measure this
change on the canonical ARM64/KC build. Done, in the ARM64
`oxpinyin-validate` container (Apple Silicon, linux/arm64, Debian
testing 20260831 snapshot, libkyotocabinet-dev, Rust 1.97.1, cargo-c
0.10.25) — the same environment family as the 2026-08-31 KC baseline.
Both sides built cold with `cargo cinstall --locked --release -p
oxpinyin-capi --prefix=/usr` and were stripped with `strip --strip-all`;
the export tables were regenerated once from the pinned model20 by the
after tree's datagen and shared by both sides.

| | bytes | KiB |
|---|---:|---:|
| before (rebase tip, no release profile) | 1,643,232 | 1,604.7 |
| after (`lto = "fat"`, `codegen-units = 1`) | 1,446,528 | 1,412.6 |
| **delta** | **−196,704 (−11.97%)** | **−192.1** |

The KC win (−11.97%) is larger than the x86_64/redb one (−7.54%).
Under KC the `.so` carries only the port's own code, so the cross-crate
deduplication has proportionally more of the image to act on — the
redb build's extra in-image DB engine was already LTO-merged per-CGU.
Section deltas (unstripped, `readelf -S`): `.text` 743,572 → 647,508 B
(−12.9%), `.rodata` 61,688 → 43,200 B (−30.0%), unwind (`.eh_frame` +
`.eh_frame_hdr`) 125,812 → 72,556 B (−42.3%), `.data.rel.ro`
267,192 → 261,536 B (−2.1%), `.rela.dyn` 347,616 → 340,128 B (−2.2%).
Same pattern as x86_64: the backend-independent levers (unwind cut,
dead-code pruning) carry, and the static tables / relocation mass
remain — the root-cause split below stands.

Against the pinned oracle (`789,512 B` stripped), the ARM64/KC ratio
moves 2.081× → 1.832×.

Steady-state: `guess_candidates/offset_0` (stage2 criterion,
`taskset -c 0`, four alternating rounds × 20 samples, shared tables):
before median 11.27 ns, after median 8.67 ns — **−23.1%**, after
faster on every round, no regression. The absolute nanosecond scale
(differs from the µs the x86_64 host saw) reflects the P1–P8 data
rewrite's candidate-path cost change, which affects both sides equally;
only the before/after comparison is claimed.

Note on the before-value: it is 1,643,232 B, not Correction 2's
1,708,768 B — 30 commits (the P1–P8 data rewrite) landed between the
two measurements and changed the artifact; the pair here was measured
back-to-back at one tip.

Gates on ARM64/KC: `real_tables` fixture-freshness **PASS** (2/2,
executed — the container has the oracle); clippy `-D warnings` and
`cargo fmt --check` clean. The `sentence_surface` §12 pin failed
**identically on both trees** (1-best agreement 491 vs the frozen 488,
`guessed_disagree` still 0) — a pre-existing drift introduced by the
P1–P8 rewrites, not by this change: a codegen profile cannot alter
deterministic predictions, and both builds ran on identical tables.
The frozen residual needs a maintainer-signed re-freeze (§12) — flagged,
not done here.

Measuring on KC required one companion fix on this branch: the stage2
benches still staged the pre-P1–P5 `.kct` table names while the KC
runtime opens libpinyin-native names (`SystemDbm::file_name`), so
`pinyin_init` failed before the profile change could be benched;
`fix(bench): stage2 staged DBMs under the pre-P1-P5 .kct names`
restores them to the names `export_dir` asserts and the runtime opens.

## x86_64/redb detail (original measurement)

### Before/after (stripped `.so`)

| | bytes | KiB |
|---|---:|---:|
| before (no release profile) | 2,914,304 | 2,846.0 |
| after (`lto = "fat"`, `codegen-units = 1`) | 2,694,568 | 2,631.4 |
| **delta** | **−219,736 (−7.54%)** | **−214.6** |

After/before ratio: **0.925×**. Unstripped: 3,832,344 → 3,125,528 B
(the extra −706,816 B is mostly `.symtab` shrink from inlined-away
symbols, irrelevant once stripped).

### Steady-state performance (regression gate)

`guess_candidates/offset_0`, criterion, 20 samples per side, identical
fixtures and features, run back-to-back:

| side | median | interval |
|---|---:|---|
| before | 6.9446 µs | [6.7928, 7.2172] |
| after | 6.4667 µs | [6.1440, 6.8580] |

Criterion's own change estimate: **−7.095%** [−10.899, −3.521],
p = 0.00. LTO enables cross-crate inlining and made the measured
path faster, not slower; the 5% regression gate is nowhere near tripped.

### Section breakdown (stripped, `size -A`)

| section | before (B) | after (B) | delta |
|---|---:|---:|---:|
| `.text` | 1,832,741 | 1,700,885 | −131,856 (−7.2%) |
| `.rela.dyn` | 391,248 | 385,680 | −5,568 |
| `.data.rel.ro` | 292,232 | 288,056 | −4,176 |
| unwind total (`.eh_frame` + `.eh_frame_hdr` + `.gcc_except_table`) | 298,852 | 227,176 | **−71,676 (−24.0%)** |
| `.rodata` | 78,360 | 72,136 | −6,224 |
| `.data` / `.bss` | 2,560 / 144 | 2,560 / 144 | 0 |

The 24% unwind-table cut is consistent with fat LTO proving more
functions `nounwind` after inlining (no compiler-level evidence
collected); the ~222 KiB that remains is what `panic = "unwind"`
keeps paying for.

### Top-10 symbols (unstripped `nm --size-sort`)

| before | symbol | after | symbol |
|---:|---|---:|---|
| 83,608 | `oxpinyin_core::zhuyin_map::ZHUYIN_PINYIN_MAP` (data) | 83,608 | unchanged |
| 45,864 | `oxpinyin_chewing::chewing_key_data::CONTENT_TABLE` (data) | 45,864 | unchanged |
| 28,000 | `oxpinyin_core::zhuyin_map::HSU_ZHUYIN_INDEX` (data) | 28,000 | unchanged |
| 26,992 | `oxpinyin_core::zhuyin_map::ETEN26_ZHUYIN_INDEX` (data) | 26,992 | unchanged |
| 18,684 | redb `WriteTransaction::commit_inner_helper` | 37,280 | same fn, ×2 — callees inlined |
| 18,375 | redb `btree_mutator::MutateHelper<&InternalTableDefinition>::apply_child_deletion_result` | 32,844 | `oxpinyin_data::dict::derive_pinyin` (inlined into caller) |
| 18,238 | redb `MutateHelper<TransactionIdWithPagination>` `apply_child_deletion_result` | 28,000 | (table row, see above) |
| 18,238 | redb `MutateHelper<AllocatorStateKey>` `apply_child_deletion_result` | 26,992 | (table row, see above) |
| 17,881 | redb `MutateHelper<TransactionIdWithPagination>::insert_helper` | 21,732 | `oxpinyin_user::lookup::UserLookup::from_store` (grew from 17,691 — inlined) |
| 17,850 | redb `MutateHelper<AllocatorStateKey>::insert_helper` | 17,323 | redb `MutateHelper` instantiation |

Distinct redb `btree_mutator` monomorphizations: **40 → 26** (14
duplicate/dead instantiations eliminated).

### Crate attribution of symbol bytes (unstripped `nm`)

| group | before (KiB) | after (KiB) |
|---|---:|---:|
| redb | 634.7 | 609.1 |
| hashbrown (redb dep) | 46.5 | 19.7 |
| core + alloc + std + backtrace symbolizer + std internals | 778.5 | 646.2 |
| — of which std backtrace symbolizer (gimli/addr2line) | 142.3 | **142.3 (unchanged)** |
| oxpinyin crates total | 547.8 | 501.8 |
| — `pinyin_capi` FFI wrappers | 66.7 | 19.3 |
| other (C/asm/anon llvm blobs) | 50.6 | 91.9 |

LTO dissolved the single-use FFI wrappers into callers, pruned
~175 KiB of `alloc`/`core` duplication, and merged 14 redb generic
instantiations. Some dissolved code reappears as anonymous `llvm.*`
blobs inside the surviving callers (`commit_inner_helper` doubling is
inlining, not bloat).

## Root cause of the 2.164× gap — where it stands

The ARM64/KC re-measurement above moved the canonical ratio
2.081× → 1.832× (1,643,232 → 1,446,528 B at the rebase tip). What
x86_64/redb says about the shape of the problem:

- **LTO+CGU=1 recovers single digits to low double digits, not a fold
  change.** Codegen settings alone cannot close a ~1.8× gap.
- **A large share of the stripped image is untouchable by LTO**:
  static data tables (`ZHUYIN_PINYIN_MAP` 83.6 KiB, chewing
  `CONTENT_TABLE` 45.9 KiB, HSU/ETEN indexes 27 KiB each — ~227 KiB
  of `oxpinyin_core` data), relocation mass (`.rela.dyn` + the
  `.data.rel.ro` it serves), and the unwind tables that survive under
  `panic = "unwind"` (72.6 KiB on ARM64/KC after the cut, 227 KiB on
  x86_64/redb).
- **The std backtrace symbolizer (142.3 KiB on x86_64/redb) survived
  fat LTO** — it is referenced by the panic machinery, not dead code.
  Only `panic = "abort"` (or a customized std) removes it.
  *(Correction, 2026-09-05: `panic = "abort"` does **not** remove it —
  measured unchanged on ARM64/KC with 168 gimli/addr2line symbols.
  What holds it is std's default panic hook, not the panic strategy
  and not `std::io::Error`; see the amended bullet under "Further
  reduction options". A second correction the same day retracted the
  `std::io::Error` attribution — see "Symbolizer attribution —
  corrected".)*
- On x86_64/redb, redb + hashbrown still cost ~629 KiB — a KC build
  does not carry this mass at all; the KC re-measurement confirmed
  the backend-independent parts of the win (unwind-table cut,
  alloc/core pruning, wrapper dissolution) carry, and the redb-
  specific merges do not exist there.

## Further reduction options

- **`panic = "abort"`** — ~~would remove the remaining ~222 KiB of
  unwind tables plus the 142 KiB backtrace symbolizer~~. **Adopted
  2026-09-05** (`perf/remove-ffi-catch-panic-abort`). Measured on
  ARM64/KC back-to-back at one tip (rustc 1.97.1, default KC features,
  `strip --strip-all`): stripped 1,512,160 → 1,446,624 B
  (**−65,536 B, −4.33%**); unwind sections (`.eh_frame` +
  `.eh_frame_hdr` + `.gcc_except_table`) 120,948 → 88,576 B
  (−32,372); `.text` −54,016 B. Re-measured 2026-09-05 back-to-back
  after the branch rebased onto the `oxpinyin-facade` extraction tip
  (`b40e3542`): stripped 1,577,696 → 1,446,624 B
  (**−131,072 B, −8.31%**) — the facade crate's added orchestration
  code carries unwind mass and landing pads that `abort` removes, so
  the delta grew with it; unwind sections at the branch tip 93,248 B,
  symbolizer still 168 symbols. **Correction to the original
  prediction: the 116.3 KiB std backtrace symbolizer (gimli/addr2line)
  survives `panic = "abort"` unchanged** — 168 symbols before and
  after, byte count identical. ~~Probe builds pin the mechanism: it is
  held by `std::io::Error` backtrace capture~~ — **retracted, see
  "Symbolizer attribution — corrected" below**; the mechanism is std's
  default panic hook.
  The ~88.6 KiB of unwind sections that remain under `abort` serve the
  exported `extern "C"` boundaries and C code in the image; shrinking
  those further is a separate investigation. See also the
  safety-policy override note in docs/safety/enforcement-matrix.md §B.
- **`panic_immediate_abort` (`-Z build-std-features`)** — the only
  known lever that removes the symbolizer, and **not available**: `-Z`
  requires nightly, and `rust-toolchain.toml` (stable 1.97.1) is the
  only supported toolchain per AGENTS.md. Confirmed by attempt —
  `cargo build -Z build-std=std,panic_abort -Z
  build-std-features=panic_immediate_abort` on the pinned toolchain
  fails with "the `-Z` flag is only accepted on the nightly channel of
  Cargo". Recorded as the remaining lever and closed; reopening it is
  a toolchain-policy question, not a code change.
- **`opt-level = "z"`** — expects another single-digit % at a real
  speed cost, working against the Stage-2 steady-cycle parity goal
  (1.079× at the KC baseline). Not recommended while speed parity is
  the binding constraint.
- **Static tables** — the ~227 KiB of `oxpinyin_core` lookup data is
  inherent payload; shrinking it means format work (e.g. perfect-hash
  re-encoding), out of scope here.

## Symbolizer attribution — corrected (2026-09-05)

The 2026-09-05 correction above attributed the symbolizer to
`std::io::Error` backtrace capture (its wording is superseded in place;
`git log` has the original). That was wrong, and the "next size target"
it set — audit and replace `std::io::Error` construction sites — is
**retired**: it would not have removed one byte of the symbolizer.

**What actually holds the symbolizer:** std's default panic hook.
`std::panicking::default_hook` and `std::panicking::panic_handler`
reach `std::sys::backtrace::BacktraceLock::print` / `__print_fmt`,
which reach `backtrace_rs::symbolize::gimli`; `std::alloc::rust_oom`
reaches the same chain. `panic = "abort"` does not drop it, because the
hook still runs to print message and backtrace before aborting. Any
crate with a reachable panic path therefore carries it.

**`std::io::Error` has no `Backtrace` field** in this std — it is a
tagged pointer (`Os(i32)` / `Simple(ErrorKind)` / `SimpleMessage` /
`Custom(Box<Custom>)`), so there is no capture for an `io::Error` to
perform. That is a property of the type at the pinned toolchain, and
it is what retires the earlier claim.

**Symbol absence is not code absence — do not argue from it.** Under
`lto = "fat"` a generic or inlinable function is routinely inlined out
of the symbol table while its code remains in the image. Measured
here: `nm` reports **zero** `io::Error::other` symbols in the artifact
built from the tree that still calls it. So a name failing to appear
in `nm` proves nothing, and nothing below rests on one — the argument
is the probe pair and the gimli byte mass, both of which are positive
measurements.

### Probe evidence

Seven throwaway `cdylib`s, ARM64 `oxpinyin-validate` container
(aarch64, rustc 1.97.1, binutils 2.47), each a single `#[no_mangle]
pub extern "C" fn probe()` over the contents in the table, no
dependencies, each built `cargo build --release` with the shipped
release settings (`lto = "fat"`, `codegen-units = 1`,
`panic = "abort"`) and counted with
`nm <lib>.so | grep -c 'gimli\|addr2line'`:

| probe | contents | gimli syms | gimli bytes | stripped `.so` |
|---|---|---:|---:|---:|
| p0 | arithmetic only, no reachable panic | **0** | 0 | 67,512 |
| p1 | OS-received `io::Error`, no construction | 137 | 117,065 | 266,736 |
| p2 | `io::Error::other("test")` | 137 | 117,065 | 266,736 |
| p3 | enum variant holding an OS-received `io::Error` | 137 | 117,065 | 266,736 |
| p4 | enum + `Display` + `Error::source` (the `StoreError` shape) | 137 | 117,065 | 266,736 |
| **p5** | **no `io::Error` at all — one `Vec` index** | **137** | **117,065** | 266,736 |
| p6 | the proposed replacement: local `Error` type, boxed | 137 | 117,065 | 266,736 |

p5 carries the identical symbols and identical bytes with zero
`std::io::Error` in the crate; p6 — the proposed fix — carries them
too. Every probe with a reachable panic path is byte-identical. The
only probe at zero is the one with no reachable panic path.

### Trial build

`main` carried **four** `std::io::Error` construction sites in library
code, not three — an earlier revision of this section missed the
`last_os_error` one because it grepped only for `::new` and `::other`:

| site | in the shipped KC `.so`? |
|---|---|
| `oxpinyin-runtime/src/lib.rs` (`require_file`), `::other` | yes — removed by this change |
| `oxpinyin-data/src/phrase_library.rs:205`, `::last_os_error` | **yes — still there** |
| `oxpinyin-store/src/lmdb.rs:478`, `::other` | no (non-default `lmdb`) |
| `oxpinyin-store/src/tkrzw/mod.rs:194`, `::other` | no (non-default `tkrzw`) |

`phrase_library.rs:205` sits in `#[cfg(unix)] fn from_file` under an
unconditionally compiled module, and `oxpinyin-data` is a mandatory
dependency of `oxpinyin-capi`. So removing the `require_file` site does
**not** leave the shipped image `io::Error`-free, and any future audit
that works from a three-site list is working from the wrong list.

Removing it anyway and rebuilding, back-to-back on the ARM64/KC
container, both sides `cargo build --locked --release -p
oxpinyin-capi` with default KC features and `strip --strip-all` — note
this is a plain `cargo build`, **not** the `cargo cinstall` used for
the ARM64/KC section above, so these rows are comparable to each other
and not to the cinstall figures:

| | `.text` | stripped file | gimli syms | gimli bytes |
|---|---:|---:|---:|---:|
| base (`77c3fb78`) | 642,756 | 1,446,624 | 168 | 119,073 |
| construction site removed | 638,388 | 1,446,624 | 168 | 119,073 |
| **delta** | **−4,368** | **0** | **0** | **0** |

**The symbolizer is untouched: 168 symbols and 119,073 bytes on both
sides.** That is the only claim this table supports, and it is the one
that retires the target.

The `.text` and stripped-file columns disagree, and `.text` is the one
to trust. Stripped file size is page-quantized on this host and cannot
resolve a 4 KiB change — this document's two `panic = "abort"` deltas
are exactly −65,536 and −131,072 B, and all seven probe cdylibs above
report an identical 266,736 B. **Do not cite a zero stripped-size
delta as evidence that a change costs nothing.** Removing the
construction site does shrink code, by 4,368 B of `.text` (the
`Custom` / `Box<dyn Error>` machinery it pulled in). What it does not
shrink, by any amount, is the symbolizer.

Landed as the `OpenError::NotRegularFile` variant on
error-classification grounds — not as a size change; the 4,368 B is a side
effect, and the symbolizer figure above is why the size target is
retired.

### Measurement-method note

**`nm -D` cannot see these symbols.** The dynamic symbol table reports
0 gimli/addr2line entries for every probe and for the real `.so`,
stripped and unstripped. The 168-symbol figure quoted throughout this
document comes from `nm` on the **unstripped** copy — note that
the Method block at the top of this file is scoped to the x86_64/redb
build, while every 168-symbol figure here is ARM64/KC, so use the
command in "Trial build" above, not that one. A gate of the form `nm -D <stripped.so> | grep -c
"gimli\|addr2line"` returns 0 on unmodified `main` and measures
nothing; use `nm` on the unstripped artifact.

## Verification

- Parity (x86_64/redb): `sentence_surface` §12 pin — **PASS** (1/1,
  non-vacuous, 44 s run); `real_tables` compile-check — **PASS**
  (oracle-gated, 0 executed on that host, exit 0 per plan).
- Parity (ARM64/KC): `real_tables` fixture-freshness — **PASS** (2/2,
  executed); `sentence_surface` §12 pin — **FAIL, identical on before
  and after trees** (1-best 491 vs the frozen 488, `guessed_disagree`
  0). Attributed to the P1–P8 data rewrites on `main`, not to this
  change; the frozen residual needs a maintainer-signed §12 re-freeze.
- `cargo clippy --workspace --no-default-features --features redb
  --all-targets -- -D warnings` — **PASS** (x86_64/redb); ARM64/KC:
  clippy `-D warnings` and `cargo fmt --check` — clean.
- `[profile.profiling]` untouched: its explicit `lto = "thin"` keeps
  winning over release's `lto = "fat"` (Cargo inheritance: child
  settings override, unset settings inherit), and it now inherits
  `codegen-units = 1` — profiling builds compile slower, measure the
  same.
