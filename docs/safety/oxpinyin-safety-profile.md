# The oxpinyin safety profile — "OXP-SAFE" (proposal)

A MISRA C:2025 Addendum 6-derived Rust safety profile, specialized to this
repository. It is deliberately *not* MISRA compliance; it is the set of rules
this repo can mechanically enforce, layer by layer, with the tools evaluated
in `tooling-evaluation.md`.

## Design principle

> A human contributor and an AI coding agent must hit the same compiler,
> linter, test, and CI failures when they break the same rule.

Every rule below is tagged with an enforcement tier:

- **HARD GUARANTEE** — holds by language/toolchain construction; cannot be
  violated without `unsafe`/toolchain change.
- **HARD CI GATE** — a PR that violates it cannot merge (compile error,
  lint error, or red check).
- **WARNING** — surfaces in CI as non-blocking noise for the author.
- **SCHEDULED ANALYSIS** — runs nightly (verify-nightly, 03:00); findings become issues, not
  merge blocks (unless ratcheted).
- **HUMAN REVIEW** — remains judgment; the profile's job is to shrink this
  set to what genuinely needs judgment.

## Layer 1 — Rust language guarantees (HARD GUARANTEE)

The safe-Rust crates get, by construction: memory safety, definite
initialization, null-freedom, data-race freedom, exhaustive matching,
strict evaluation order, no preprocessor. We do not re-lint these (that
would be cargo-cult). The boundary is the `unsafe` policy (Layer 2) and the
*panic* question (Layer 3), where Rust's guarantees genuinely stop.

## Layer 2 — unsafe policy (HARD CI GATE)

| Crate(s) | Rule | Mechanism |
|---|---|---|
| core, engine, user, segment, counter, lambda, emitter, corpus, dictool | **no unsafe, ever, not even locally** | crate-root `#![forbid(unsafe_code)]` (the pattern `oxpinyin-core` already proves composes with the workspace `deny`); `forbid` cannot be re-allowed by any inner attribute |
| data | no unsafe today; one documented future exception (mmap) | keeps `#![deny(unsafe_code)]` + module-scoped allows when/if mmap lands — `forbid` would foreclose the reserved exception, so deny is the deliberate choice |
| store | unsafe confined to `lmdb.rs` and `tkrzw/*` | workspace `deny` + the existing module-scoped `#![allow(unsafe_code)]` in exactly those files (a module allow is precisely what `forbid` forbids — deny+scoped-allow *is* the minimal trusted region here) |
| capi, oracle | unsafe allowed, but every block justified & every `unsafe fn` documented | `[lints.clippy] undocumented_unsafe_blocks = "deny"`, `missing_safety_doc = "deny"`, plus `rust::unsafe_op_in_unsafe_fn = "deny"`; `// SAFETY:` prose stays as the human-readable half |
| all | dependency unsafe inventoried | scheduled `cargo geiger` report; first-party unsafe inventory is the lint structure itself |

Deviation mechanism: none for `forbid` crates (by design — that is the
point); for capi/oracle/store the `// SAFETY:` comment *is* the deviation
record, enforced present-but-not-verified by Clippy, verified by review.

## Layer 3 — panic & failure policy (HARD CI GATE for library code)

1. Public APIs return `Result`/`Option` for every fallible operation
   (constitution §4) — enforced by type-system convention + review.
2. **Library crates** (core/engine/user/data/store/segment): `unwrap_used`,
   `expect_used`, `panic`, `panic_in_result_fn` denied at crate root;
   `#[cfg(test)]` modules carry a single justified `#![allow]` each. Today
   this passes with **zero** code changes (measured) — it locks the
   status quo against regressions.
3. `assert!`/`debug_assert!` policy: FFI input validation always-on;
   provable internal invariants may be `debug_assert!`; each surviving
   release `assert` (2 today, in `parser.rs`) carries its "internal bug
   trip" justification comment. Asserts in tests unrestricted.
4. FFI boundary: `ffi_catch` wraps 53 of the 55 C API entry points — F-7
   brought the three iterator-`end` drops under the wrapper. The two
   remaining unwrapped entry points (the trivial scalar writers
   `pinyin_get_pinyin_key_rest`/`..._positions` in `cursor.rs`) are
   intentional: null-check-and-write bodies documented as non-panicking;
   they gain the wrapper the day that stops holding.
5. `panic = "abort"` is **not** part of the profile (it would neutralize
   `ffi_catch` for the cdylib). Rust ≥1.81's abort-at-ABI is the backstop
   for *escapes*, which Layer 3.2 makes structurally unlikely.

## Layer 4 — arithmetic & conversion policy (HARD CI GATE + WARNING)

- Overflow-critical numeric paths use checked/saturating ops (existing
  pattern, ~100 sites). New raw `+`/`*` on scores/counts/lengths in
  trust-boundary code is a review blocker; the parser asserts remain the
  bug-trips.
- Casts: `cast_possible_truncation`, `cast_sign_loss`,
  `cast_precision_loss` at **warn** (107 sites today); FFI seams may
  `#[allow]` with justification (the deviation record). Narrowing at ABI
  edges uses `try_from(...).unwrap_or(clamp)` (existing capi pattern).
- `overflow-checks` + `debug-assertions` ON in a scheduled release-test
  lane; OFF in shipped profiles (Stage-2 budget).
- The three arithmetic defects found by this study (F-1, F-2, F-3) are
  fixes to file, not policy.

## Layer 5 — must_use & API contract (HARD CI GATE)

- `rust::unused_must_use = "deny"`.
- `clippy::must_use_candidate = "warn"`; getters/queries that return values
  get `#[must_use]`. (Historical — F-10's gap files `data/content.rs`,
  `user/codec.rs` and `store` were closed in PR-1/#200 (4ea4355): the 23
  flagged sites gained the attribute and the lint has sat at zero hits
  since.)
- Public error enums stay `#[non_exhaustive]`; extension traits grow only by
  defaulted methods (structure.md freeze — unchanged).

## Layer 6 — dependency & supply chain (HARD CI GATE)

- `cargo deny --locked check` on every PR, for the root workspace
  (default and `--all-features`) and the fuzz workspace's own graph:
  vulnerabilities = deny; **yanked = deny**; unmaintained advisories are
  configurable in cargo-deny 0.20.2
  (`unmaintained = "workspace" | "transitive" | "all"`), and because
  `deny.toml` omits the setting the default `unmaintained = "all"`
  applies — they fail unless recorded as an `ignore` entry in
  `deny.toml` (the deviation registry, currently one: bincode via
  heed-types/lmdb, with reason and review-by date), and a CI
  `cargo tree` assertion keeps the default graph bincode-free so the
  global ignore cannot mask it leaking beyond the lmdb path; licenses =
  allow-list (GPL-3.0-or-later + permissive set, NCSA scoped to
  libfuzzer-sys via `[[licenses.exceptions]]`); **sources = crates.io
  registry only — git sources disallowed** (`unknown-git = "deny"`,
  empty `allow-git`); multiple-versions = warn (informational for the
  Stage-2 size budget). The release tarball that CI executes is
  checksum-verified before install.
- No new runtime dependency without ask (constitution) — `deny.toml`
  `bans.deny = [{ name = "..." }]` only if a concrete ban ever becomes
  policy; start empty.
- Optional unsafe deps (`heed`, `cxx`) stay feature-gated and off the
  default build; geiger report tracks them.

## Layer 7 — dynamic & formal verification (SCHEDULED ANALYSIS)

| Activity | Cadence | Scope |
|---|---|---|
| fuzz smoke | every PR | parser target only (10s); the four newer targets run in the nightly soak |
| fuzz soak | nightly | all targets, 10–30 min, corpus committed |
| Miri | nightly | `-p oxpinyin-core -p oxpinyin-store` tests + corpus replay |
| overflow-checks release test | nightly | `cargo test --release` with `-C overflow-checks -C debug-assertions` |
| mutation score | nightly (trial) | core parser/full-pinyin index/scheme/scoring + user/store |
| coverage report | nightly | llvm-cov, report-only |
| ~~Kani harnesses~~ | dropped (toolchain age) | — |
| Lizard | nightly | CCN≤40 ratchet from current max 38 |
| geiger | nightly | dependency unsafe diff |

None of these claim correctness; each is bug-finding machinery pointed at
the highest-risk surfaces identified in `oxpinyin-audit.md`.

## Layer 8 — FFI-specific policy (HARD CI GATE + HUMAN REVIEW)

Mechanized: the 55-symbol ABI is pinned to the checked-in `pinyin.h`
(verified by the C++ smoke gate and contract tests); SAFETY comments
enforced (Layer 2); panic containment enforced-by-review with the F-7
cleanup. Remains judgment: ownership lifetime contracts of borrowed
candidate pointers, the `g_free`/malloc pairing assumption, GArray layout
reads in oracle — each already documented at its site; the profile adds a
standing review checklist item rather than pretending a tool covers it.

## Layer 9 — style & docs (WARNING)

rustfmt stable defaults (existing gate). `missing_docs` warn; proposed
warn-level `missing_errors_doc`/`missing_panics_doc`. Nothing cosmetic
blocks merges beyond the existing fmt/clippy gates.

## What this profile intentionally does NOT do

- No MISRA compliance claims, ever.
- No pedantic/nursery group enables; no restriction group enables.
- No coverage/mutation thresholds until baselines exist.
- No formal verification beyond the four trial harnesses.
- No runtime hardening switches (`panic=abort`, release overflow checks) in
  shipped artifacts.
- No re-linting of compiler guarantees.

## Rollout status (PR-1 = #200, PR-2 = #201)

PR-1 (mechanics) landed as 94682d5…c3155e5 plus four review rounds
(4885dc3, 409956e, 8722e79, 16ec7cf): forbid sweep, panic-containment
denies, workspace must_use/unsafe_op_in_unsafe_fn, clippy.toml,
FFI-crate SAFETY lints, must_use sweep (23 sites, closing F-10), and
the hardened deny job (locked graphs incl. the fuzz workspace,
checksum-verified cargo-deny install, bincode default-graph assertion,
NCSA scoped to libfuzzer-sys). The fmt-only pre-commit hook and
.vscode config proposed as PR-1g were dropped in review as unnecessary.
PR-2 landed as a446b27/51eeb40/8fff932: F-3, F-1/F-2 (+F-9), F-7 with
regression tests. Full workspace clippy -D warnings and cargo test are
green. PR-3 landed as #211 (fuzz-target expansion + the verify-nightly
lanes) and PR-4 as #212 (the cargo-mutants and nextest trial lanes).
Kani was scoped into PR-4 and then dropped: no Kani release supports
the pinned toolchain (see tooling-evaluation §19).

Mechanics correction discovered during implementation: CI's blanket
`-D warnings` promotes *every* warn-level lint to a merge blocker, so
this repository has no soft-warning tier in practice. The workable
pattern (used throughout PR-1) is: enable a lint at warn **together with**
zeroing its current hits — existing intentional sites get justified
`#[allow]`s (deviation records), and any *new* violation blocks CI. The
cast/docs lint sweeps (107/82 sites) are therefore separate, chunkier
follow-up PRs, not part of PR-1.

## Rollout

1. **PR-1 (mechanics)**: per-crate `forbid`/scoped-allow unsafe policy;
   curated clippy set; `unused_must_use` deny; `unsafe_op_in_unsafe_fn`;
   `deny.toml` + CI job. All measured green except must_use gap fixes.
2. **PR-2 (hygiene)**: close F-7 (five unwrapped entries), F-1/F-2/F-3
   arithmetic fixes (separate, individually reviewed). The must_use gaps
   (F-10) ended up in PR-1 instead and were closed there (4ea4355).
3. **PR-3 (lanes)**: scheduled workflow: fuzz expansion + nightly soak,
   Miri, overflow lane, llvm-cov, geiger, Lizard ratchet.
4. **PR-4 (trials)**: scoped cargo-mutants; nextest adoption for the
   unit lane. (Kani was scoped here too, then dropped: no release
   supports the pinned toolchain — see tooling-evaluation §19.)
5. AGENTS.md shrinks as rules land (see `AGENTS-reduction.md`).
