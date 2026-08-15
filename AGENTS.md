# AGENTS.md — contract for all coding agents

pinyin-rs is a portable Rust re-expression of libpinyin. Stage 1 = parity
with the pin-built oracle; Stage 2 = measured upgrades. Roadmap:
`ROADMAP.md`. Crate map: `.kiro/steering/structure.md`.

Kiro always-loads `.kiro/steering/`; this file wins on any conflict.

## Constitution

1. Broad appeal only — no niche features at cost to everyone.
2. Install-size budget: default payload ≤ pinned reference stack +10%.
3. No local AI (no client neural/LLM inference).
4. Nothing panics on any input; public APIs return `Result`.
5. `unsafe`: `forbid` in pinyin-core; `deny` in data/user/engine (documented
   mmap exception in data only); FFI only in capi/oracle/migrate with
   `// SAFETY:` per block.
6. Determinism: output is a pure function of (input, user state, config).
7. No dependency on other pinyin/IME crates; no transpiler dumps. Pin-built
   libpinyin is a test/migration **subject**, not a linked dependency of
   shipping code.
8. When in doubt, STOP — do not improvise.

## Spec discipline

Implement only from frozen `docs/findings/` SPECs and fixtures. Never read or
copy upstream C/C++ to implement. If behaviour contradicts a SPEC, STOP.

## Attribution

```
Assisted-by: AGENT_NAME:MODEL_VERSION
```

Nothing after the model. Trailers are a **set** (no duplicates). Never use
`Co-Authored-By` for agents.

The commit-message linter (`.github/scripts/lint-commits.sh`) enforces this on
every PR commit (R1–R4) and at commit time via `.githooks/commit-msg`
(R1–R3):

- **R1** — no AI agent identity in `Co-authored-by:` (email match, never name
  match).
- **R2** — `Assisted-by:` house form: `AGENT:MODEL` shape with nothing after
  the model; the `MODEL` token must contain at least one ASCII letter (a bare
  version number names no model — `Grok:4.6` fails, `Grok:grok-4.6` passes);
  no placeholder text; no duplicate lines (set semantics).
- **R3** — `AI-session: true` (exact value) promotes `Assisted-by:` to
  required; any other value is rejected.
- **R4** — no AI agent identity as git author or committer (CI-only: the
  commit-msg hook runs before the commit exists, so there is no identity to
  inspect).

## STOP → do not improvise

Ambiguous task · needs interface/ABI/dep change · test cannot pass without
breaking the constitution · SPEC contradicts observed pin behaviour ·
implementation would require reading upstream C++.

## Hard forbids

Add/upgrade deps without ask · edit frozen SPECs/goldens/CI policy without
ask · `unsafe` outside allowlisted crates · copy upstream code · silence
lints.

## Toolchain

`rust-toolchain.toml` is the only supported toolchain. Portable crates:
Linux/macOS/Windows. Oracle, capi, migrate: Linux-first.
