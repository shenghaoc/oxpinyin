# oxpinyin source audit — findings (working tree `2382bdd`)

Methods: full-tree greps with per-hit manual classification; two read-only
sub-audits (unsafe/FFI; panic/arithmetic) over all 13 crates; Lizard 1.24.0
and Clippy 1.97 runs. Textual matches were **not** counted as violations
without inspection; every number below distinguishes production library
code from `#[cfg(test)]`/benches/bins.

## 1. Scale and shape

| Metric | Value |
|---|---|
| Crates | 13 (10 safe-policy, capi, oracle, store-with-features) |
| src LOC | ~51k (capi 7.8k, core 12.2k, engine 6.9k, oracle 7.5k, data 4.2k, user 4.1k, store 2.9k, tooling crates ~5.7k) |
| Functions (Lizard) | 2,283; avg CCN 2.5; NLOC 46,195 |
| Lockfile packages | 179; runtime deps: smallvec, compact_str, redb (+ heed/cxx behind features) |
| Fuzz targets | 1 (`parser`, 16 lines, safe Rust) |
| Tests | 26 integration test files + inline `#[cfg(test)]` modules throughout |

## 2. unsafe inventory (all of it)

| Crate | Blocks/fns/impls | SAFETY comments | Risk profile |
|---|---|---|---|
| oxpinyin-capi | 151 blocks + 6 `unsafe fn` + 4 `unsafe extern` | 100% | opaque-handle derefs (trust-based) dominate |
| pinyin-oracle | 39 blocks + 2 extern blocks | 100% | hand-written libpinyin decls pinned to header SHA; owned strings freed exactly once on all paths |
| oxpinyin-store | 5 blocks + 2 `unsafe impl Send/Sync` + 1 extern (cxx bridge), all behind `lmdb`/`tkrzw` features | 100% | heed flag call; cxx-generated marshalling; `from_raw_parts` over shim-provided views |
| all others + fuzz + tools | **zero** | — | — |

Recurring capi patterns (each audited): opaque-handle cast helpers (P1,
highest residual risk: stale/double-freed handle is UB by contract),
iterator-handle derefs (P2), caller C-string reads (P3), null-checked
out-param writes (P4, low), malloc/free string marshalling with
cleanup-on-partial-failure (P5), matched `Box::into_raw`/`from_raw`
lifecycles (P6). No caller-supplied buffer lengths exist anywhere — the
classic C ABI overrun class is structurally absent. Panic containment:
`ffi_catch` (`catch_unwind` → fallback) on 53 of 55 entry points — F-7 brought the three iterator-`end` entry points under the wrapper; the two `cursor.rs` scalar writers stay intentionally unwrapped (documented non-panicking).

## 3. Panic inventory

Production library code (non-test, non-bin): **zero** `unwrap`/`expect`/
`panic!`/`todo!`/`unimplemented!`/`unreachable!` — verified per hit against
`#[cfg(test)]` boundaries. The 545/428/1338 raw grep counts are ≥99%
in-file test modules and test files (`e2e_tests.rs`, `contract_tests.rs`,
`guess_offset_tests.rs`, `union_e2e_tests.rs`, `test_support.rs`).
Survivors, classified:

- `core/src/parser.rs:281,399` — two `assert_eq!` count/enumeration guards,
  commented internal-bug trips (intentional; keep).
- `core/src/cost.rs:70` — `debug_assert!` positive-log precondition
  (caller guards; correct use).
- Mutex poisoning recovered via `unwrap_or_else(poison.into_inner())`
  (user/store.rs:311, registry.rs) — deliberate, constitution-cited.
- Infallible-looking `unwrap_or` saturations (graph.rs:218, cost.rs:105) —
  not panic paths at all.
- capi FFI entry points: no unwrap/expect (the `expect("UTF-8 path")`
  hits are test-only temp-path helpers).

## 4. Findings register

| # | Severity | Location | Finding | Class | Status |
|---|---|---|---|---|---|
| F-1 | medium | `core/src/fixture.rs:309` (file since moved to `testsupport/src/fixture.rs`) | unchecked `*totals += count` on u64 bigram totals — crafted fixture overflows (debug panic / release wrap); 16 lines above, the sibling code already uses `saturating_add` | arithmetic, fix | **FIXED** (51eeb40: saturating totals) |
| F-2 | medium | `core/src/fixture.rs:364-366` (file since moved to `testsupport/src/fixture.rs`) | unchecked u128 products in `model_cost` (`bigram*unigram` totals) — overflow with counts ≥ ~2^63; production twin (`lm/mod.rs interpolate_ratio`) is fully `checked_mul` | arithmetic, fix | **FIXED** (51eeb40: checked u128, degrade to UNKNOWN_COST) |
| F-3 | medium | `data/src/content.rs:367` | `Vec::with_capacity(hdr.nitems as usize)` trusts an untrusted u32 header field → ~180-byte file can request ~170 GB (allocator abort = uncatchable DoS); header cross-checks constrain `data_size` but not `nitems` | trust boundary, fix (clamp + validate) | **FIXED** (a446b27: capacity clamp + regression test) |
| F-4 | low | `capi/src/candidates.rs:33,372`, `parse.rs:320` | three bare `as` narrowing casts at the ABI (usize→u32, usize→c_int truncation past 2 GiB, c_char→u8 wrap) — unreachable-in-practice today, unmarked | conversion, clamp + comment | open |
| F-5 | low | `user/src/store.rs` quartet | CCN 38/33/31/22 (`add_phrase_in`, `mask_out`, `remove_user_phrase`, `promote_addon_phrase`) — highest-complexity cluster; the natural coverage/mutation priority | complexity, refactor-later | open (roadmap) |
| F-6 | low | capi P1/P2 + `ffi.rs:43` | opaque-handle trust surface (stale/double-free handle = UB) and `g_free`≡libc-`free` pairing assumption (true on glibc, would break under custom GLib allocator) | FFI contract | open (documented residual) |
| F-7 | low | `iterators.rs:175,298,424` | three iterator-`end` entry points without `ffi_catch` (the import end calls `mark_modified()` — a panic there would cross the ABI as abort). The two `cursor.rs` writers cited originally are intentionally unwrapped: null-check-and-write bodies documented as non-panicking, deliberately left outside the wrapper | FFI hygiene, small fix | **FIXED** (8fff932: ffi_catch on all three iterator-ends; the cursor writers stay unwrapped by design) |
| F-8 | low | `fuzz/Cargo.toml` | separate workspace, edition 2021, **no `[lints]`** — the fuzz crate does not inherit `unsafe_code = "deny"` (currently moot: zero unsafe there, but the gate is missing) | policy gap | **FIXED** (deny added) |
| F-9 | info | `data/src/initials.rs:205-207` | `slot_shift` would underflow u32 for position ≥ 25; callers cap at 24 — pin with `debug_assert!` | hardening | **FIXED** (51eeb40: debug_assert pins the slot bound) |
| F-10 | info | `data/content.rs:391-421`, `user/codec.rs:57-157`, all of `store` | `#[must_use]` gaps (queries/getters/encoders) | hygiene | **FIXED** (4ea4355: 23-site sweep + must_use_candidate warn) |
| F-11 | info | oracle `live.rs:626-629` | GArray struct-layout read (two public fields) — documented, mitigated by pinned element size | FFI residual | open (documented residual) |
| F-12 | info | AGENTS.md vs Cargo | AGENTS/structure.md say "forbid in core, deny elsewhere"; mechanics are workspace-`deny` + inner attributes in 7 crates — engine/user/store/dictool rely on inheritance only; the *distinction* is prose, not Cargo | policy mechanization (this study's PR-1) | **FIXED** (94682d5: per-target forbid closes the prose gap) |

No high-severity defects were found. The three medium findings are all in
fixture/data-ingest paths, not the shipped decode path. Status: F-1, F-2,
F-3, F-7, F-9, F-10 and F-12 are fixed on branch `safety-study` (see the
register); F-4 (three benign ABI casts) and F-6/F-11 (documented FFI
residuals) remain open by design, F-5 is a roadmap item, F-8 is fixed
(fuzz workspace now denies unsafe_code).

## 5. Arithmetic & cast posture (summary)

Decode path: systematically saturating/checked (~100 sites audited; λ in
exact u128 rationals; `Cost = i64` fixed-point). Trust boundaries:
`try_from().unwrap_or(clamp)` at capi widening seams. Residual raw
arithmetic is loop-bounded by construction (graph arenas capped at
`MAX_GRAPH_INPUT = 65_535`, `MAX_PHRASE_LENGTH = 16`). Division/remainder:
constant or gcd≥1-proven divisors; float division guarded by
`total == 0` early-returns. Recursion: 4 sites, all depth-bounded
(nesting ≤ 2, phrase length ≤ 16). `as`-casts: ~150 numeric casts in src,
concentrated in data/content.rs (21), core/kbest.rs (11), graph.rs (8) —
the cast lints at warn (proposed) make this inventory visible from now on.

## 6. must_use

341 explicit attributes; `Result`/`Option` covered by type. Gaps: F-10.
Proposed: `unused_must_use = deny` + `must_use_candidate = warn`.

## 7. Complexity (Lizard, measured)

26 functions above CCN 15 across crates+fuzz (1.1% of functions); 15 in
production library files (table in `tooling-evaluation.md` §12).
Classification: `user/store.rs` quartet = genuine refactor candidates;
parsers/decoders (`parse_input`, `scheme::parse`, `decode_tokens`,
`from_decimal`) = legitimate table-driven complexity;
`nbest_sentences`/`build_scan_matrix` = parity-critical, deliberately
unrefactored until Stage 2; oracle/dictool = tooling.

## 8. Cross-reference: risk × verification coverage

| Surface | Risk | Current coverage | Proposed addition |
|---|---|---|---|
| capi ABI (55 symbols, handle lifecycle) | high residual (F-6/F-7) | contract tests + C++ smoke gate | `capi-commands` fuzz target (libchewing precedent); F-7 fix |
| data decode (content.rs) | medium (F-3) | unit tests on fixtures + F-3 regression test | `dict-loader` fuzz target; a Kani bounds harness remains deferred, conditional on a Kani release supporting the pinned toolchain (trial dropped — see tooling-evaluation §19) |
| fixture ingest (fixture.rs) | medium (F-1/F-2) | none specific | fixes + regression tests |
| user/store persistence | medium (F-5) | integration tests | coverage report priority (the cargo-mutants scope was retired 2026-09-01) |
| core parser/scheme | low (mature) | proptest + fuzz + parity corpus | expanded corpus soak; mutation score |
| oracle FFI | low-medium | pinning + differentials | keep; Miri not applicable (C side) |
| store lmdb/tkrzw/kyotocabinet | medium (unsafe deps) | feature-gated; four peer backends, tkrzw is the default selection, the other three explicit | the geiger inventory and Miri lanes were retired 2026-09-01; the C-backed peers stay covered by the ABI smoke gate and integration tests |
