# Rust conventions

## Toolchain

Use rustup everywhere.
`rust-toolchain.toml` is the sole toolchain source, and CI follows it.
Distribution-packaged Rust is unsupported for development.
Update the toolchain in a dedicated PR.

## Unsafe code

The [constitution](constitution.md) (§5) sets which crates forbid, deny or scope `unsafe`; the [crate table](architecture.md#crates) shows each crate's current level.

A store module waiver is an `#![expect(unsafe_code, reason = "...")]` rather than an `allow`, so a waiver that stops being needed draws `unfulfilled_lint_expectations`, an error under `-D warnings`.
In a crate that denies `clippy::undocumented_unsafe_blocks`, Clippy enforces that a `// SAFETY:` comment is present, not that it is true; review verifies it.

## Panics and errors

The [constitution](constitution.md) (§4) decides where a panic is permitted: only where the pin asserts or aborts on the corresponding condition. These lint conventions apply it.

Keep the non-test crate-root denials of `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic` and `clippy::panic_in_result_fn`.
For a permitted site, use the narrowest scoped `#[expect(..., reason = "...")]`.
Cite the pin's exact abort site or an established invariant that makes failure unreachable.
Account for both panic lints if both fire for `panic!` in a `Result`-returning function.
Do not add broad module suppressions or relax crate-wide denials.
Do not add `arithmetic_side_effects` or `indexing_slicing` restrictions.
An `#[expect]` proves only that Clippy saw the construct.
Review must still establish correspondence with the pin's failure condition.

Assertions, including `debug_assert!`, follow §4; a comment calling something an internal invariant is not by itself proof.
Assertions in tests remain unrestricted.

A permitted panic's message can differ from the pin's C assertion message; that is a [language artefact](source-policy.md#internal-language-artefacts), so do not imitate glibc's assertion format.

### Poisoned locks

Lock poisoning signals that a Rust panic occurred while a lock was held.
Production code must treat poison as an internal integrity failure and end
the process with `std::process::abort()`. Never recover poisoned state, use
`unwrap` or `expect`, or raise a new panic on poison. This rule applies to
Rust's lock state. The pin has no lock poisoning. Test code keeps its
existing lock handling.

## Enums and matches

**Enums and matches (maintainer decision 2026-10-30 UTC):**
`#[non_exhaustive]` is for public APIs that other teams use (Google Rust
style guide). oxpinyin has no such API, because its Rust interfaces are
internal (2026-10-18 ruling). Do not mark an enum or a struct
`#[non_exhaustive]`. A match on an oxpinyin enum lists every variant and
has no wildcard arm. Then the compiler flags every in-tree match when a
variant is added. If only one variant is necessary, use `if let`,
`let ... else` or `matches!`. A match on literals, ranges, or std or
foreign types can use `_`.

## Integer arithmetic

The [source policy](source-policy.md#internal-language-artefacts) decides when the pin's unsigned wrap is reproduced; these are the Rust methods for each case.

Use matching `wrapping_add`, `wrapping_sub` or `wrapping_mul` when the pin's unsigned arithmetic wraps and the result is observable.
Do not use a plain operator where input can cause overflow.
It panics in debug builds.
Use `checked_*` where the pin checks and refuses.
Use checked or widened arithmetic for missing bounds where overflow would be memory-unsafe, under class (b).
This includes sizes, offsets and indexes computed from external input.
Shipped profiles keep `overflow-checks` off.

## Determinism, backends and dependencies

[Constitution](constitution.md) §6 sets the determinism rule.
The [architecture](architecture.md#backend-selection) owns backend selection and [dependencies](architecture.md#dependencies).

## C struct parsing

Parse packed upstream structures with explicit byte-offset reads, such as `u32::from_le_bytes`, into owned fields.
Never cast a byte slice to a packed struct or use unaligned pointer reads.
The reference pattern is `oxpinyin-data/src/chunk_format.rs`.
Its 8-byte header contains a u32 LE length and a u32 XOR checksum over the data section.
The checksum mirrors `memory_chunk.h::get_check_sum`.
The payload follows the header.
Verify the checksum before use.

## Internal API simplicity

The Rust interfaces are internal ([interface ruling](constitution.md#external-interfaces)).
Write each interface for its callers in the tree.

Use concrete types, borrowed parameters and direct implementations.
Add a generic parameter, a trait, a wrapper, a macro or a configurable builder only in one of these conditions:

- The callers in the tree need it, for example two callers that use different types.
- It removes duplication that is in the tree now.

Use a named type, not a Boolean argument, when the Boolean hides behaviour at the call site ([C-CUSTOM-TYPE](https://rust-lang.github.io/api-guidelines/type-safety.html#c-custom-type)).
Use a struct with named fields, not a tuple, when the tuple hides the meaning of its fields.
Write comments for contracts, invariants and rationale that the code does not show.
Do not refactor code only for style.

## Tests and style

Use fixtures first.
Portability acceptance uses the platform-free frozen goldens F-A–F-D.
F-E is the evidence register shared across workstreams.
It can cite Linux-only or advisory platform evidence.
It is not a platform-free golden.

Use the applicable test kinds:

- Curated goldens.
- Property tests with `proptest` for totality and determinism.
- Fuzz tests with `cargo-fuzz`, from W1.
- Hard-kill crash-path tests on all operating systems.
- Differentials against frozen captures of pinned libpinyin.

Derive behaviour from the pin's source as the [source policy](source-policy.md#stage-2) describes; a differential verifies it and never discovers it.
Use `cargo fmt --check` and `clippy -D warnings`.
Formatting uses rustfmt's stable defaults; there is no `rustfmt.toml`.
Follow the [verification procedure](runbooks/verification.md) for applicable checks.
