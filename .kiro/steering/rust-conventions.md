---
inclusion: always
---
# Rust conventions

**Toolchain:** rustup-managed everywhere. `rust-toolchain.toml` is the single
source of truth and CI respects it. Distribution-packaged Rust is unsupported
for development; bumps use a dedicated human-reviewed PR.

**unsafe tiers:** core `forbid` · data/user/engine `deny` (documented mmap
exception, `// SAFETY:` + module soundness note) · capi/oracle `allow`
at FFI only, `// SAFETY:` on every block.

**Errors:** every public API returns `Result`; nothing panics on any input — a
panic is a defect of the same severity as data loss. Public error enums are
`#[non_exhaustive]`.

**Determinism:** engine output is a pure function of (input, user state,
config). Graphs use index-based arenas.

**Backend selection:** compile-time only, one backend per binary —
`DefaultStore` resolves through a `#[cfg]` chain (kyotocabinet > tkrzw >
lmdb > redb), following libpinyin's own `--with-dbm` model. No runtime
dispatch; redb is the no-feature fallback.

**C struct parsing:** packed upstream structures are parsed by explicit
byte-offset reads (`u32::from_le_bytes` and friends) into owned fields —
never by casting a byte slice to a packed struct, never via unaligned
pointer reads. The reference pattern is `oxpinyin-data/src/memory_chunk.rs`:
an 8-byte header (u32 LE length, then u32 XOR checksum over the data
section, mirrored from `memory_chunk.h::get_check_sum`) followed by the
payload, checksum verified before use.

**Tests:** fixture-first — Lane-P acceptance uses platform-free frozen goldens
F-A–F-D. F-E is the cross-lane evidence register and may cite Linux-only or
advisory platform evidence; it is not itself a platform-free golden. Golden
(curated) · property (`proptest`: totality, determinism) · fuzz (`cargo-fuzz`,
from W1) · crash-path (hard-kill, all OSes) · migration round-trip (legacy
store untouched, asserted by hash).

**Style:** `cargo fmt --check`, `clippy -D warnings`.
