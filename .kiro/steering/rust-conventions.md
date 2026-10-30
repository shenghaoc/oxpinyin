---
inclusion: always
---
# Rust conventions

**Toolchain:** rustup-managed everywhere. `rust-toolchain.toml` is the single
source of truth and CI respects it. Distribution-packaged Rust is unsupported
for development; bumps use a dedicated human-reviewed PR.

**unsafe tiers (target):** data, core, user, engine and runtime `forbid`;
`oxpinyin-input` permits scoped external-I/O modules; store `deny` with
scoped backend allows; capi/zhuyin-capi/oracle allow ABI FFI. Every block
has `// SAFETY:`. Stage 1 of the boundary is in place (`oxpinyin-input`,
crate-root `deny` with one scoped allow); data keeps `deny` and its mmap
exception until stage 2 moves the mapping. See AGENTS.md constitution §5.

**Panics and errors (maintainer decision 2026-10-29 UTC):** follow constitution
§4 and libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`'s observable
failure behaviour. A panic is permitted only for a corresponding pin
assertion or abort, except where an existing compatibility ruling requires
a different outcome. Where the pin continues or returns an error, Rust must
not introduce a panic, including through ordinary operations or assertions.
Use idiomatic error handling; internal Rust APIs need not universally return
`Result`. Existing class (b)/(c) rulings remain authoritative: class (c)
refusals keep `false`/`Err` and exactly one warning per occurrence.

**Enums and matches (maintainer decision 2026-10-30 UTC):**
`#[non_exhaustive]` is for public APIs that other teams use (Google Rust
style guide). oxpinyin has no such API, because its Rust interfaces are
internal (2026-10-18 ruling). Do not mark an enum or a struct
`#[non_exhaustive]`. A match on an oxpinyin enum lists every variant and
has no wildcard arm. Then the compiler flags every in-tree match when a
variant is added. If only one variant is necessary, use `if let`,
`let ... else` or `matches!`. A match on literals, ranges, or std or
foreign types can use `_`.

**Integer arithmetic.** Where the pin's unsigned arithmetic wraps and the
result is observable, use the matching standard method (`wrapping_add`,
`wrapping_sub`, `wrapping_mul`). Never use a plain operator where overflow
is reachable from input: it panics in debug builds. Use `checked_*` where
the pin checks and refuses. Use checked or widened arithmetic for bounds
the pin lacks, such as sizes, offsets and indexes computed from external
input, where an overflow would be memory-unsafe (class (b)).

Keep the non-test crate-root denials of `clippy::unwrap_used`,
`clippy::expect_used`, `clippy::panic` and `clippy::panic_in_result_fn`.
For a permitted site, use the narrowest scoped
`#[expect(..., reason = "...")]`, citing the pin's exact abort site or
an established invariant that makes the failure unreachable. A `panic!`
in a `Result`-returning function must account for both panic lints if both
fire. No broad module-level suppressions, no relaxed crate-wide denials,
and no new `arithmetic_side_effects` or `indexing_slicing` restrictions.
An `#[expect]` proves only that Clippy saw the construct, not that its
failure condition matches the pin; that correspondence still needs review.

Where a permitted panic reproduces a pin assertion's process termination,
the different Rust and C assertion messages are a language artefact; do not
imitate glibc's message format. Existing diagnostic contracts still govern.

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
them among the pin's 77. This is the current tree. The
`oxpinyin-input` boundary has a small C shim (`native/config.c`, stage 1)
and will add a tool-only C++ stream shim, built with the existing `cc`
build dependency, with narrow hand-written extern declarations and no
direct `libc` crate dependency.

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
