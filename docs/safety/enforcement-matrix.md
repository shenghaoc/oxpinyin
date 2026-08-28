# Enforcement matrix — every rule → its mechanism

Companion to `oxpinyin-safety-profile.md`. "Today" reflects commit `2382bdd`;
"Proposed" reflects the profile. Legend: HG = hard guarantee (language/
toolchain), HGATE = hard CI gate (merge blocked), WARN = non-blocking
warning, SCHED = scheduled analysis, REV = human review.

## A. Language & memory safety

| Rule | Today | Proposed | Mechanism |
|---|---|---|---|
| No UB in shipped library code | HG (safe Rust) | HG | rustc borrow/type checker; safe crates contain zero unsafe (verified) |
| No uninitialized reads | HG | HG | definite initialization; no `MaybeUninit` in workspace |
| No null derefs in safe code | HG | HG | references non-null; `Option` checked |
| No data races | HG | HG | `Send`/`Sync`; only two manual impls (store/tkrzw, documented) |
| No unsafe outside capi/oracle/store(-features) | HGATE (workspace `deny` + per-crate allows) | HGATE (stronger: `#![forbid]` in 9 crates — core + 8; `data` keeps `deny` reserving its documented mmap exception; store stays deny+scoped-allow) | Cargo `[lints]` + crate-root attributes; `forbid` cannot be re-allowed |
| Every unsafe block justified | REV (prose) | HGATE | `clippy::undocumented_unsafe_blocks = deny` (capi/oracle/store) |
| Every `unsafe fn` documented | REV | HGATE | `clippy::missing_safety_doc = deny` |
| No unsafe ops silently inside `unsafe fn` bodies | not enforced | HGATE | `rust::unsafe_op_in_unsafe_fn = deny` |
| Dependency unsafe inventoried | not enforced | SCHED | weekly `cargo geiger` diff report |

## B. Panics & failure

| Rule | Today | Proposed | Mechanism |
|---|---|---|---|
| Public fallible APIs return Result/Option | REV (constitution §4) | REV + partial | type convention; `clippy::result_unit_err` already denied |
| No `unwrap`/`expect`/`panic!` in library code | factual (0 hits) but unenforced | HGATE | `clippy::unwrap_used`/`expect_used`/`panic`/`panic_in_result_fn` denied at the crate root of the eleven library crates — core/engine/user/data/store/segment plus runtime, python, datagen, capi and pinyin-oracle; `not(test)` exempts inline `#[cfg(test)]` modules (see §J note on why the exemption is deliberate) |
| Release `assert`s only as documented bug-trips | factual (2, commented) | REV + comment anchor | review checklist keyed on the two `parser.rs` sites |
| No panic crosses the C ABI | near-factual (`ffi_catch` on 50/55) | HGATE after F-7 | review + contract tests; Rust ≥1.81 abort-at-ABI is backstop, not license |
| Tests/benches/examples unrestricted | yes | yes | explicit non-goal to lint them |
| `panic = "abort"` in shipped artifacts | absent | **forbidden by profile** | documented decision (would defeat `ffi_catch`) |

## C. Arithmetic & conversions

| Rule | Today | Proposed | Mechanism |
|---|---|---|---|
| Checked/saturating ops on score/count merge paths | factual (~100 sites) | REV | review pattern; Kani harnesses prove the four numeric cores |
| Cast truncation/sign-loss visible | invisible | WARN | `cast_possible_truncation`/`cast_sign_loss`/`cast_precision_loss` (107 sites measured) |
| FFI narrowing uses `try_from().unwrap_or(clamp)` | mostly (3 bare `as` remain, F-4) | HGATE-style convention | review + cast warnings point at the seams |
| Latent overflow in optimized builds surfaces | untested | SCHED | nightly `cargo test --release` with `-C overflow-checks -C debug-assertions` |
| No release `debug_assert`-only safety checks | factual | REV | audit found zero violations |

## D. Result handling & API discipline

| Rule | Today | Proposed | Mechanism |
|---|---|---|---|
| `Result`/`Option` never silently dropped | WARN (via CI `-D warnings` → HGATE) | HGATE | `rust::unused_must_use = deny` workspace lints |
| Value-returning queries are `#[must_use]` | partial (341 attrs; store=0, codec/content gaps) | WARN→HGATE | `clippy::must_use_candidate = warn` + gap fixes; then the deny above bites |
| Errors documented | partial | WARN | `clippy::missing_errors_doc`, `missing_panics_doc` |
| Public API stability freezes (non_exhaustive, defaulted methods) | REV + cargo-public-api snapshots (structure.md) | unchanged | existing process |

## E. Complexity & maintainability

| Rule | Today | Proposed | Mechanism |
|---|---|---|---|
| CCN ≤ 40 per function | factual (max 38) | SCHED ratchet | weekly Lizard `-Tlimit 40`; new >40 blocks the weekly report, not the PR |
| user/store.rs quartet refactored | no | roadmap item | flagged by Lizard + mutation priority, not a gate |

## F. Dependencies & supply chain

| Rule | Today | Proposed | Mechanism |
|---|---|---|---|
| No known-vulnerable deps | unenforced | HGATE | `cargo deny check advisories` per PR (RustSec DB) |
| License hygiene | unenforced | HGATE | `cargo deny check licenses` (GPL-3.0-or-later + permissive list) |
| No unexpected sources | unenforced | HGATE | `cargo deny check sources` (crates.io + github) |
| Duplicate-version drift visible | unenforced | WARN | `cargo deny check bans multiple-versions = warn` (Stage-2 size signal) |
| New deps need ask | prose only | prose + geiger/advisory visibility | constitution rule stays (it's a process rule); deny.toml `bans` starts empty |
| Unmaintained deps tracked | unenforced | WARN + expiry | advisory ignores carry `reason` + `expired` dates in `deny.toml` |

## G. Dynamic verification

| Rule | Today | Proposed | Mechanism |
|---|---|---|---|
| Parser fuzz smoke per PR | yes (10s) | yes, plus content/codec/scheme targets | cargo-fuzz pinned nightly (existing job) |
| Corpus replay under Miri | no | SCHED nightly | `cargo +nightly miri test` core/store + corpus |
| Fuzz soak | no | SCHED nightly | 10–30 min runs, committed corpus |
| Coverage visibility | no | SCHED nightly | cargo-llvm-cov report, no threshold |
| Mutation score | no | SCHED trial weekly | cargo-mutants scoped to core + user/store |

## H. Formal verification

| Rule | Today | Proposed | Mechanism |
|---|---|---|---|
| cost.rs shift-loop invariants proven | debug_assert only | SCHED trial | Kani harness 1 |
| table_conf gcd ≥ 1 / exact λ rationals | invariant comments | SCHED trial | Kani harness 2 |
| content.rs decode bounds for all byte inputs | length-guard comments | SCHED trial (post F-3 fix) | Kani harness 3 |
| graph.rs starts monotonicity | loop-structure argument | SCHED trial | Kani harness 4 |
| Whole-program verification | — | rejected | scope decision |

## I. Formatting & docs

| Rule | Today | Proposed | Mechanism |
|---|---|---|---|
| rustfmt clean | HGATE | HGATE | `cargo fmt --all --check` (existing lint job) |
| No nightly-only fmt options | factual | policy | no `rustfmt.toml` (stable defaults only) |
| Public items documented | WARN (`missing_docs`) | WARN (deny later for engine/capi) | rustc lint |

## J. Process & meta rules (stay prose)

| Rule | Why it stays prose |
|---|---|
| Constitution items 1–3 (broad appeal, size budget, no local AI) | product judgment |
| Freeze discipline (SPECs, scorer API, path-set parity) | already enforced by review + oracle differentials; linting "don't change frozen semantics" is not mechanizable |
| Rebase/worktree etiquette | git workflow, not code |
| Commit trailer form | already mechanical: `.githooks/commit-msg` + CI R1–R4 |
| "When in doubt, STOP" | the meta-rule; cannot be a lint |

## Coverage of the MISRA-derived rule set

Of ADD6 Table 1 (61 applicable-to-safe-Rust guidelines): after rollout,
**9 are HG/HGATE via rustc alone, 12 via Cargo lints, 11 via curated
Clippy, 6 via scheduled lanes, 15 are review-anchored with mechanical
assists, and 8 are process/documentation with no mechanical form** (details
per-guideline in `misra-rust-mapping.md`). Tables 2–3 are covered by the
unsafe-crate policy (Layer 2) and by construction respectively.
