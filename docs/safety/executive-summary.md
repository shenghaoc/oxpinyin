# Executive summary — oxpinyin safety & verification study

Working tree `2382bdd`, study date 2026-08-27. Deliverables in this
directory; nothing outside `docs/safety/` was modified. This is **not** a
MISRA-compliance claim; the output is a *MISRA C:2025 Addendum 6-derived
Rust safety profile*. The Addendum 6 PDF itself was retrieved and parsed
(working URL `misra.org.uk/app/uploads/2025/03/MISRA-C-2025-ADD6.pdf`;
classification columns committed as `misra-add6-table.csv`): 223
guidelines — 61 safe-Rust-applicable, 68 unsafe-only, 94 not applicable —
with the three buckets verified set-identical to both the primary
two-column model and the Safety-Critical-Rust-Consortium distillation;
18 guidelines are *Partial for safe Rust* (cast-family-dominated) and 48
carry Rust-adjusted categories (see `misra-rust-mapping.md`).

**Headline**: oxpinyin's code is already close to the profile's target
state — zero unsafe outside three audited crates, 100% SAFETY-comment
coverage, zero unwrap/expect/panic in library production code,
saturating arithmetic on the decode path, clean `clippy::all`. What is
missing is *mechanization*: most of that excellence is prose-enforced
(AGENTS.md) rather than compiler-enforced. The proposed changes are small
(attribute-level), measured green against the current tree, and convert the
biggest rules to hard failures.

## A. Top 20 highest-value changes (ordered)

1. `#![forbid(unsafe_code)]` in the 8 remaining safe crates (core proves
   the pattern) — makes the constitution's §5 allowlist mechanical. F-12.
2. `clippy::undocumented_unsafe_blocks` + `missing_safety_doc` denied in
   capi/oracle — makes "SAFETY per block" (195/195 today) enforced, not
   admired.
3. Panic containment denies (`unwrap_used`, `expect_used`, `panic`,
   `panic_in_result_fn`) via `cfg_attr(not(test))` in the eleven library crates (incl. capi, oracle, runtime, python, datagen)
   — locks in the existing zero-panic state at zero churn (measured).
4. `cargo deny` (advisories/bans/licenses/sources) as the sole supply-chain
   PR gate — closes the only fully unenforced layer.
5. Fix F-3 (`content.rs:367` header-driven `with_capacity` → allocation
   abort DoS from a tiny hostile file).
6. Fix F-1/F-2 (`fixture.rs:309,364` unchecked u64/u128 arithmetic — the
   production twins are already saturating; convert these too).
7. `unused_must_use = deny` + `must_use_candidate = warn` + close the
   must_use gaps (store=0 attrs; content/codec) — MISRA R.17.7 mechanized.
8. Cast lints at warn (`cast_possible_truncation`/`precision_loss`/
   `sign_loss`, 107 sites) — the largest untracked safe-Rust hazard class
   becomes visible; FFI seams get justified allows.
9. New fuzz targets: `dict-loader` (hostile data files; F-3 class) and
   `capi-commands` (stateful ABI session fuzzer, libchewing-precedented)
   — the two highest-yield shapes for an IME.
10. F-7: bring the five unwrapped capi entry points under `ffi_catch` (or
    document their non-panicability at the fn).
11. fuzz workspace lints (F-8): `[lints.rust] unsafe_code = "deny"`.
12. Nightly fuzz soak with committed corpus + ASan (10–30 min).
13. Nightly "paranoid" lane: release tests with `overflow-checks` +
    `debug-assertions` on — the NDEBUG answer done as CI, not as shipped
    config.
14. Miri lane for `-p oxpinyin-core -p oxpinyin-store` + corpus replay.
15. Kani trial: 4 harnesses (cost.rs shift-loop, table_conf gcd,
    content decode bounds, graph starts monotonicity).
16. cargo-llvm-cov nightly report (no threshold yet) — visibility first.
17. Issue-named regression-test convention (libchewing) — every fuzz/bug
    finding becomes a permanent test.
18. `missing_errors_doc`/`missing_panics_doc` at warn (82 hits) — the
    mechanical half of the interface-documentation directives.
19. Lizard CCN≤40 ratchet (current max 38) + refactoring roadmap for the
    `user/store.rs` quartet (38/33/31/22).
20. cargo-nextest for the unit lane (+ doctest step) — cheaper testing
    enables more testing; verify fixture-cwd behavior in trial.

## B. HARD compile/CI failures (proposed)

`must_use_candidate` (warn-level in Cargo, but CI's blanket `-D warnings`
promotes any new hit to a merge blocker — kept at zero hits) ·

`forbid(unsafe_code)` ×9 crates · undocumented unsafe blocks / missing
safety docs (capi, oracle) · `unwrap`/`expect`/`panic!` in library crates ·
dropped `#[must_use]` results · known-vulnerable or yanked dependencies ·
disallowed licenses/sources · fmt drift (already) · `clippy::all` (already)
· the entire existing warn-by-default rustc set via `-D warnings` (already,
and quietly covering ~10 MISRA rules).

## C. Remain WARNINGS

Cast triple (truncation/precision/sign-loss) ·
`missing_errors_doc`/`missing_panics_doc`/`missing_docs` ·
`unreadable_literal`, `map_unwrap_or`,
`redundant_closure_for_method_calls` · duplicate dependency versions ·
unmaintained advisories (with expiry-dated ignores) · Lizard 15<CCN≤40.

## D. Should NOT be enforced

`unwrap`/`indexing` in tests, benches, examples · shadowing lints ·
`float_cmp` on the two documented bit-parity ports · recursion (no stable
lint; 4 bounded sites stay review-documented) · pub-minimization as a hard
rule · any complexity hard-gate · coverage percentage.

## E. Rust guarantees that make MISRA rules unnecessary

The 94 Table-3 guidelines (per ADD6): memory/pointer rules (ownership),
initialization, essential-type conversions (§10 — no implicit
promotions), evaluation order (§13), switch/for control-flow rules
(§15/§16 fallthrough and bound mutation), function-declaration mechanics
(§17.3–17.5), the entire preprocessor section (§20), null-pointer
constants (§11.9 → `Option`), stdlib footguns (§21 subset), plus
`Send`/`Sync` data-race freedom and exhaustive matching — no rule should
re-state these.

## F–J. Tool verdicts

- **ADOPT**: curated Clippy subset; cargo-deny; fuzz expansion (+2
  targets); llvm-cov (report-only); nextest (unit lane); rustfmt gate &
  absent-toml policy (already in place).
- **ADOPT SELECTIVELY**: Miri (core+store only); geiger (scheduled
  report); Lizard (ratchet, report). *(The Miri, geiger and cargo-mutants
  lanes were retired 2026-09-01 — `docs/findings/verify-nightly.md`; Lizard
  remains.)*
- **TRIAL**: cargo-mutants (scoped to core + user/store);
  `significant_drop_tightening` during Stage-2 profiling. Kani was a
  trial candidate until dropped for toolchain age (see O). *(cargo-mutants
  retired 2026-09-01.)*
- **DEFER**: cargo-audit (redundant with deny's RustSec DB; keep as
  documented local convenience); coverage thresholds (until baseline);
  `redundant_pub_crate` as a signal.
- **REJECT**: `no-panic`; Prusti (today's support window); pedantic/nursery
  group enables; restriction group enables; release `overflow-checks` in
  shipped profiles (nightly lane instead); nightly-only rustfmt options.
  *(One dated override, 2026-09-05, commit b6dd5c6f: `panic = "abort"`
  for shipped artifacts — rejected above as defeating `ffi_catch` — is
  now CONDITIONAL ACCEPT in `[profile.release]`. The REJECT's premise
  is gone twice over: the UB rationale for catching at `extern "C"`
  boundaries has been false since Rust 1.81 (rust-lang/rust#116088),
  and with the no-panic lints green the catch sites were operationally
  inert — `panic = "abort"` made them literally dead, and all three
  were removed. Measured −64 KiB stripped on ARM64/KC; the 116 KiB
  symbolizer is unaffected (docs/perf/perf-so-size-2026-09.md).
  `[profile.profiling]` keeps an explicit `panic = "unwind"`.
  **Reverted 2026-09-05** (`perf/revert-release-panic-abort`): the
  profile line measured +5.5% on the ARM64/KC keystroke cycle against
  −64 KiB and recovers without it (docs/perf/perf-baseline-kc-2026-09.md);
  the release profile is back on `unwind` on performance grounds, while
  the `ffi_catch` removal and the abort-at-ABI containment stand.)*

## K. Highest-risk existing findings

F-3 (allocation-abort DoS via hostile data header) · F-1/F-2 (fixture
arithmetic overflow) · F-6 (opaque-handle trust surface + `g_free`≡`free`
pairing assumption — inherent to the C ABI, documented) · F-7 (five entry
points without panic containment) · F-4 (three bare ABI `as` casts) ·
F-11 (GArray layout read). No high-severity defects.

## L. Highest-complexity functions (Lizard, measured)

`user/store.rs`: `add_phrase_in` 38, `mask_out` 33, `remove_user_phrase`
31, `promote_addon_phrase` 22; `engine/nbest.rs nbest_sentences` 22 and
`session.rs build_scan_matrix` 16 (parity-critical — leave until Stage 2);
parsers 16–19 (legitimate table-driven); everything else ≤15 (avg CCN
2.5 across 2,283 functions).

## M. Highest-value fuzz targets

`capi-commands` (stateful 55-symbol ABI driving incl. adversarial iterator
lifecycles and config mutation) > `dict-loader` (bytes→data decode, F-3
class; libchewing `trieloader` precedent) > `scheme`/`codec` > existing
`parser` with parity-corpus seeds and nightly soak.

## N. Highest-value Miri tests

core parser/scheme corpus replay (pure functions) · user codec roundtrip ·
store tests under `--no-default-features --features redb` (the pure-Rust
peer backend; Miri covers this one, not the C-backed peers) · graph/kbest
invariants. Not applicable: libpinyin/kyotocabinet/tkrzw/LMDB C sides.

## O. Highest-value Kani harnesses

DROPPED (2026-08): no Kani release supports the pinned 1.97.1 toolchain
(0.67.0, the newest, bundles nightly 2025-11-21); revisit when one does.
The candidates considered were:

cost.rs `reduce_ratio`/`log2_fixed` postconditions · table_conf gcd/λ
exactness · content.rs `decode_tokens` bounds-for-all-inputs · graph.rs
starts monotonicity. Four harnesses, no whole-program ambitions.

## P. Coverage/mutation priorities

`user/store.rs` quartet first (complexity + persistence risk), then
capi error paths, then core parser/scheme edge cases. Mutation scope =
same list; decide ratchets after the first nightly's distribution.

## Q. FFI-specific risks

Opaque-handle provenance (stale/double-free = UB by contract) · iterator
begin/end pairing convention · borrowed candidate-string lifetimes ·
`malloc`/`g_free` cross-allocator assumption · GArray layout read ·
`usize→c_int` truncations · unwind containment (ffi_catch + Rust ≥1.81
abort-at-ABI backstop). Mitigations: contract tests, C++ smoke gate,
SAFETY-lint enforcement, the capi-commands fuzzer, and the review
checklist items already documented at each site.

## R. MSRV/toolchain impact

None. Every adopted tool is stable-binary or already wired (pinned
nightly fuzz job unchanged); Miri lives in a scheduled lane with its own
toolchain; `rust-version = "1.97.1"` and `rust-toolchain.toml`
untouched.

## S. Estimated CI cost by tier

PR gate: +~2 min (deny job ~40–90s; extra fuzz smoke ~30s; lint deltas
~0). Extended (label): +5–8 min. Nightly: ~30–60 min single runner.
Release: +~15 min. (The fmt-only pre-commit hook proposed for local runs was dropped in review.)

## T. What cannot realistically be mechanized

Requirements traceability (D.1.1) · determinism-as-purity (constitution
§6 — differentials are the evidence, not proof) · upstream-parity
semantics · ownership-lifetime contracts of borrowed FFI pointers ·
"broad appeal" product judgment · the frozen-freeze discipline · "when in
doubt, STOP".

## U. AGENTS.md rules that become removable after implementation

§5's unsafe allowlist text (→ Cargo `[lints]` + Clippy) · §4's no-panic
half (→ library-crate lint denies + fuzz) · the SAFETY-comment clause
(→ Clippy) · license compatibility half of the source policy (→
`cargo deny licenses`). Keep as one-line pointers; full mapping in
`AGENTS-reduction.md`. Roughly 15% of AGENTS.md's normative lines shrink;
nothing judgment-bearing is deleted.

---

### Study documents

| File | Content |
|---|---|
| `misra-rust-mapping.md` | ADD6-derived mapping, all 223 guidelines → classes A–L → enforcement |
| `misra-add6-table.csv` | ADD6 classification columns, machine-extracted from the primary PDF |
| `tooling-evaluation.md` | every tool evaluated, with measured clippy/Lizard/fuzz data |
| `oxpinyin-safety-profile.md` | the OXP-SAFE profile, layered, with rollout PRs |
| `enforcement-matrix.md` | every rule → mechanism → tier |
| `oxpinyin-audit.md` | source-tree findings register F-1…F-12 |
| `ci-strategy.md` | 4-tier CI design |
| `AGENTS-reduction.md` | prose → mechanics migration table |
| `upstream-test-strategies.md` | libpinyin/libchewing tests+fuzzer study and imports |
| `proposed-config-diffs.md` | reviewable patches for Cargo/CI/clippy/deny/hooks |
