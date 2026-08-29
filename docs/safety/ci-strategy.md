# CI strategy — tiered verification for oxpinyin (proposal)

Design goal: maximum confidence per CI-minute, four tiers, nothing heavy on
the PR path. Existing jobs (keep): `lint` (fmt ×2 workspaces + clippy
`-D warnings`), `test` (+ C++ smoke gate + live-typing differential),
`test-portable` (mac/win), `fuzz` (pinned nightly, parser smoke).
Estimated costs below are rough additive deltas on a cached runner.

## Tier 1 — FAST PR GATE (every push/PR, ~+2 min over today)

```text
cargo fmt --all --check                        # existing
cargo fmt --manifest-path fuzz/Cargo.toml --all --check   # existing
cargo clippy --locked --workspace --all-targets -- -D warnings   # existing, now with curated lints
cargo clippy --locked -p oxpinyin-capi -p pinyin-oracle --all-targets -- -D warnings   # + FFI-crate lints
cargo nextest run --workspace (or cargo test)  # existing runner, nextest optional
cargo test --doc                               # only if nextest adopted
cargo deny check advisories bans licenses sources   # NEW, ~40–90s cold, cacheable DB
fuzz smoke: parser + dict-loader + scheme (~30–60s aggregate)   # extend existing job
```

Rationale per addition: `cargo deny` is the only supply-chain gate (one
config, RustSec + licenses + sources + dup-versions signal); the extra
clippy invocation exists only if the unsafe-crate lints
(`undocumented_unsafe_blocks`, `missing_safety_doc`) are scoped per-crate in
Cargo `[lints]` — then no extra invocation is needed at all (preferred).
Doctest step only if nextest lands. Gates: all hard.

## Tier 2 — EXTENDED QUALITY (PR-optional label / nightly-lite, ~+5–8 min)

- `cargo llvm-cov` report artifact (no threshold; comment on PR when
  labeled `coverage`).
- `cargo geiger` report artifact (unsafe-in-deps diff vs main).
- Lizard report with CCN capped at 40 (`lizard crates/ -l rust -C 40`;
  ratchet vs current max 38).

> STATUS: Tier 2 is documented but **not built** — no label-triggered
> workflow exists. All three tools above currently run in Tier 3's
> verify-nightly schedule.
- Windows/macOS keep today's portable test job; optionally add a
  `--no-default-features` store build to prove the feature-gated unsafe
  crates compile-out of the default path.

## Tier 3 — NIGHTLY / SCHEDULED (one runner, serial, ~30–60 min)

1. **Fuzz soak**: all targets, 10–30 min total (`-max_total_time` split),
   committed corpus (`fuzz/corpus/`), ASan default. New targets:
   `capi-commands` (stateful ABI session fuzzer — libchewing `fuzzer.rs`
   shape: byte→command alphabet over keys, guess, candidate walks,
   config setters, adversarial iterator begin/end ordering; exercises
   F-6/F-7 surface), `dict-loader` (bytes→data decode; F-3 class),
   `scheme` (double-pinyin/config parsing), `codec` (user DB roundtrip +
   hostile bytes).
2. **Miri**: `cargo +nightly miri test -p oxpinyin-core -p oxpinyin-store`
   + corpus replay through the parser target under `-Zmiri`.
3. **Paranoid release lane**: `RUSTFLAGS="-C overflow-checks=y
   -C debug-assertions=y" cargo test --workspace --release`.
4. ~~**Kani** (trial)~~ — dropped: no release supports the pinned
   toolchain (newest bundles nightly 2025-11-21 < 1.97.1).
5. **cargo-mutants** (trial, on the nightly schedule):
   scoped `-p oxpinyin-core` file filters (parser, scheme, scoring) +
   `oxpinyin-user/src/store.rs`.

Failure policy: nightly findings open issues (with libFuzzer repro
artifacts committed under `fuzz/artifacts/` and regression tests per the
libchewing convention), they do not auto-block unless a ratchet exists
(coverage/Lizard/complexity ratchets only ever tighten).

## Tier 4 — RELEASE GATE (tag/manual, ~+15 min)

- Full `cargo deny` (advisories re-checked at tag time), geiger report
  attached to the release notes.
- Release-profile validation: build the cargo-c artifact, verify the
  exported symbol set equals `pinyin.h`'s 55 (scripted `nm` diff — today
  this is implied by the smoke gate; make it explicit at release).
- FFI checks: C++ smoke gate + contract tests on the built artifact
  (existing content, promoted to required-for-tag).
- Confirm no `panic=abort` / no `overflow-checks` in shipped profiles
  (assert via `cargo rustc -- --print`-style script or profile lint
  comment review — cheap, prevents accidental profile flips).
- Portable matrix re-run (existing test-portable).

## Lane/tool matrix

| Tool | T1 PR | T2 (not built) | T3 nightly | T4 (not built) |
|---|---|---|---|---|
| fmt / clippy (curated) | ✔ | | | planned (T4 not built) |
| nextest/cargo test + doctests | ✔ | | | planned (T4 not built) |
| portable tests | ✔ (mac/win) | | | planned (T4 not built) |
| C++ smoke + differential | ✔ | | | planned (T4 not built) |
| cargo-deny | ✔ | | | planned (T4 not built) |
| fuzz smoke | ✔ | | | |
| fuzz soak + corpus | | | ✔ | |
| Miri | | | ✔ | |
| overflow release lane | | | ✔ | |
| Kani | | | dropped | |
| cargo-mutants | | | ✔ (nightly schedule) | |
| llvm-cov | | planned (T2 not built) | ✔ report | planned (T4 not built) |
| geiger | | planned (T2 not built) | ✔ report | planned (T4 not built) |
| Lizard ratchet | | planned (T2 not built) | ✔ | |

## Cost/confidence rationale

- The PR tier stays compile+test dominated; deny/fuzz add ~2 min.
- Everything interpreting or mutating semantics (Miri/mutants/soak)
  is scheduled: high value, too slow per-PR, zero MSRV impact.
- Tier 2 exists so contributors can *request* deeper signal without
  making everyone pay for it.
- The IME-hosting risk profile (long-lived process, hostile-ish data files,
  C consumers) is what selects `capi-commands` + `dict-loader` as the two
  new fuzz investments — both upstream-precedented
  (`upstream-test-strategies.md`).
