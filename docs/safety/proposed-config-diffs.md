# Proposed configuration diffs (for review — not applied)

Study output per the brief: patches for review. Each is scoped, measured
against the current tree (`2382bdd`), and ordered as profile PR-1…PR-4.
Line counts are the review surface, deliberately small.

## 1. Workspace `Cargo.toml` — lint foundation (PR-1)

```diff
 [workspace.lints.rust]
 unsafe_code = "deny"
 missing_docs = "warn"
+unused_must_use = "deny"
+# Belt-and-braces: edition 2024 already warns; RUSTFLAGS=-D warnings already
+# errors it in CI. Explicit deny protects local builds without the env var.
+unsafe_op_in_unsafe_fn = "deny"

 [workspace.lints.clippy]
 all = { level = "deny", priority = -1 }
+# Curated pedantic subset (measured hits on this tree in brackets).
+# Casts are the largest untracked safe-Rust hazard class; must_use/docs grow
+# API-contract coverage (MISRA R.17.7/D.4.9 analogues). Group enables of
+# pedantic/nursery were measured (606/201 warnings) and rejected.
+cast_possible_truncation = "warn"            # [58]
+cast_precision_loss = "warn"                 # [35]
+cast_sign_loss = "warn"                      # [14]
+must_use_candidate = "warn"                  # [18]
+missing_errors_doc = "warn"                  # [82]
+missing_panics_doc = "warn"
+unreadable_literal = "warn"                  # [24]
+map_unwrap_or = "warn"                       # [24]
+redundant_closure_for_method_calls = "warn"  # [29]
```

Deliberately absent: `borrow_as_ptr` (35 hits — the capi handle pattern),
`doc_markdown` (98 — cosmetics), `float_cmp` (4 documented bit-parity ports),
`single_match_else`, `too_many_lines`.

## 2. Per-crate unsafe policy (PR-1)

Proven pattern: `oxpinyin-core` already ships `#![forbid(unsafe_code)]`
(lib.rs:4) alongside the workspace `deny`, proving the combination works
(inner `forbid` dominates the inherited level). Rolling out:

```diff
--- a/crates/oxpinyin-engine/src/lib.rs
+++ b/crates/oxpinyin-engine/src/lib.rs
@@
+// The compiler enforces the constitution's unsafe clause here: this crate
+// cannot contain `unsafe` at all, and unlike `deny` this cannot be
+// re-allowed by a stray inner attribute.
+#![forbid(unsafe_code)]
```

Same one-liner for: `engine`, `user`, `segment`, `counter`, `lambda`,
`emitter`, `corpus`, `dictool` (8 crates; `core` already has it).

**`data` deliberately stays at `#![deny(unsafe_code)]`** (present today at
lib.rs:9): structure.md reserves a documented mmap exception for this crate;
`forbid` would make that impossible without a policy U-turn. This is the
`forbid`-vs-`deny` semantics doing real design work: deny = "exceptions are
scoped modules with justification" (store's `lmdb.rs`/`tkrzw/*` pattern),
forbid = "no exceptions ever".

**`store`**: unchanged (workspace deny + existing module-scoped
`#![allow(unsafe_code)]` in `lmdb.rs:73`, `tkrzw/bridge.rs:28`,
`tkrzw/mod.rs:85` is already the minimal trusted region). Optional polish:
hoist lmdb's per-fn `#[allow]` to module scope for symmetry.

**`capi` / `oracle`** — add the SAFETY-enforcers:

```diff
--- a/crates/oxpinyin-capi/Cargo.toml
+++ b/crates/oxpinyin-capi/Cargo.toml
@@ [lints.clippy]
 all = { level = "deny", priority = -1 }
+undocumented_unsafe_blocks = "deny"  # mechanizes "// SAFETY: on every block"
+missing_safety_doc = "deny"          # mechanizes "# Safety" doc sections
```

(same hunk for `pinyin-oracle`; plus `[lints.rust]` there gains
`unsafe_op_in_unsafe_fn = "deny"` next to the existing `unsafe_code =
"allow"`.)

**fuzz workspace** (its own workspace; inherits nothing today — F-8):

```diff
--- a/fuzz/Cargo.toml
+++ b/fuzz/Cargo.toml
@@ [package]
 edition = "2021"
+
+[lints.rust]
+# FFI-driven harnesses (capi-commands target) will carry scoped unsafe with
+# SAFETY comments; deny, not forbid, on purpose.
+unsafe_code = "deny"
```

## 3. Library panic-lint containment (PR-1)

Cargo `[lints]` cannot distinguish the lib target from integration tests,
so the panic-abstinence denies live as a crate attribute that
`cfg(test)`-switches (inline `#[cfg(test)]` modules keep their unwraps;
`tests/` targets are unaffected):

```diff
--- a/crates/oxpinyin-engine/src/lib.rs
+++ b/crates/oxpinyin-engine/src/lib.rs
@@
 #![forbid(unsafe_code)]
+// Constitution §4, mechanically: library code may not unwrap/expect/panic.
+// Test modules opt out once, here, with this justification comment.
+#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used,
+                           clippy::panic, clippy::panic_in_result_fn))]
```

Applied to the eleven library crates (core/engine/user/data/store/segment
+ runtime/python/datagen/capi/oracle). Note: if `clippy::panic`
fires on the two commented `assert_eq!` bug-trips in `parser.rs`, they get a
targeted `#[allow(clippy::panic)]` carrying the existing justification —
that is the deviation record, not a policy hole. Measured: these crates are
clean today, so PR-1 introduces zero code churn beyond attributes.

## 4. `clippy.toml` (PR-1)

```toml
# Tuned lint suggestions to the pinned toolchain (keeps e.g. manual-let-else
# and redundant-field-names suggestions consistent with what CI enforces).
msrv = "1.97.1"
```

## 5. `rustfmt.toml` — intentionally absent (decision record)

Stable-defaults-only formatting is the policy; the existing
`cargo fmt --all --check` gate is complete. Nightly-only options
(`imports_granularity`, `group_imports`, `wrap_comments`) are rejected:
they would require nightly fmt in CI and create churn. No file.

## 6. `deny.toml` (new, PR-1)

```toml
# Supply-chain gate. Advisories share the RustSec DB with cargo-audit;
# running both in CI is redundant — this file is the single policy point.
[graph]
all-features = false          # audit the default build; feature lanes re-run with --all-features

[advisories]
version = 2
yanked = "deny"
# Deviation registry: cargo-deny 0.20's ignore schema takes only `id` and
# `reason` — the review-by date goes inside the reason string.
ignore = [
    # { id = "RUSTSEC-0000-0000", reason = "...; review by YYYY-MM-DD" },
]

[bans]
multiple-versions = "warn"    # informational: Stage-2 size-budget signal
deny = []                     # start empty; constitution's "no IME crates" stays a review rule
highlight = "simplest-path"   # 0.20 rejects "simplest"

[licenses]
version = 2
allow = [
    "GPL-3.0-or-later",       # the project license
    "Apache-2.0", "MIT", "MIT-0", "BSD-2-Clause", "BSD-3-Clause",
    "ISC", "Zlib", "Unicode-3.0", "CC0-1.0", "MPL-2.0", "BSL-1.0",
    "OpenSSL", "CDLA-Permissive-2.0",
]
confidence-threshold = 0.93

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
allow-git = []
```

(Validate with `cargo deny check` before merge; trim the license list to
what the 179-package lockfile actually needs.)

## 7. CI additions (PR-1 fast gate; PR-3 nightly lane)

Follows the repo convention of pinning third-party actions by SHA.

```diff
--- a/.github/workflows/ci.yml
+++ b/.github/workflows/ci.yml
@@ jobs:
+  deny:
+    runs-on: ubuntu-latest
+    steps:
+      - uses: actions/checkout@<sha> # v7.0.1
+        with: { persist-credentials: false }
+      - uses: taiki-e/install-action@<sha> # pin; tool below
+        with: { tool: cargo-deny }
+      - run: cargo deny --manifest-path Cargo.toml check advisories bans licenses sources
+      - run: cargo deny --all-features check advisories   # feature-gated deps too
```

Nightly lane (new file `verify-nightly.yml`, `schedule: cron: '0 3 * * *'`
+ `workflow_dispatch`, same concurrency discipline as ci.yml):

```yaml
jobs:
  fuzz-soak:      # all targets, -max_total_time split, corpus committed under fuzz/corpus/
  miri:           # cargo +nightly miri test -p oxpinyin-core -p oxpinyin-store (+ corpus replay)
  overflow-lane:  # RUSTFLAGS="-C overflow-checks=y -C debug-assertions=y" cargo test --workspace --release
  kani:           # trial: 4 harnesses
  geiger:         # cargo geiger report artifact
  lizard:         # lizard crates/ -Tlimit 40 (ratchet from current max 38)
```

Fuzz target additions (PR-3, per `upstream-test-strategies.md`):
`dict-loader` (bytes → `oxpinyin-data` decode), `scheme`, `codec`, and the
stateful `capi-commands` ABI fuzzer (libchewing `fuzzer.rs` shape — drives
the 55-symbol ABI with adversarial handle lifecycles; runs in the
Linux-only fuzz job).

## 8. `.githooks/pre-commit` — **dropped in review** (PR-1g)

```sh
#!/bin/sh
# fmt-only by design: <1s, so nobody reaches for --no-verify. Not a security
# boundary; CI remains authoritative.
set -eu
staged=$(git diff --cached --name-only --diff-filter=ACM | grep '\.rs$' || true)
[ -z "$staged" ] && exit 0
# rustfmt on the staged files only (via the pinned toolchain)
echo "$staged" | xargs rustfmt --check --edition 2024 >/dev/null 2>&1 || {
    echo "pre-commit: rustfmt would change staged .rs files; run cargo fmt." >&2
    exit 1
}
```

(plus keep `core.hooksPath .githooks` as documented in CONTRIBUTING.md)

## 9. `.vscode/settings.json` — **dropped in review** (PR-1g)

```json
{
  "rust-analyzer.check.command": "clippy"
}
```

Editor surfaces the same clippy CI enforces; explicitly not a CI concern.

## 10. Explicitly rejected configuration (record)

- `[profile.release] panic = "abort"` — kills `ffi_catch` for the cdylib.
- `[profile.release] overflow-checks = true` — shipped-artifact cost; the
  nightly paranoid lane gets the detection instead.
- pedantic/nursery group enables; `restriction` group enables.
- `rustfmt.toml` with nightly-only options.
- `no-panic` dependency (also requires a dep-addition ask per constitution).
