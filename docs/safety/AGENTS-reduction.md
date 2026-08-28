# AGENTS.md reduction — prose rules that can become mechanics

Companion to the profile. Principle: AGENTS.md should keep architecture,
workflow, rationale, commands, and inherently-contextual instructions; every
rule that a tool can enforce should migrate to Cargo/CI and then *shrink to a
pointer* in AGENTS.md (or be deleted once CI enforces it).

Legend: **mechanized** = delete the rule text after PR-1 lands, keep at most
a one-line pointer; **assisted** = keep the rule, add the named mechanical
tripwire that catches the common violation; **stays prose** = no honest
mechanization exists.

## Constitution items

| AGENTS.md clause | Today | After this study | Disposition |
|---|---|---|---|
| §4 "Nothing panics on any input; public APIs return `Result`" | prose + de-facto true | `clippy::unwrap_used/expect_used/panic/panic_in_result_fn` denied in library crates; `unused_must_use` denied; fuzz targets | **mechanized** (the Result half stays prose — type design is judgment; the panic half now fails compile) |
| §5 `unsafe`: forbid in core; deny in data/user/engine; FFI only in capi/oracle/migrate with `// SAFETY:` per block | workspace `deny` + per-crate allows + prose | per-crate `forbid`/scoped-allow in Cargo; `undocumented_unsafe_blocks` + `missing_safety_doc` denied in FFI crates; `unsafe_op_in_unsafe_fn` denied | **mechanized** (structure.md's unsafe column becomes generated-by-Cargo truth; F-12 closes) |
| §6 Determinism: output pure function of (input, user state, config) | prose + oracle differentials | no lint can state this | **assisted** (differentials are the tripwire; keep prose) |
| §2 install-size budget | prose + Stage-2 pins | `cargo deny bans multiple-versions=warn` gives the dup-dep signal; size pins remain the real gate | **assisted** |
| §7 no pinyin/IME crate deps; pin-built libpinyin not a linked dep | prose | `cargo deny bans` list (empty today, one line when a concrete ban lands) + lockfile review | **assisted** |
| §3 no local AI; §1 broad appeal | product judgment | none | **stays prose** |
| §8 "When in doubt, STOP" | meta-rule | none | **stays prose** |

## Hard-forbid items

| Clause | Disposition |
|---|---|
| "Add/upgrade deps without ask" | **assisted**: every dependency change is declared in a Cargo.toml manifest, so the manifest diff in the PR is what makes additions visible; the relevant lockfiles are checked when they change, and deny.toml policy-checks licenses/advisories/sources — but none of that enforces the approval *ask*, which stays prose |
| "edit frozen SPECs/goldens/CI policy without ask" | **assisted**: golden fixture hashes (existing practice) + CODEOWNERS-style convention; stays prose |
| "`unsafe` outside allowlisted crates" | **mechanized** (see §5 above) |
| "silence lints" | **assisted**: `#[allow]`/`--cap-lints` greps in review; a deny-by-default lint surface makes silence *visible* (each allow needs a justification comment per profile) — stays review because legitimate allows exist |

## Attribution (commit trailers)

Already mechanical: `.githooks/commit-msg` + CI R1–R4 lint every commit.
AGENTS.md keeps the spec; the mechanics exist. **No change.**

## Toolchain / rebase / worktree / concurrent-session sections

Workflow prose; no honest mechanization (branch-name cops cause more harm
than good). **Stays prose**, exactly as written.

## Source-policy & divergence notes

Rationale + method; the LICENSE compatibility is enforced by
`cargo deny licenses` (GPL-3.0-or-later list). **Assisted only** for the
license half; the method text stays.

## Suggested AGENTS.md delta (after PR-1..PR-4 land)

```diff
 5. `unsafe`: `forbid` in oxpinyin-core; `deny` in data/user/engine …
+5. The allowlist is mechanical: safe crates carry crate-root
+   `#![forbid(unsafe_code)]` (or `[lints]` tables, as oxpinyin-python and
+   oxpinyin-runtime do); `data` stays `#![deny]` reserving its documented
+   mmap exception; store scopes allows to lmdb.rs/tkrzw; capi/oracle allow
+   with `// SAFETY:` per block, enforced by Clippy's
+   undocumented_unsafe_blocks/missing_safety_doc — CI will tell you; this
+   line is context, not the gate.
```

Net effect: roughly 15% of AGENTS.md's normative lines become pointers to
mechanics; the document keeps its role as the *why* + workflow authority.
Nothing is deleted that a human or agent actually needs for judgment.
