# MISRA C:2025 Addendum 6 — Rust applicability mapping for oxpinyin

Status: proposal (study output). Nothing here claims MISRA compliance for
oxpinyin. The working terminology is a **"MISRA C:2025 Addendum 6-derived Rust
safety profile"** — a set of enforceable rules inspired by the Addendum's
applicability assessment, expressed in idiomatic Rust tooling.

## Sources and method

- **Primary source (retrieved and fully parsed):** MISRA C:2025 Addendum 6,
  *Applicability of MISRA C:2025 to the Rust Programming Language*, March
  2025, ISBN 978-1-911700-22-7, 9 pages — downloaded from
  `https://misra.org.uk/app/uploads/2025/03/MISRA-C-2025-ADD6.pdf` (note
  `app/uploads`, not `apploads`; the latter path 404s/WAF-blocks). Its
  §3 "Rust Cross Reference" table — all 223 guidelines × (C status,
  decidability, scope, rationale, two applicability columns, Rust-adjusted
  category, comment) — was extracted with `pdftotext -layout` and parsed
  programmatically; the classification columns are reproduced verbatim as
  facts in `misra-add6-table.csv` (comments are *not* copied — the document
  is © The MISRA Consortium Limited, all rights reserved; quote-short only).
- **Independent cross-check:** the Safety-Critical Rust Consortium's
  cross-reference (Apache-2.0), which re-buckets ADD6's two-column model
  into three tables. Programmatic set-comparison confirms the buckets used
  here equal the SCRC tables exactly (see derivation rule below).
- Also consulted: the exploratory 223-guideline MISRA→FLS mapping in
  `rust-lang/goals src/2026/safety-critical-lints-in-clippy.md`.
- Guideline *requirement text* is paraphrased at theme level; the normative
  text lives in MISRA C:2025 (paywalled). ADD6 itself does not restate the
  C requirement texts — it is purely an applicability matrix. Comments
  quoted in the tables below are ADD6's own (via the SCRC copy unless
  marked). Where a gist is uncertain it is deliberately generic rather
  than invented.

### The Addendum's own classification model

ADD6 rates each guideline on *two independent applicability axes*:

1. **Rust in general** — "including safe, unsafe, and foreign function
   interfaces";
2. **Safe Rust** — "excluding unsafe and extern".

each as **Yes / No / Partial**, and assigns a **Rust-adjusted category**:
Required, Advisory, **Disapplied** ("compliance is not required … any
non-compliance may be disregarded"), or N/A — with the explicit note that
*Mandatory is never used for Rust*. It also carries per-guideline
**rationale codes** (UB / IDB / CQ / DC), **decidability**
(Decidable/Undecidable) and **scope** (STU/System) from MISRA C:2025.

The three buckets used throughout this document are *derived* from the two
columns by exact rule, and the derivation was verified row-by-row against
the SCRC tables (set equality on all three):

| Bucket | Derivation from ADD6 columns | Count |
|---|---|---|
| Table 1 (applies to safe Rust) | Safe-Rust = **Yes** | 61 (16 D + 45 R) |
| Table 2 (unsafe/FFI-dependent) | general ≠ No **and** Safe-Rust ≠ Yes | 68 (3 D + 65 R) |
| Table 3 (not applicable) | general = **No** | 94 (3 D + 91 R) |
| **Total** | | **223** |

Verified distributions (from the parsed table, see the CSV):
Safe-Rust column = 61 Yes / **18 Partial** / 144 No; Rust category =
82 Required / 42 Advisory / **5 Disapplied** / 94 n/a.

### Applicability totals

| Table | Meaning | Directives | Rules | Total |
|---|---|---|---|---|
| 1 | Applicable to Rust in general (safe Rust) | 16 | 45 | 61 |
| 2 | Applicable only in the presence of `unsafe` | 3 | 65 | 68 |
| 3 | Not applicable to Rust | 3 | 91 | 94 |
| | **Total** | **22** | **201** | **223** |

Roughly: for oxpinyin — whose shipped surface is overwhelmingly safe Rust with
a thin, audited FFI layer — **42% (94) of MISRA C:2025's guidelines are moot
by language design** (Table 3), **30% (68) apply only where unsafe/FFI code
exists and there collapses onto three crates** (`oxpinyin-capi`,
`pinyin-oracle`, `oxpinyin-store` behind features — Table 2 guidance stays
live for exactly those), and **27% (61) is the safe-Rust residue of Table 1**,
most of which safe Rust + the existing workspace lints already cover.

## Classification scheme

- **A** — automatically guaranteed by safe Rust / rustc
- **B** — enforceable by rustc / Cargo lint configuration
- **C** — enforceable by Clippy
- **D** — enforceable by another existing tool (fuzz, Miri, Kani, geiger, …)
- **E** — partially enforceable (tool catches a subset; review covers the rest)
- **F** — requires custom static analysis
- **G** — requires dynamic testing
- **H** — requires formal verification
- **I** — human review / process only
- **J** — not applicable to Rust (per ADD6)
- **K** — only relevant to unsafe Rust / FFI
- **L** — not appropriate to transfer to Rust

"oxpinyin today" statements were verified against the working tree at commit
`2382bdd` (audit details in `oxpinyin-audit.md`).

### Finding: the 18 "Partial for safe Rust" guidelines

The SCRC three-bucket view flattens ADD6's most decision-relevant nuance:
guidelines whose concern **partially survives in safe Rust** (Safe-Rust
column = Partial). For oxpinyin these are precisely "safe Rust is not
enough" — and the list is dominated by the cast/conversion family, which
independently confirms the profile's cast-lint choice:

> D.5.1 (concurrency), R.1.1, R.5.1, R.5.5, R.5.10 (identifiers / FFI
> names), R.8.17 (alignment), R.10.5, R.10.8, R.11.1, R.11.8, R.12.2
> (**the cast/transmute family**), R.14.1 (loop termination), R.20.4,
> R.20.7 (macro hygiene), R.22.11, R.22.14, R.22.15, R.22.20 (resource
> ordering that can involve FFI).

### Finding: Rust-adjusted categories differ from the C categories in 48 cases

ADD6 re-categorizes for Rust rather than inheriting MISRA C's severity:
5 **Disapplied** (R.15.5, R.17.8, R.22.8, R.22.9, R.22.10 — applicable
in principle, compliance explicitly not required), 16 **Mandatory→Required**
(the Rust column never uses Mandatory — no deviation-proof rules exist for
Rust), 6 **Advisory→Required upgrades** (D.1.2 unstable features, D.4.2,
D.4.13, R.11.4, R.11.5, R.11.11), and 21 **Required→Advisory downgrades**
(D.4.3, D.4.7, D.4.11, D.4.12, R.2.1, R.3.1, R.5.3, R.5.5, R.5.6, R.5.8,
R.7.1, R.7.2, R.8.5, R.10.8, R.11.6, R.12.2, R.13.1, R.13.5, R.15.7,
R.20.7, R.5.10). The "MISRA cat" column in the tables below shows the
**C** category; where ADD6's Rust-adjusted category differs it matters
only for deviation bookkeeping, not for the enforcement mapping this
study proposes.

Rationale-code distribution (parsed): Table 1 = 24 UB / 34 DC / 2 IDB /
1 CQ (+16 directives n/a); Table 2 = 49 UB / 15 DC / 3 IDB / 1 CQ;
Table 3 = 41 UB / 50 DC / 3 IDB. The unsafe-only table is UB-dominated —
exactly why its enforcement collapses onto the SAFETY-comment + review +
Miri machinery, while Table 1's DC/CQ mass is what lints and docs cover.

---

## Table 1 — Guidelines applicable to safe Rust (61)

Enforcement abbreviations: workspace = existing `[workspace.lints]`;
CI = `.github/workflows/ci.yml`; proposals are specified in
`oxpinyin-safety-profile.md` and costed in `enforcement-matrix.md`.

### Directives (16)

| ID | Theme (gist) | MISRA cat | Class | oxpinyin: today → proposed |
|---|---|---|---|---|
| D.1.1 | Requirements traceability of code | Required | I | Process only. Kiro specs + review; no mechanical home (nor should there be). |
| D.1.2 | Restricted language subset; unstable features need justification | Advisory | B | Stable toolchain pinned (`rust-toolchain.toml` 1.97.1); `#![feature]` fails to compile on stable → mechanically excluded today. Keep. |
| D.2.1 | Build/toolchain discipline (reproducible builds) | Required | E | `--locked` in CI + committed `Cargo.lock` (179 pkgs) + pinned toolchain. Remaining: none material. |
| D.3.1 | Source documentation | Required | E | `missing_docs = "warn"` workspace-wide; `// SAFETY:` discipline on 195/195 unsafe blocks (audited). Proposal: `missing_docs = "deny"` for the supported surface (`engine`, `capi`) after cleanup. |
| D.4.1 | Run-time failures minimized | Required | E | Constitution §4. Today: zero `unwrap`/`expect`/`panic!` in library production code (audited); `ffi_catch` at the ABI. Proposal: `clippy::unwrap_used`/`expect_used`/`panic`/`indexing_slicing` as **deny for lib targets** via per-crate `[lints]`, allowed in `#[cfg(test)]`/benches/bins. |
| D.4.4 | Conditional compilation used deliberately | Advisory | E | Feature surface is tiny (`lmdb`, `tkrzw`, `oracle-ffi`); no `cfg(target_os)` in portable crates (structure.md). `unexpected_cfgs` lint is stable (1.80+) and already active via `-D warnings`. Review residual. |
| D.4.5 | Documented (not ambiguous) constructs | Advisory | I | Review only; ADD6 comment: "ambiguity is determined by the project". |
| D.4.7 | No in-band error signals | Required | A/E | Type system: fallible APIs return `Result`/`Option` throughout (constitution §4). Clippy `result_unit_err` (in `all`) already denied. |
| D.4.9 | Function design (single-purpose, documented pre/post) | Advisory | I | Review; `missing_errors_doc` (82 pedantic hits) is the mechanical half → propose warn. |
| D.4.11 | Module boundaries / interface design | Required | E | `missing_docs` warn; structure.md crate map; `redundant_pub_crate` (63 nursery hits) as the pub-minimization signal → TRIAL at warn. |
| D.4.12 | No hidden conversions/sharing at interfaces | Required | E | Ownership/move semantics make accidental aliasing impossible; review for semantic (not mechanical) sharing. |
| D.4.13 | Data/ordering guarantees expressed in types | Advisory | A | ADD6: "many Rust APIs use the type system to enforce ordering". Exemplified here by `Session<'oracle>` borrow lifetimes in `pinyin-oracle`. No action. |
| D.4.14 | Validate data crossing trust boundaries | Required | E/G | Decode paths are `Result`-returning and fuzzed (`parser` target). Audit found one gap class: `content.rs:367` trusts a header count for `with_capacity` — fix filed as F-3. Expand fuzz targets (see ci-strategy). |
| D.4.15 | Floating-point implementation understood | Required | E | Engine is fixed-point by design (`Cost = i64`, u128 rationals); the two f64 sites (segmenter trellis, `amplified_frequency`) are documented bit-parity ports. `clippy::float_cmp` fires 4× (pedantic) → warn. |
| D.5.2 | Pointer use only where unavoidable | Required | A/K | Safe crates: zero raw pointers (audited). FFI crates: reviewed pattern inventory (audit §unsafe). |
| D.5.3 | Concurrency discipline | Required | A/K | Send/Sync statically derived; only manual impls are `unsafe impl Send/Sync for ffi::Db` (store/tkrzw, justified). Oracle holds a process-wide lock before entering non-reentrant libpinyin. |

### Rules (45)

| ID | Theme (gist) | MISRA cat | Class | oxpinyin: today → proposed |
|---|---|---|---|---|
| R.1.3 | No undefined/critical unspecified behaviour | Required | A/K | Safe Rust: guaranteed. Unsafe: 100% SAFETY-commented; propose Miri lane for `store` default feature. |
| R.1.5 | Avoid deprecated language/library features | Required | B | rustc `deprecated` warn-by-default; CI `RUSTFLAGS: -D warnings` makes it a hard error. ✓ already mechanical. ADD6: "this applies to deprecated APIs". |
| R.2.1 | No unreachable code | Required | B | rustc `unreachable_code` (warn → error via `-D warnings`). ✓ |
| R.2.2 | No dead code | Required | B | rustc `dead_code`. ✓ (same mechanism) |
| R.2.3 | No unused type/variable declarations | Advisory | B | rustc `dead_code`/`unused_variables`. ✓ |
| R.2.5 | No unused label declarations | Advisory | B | Labels are statement-scoped and linted (`unused_labels`). ✓ |
| R.2.6 | Unused declarations (type-level) | Advisory | B | rustc dead-code family. ✓ |
| R.2.7 | Unused function parameters | Advisory | B | rustc `unused_variables` covers `let`-like params (underscore convention). ✓ |
| R.2.8 | No unused storage/objects | Advisory | B | Same family. ✓ |
| R.3.1 | Comment structure (nested comments) | Required | J | ADD6: "nested comments are fully supported" — Rust block comments nest by definition; the C hazard cannot arise. No action. |
| R.5.2 | Identifiers in scope distinct enough / length policy | Required | I | No character limit in Rust; namespaces exist. Project decision: keep rustfmt defaults; review. |
| R.5.3 | No identifier shadowing in nested scopes | Required | E | rustc allows shadowing (idiomatic). Clippy `shadow_unrelated` exists (restriction) — noisy; propose review-only, not lint. SCRC keeps a rule here (their gui rule). |
| R.5.6 | Related identifiers not reused for unrelated meanings | Required | A | ADD6: "the proper module system makes surprise name conflicts much less likely". Review residual. |
| R.5.8 | No identifier reuse across namespaces in same TU | Required | A | Module paths disambiguate. ✓ |
| R.5.9 | No identifier reuse across TUs | Advisory | A | Crate-qualified paths. ✓ |
| R.7.1 | Octal-style literal confusion | Required | A | ADD6: "Rust octals have a distinct prefix"; no implicit octal exists. Guaranteed. |
| R.7.2 | Literal suffix policy / representability | Required | B | ADD6: "this is an error by default but can be enabled" — `overflowing_literals` is rustc deny-by-default. ✓ |
| R.8.7 | Functions/objects used across TUs declared once | Advisory | E | Rust analogue: minimize `pub`. `redundant_pub_crate` (63 hits) → TRIAL warn; `unreachable_pub` exists for never-pub'd items → propose warn. |
| R.8.9 | Block-scope function declarations | Advisory | E | Nested `fn` items are inert (no capture); style matter. Review. |
| R.8.13 | `mut` only where necessary | Advisory | B | rustc `unused_mut` warn → error under `-D warnings`. ADD6: "mut should be avoided unless necessary". ✓ |
| R.9.1 | No reads of uninitialized storage | Mandatory | A/K | Definite-initialization enforced by rustc (ADD6: "enforced by rustc but can be bypassed by unsafe"); unsafe paths audited; `MaybeUninit` absent from all crates. |
| R.9.4 | Initialization correctness (enum/aggregate) | Required | A | ADD6: "enforced by rustc". Invalid enum discriminants unconstructible in safe Rust. |
| R.11.3 | No casts between unrelated object pointer types | Required | K | Safe crates: zero pointer casts (audited). FFI crates: `.cast::<T>()` on opaque handles is the pattern (35 `borrow_as_ptr` pedantic hits) — keep review + `missing_safety_doc`. |
| R.11.4 | Pointer casts used only for provenance-preserving views | Advisory | K | Same surface as R.11.3; `candidate_ref` deliberately avoids `offset_from` on foreign pointers (good). |
| R.11.11 | Pointer casts through compatible types only | Advisory | A | ADD6: "enforced by rustc" (type-checked `as`/`cast`). |
| R.12.1 | Operator precedence made explicit | Advisory | C | Clippy precedence/`eq_op` family — inside `clippy::all` (denied). ✓ |
| R.13.1 | Side effects ordered deterministically | Required | A | ADD6: "order of evaluation is strict in Rust". Guaranteed. |
| R.13.5 | Right operand of `&&`/`\|\|` side-effect-free | Required | E | Short-circuit is defined, but side effects in the operand remain possible (C concern survives). Review; no lint worth enabling. |
| R.14.3 | Controlling expressions essentially boolean | Required | A | `if`/`while` require `bool`; `!`-on-integral impossible. Guaranteed. |
| R.14.4 | Controlling expression not constant-non-boolean | Required | A | ADD6: "enforced by rustc". |
| R.15.4 | Switch default handling | Advisory | J | Rust `match` on uninhabited/total enums needs no default; `Option`/`Result` force arms. The C hazard is structurally absent. |
| R.15.5 | Single break per switch-block | Advisory | J | Match arms don't fall through. Absent. |
| R.15.7 | if/else-if chains terminated with else | Required | E | Rust analogue: exhaustive `match` preferred. `clippy::match_like_matches`-adjacent guidance only; review. |
| R.17.2 | Recursion bounded/eliminated | Required | E/G | No stable lint (known Clippy gap; SCRC rule exists). Audit: 4 recursive sites, all depth-bounded by `MAX_PHRASE_LENGTH = 16` / nesting cap 2 — documented. Proposal: comment-anchored review rule + optional Kani harness. |
| R.17.7 | Value returned by non-void function shall be used | Required | B/C | `Result`/`Option` are `#[must_use]` by type; 341 explicit `#[must_use]` present. Gaps: `store` (0), `data/content.rs` accessors, `user/codec.rs`. Proposal: `unused_must_use = "deny"` workspace + `must_use_candidate = "warn"` (18 hits) + fix the three gap files. |
| R.17.8 | No modification of function parameters copied in | Advisory | A | ADD6: "this cannot be done accidentally without declaring parameters mut". |
| R.17.11 | Non-returning functions typed as such | Advisory | A | The `!` Never type, enforced by rustc (T2's R.17.9 is the unsafe-side twin). |
| R.18.3 | No relational comparison of unrelated pointers | Required | K | Only in FFI crates; audit found none (candidate identity uses `ptr::eq`, which is exactly the compliant tool). |
| R.18.5 | Pointer arithmetic within bounds only | Advisory | K | capi/oracle arithmetic is confined to `ptr::copy_nonoverlapping` with computed lengths; audit clean. |
| R.19.2 | Overlapping storage only for packing (unions) | Advisory | K | Rust unions are unsafe-only; zero unions in the workspace. Guaranteed for safe crates. |
| R.19.3 | Union member access discipline (new in C:2025) | Required | K | Same: `unsafe` union access cannot appear in safe crates. |
| R.21.25 | Std-lib usage discipline (C:2025 addition) | Required | I | Generic stdlib discipline; no Rust-unsafe analogue identified beyond review. |
| R.22.13 | Resource/stdio discipline (streams) | Required | E | Files are `std::fs` + `Result`; no `unsafe` FILE plumbing outside oracle (which pins upstream semantics). |
| R.22.18/22.19 | Stream/state resource ordering | Required | E | Ownership/Drop ordering replaces FILE discipline; audit found matched acquire/release pairs on all FFI paths (import/export iterators, glib strings). |

## Table 2 — Guidelines applicable only in the presence of `unsafe` (68)

These bind exactly three places in oxpinyin: `oxpinyin-capi` (151 unsafe
blocks + 6 `unsafe fn` + 4 `unsafe extern` blocks; 55 exported symbols),
`pinyin-oracle` (39 blocks + 2 extern blocks), and `oxpinyin-store` behind the
`lmdb`/`tkrzw` features (5 blocks + 2 `unsafe impl` + 1 extern block +
cxx-generated bridge). Every block carries a `// SAFETY:` comment today
(195/195 audited) — the mechanical goal is to keep that true without trusting
prose: `clippy::undocumented_unsafe_blocks`, `clippy::missing_safety_doc`,
`unsafe_op_in_unsafe_fn`, plus a `cargo-geiger` diff gate (see profile).

| Group | IDs | Rust-side meaning | Enforcement (today → proposed) |
|---|---|---|---|
| Directives | D.4.2, D.4.3, D.5.1 | commenting/commenting-out discipline, code hygiene, concurrency primitives | Review inside unsafe crates; D.5.1's "not all safe Rust types are race-free" note maps to the two `unsafe impl Send/Sync` in store (justified, documented). |
| R.1.1, R.5.1, R.5.5, R.5.10, R.20.4, R.20.7 | lexical/name/syntax violations (duplicate names, macro pitfalls, reserved identifiers) | Only reachable via `macro_rules!`/proc-macro or `extern "C"` symbol collision. | Review; `no_mangle` surface is pinned to the checked-in `pinyin.h` (55 symbols) — CI smoke gate already compiles the fork against it. |
| R.8.3, R.8.5, R.8.6, R.8.15, R.8.17 | declarations compatible with their definitions; parameter type consistency | `extern` blocks must match the C ABI. | Hand-written extern blocks are pinned to header SHA (`pinyin-oracle/src/ffi.rs`); capi is verified by the C++ smoke gate + contract tests. Keep both. |
| R.9.7 | aggregates not partially initialized | `MaybeUninit` misuse — absent from workspace. | Geiger + review; would be a Miri finding. |
| R.10.5, R.10.8, R.11.1, R.11.2, R.11.5, R.11.6, R.11.8, R.12.2 | pointer/integral conversion and cast discipline | The heart of FFI cast rules (`as`/`transmute`). | `transmute`: zero in workspace. `as`: ~150 numeric casts, concentrated in capi/data. Proposal: `cast_possible_truncation`/`cast_sign_loss`/`cast_precision_loss` at **warn** (58/14/35 hits) with `#[allow]`+justification at FFI seams; `clippy::transmute_*` family already denied via `all`. |
| R.12.4 | enum/bitfield evaluation | C semantics; "either well-defined or will not occur" (ADD6). | N/A-in-practice. |
| R.14.1 | `while` controlling-expression termination | Loop termination — general concern, review. | Review; engine loops are input-bounded (`MAX_GRAPH_INPUT = 65_535`). |
| R.17.9 | non-returning functions passed as callbacks | `!` type enforces (ADD6). | Guaranteed. |
| R.18.1–R.18.6 | pointer/array arithmetic and bounds | FFI pointer arithmetic. | Audited clean; `ptr::copy_nonoverlapping` with computed sizes only. |
| R.19.1 | objects not treated as overlapping storage | union/aliasing — Miri's home turf. | Zero unions; Miri lane would prove the store paths. |
| R.21.3–R.21.10, R.21.12–R.21.21, R.21.24, R.21.26 | std-lib facilities with undefined/dangerous behaviour (`atexit`, signals, setjmp, qsort comparators, stdio internals…) | Reachable only via `extern "C"` re-implementation | None used; geiger/review guard. |
| R.22.1–R.22.12, R.22.14–R.22.17, R.22.20 | resource acquire/release pairing (malloc/free, streams, locks) | The FFI ownership discipline. | Audited: every `malloc`'d string freed by contract (`g_free`), every `Box::into_raw` matched with `from_raw`; ownership rules documented per symbol. Residual risks logged (F-6 glib allocator pairing). |

## Table 3 — Guidelines not applicable to Rust (94)

Grouped by why they are moot (ADD6/Consortium comments quoted where present):

- **§4 language, §6–§7 types/literals, §9.2–9.6 initializers, §10 essential-type
  model (R.4.1–4.2, 6.1–6.3, 7.3–7.6, 9.2–9.6, 10.1–10.7, 12.3, 12.5–12.6):**
  C's essential-type conversions, implicit int promotions, mixed-sign compare —
  all replaced by explicit typed conversions; `TryFrom` covers narrowing.
  *Guaranteed by the type system (A/J).*
- **§8 declarations (R.8.1–8.2, 8.4, 8.8, 8.10–8.12, 8.14, 8.16, 8.18–8.19):**
  function/inline/typedef declaration mechanics; "no separate tag name space in
  Rust"; alignment "cannot be explicitly specified. Only ZSTs have this
  alignment". *(J)*
- **§13 evaluation order (R.13.2, 13.3, 13.4, 13.6):** "order of evaluation is
  strict in Rust"; `?:`/comma-operators do not exist. *(A/J)*
- **§14–§16 control flow (R.14.2, 15.1–15.3, 15.6, 16.1–16.7):** switch
  fallthrough, dangling else, `for` loop bound modification — "a corresponding
  match expression must be complete" (exhaustiveness), iterators are immutable
  borrows. *(A/J)*
- **§17 function mechanics (R.17.1, 17.3–17.5, 17.10, 17.12–17.13):** implicit
  declarations, ellipsis `...`, `return`-less paths — "the return keyword is
  not needed to return a value in Rust, only to exit". *(A/J)*
- **§18 pointer declarations (R.18.7–18.10):** "no external interface" for
  `extern`-qualified object declarations in safe code. *(J)*
- **§20 preprocessor (R.20.1–20.15, 15 rules):** "rules specific to the C
  preprocessor do not apply to Rust" — `#if/#include` hygiene becomes
  `cfg`/module discipline, already covered by D.4.4. *(J)*
- **§21 std lib (R.21.11, 21.22–21.23):** "no external interface" (register
  access etc.). *(J)*
- **§23 miscellaneous (R.23.1–23.8):** C23 `_Generic`/related — no Rust
  counterpart. *(J)*
- **Misc (R.1.4, R.2.4, R.3.2, R.5.4, R.5.7, R.11.9, R.11.10):** C
  versioning, tag namespaces, trigraphs/line splicing, null pointer constants
  ("Rust does not have a null pointer constant") — `Option`/references. *(J)*

## What this mapping changes in oxpinyin

The genuinely load-bearing transfers from MISRA into the oxpinyin profile are
few, because Rust's floor is high:

1. **R.17.7 / D.4.7 (return values / in-band errors)** — the only Table-1 area
   with measurable gaps today (`must_use` holes, 82 `missing_errors_doc`).
2. **D.4.1 (run-time failures)** — panic-abstinence in library code is
   *already factual* here but not *enforced*; clippy restriction lints on lib
   targets would mechanize the constitution's §4.
3. **R.10.x/R.11.x cast discipline** — the `as`-cast inventory (~150) is the
   largest untracked safe-Rust hazard class; warn-level pedantic cast lints
   plus TryFrom-conversion at FFI seams close it.
4. **The unsafe-only tables** — collapse onto SAFETY-comment enforcement +
   geiger inventory + Miri for `store`, all cheaper than the C-world
   equivalents they replace.
