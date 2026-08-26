# Tooling evaluation — oxpinyin safety/quality toolchain study

Status: proposal. Every recommendation is classified ADOPT / ADOPT SELECTIVELY /
TRIAL / DEFER / REJECT with evidence from this working tree (commit `2382bdd`)
and, where marked *measured*, from runs performed during the study
(2026-08-27). Toolchain facts reflect the pinned stable 1.97.1.

---

## 1. The rustc baseline (what we get for free and should not duplicate)

Rust's compiler already guarantees, statically, the properties behind a large
fraction of MISRA C:2025's Table-3 (non-applicable) guidelines — see
`misra-rust-mapping.md`. Concretely, for **safe Rust**:

- **Memory safety**: no use-after-free, no dangling references, no double
  free; ownership + borrow checking (MISRA §18, §22 largely moot).
- **Initialization**: definite-initialization; no uninitialized reads
  (R.9.1). `MaybeUninit` appears nowhere in this workspace.
- **Null safety**: references cannot be null; `Option<T>` is checked
  (R.11.9-style concerns vanish).
- **Type safety**: no implicit integer promotions/conversions like C's;
  enum discriminants cannot be invalid; exhaustive `match` (R.16.x).
- **Data races**: `Send`/`Sync` statically prevent them in safe code.
- **Evaluation order**: strictly defined (R.13.2/13.3/13.4 concerns vanish).
- **No preprocessor** (R.20.x): `cfg` and declarative macros replace it.

**Policy consequence**: the project should not write Clippy/CI rules that
re-state compiler guarantees (e.g., banning `&` "because aliases are
dangerous"). Where rustc is stronger than MISRA, say so and move on. The
workspace already leans on this correctly: `RUSTFLAGS: -D warnings` in CI
promotes every warn-by-default rustc lint (`dead_code`, `unreachable_code`,
`unused_must_use`, `deprecated`, `unused_mut`, `overflowing_literals`, …) to
errors — that single line *is* the mechanical enforcement of ~10 MISRA rules
(R.1.5, R.2.1–2.8, R.7.2, R.8.13, …) with zero maintenance cost.

**Where rustc is weaker than the C intuition**: panics (compile-invisible),
integer overflow in release (wrapping, silent), casts (`as` truncation is
silent), arithmetic on untrusted lengths, recursion, logical errors. Those
gaps are what the rest of this document addresses.

## 2. Cargo lint policy (existing foundation, proposed changes)

Today: `[workspace.lints.rust] unsafe_code = "deny"`, `missing_docs = "warn"`;
`[workspace.lints.clippy] all = { level = "deny" }`; crates opt in via
`[lints] workspace = true`; `oxpinyin-capi` and `pinyin-oracle` override
`unsafe_code = "allow"` locally; `oxpinyin-store` scopes `#![allow(unsafe_code)]`
to `lmdb.rs` and `tkrzw/*`. **Baseline `clippy --all-targets -D warnings` is
green (measured).**

Proposed additions (rationale per line; full matrix in
`enforcement-matrix.md`, diffs in `proposed-config-diffs.md`):

```toml
[workspace.lints.rust]
unsafe_code = "deny"
missing_docs = "warn"
unused_must_use = "deny"          # R.17.7: dropped Result/Option/must_use is a bug
unsafe_op_in_unsafe_fn = "deny"   # unsafe fns must re-state unsafe blocks (2024 ed.)

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
# curated pedantic, measured on this tree (606 pedantic warnings total):
cast_possible_truncation = "warn"   # 58 hits — the biggest real hazard class
cast_sign_loss = "warn"             # 14
cast_precision_loss = "warn"        # 35
must_use_candidate = "warn"         # 18 — grows #[must_use] coverage
missing_errors_doc = "warn"         # 82 — half of D.4.9/D.4.11
missing_panics_doc = "warn"
unreadable_literal = "warn"         # 24 — zero-cost readability
map_unwrap_or = "warn"              # 24
redundant_closure_for_method_calls = "warn"  # 29
```

Deliberately **not** enabled workspace-wide (measured noise, low value):
`doc_markdown` (98 hits — backtick pedantry), `borrow_as_ptr` (35 — the capi
handle pattern is intentional), `single_match_else`, `too_many_lines`,
`items_after_statements`, `float_cmp` (4 sites are documented bit-parity
ports; adding `#[allow]`s at each is worse than review), pedantic/nursery as
groups (606/201 warnings — see §5).

**`pedantic`/`nursery` verdict**: no group enable. 606 + 201 warnings of which
the top items are documentation cosmetics (`doc_markdown`,
`missing_errors_doc`, `redundant_pub_crate`, `missing_const_for_fn`). A staged
rollout is unnecessary when a 10-lint curated subset captures the safety value
(casts + must_use) at ~5% of the noise. Revisit when the Consortium's
safety-critical lints land in Clippy (rust-lang goal in flight) — that is the
moment a group flip becomes interesting.

## 3. `unsafe` policy: `deny` vs `forbid` vs per-crate allows

Semantics that matter:

- `deny(unsafe_code)` can be overridden by an inner `#![allow]` (exactly what
  capi/oracle/store do today).
- `forbid(unsafe_code)` **cannot** be overridden by inner attributes — the
  canonical "this crate is incapable of unsafe" statement. (`--cap-lints` and
  certain proc-macro expansion edge cases aside.)
- Workspace `[lints]` are inherited per crate via `[lints] workspace = true`
  and can be specialized per crate — already the pattern here.

**Today**: workspace `deny`; `oxpinyin-core` additionally carries crate-level
`#![forbid(unsafe_code)]` (lib.rs:4); corpus/emitter/data/lambda/counter/
segment carry redundant-but-harmless inner `#![deny]`; engine/user/store/dictool
rely on the workspace deny; structure.md says core=forbid, data=deny(+mmap),
others=deny — matching reality except that the *distinction* is only
documented, not uniformly mechanized.

**Proposal (ADOPT)**: move the distinction into Cargo so AGENTS.md prose isn't
load-bearing:

```toml
# oxpinyin-core/Cargo.toml
[lints.rust]
unsafe_code = "forbid"      # stronger than workspace deny; cannot be re-allowed
missing_docs = "workspace"  # hmm — see note
```

Cargo lint inheritance does not support "inherit one, override one" cleanly in
one table: a crate that sets `[lints.rust] unsafe_code` locally stops
inheriting the workspace rust lints unless it re-states them
(`missing_docs = "warn"`) — acceptable, three lines. Recommended per-carte:

| Crate | policy |
|---|---|
| oxpinyin-core, oxpinyin-engine, oxpinyin-user, oxpinyin-data, oxpinyin-segment, counter, lambda, emitter, corpus, dictool | `forbid` (core keeps its inner attribute as belt-and-braces; data currently has no unsafe and no mmap exception — if mmap arrives it needs its own scoped module like store's) |
| oxpinyin-capi, pinyin-oracle | `deny` + `// SAFETY:` per block (already 100%) + clippy `undocumented_unsafe_blocks`, `missing_safety_doc` denied |
| oxpinyin-store | `forbid` at crate level with module-scoped `#![allow(unsafe_code)]` in `lmdb.rs`/`tkrzw/*` (inversion of today's allow-inside-deny — shrinks the trusted region to named files) |

`forbid` applies to tests/benches/examples of the same crate too — verified
non-issue: those crates' test code contains no unsafe (fuzz is a separate
workspace; see finding F-8 — fuzz crate has no lints table at all).

**cargo-geiger (ADOPT SELECTIVELY, scheduled)**: first-party unsafe is already
inventoried by the lint structure; geiger's value is the *dependency* unsafe
inventory (Cargo.lock has 179 packages; `heed`/`cxx`/`libfuzzer-sys` carry
unsafe; `redb` is pure Rust). Run in a scheduled lane, keep the report as a
diff artifact; not a PR gate (geiger runtime ~minutes, occasional parser
bitrot — pin its version).

## 4. Panic and failure safety

*Measured inventory* (production library code = `src/` minus `#[cfg(test)]`
modules, `src/bin/`, benches, examples):

- `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`/`unreachable!`:
  **zero** in the library code of core/engine/data/user/store/segment; zero
  in capi's FFI entry points (the `expect("UTF-8 path")` hits are in
  `#[cfg(test)]` helpers).
- Two deliberate `assert_eq!`s in `core/src/parser.rs:281,399` guard
  count/enumerate correspondence and are commented as internal-bug trips;
  one `debug_assert!` in `core/src/cost.rs:70`.
- Panic-capable operations (indexing/slicing/division) are
  invariant-guarded throughout; the guards are documented at the load-bearing
  sites (e.g. the `consumed/anchor` law at `engine/src/session.rs:774`).
- FFI: every capi entry point wraps its body in `ffi_catch`
  (`catch_unwind` → fallback value) except five trivially-non-panicking
  ones (two scalar writers, three iterator-`end` functions) — audit F-7.

**Policy proposal (ADOPT)**: mechanize the current de-facto abstinence.

- `clippy::unwrap_used`, `expect_used`, `panic`, `indexing_slicing`,
  `string_slice`, `integer_division`(?): **REJECT** blanket use — measured
  objection: tests/benches legitimately use 900+ unwraps; `indexing_slicing`
  would flag thousands of invariant-guarded sites.
- Instead: apply `unwrap_used`/`expect_used`/`panic`/`panic_in_result_fn` as
  **deny only in the library crates' non-test code** via per-crate
  `[lints.clippy]` + `#![cfg_attr(not(test), deny(...))]`… — since Clippy
  lint levels can't distinguish test cfg from `[lints]`, the workable
  mechanism is `clippy.toml`-free per-crate attributes:
  `#![deny(clippy::unwrap_used)]` at the crate root (test modules inside
  src get `#[cfg_attr(test, allow(...))]` — or simply keep tests in
  `tests/`). Concretely: engine/user/store/data/segment/core add the four
  denies; their inline `#[cfg(test)]` modules add one `#![allow]` each with
  a justification comment. Cost: ~8 allow-attributes total, verified by CI.
- `assert!`/`debug_assert!` stay unrestricted (see §6); tests stay
  unrestricted.

This turns "Nothing panics on any input" (constitution §4) from prose into a
compile failure for the common cases, while the two parser asserts remain as
documented internal-bug trips (a legitimate MISRA-style deviation, recorded).

## 5. Clippy groups audit (measured)

| Group | Warnings | Dominant lints | Verdict |
|---|---|---|---|
| `all` (today: deny) | 0 | — | keep |
| `pedantic` | 606 | doc_markdown 98, missing_errors_doc 82, cast_possible_truncation 58, cast_precision_loss 35, borrow_as_ptr 35, redundant_closure_for_method_calls 29, unreadable_literal 24, map_unwrap_or 24, single_match_else 20, must_use_candidate 18, unnecessary_debug_formatting 15, cast_sign_loss 14, too_many_lines 10, format_push_string 10 | curated subset only |
| `nursery` | 201 | redundant_pub_crate 63, missing_const_for_fn 53, option_if_let_else 22, significant_drop_tightening 11, too_long_first_doc_paragraph 9 | none now; `significant_drop_tightening` worth a look in Stage-2 profiling, `redundant_pub_crate` as an informational R.8.7 signal |
| `restriction` | (not measured wholesale — by design) | — | never group-enable; individual members justified individually (`unwrap_used` etc. per §4) |

Notable single members:

- `clippy::undocumented_unsafe_blocks` + `clippy::missing_safety_doc` —
  mechanize the `// SAFETY:` constitution clause for capi/oracle/store.
  Today's 195/195 coverage becomes enforced instead of admired. ADOPT.
- `clippy::allow_attributes` — edition-2024 style (`#[unsafe(no_mangle)]`
  already used in capi). Optional, cosmetic.
- `clippy::exhaustive_enums`/`exhaustive_structs` — engine already uses
  `#[non_exhaustive]` where it matters (structure.md); enabling would fight
  internal crates. REJECT.

## 6. `panic = "abort"` (profile question)

What it does: replaces unwinding with immediate process abort on panic.
What it does **not** do: remove panic paths, make them safe, or convert them
to errors — it only changes the crash mode.

For oxpinyin specifically:

- The shipped artifact is a **cdylib loaded into an IME process**. A panic
  escaping `extern "C"` is UB only pre-1.81 semantics; modern rustc aborts at
  the boundary. But capi *wants* better than abort: `ffi_catch` converts
  panics into `false`/`NULL` fallbacks so a library bug degrades one
  keystroke instead of killing the user's input method. Setting
  `panic = "abort"` in `[profile.release]` would make `catch_unwind` dead —
  every would-be-caught panic aborts the host process. That is strictly
  **worse** for this product.
- Cargo profiles only apply to the final binary target being built; a library
  crate cannot impose abort on its consumers anyway — but since the workspace
  *builds* the cdylib here, the profile would take effect. Recommendation:
  **REJECT** for release profiles of capi-linked targets. Consider it only
  for standalone CLI bins (dictool/oracle tools) where crash-vs-unwind is
  immaterial — and even there the value is binary-size only.
- Do adopt the *documentation* that panic paths must not cross the ABI
  (already true via ffi_catch + the five trivial exceptions, F-7).

## 7. `no-panic` crate

Mechanism: `#[no_panic]` injects a reference to an undefined
`extern "C"` symbol into the annotated fn; if codegen keeps a panic path, the
link fails. So it detects *some* statically-visible panics at link time, in
optimized builds only (`opt-level > 0`; in debug the reference is optimized
away and everything passes — a silent false negative).

Limitations relevant here: no generics (monomorphization breaks the trick);
false negatives via inlining; false positives are rare but the lint
interacts badly with `#[inline]` consumers; maintenance is sporadic (0.1.x,
years between releases); MSRV fine. Compared against the alternatives: rustc
+ the §4 Clippy denies catch the *stylistic* panic sources statically;
fuzzing catches the *semantic* ones dynamically; `no_panic` sits between
with narrow coverage and a build-mode trap.

**Verdict: REJECT.** The workspace's zero-unwrap baseline plus the proposed
lints + fuzzing dominate it, without a link-time surprise machine.

## 8. Integer / arithmetic policy

Types on the hot paths: `Cost = i64` fixed-point (`core/src/lib.rs:53`);
counts `u32`/`u64`; λ as exact `u128` rationals (`data/src/table_conf.rs`);
f64 confined to the documented bit-parity segmenter and
`amplified_frequency`. The decode path is already systematically
saturating/checked (100+ sites: `cost.rs`, `scoring.rs`, `lm/mod.rs`
`interpolate_ratio` fully `checked_mul`, `user/store.rs` 16 saturating
sites) — upstream-libpinyin semantics are preserved by design.

Classification of the remaining raw arithmetic (from the audit):

1. **Overflow impossible by construction** — the overwhelming majority
   (loop-bounded indices, `MAX_GRAPH_INPUT = 65_535` arenas,
   prefix-sum invariants). No action.
2. **Overflow indicates a bug** — the two `assert_eq!`s in parser.rs;
   keep, documented.
3. **Intentional wrapping** — none found.
4. **Saturation semantically correct** — everywhere scores/counts merge;
   already saturating.
5. **Checked required** — `interpolate_ratio` (done right); **two fixture
   exceptions filed as F-1/F-2** (`fixture.rs:309` unchecked `+=`;
   `fixture.rs:364` unchecked u128 products) — the production LM path was
   converted but the fixture twin was not.
6. **Requires investigation** — `initials.rs:205` `slot_shift` underflow
   (caller-bounded at 25; add the `debug_assert`), `PreeditSpan::len`
   (unconstructible inverted spans; assert if construction ever goes pub).

FFI conversions: capi uses `try_from(...).unwrap_or(MAX)` clamps at the
widening seams; three bare `as` casts remain (`candidates.rs:33` len→u32,
`:372` usize→c_int — the last truncates only past 2 GiB input, unreachable
via C strings; `parse.rs:320` c_char→u8 wrap, benign) — audit F-4.

**`overflow-checks = true` in release (profile question)**: for the shipped
IME — REJECT as default-on (costs real decode bandwidth for a class the
design already avoids; contrary to Stage-2 RAM/speed goals). ADOPT as a
**CI lane**: one scheduled `RUSTFLAGS="-C overflow-checks -C debug-assertions"
cargo test --workspace --release` run to surface latent overflow under
optimized codegen (different inlining exposes different bugs). This is the
"NDEBUG" discussion done right: debug assertions and overflow checks are
test-time amplifiers, not shipping configuration.

## 9. `#[must_use]`

Today: 341 attributes (core 106, engine 66, user 32, data 28, segment 18,
store 0). All `Result`/`Option` returns are `must_use` by type. Gaps:
`data/content.rs` accessors, `user/codec.rs` encode/decode free functions,
all of `oxpinyin-store`. Proposal (ADOPT): `rust::unused_must_use = "deny"`
(hard error on ignored must_use results — currently warn→error only under
CI's `-D warnings`), plus `clippy::must_use_candidate = "warn"` (18 hits) to
grow coverage mechanically; fix the three gap files. This is MISRA R.17.7's
Rust-native form. Builders/iterators/setters stay exempt by convention
(Clippy's candidate lint already models most of the exception list).

## 10. Debug vs release assertions

Rust's real model: `debug_assert!` and overflow checks follow
`debug-assertions` (dev/test on, release off); `assert!` is always on. The
project already uses this correctly (one `debug_assert!` on a provable
precondition, two release `assert_eq!` bug-trips). Policy worth writing down
in the profile: FFI input validation must be `assert!`/`Result` (always on —
it's the trust boundary); deep internal invariants that cost cycles may be
`debug_assert!`; nothing safety-relevant may exist *only* as a debug check
unless the safe-Rust invariant makes its violation impossible. The audit
found exactly zero violations of this policy.

## 11. rustfmt

Today: **no rustfmt.toml** — pure stable defaults — and `cargo fmt --all
--check` is already a CI job (plus the fuzz workspace). Verdict: this is the
minimal useful configuration, and it is already in place. Do **not** adopt
`imports_granularity`/`group_imports`/`wrap_comments`: they are
nightly-only rustfmt options; enabling them would force a nightly fmt in CI
(fmt --check would disagree with stable rustfmt output), create version-pinned
churn, and buy cosmetics. Formatting is not a safety claim; the existing
gate is the right one. (A `rustfmt.toml` containing only `edition` is
tolerable if ever needed; currently unnecessary since edition is inferred.)

## 12. Lizard (run performed)

Lizard 1.24.0 over `crates/` + `fuzz/`: **46,195 NLOC, 2,283 functions, avg
CCN 2.5, avg NLOC/fn 15.4, 26 functions above CCN 15** (function risk rate
1.1%, NLOC risk rate 6%). Production-only CCN>15 outliers:

| CCN | NLOC | Location |
|---|---|---|
| 38 | 103 | `user/store.rs:555` `add_phrase_in` |
| 33 | 91 | `user/store.rs:928` `mask_out` |
| 31 | 87 | `user/store.rs:1028` `remove_user_phrase` |
| 22 | 125 | `engine/nbest.rs:325` `nbest_sentences` |
| 22 | 72 | `user/store.rs:666` `promote_addon_phrase` |
| 19 | 83 | `capi/sentence.rs:324` `pinyin_guess_candidates` |
| 19 | 102 | `dictool/format.rs:84` `parse` |
| 19 | 77 | `oracle/sentence_tail.rs:306` `measure` |
| 18 | 50 | `core/parser.rs:118` `parse_input` |
| 18 | 42 | `oracle/capture.rs:155` `to_observation` |
| 17 | 108 | `core/scheme.rs:1719` `parse` |
| 17 | 62 | `data/content.rs:233` `decode_tokens` |
| 16 | 40 | `data/table_conf.rs:81` `from_decimal` |
| 16 | 124 | `engine/session.rs:2167` `build_scan_matrix` |
| 16 | 53 | `dictool/import.rs:111` `run` |

Classification: the `user/store.rs` quartet is the genuine refactoring
candidate (stateful persistence logic, mutex-adjacent, the crate where all
four >20 CCN functions live — also the natural coverage/mutation priority);
`parse`/`decode_tokens`/`from_decimal` are legitimate table-driven parser
complexity (data-format ports); `nbest_sentences`/`build_scan_matrix` are
the parity-critical decoder (refactoring risks divergence from upstream —
explicitly *not* worth it pre-Stage-2); oracle/dictool are tooling.

**Thresholds**: informational now; propose `-Tlimit 40` (no function exceeds
CCN 40) as a *scheduled-report* gate with ratchet, never a PR blocker.
Lizard as a safety proof: REJECT the claim, keep the signal.

## 13. Miri

Detects (by interpreting, not executing): UB in unsafe Rust — pointer
provenance violations, misalignment, out-of-bounds, data races in some
cases, uninit reads, invalid mem::forget-style leaks under `-Zmiri-track-raw`
… Cannot run foreign C/C++ (libpinyin, tkrzw, LMDB) and has partial std
support (file I/O needs isolation flags). For this workspace:

- First-party unsafe is FFI-dominated → largely out of Miri's reach.
- Reachable valuable targets: `oxpinyin-store` default (redb, pure Rust)
  tests; the safe-core fuzz corpus replayed under Miri (parser/scheme/
  codec are pure functions — ideal); `oxpinyin-core` graph/k-best invariants.
- Cost: nightly-only, ~20–100× slowdown, scheduled lane only.

**Verdict: ADOPT SELECTIVELY (scheduled)** — `cargo +nightly miri test -p
oxpinyin-core -p oxpinyin-store` plus corpus replay; never claims of global
UB-freedom (it can't see the C side; that's the ABI smoke gate's job).

## 14. cargo-audit vs cargo-deny

`cargo-deny` covers advisories (same RustSec DB cargo-audit uses), licenses,
sources, and duplicate versions in one config file (`deny.toml`) with a
`--hide-inclusion-graph` fast path suitable for PRs. `cargo-audit` is the
focused vulnerability scanner. Running both in CI is redundant: same
advisory DB, same findings.

**Verdict: ADOPT cargo-deny** as the single CI gate (advisories: deny
vulnerabilities, warn unmaintained with expiry-dated ignores; licenses:
GPL-3.0-or-later + permissive allow-list matching the current 179-package
lockfile; sources: crates.io + github only; duplicates: warn). **cargo-audit:
DEFER** to a documented local-dev convenience (`cargo audit` in README), not
CI. Rationale: one policy file, one source of truth for the deviation list.

## 15. cargo-nextest

Benefits: per-test process isolation (a panicking/corrupting test can't
poison others), better retry/failed-first ergonomics, faster via parallel
scheduling, first-class JUnit/coverage integration (`cargo llvm-cov
nextest`). Semantics caveats vs `cargo test`: doctests are *not* run by
nextest (keep a `cargo test --doc` step); test binary cwd and env behavior
matches cargo's (package-root cwd) — the workspace's fixture-relative tests
(`tests/parity`, oracle pins) rely on cwd — verify in the trial; tests that
depend on shared mutable global state would now genuinely isolate (none
found — oracle's process-wide lock is per-process by design).

**Verdict: ADOPT** for unit/integration lanes in CI with a doctest step;
keep `cargo test --workspace` as the portable-mac/win jobs' runner until
nextest is validated there (it's a prebuilt binary — cheap to try). Not a
safety tool per se; a CI-efficiency tool that makes *more* testing
affordable.

## 16. cargo-fuzz (existing: one target)

Today: `fuzz/fuzz_targets/parser.rs` (16 lines, safe Rust over
`oxpinyin-core`), pinned nightly-2026-08-01 + cargo-fuzz 0.13.2, 10s PR
smoke run. libFuzzer finds: panics, aborts, timeouts, OOM — i.e., exactly
constitution §4 violations and parser boundary bugs (e.g. the F-3 class:
untrusted bytes → decode paths).

Highest-value additional targets (from the audit's trust boundaries):

1. `data/content.rs` decode — arbitrary bytes as `.text`/`.bin` tables; the
   F-3 finding lives here; regression seed corpus from `fixtures/`.
2. `core/scheme.rs` + double-pinyin tables — config-driven parsing.
3. `user/codec.rs` roundtrip + arbitrary bytes (user DB robustness).
4. capi's `pinyin_parse_more_*` byte-string entry via the safe wrapper
   (catches length/conversion issues without spawning the FFI).
5. `segment` trellis on arbitrary token streams (training-side hardening).

Scheduling: keep ≤60s aggregate smoke on PRs; nightly 10–30 min runs with
committed corpus (`fuzz/corpus/`), ASan+no-detach. **Verdict: ADOPT
(expand)**. Fuzzing proves nothing; it finds things — state that in the
profile.

## 17. Coverage: cargo-llvm-cov vs cargo-tarpaulin

llvm-cov: region/line coverage on stable, Linux+macOS+Windows, nextest
integration, fast (reuses one instrumented build). tarpaulin: Linux-only,
historically slower and version-sensitive with proc-macro heavy trees.
**Verdict: ADOPT llvm-cov** (scheduled + per-PR optional report, never a
threshold gate initially). Coverage here is a *test-visibility* metric: the
interesting question is which safety-sensitive modules have low coverage —
candidates from the audit: `user/store.rs` quartet (mutations priority),
`capi` error paths behind `ffi_catch` (the five unwrapped entries), oracle
`live.rs` abort-guards. No numeric target until a baseline exists.

## 18. cargo-mutants

Mutates source (flip comparisons, delete statements) and checks which
mutations survive the test suite — evidence about *test strength*, catching
the "line covered but assertion-free" false confidence that line coverage
gives. Cost: dominates everything else here (each mutant ≈ one incremental
test run); the full workspace is impractical. Scoped to `oxpinyin-core`
(parser/scheme/scoring) + `oxpinyin-user/src/store.rs` it is a credible
nightly lane (~1–3h). **Verdict: TRIAL (scheduled, scoped)** — produce a
score, decide ratchets only after seeing the distribution.

## 19. Kani

Bounded model checker: symbolic execution of Rust to prove assertions/
overflow/panics absent for *all* inputs within harness bounds. Ships its own
toolchain bundle (no workspace MSRV impact; separate install in CI).
Loop-heavy symbolic runs explode; the win is small pure functions with
numeric invariants. Concrete candidates grounded in this codebase:

1. `core/src/cost.rs` — `reduce_ratio`/`log2_fixed`/`cost`: prove the
   shift-loop postconditions (both halves < 2^63) that today justify
   `unwrap_or(u64::MAX)`; prove no overflow in `saturating` composition.
2. `data/src/table_conf.rs` — λ parsing/gcd: prove `gcd ≥ 1` and exact
   rational invariants (pairs with the div-by-gcd sites).
3. `data/src/content.rs` `decode_tokens` — prove the length guards entail
   in-bounds slices for *all* byte inputs (regression-proof version of F-3's
   fix class).
4. `core/src/graph.rs` — starts-array monotonicity lemma feeding
   `starts[node+1]` indexing safety.

**Verdict: TRIAL (scheduled lane, 3–4 harnesses)**; whole-program
verification REJECTed by scope. Harnesses double as `debug_assert!`
documentation.

## 20. Prusti

Contract-based verifier (pre/post/loop invariants) with a dedicated rustc
fork; in practice it trails stable rustc by months, supports a language
subset, and the annotation burden is real (its own docs position it as
research). Pinned 1.97.1 + edition-2024 + this codebase's heavy
iterator/generic style is far from its sweet spot today; the Consortium
ecosystem (Ferrocene/FLS, SCRC lints) is the more alive thread.

**Verdict: REJECT today** (revisit on a trigger: stable-toolchain Prusti
release or SCRC endorsement). Do not adopt a verification tool because it is
theoretically interesting.

## 21. rust-analyzer / IDE

A `.vscode/settings.json` with `"rust-analyzer.check.command": "clippy"`
makes the editor surface the same failures CI enforces (clippy::all denied)
— the human/agent parity this study aims for. Inlay hints (types, parameter
names, chaining) are personal preference; borrow/lifetime overlays help in
the FFI crates. `.lsp.json` presence in a checkout is tool noise, not
project config. **Verdict: ADOPT (one committed settings file, explicitly
not CI-enforced).**

## 22. Pre-commit / local workflow

Existing: `.githooks/commit-msg` → trailer linter (fast, single source of
truth with CI). Proposing a `pre-commit` that runs `cargo fmt --check` on
staged `.rs` files only (~0.3s) plus the existing hook. Full `clippy
--all-targets` (≈ minutes cold) as pre-commit would get `--no-verify`'d into
oblivion — hooks are a convenience, not a boundary; CI stays authoritative.
**Verdict: ADOPT (fmt-only pre-commit, documented as bypassable).**

## 23. Toolchain / MSRV impact summary

| Tool | Channel | MSRV effect | CI install cost |
|---|---|---|---|
| clippy/rustfmt lints (proposed) | pinned stable | none | none (components already declared) |
| cargo-deny | stable binary | none | ~20s download |
| cargo-nextest | stable binary | none | ~10s download |
| cargo-llvm-cov | stable binary | none | ~30s |
| cargo-fuzz (exists) | pinned nightly | none (separate workspace) | already wired |
| Miri | nightly (component) | none | lane-only |
| cargo-mutants | stable | none | ~20s |
| Kani | bundled toolchain | none | ~2 min (installer) |
| cargo-geiger | stable | none | ~20s |
| Prusti | custom rustc | — | REJECTed |
| no-panic | crate dep | none | REJECTed (would be a new dep — needs ask) |

No proposal raises the workspace MSRV (1.97.1) or touches
`rust-toolchain.toml`. Everything heavy lives in scheduled lanes on
ephemeral runners.

## 24. Comparison table (verdict recap)

| Tool | Static/dynamic/formal | Detects | Does not | FP risk | CI cost | Verdict |
|---|---|---|---|---|---|---|
| rustc + `-D warnings` | static | the Table-3 guarantees, dead/unreachable/deprecated | panics, logic | ~0 | free | ADOPT (already) |
| Cargo `[lints]` | static | unsafe policy, must_use, lint levels | behavior | ~0 | free | ADOPT (extend) |
| Clippy (curated) | static | cast/panic/must_use/doc classes | logic | low | ~1 min | ADOPT |
| Clippy pedantic/nursery groups | static | style+docs | — | high (606/201) | ~1 min | REJECT as groups |
| rustfmt | — | formatting drift | everything else | 0 | seconds | ADOPT (already) |
| cargo-deny | static | advisories/licenses/sources/dups | code defects | low | ~1 min | ADOPT |
| cargo-audit | static | RustSec | licenses/sources | low | ~40s | DEFER (redundant w/ deny) |
| cargo-nextest | runtime infra | — (isolation/reporting) | — | 0 | neutral | ADOPT |
| cargo-fuzz | dynamic | panics/aborts/OOM/timeouts on parsed inputs | correctness | low | 10s PR / 30m nightly | ADOPT (expand) |
| Miri | dynamic (interp.) | UB in reachable Rust | C-side UB | low | nightly lane | ADOPT SELECTIVELY |
| cargo-llvm-cov | measurement | coverage gaps | correctness | 0 | +3–5 min | ADOPT (report-only) |
| cargo-mutants | dynamic | weak-assertion tests | — | med | hours | TRIAL (scoped) |
| Lizard | static metric | complexity outliers | safety | 0 | seconds | ADOPT SELECTIVELY (report) |
| cargo-geiger | static inventory | unsafe in deps | reachability/safety | low | ~2 min | ADOPT SELECTIVELY |
| Kani | formal | numeric/bounds proofs, panic-freedom (bounded) | everything outside harness | low | lane (min–h) | TRIAL |
| Prusti | formal | contracts | — | — | high | REJECT |
| no-panic | link-time | some panic paths (opt builds) | debug-mode paths, generics | med | small | REJECT |
| `panic = "abort"` | runtime policy | (crash mode) | panic existence | — | free | REJECT (kills ffi_catch) |
| release `overflow-checks` | runtime policy | latent overflow | — | 0 | one lane | ADOPT (CI lane only) |
