---
inclusion: always
---
# Rust conventions

**Toolchain:** rustup-managed everywhere. `rust-toolchain.toml` is the single
source of truth and CI respects it. Distribution-packaged Rust is unsupported
for development; bumps use a dedicated human-reviewed PR.

**unsafe tiers:** data `deny` (the documented mmap exception, `// SAFETY:`
+ module soundness note) · every other portable crate `forbid` (the
constitution's floor for user/engine is `deny`; they sit at `forbid` in
practice) · capi/zhuyin-capi/oracle `allow` at FFI only, `// SAFETY:` on
every block (mechanically enforced — AGENTS.md constitution §5).

**Errors:** every public API returns `Result`; nothing panics on any input — a
panic is a defect of the same severity as data loss. Public error enums are
`#[non_exhaustive]`.

**Determinism:** engine output is a pure function of (input, user state,
config). Graphs use index-based arenas.

**Backend selection:** compile-time only, exactly one backend per binary
— `DefaultStore` is a `#[cfg]` alias for whichever backend feature is
enabled (tkrzw in the default set; `--no-default-features --features
{kyotocabinet|lmdb|redb}` for a peer), following libpinyin's own
`--with-dbm` model. No runtime dispatch and no fallback: `oxpinyin-store`
emits a `compile_error!` when zero or more than one backend is enabled.

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
from W1) · crash-path (hard-kill, all OSes) · differential (the
`pinyin-oracle` harness against the pinned libpinyin, Linux-only).

**Style:** `cargo fmt --check`, `clippy -D warnings`.
