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
enabled (Berkeley DB in the default set since 2026-09-20;
`--no-default-features --features {kyotocabinet|tkrzw}` for a peer),
following libpinyin's own
`--with-dbm` model. No runtime dispatch and no fallback: `oxpinyin-store`
emits a `compile_error!` when zero or more than one backend is enabled.

**Dependencies (maintainer, 2026-09-12; Kyoto Cabinet shim, maintainer
approval 2026-10-23 UTC):** a Rust library is used as
Rust (redb was the worked example; removed 2026-09-22, and the tree
has no pure-Rust database in its place). `glib-sys` is the C binding
of a C library. The non-Rust database
backends — Kyoto Cabinet, tkrzw, Berkeley DB — are linked to the
distribution's own libraries. tkrzw and Berkeley DB are used through the
ABIs their maintainers provide, which are C: `tkrzw_langc.h` and `db.h`,
bound with bindgen over the system headers. tkrzw's C++ interface is not
used: it carries no ABI stability across compilers, standard libraries or
library versions, and the C API is the maintainer's stability promise.

Kyoto Cabinet is used as libpinyin uses it. One C++ shim,
`crates/oxpinyin-store/src/kyotocabinet/shim.cc`, is compiled with `cc`
under the `kyotocabinet` feature. It constructs `TreeDB`, `HashDB`,
`StashDB` and `ProtoTreeDB` from the build-time headers, and its
`noexcept` C entry points (`shim.h`) always turn a C++ exception into a
failure return. Like libpinyin, it therefore depends on the system
library's C++ ABI: the facades import 68 `kyotocabinet::` names, 67 of
them among the pin's 77. Nothing else vendors or compiles C or C++ into
oxpinyin.

Kyoto Cabinet left `kclangc.h` on measurement (#1118, results:
`docs/findings/kc-shim-1118.md`). `kclangc.h` reaches every class through
`PolyDB`, and that dispatch, together with the classes the library links
but oxpinyin never opens, kept about 580 KiB more libkyotocabinet text
resident than the pin; the shim leaves the pin's own libkyotocabinet
residency. An earlier cxx bridge to the same classes was measured and not
adopted (results: `docs/findings/kc-c-vs-cxx-adapter-2026-10-22.md`;
code: PR #1043): its glue made point reads, writes and framed scans
execute 0.6–5.5% more instructions, and the cargo-built libraries
exported the cxx runtime and bridge symbols beyond the version scripts.
The shim has no glue layer. Per store operation it executes 0.980–1.004×
the C binding's instructions, each against an exact same-bytes control,
and it exports nothing beyond the version scripts. It adds about 310 KB
to each stripped library.

**C struct parsing:** packed upstream structures are parsed by explicit
byte-offset reads (`u32::from_le_bytes` and friends) into owned fields —
never by casting a byte slice to a packed struct, never via unaligned
pointer reads. The reference pattern is `oxpinyin-data/src/chunk_format.rs`:
an 8-byte header (u32 LE length, then u32 XOR checksum over the data
section, mirrored from `memory_chunk.h::get_check_sum`) followed by the
payload, checksum verified before use.

**Tests:** fixture-first — Lane-P acceptance uses platform-free frozen goldens
F-A–F-D. F-E is the cross-lane evidence register and may cite Linux-only or
advisory platform evidence; it is not itself a platform-free golden. Golden
(curated) · property (`proptest`: totality, determinism) · fuzz (`cargo-fuzz`,
from W1) · crash-path (hard-kill, all OSes) · differential (the
`pinyin-oracle` harness against the pinned libpinyin, Linux-only). A
differential gates a behaviour already derived from the pin's source; it
is not a way to discover one.

**Style:** `cargo fmt --check`, `clippy -D warnings`.
