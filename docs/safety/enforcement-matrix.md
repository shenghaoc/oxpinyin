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
| No unsafe outside capi/oracle/store(-features) | HGATE (workspace `deny` + per-crate allows) | HGATE (as landed: crate-root `#![forbid(unsafe_code)]` in 10 crates, manifest-level forbid in oxpinyin-python and oxpinyin-runtime; `data` keeps `deny` reserving its documented mmap exception; store stays deny+scoped module allows) | Cargo `[lints]` + crate-root attributes; `forbid` cannot be re-allowed |
| Every unsafe block justified | REV (prose) | HGATE | `clippy::undocumented_unsafe_blocks = deny` in oxpinyin-capi and pinyin-oracle only; oxpinyin-store carries no such lint (its scoped module allows are review-covered) |
| Every `unsafe fn` documented | REV | HGATE | `clippy::missing_safety_doc = deny` |
| No unsafe ops silently inside `unsafe fn` bodies | not enforced | HGATE | `rust::unsafe_op_in_unsafe_fn = deny` |
| Dependency unsafe inventoried | not enforced | ~~SCHED~~ retired 2026-09-01 | the scheduled `cargo geiger` report artifact was retired with its lane (`docs/findings/verify-nightly.md`); dependency unsafe is no longer inventoried by any job |

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
| Checked/saturating ops on score/count merge paths | factual (~100 sites) | REV | review pattern only. The Kani-harness part of this row is absent: no proof harness exists in the tree (the trial was dropped — no Kani release supports the pinned toolchain) |
| Cast truncation/sign-loss visible | invisible | deferred by decision (PR-1 review round 1) | none: the three cast lints are enabled nowhere in the tree; enabling them is deferred until the ~107-site sweep lands as its own PR |
| FFI narrowing uses `try_from().unwrap_or(clamp)` | mostly (3 bare `as` remain, F-4) | HGATE-style convention | review only — no cast warnings exist in the tree (see the deferred row above) |
| Latent overflow in optimized builds surfaces | untested | SCHED | nightly `cargo test --release` with `-C overflow-checks -C debug-assertions` |
| No release `debug_assert`-only safety checks | factual | REV | audit found zero violations |

## D. Result handling & API discipline

| Rule | Today | Proposed | Mechanism |
|---|---|---|---|
| `Result`/`Option` never silently dropped | WARN (via CI `-D warnings` → HGATE) | HGATE | `rust::unused_must_use = deny` workspace lints |
| Value-returning queries are `#[must_use]` | partial (341 attrs; store=0, codec/content gaps) | WARN→HGATE | `clippy::must_use_candidate = warn` + gap fixes; then the deny above bites |
| Errors documented | partial | deferred by decision (PR-1 review round 1) | none: the two doc lints are enabled nowhere in the tree; a dedicated sweep is the planned vehicle |
| Public API stability freezes (non_exhaustive, defaulted methods) | REV (intent recorded in structure.md) | unchanged | review only — no cargo-public-api snapshot or tooling artifact exists in the tree |

## E. Complexity & maintainability

| Rule | Today | Proposed | Mechanism |
|---|---|---|---|
| CCN ≤ 40 per function | factual (max 38) | SCHED ratchet | nightly Lizard `lizard crates/ -l rust -C 40` (schedule: daily 03:00); a new >40 function fails the nightly report, not the PR |
| user/store.rs quartet refactored | no | roadmap item | flagged by Lizard + mutation priority, not a gate |

## F. Dependencies & supply chain

| Rule | Today | Proposed | Mechanism |
|---|---|---|---|
| No known-vulnerable deps | unenforced | HGATE | `cargo deny check advisories` per PR (RustSec DB) |
| License hygiene | unenforced | HGATE | `cargo deny check licenses` (GPL-3.0-or-later + permissive list) |
| No unexpected sources | unenforced | HGATE | `cargo deny check sources`: crates.io registry only — git sources are disabled entirely (`allow-git = []`) |
| Duplicate-version drift visible | unenforced | WARN | `cargo deny check bans multiple-versions = warn` (Stage-2 size signal) |
| New deps need ask | prose only | prose + advisory visibility | constitution rule stays (it's a process rule); deny.toml `bans` starts empty. *(geiger visibility retired 2026-09-01.)* |
| Unmaintained deps tracked | unenforced | reviewed ignore entries | advisory ignores in `deny.toml` carry a `reason` string with the review-by date inside it (the 0.20 schema has no expiry field; dates are prose, not a parsed key) |

## G. Dynamic verification

| Rule | Today | Proposed | Mechanism |
|---|---|---|---|
| Parser fuzz smoke per PR | yes (10s) | PR gate: parser target only; the four additional targets run in the nightly soak | cargo-fuzz pinned nightly (existing job) |
| Corpus replay under Miri | no | ~~SCHED nightly~~ retired 2026-09-01 | the Miri lane was retired (`docs/findings/verify-nightly.md`) |
| Fuzz soak | no | SCHED nightly | five targets × 3 min; one committed seed (`fuzz/corpus/parser/zhuan`), the rest of the corpus is seeded at run time |
| Coverage visibility | no | SCHED nightly | cargo-llvm-cov report, no threshold |
| Mutation score | no | ~~SCHED nightly (trial)~~ retired 2026-09-01 | the cargo-mutants lane was retired (`docs/findings/verify-nightly.md`) |

## H. Formal verification

| Rule | Today | Proposed | Mechanism |
|---|---|---|---|
| cost.rs shift-loop invariants proven | debug_assert only | none | a Kani harness for this existed briefly and was removed by accc645 — no Kani release supports the pinned toolchain; no harness exists in the tree |
| table_conf gcd ≥ 1 / exact λ rationals | invariant comments | none | a Kani harness for this existed briefly and was removed by accc645 — no Kani release supports the pinned toolchain; no harness exists in the tree |
| content.rs decode bounds for all byte inputs | length-guard comments | none | never implemented in any commit — this row described a harness that never existed. The coverage that does exist is the dict-loader fuzz target and the F-3 regression test |
| graph.rs starts monotonicity | loop-structure argument | none | never implemented in any commit — this row described a harness that never existed |
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
