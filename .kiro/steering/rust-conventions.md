---
inclusion: fileMatch
fileMatchPattern: '**/*.rs'
---
# Rust conventions

**Toolchain:** rustup-managed everywhere. `rust-toolchain.toml` is the single
source of truth and CI respects it. Distribution-packaged Rust is unsupported
for development; bumps use a dedicated human-reviewed PR.

**unsafe tiers:** core `forbid` · data/user/engine `deny` (documented mmap
exception, `// SAFETY:` + module soundness note) · capi/oracle/migrate `allow`
at FFI only, `// SAFETY:` on every block.

**Errors:** every public API returns `Result`; nothing panics on any input — a
panic is a defect of the same severity as data loss. Public error enums are
`#[non_exhaustive]`.

**Determinism:** engine output is a pure function of (input, user state,
config). Graphs use index-based arenas.

**Tests:** fixture-first — Lane-P acceptance uses platform-free frozen goldens
F-A–F-D. F-E is the cross-lane evidence register and may cite Linux-only or
advisory platform evidence; it is not itself a platform-free golden. Golden
(curated) · property (`proptest`: totality, determinism) · fuzz (`cargo-fuzz`,
from W1) · crash-path (hard-kill, all OSes) · migration round-trip (legacy
store untouched, asserted by hash).

**Style:** `cargo fmt --check`, `clippy -D warnings`.
