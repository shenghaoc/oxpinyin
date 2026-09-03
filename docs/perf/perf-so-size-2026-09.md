# `.so` size — fat LTO + single codegen unit (2026-09)

Status: **profile change adopted.** The workspace had no
`[profile.release]` at all — release builds ran on cargo defaults
(`lto = false`, `codegen-units = 16`, `opt-level = 3`, `panic = "unwind"`),
leaving cross-crate duplication and per-CGU dead code in the `.so`. This
change adds `lto = "fat"` + `codegen-units = 1` and measures the result.

**Host note.** All before/after figures below are **x86_64/redb**
(Linux EL10, rustc 1.97.1, commit `bf83ffb9`, built with
`--no-default-features --features redb` because the host lacks KC dev
headers). They are **not comparable** to the canonical ARM64/KC numbers
— oracle 789,512 B vs oxpinyin 1,708,768 B stripped, ratio 2.164×
(`docs/findings/perf-baseline-kc-validation-2026-08-31.md` Correction 2).
Under redb the entire database engine compiles into the `.so`; a KC
build links the external `libkyotocabinet` instead. The before/after
comparison here is internally consistent (same host, commit, features,
and fixtures); the ARM64/KC re-measurement is pending.

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

## Before/after (stripped `.so`)

| | bytes | KiB |
|---|---:|---:|
| before (no release profile) | 2,914,304 | 2,846.0 |
| after (`lto = "fat"`, `codegen-units = 1`) | 2,694,568 | 2,631.4 |
| **delta** | **−219,736 (−7.54%)** | **−214.6** |

After/before ratio: **0.925×**. Unstripped: 3,832,344 → 3,125,528 B
(the extra −707 KiB is mostly `.symtab` shrink from inlined-away
symbols, irrelevant once stripped).

## Steady-state performance (regression gate)

`guess_candidates/offset_0`, criterion, 20 samples per side, identical
fixtures and features, run back-to-back:

| side | median | interval |
|---|---:|---|
| before | 6.9446 µs | [6.7928, 7.2172] |
| after | 6.4667 µs | [6.1440, 6.8580] |

Criterion's own change estimate: **−7.095%** [−10.899, −3.521],
p = 0.00. LTO enables cross-crate inlining and made the measured
path faster, not slower; the 5% regression gate is nowhere near tripped.

## Section breakdown (stripped, `size -A`)

| section | before (B) | after (B) | delta |
|---|---:|---:|---:|
| `.text` | 1,832,741 | 1,700,885 | −131,856 (−7.2%) |
| `.rela.dyn` | 391,248 | 385,680 | −5,568 |
| `.data.rel.ro` | 292,232 | 288,056 | −4,176 |
| unwind total (`.eh_frame` + `.eh_frame_hdr` + `.gcc_except_table`) | 298,852 | 227,176 | **−71,676 (−24.0%)** |
| `.rodata` | 78,360 | 72,136 | −6,224 |
| `.data` / `.bss` | 2,560 / 144 | 2,560 / 144 | 0 |

The 24% unwind-table cut is fat LTO proving more functions `nounwind`
after inlining; the ~222 KiB that remains is what `panic = "unwind"`
keeps paying for.

## Top-10 symbols (unstripped `nm --size-sort`)

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

## Crate attribution of symbol bytes (unstripped `nm`)

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

The canonical gap is ARM64/KC; this change does not re-measure it
(ratio reported in the Amendments row as pending). What x86_64/redb
says about the shape of the problem:

- **LTO+CGU=1 recovers 7.5%, not a fold change.** The stripped `.so`
  is still 2,631 KiB. Codegen settings alone cannot close a 2× gap.
- **~36% of the stripped image is untouchable by LTO**: static data
  tables (`ZHUYIN_PINYIN_MAP` 83.6 KiB, chewing `CONTENT_TABLE`
  45.9 KiB, HSU/ETEN indexes 27 KiB each — ~227 KiB of
  `oxpinyin_core` data), relocation mass (`.rela.dyn` + the
  `.data.rel.ro` it serves, ~663 KiB combined), and the ~222 KiB of
  unwind tables that survive under `panic = "unwind"`.
- **The std backtrace symbolizer (142.3 KiB) survived fat LTO** — it
  is referenced by the panic machinery, not dead code. Only
  `panic = "abort"` (or a customized std) removes it.
- On this backend, redb + hashbrown still cost ~629 KiB — a KC build
  does not carry this mass at all, which is why the KC-build ratio
  must be re-measured rather than extrapolated from these numbers.
  The backend-independent parts of the win (unwind-table cut,
  alloc/core pruning, wrapper dissolution) should carry to KC; the
  redb-specific merges will not exist there.

## Further reduction options

- **`panic = "abort"`** — would remove the remaining ~222 KiB of
  unwind tables plus the 142 KiB backtrace symbolizer (~13% of the
  current stripped image), and let further code prune. For a `cdylib`
  crossing a C ABI boundary, unwinding through foreign frames is
  already UB, so `abort` is semantically correct — but it changes the
  `.so`'s panic behaviour (a Rust panic aborts the process instead of
  unwinding) and is a **maintainer decision, not an agent decision**.
  Flagged here; not taken.
- **`opt-level = "z"`** — expects another single-digit % at a real
  speed cost, working against the Stage-2 steady-cycle parity goal
  (1.079× at the KC baseline). Not recommended while speed parity is
  the binding constraint.
- **Static tables** — the ~227 KiB of `oxpinyin_core` lookup data is
  inherent payload; shrinking it means format work (e.g. perfect-hash
  re-encoding), out of scope here.

## Verification

- Parity: `sentence_surface` §12 pin — **PASS** (1/1, non-vacuous,
  44 s run); `real_tables` compile-check — **PASS** (oracle-gated,
  0 executed on this host, exit 0 per plan).
- `cargo clippy --workspace --no-default-features --features redb
  --all-targets -- -D warnings` — **PASS**; `cargo fmt --check` —
  clean.
- `[profile.profiling]` untouched: its explicit `lto = "thin"` keeps
  winning over release's `lto = "fat"` (Cargo inheritance: child
  settings override, unset settings inherit), and it now inherits
  `codegen-units = 1` — profiling builds compile slower, measure the
  same.
