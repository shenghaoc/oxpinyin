# Rust conventions

## Toolchain

The toolchain comes from rustup everywhere.
`rust-toolchain.toml` is the sole toolchain source, and CI follows it.
Distribution-packaged Rust is unsupported for development.
A toolchain update is a dedicated PR.

## Unsafe code

The [constitution](constitution.md) (§5) sets which crates forbid, deny or scope `unsafe`; the [crate table](architecture.md#crates) shows each crate's current level.

Every crate inherits `[workspace.lints]` with `[lints] workspace = true`.
A crate-local `[lints]` table replaces the workspace table instead of extending it, so do not add one.
Set a crate's unsafe level, where it differs from the workspace's `deny`, with a crate-root attribute.
That attribute covers only the library or binary; a build script, test or example that uses `unsafe` carries its own waiver.

A module, build-script, test or example waiver is an `#[expect(unsafe_code, reason = "...")]` rather than an `allow`, so a waiver that stops being needed draws `unfulfilled_lint_expectations`, an error under `-D warnings`.
The workspace denies `clippy::undocumented_unsafe_blocks`: Clippy enforces that a `// SAFETY:` comment is present, not that it is true; review verifies it.

## Panics and errors

The [constitution](constitution.md) (§4) decides where a panic is permitted: only where its failure is proven unreachable.
A reachable pin assert or abort uses `std::process::abort()` in the C adapters and tools, never a panic.
These lint conventions apply the rule.

The non-test crate roots deny `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`, `clippy::panic_in_result_fn` and `clippy::unreachable`, and these denials stay.
A permitted site carries the narrowest scoped `#[expect(..., reason = "...")]`, which cites the established invariant that makes failure unreachable.
Where both panic lints fire for `panic!` in a `Result`-returning function, the expectation covers both.
The code has no broad module suppressions and no relaxed crate-wide denials.
It has no `arithmetic_side_effects` or `indexing_slicing` restrictions.
An `#[expect]` proves only that Clippy saw the construct; review still establishes the invariant.

Assertions, including `debug_assert!`, follow §4.
A comment that calls something an internal invariant is not proof by itself.
Assertions in tests remain unrestricted.

At a reproduced pin abort, the missing glibc assertion line is `POLICY-ACCEPTED` ([failure behaviour](findings/compatibility-policy.md#failure-behaviour)); oxpinyin does not imitate glibc's assertion format.

### Poisoned locks

Lock poisoning signals that a Rust panic occurred while a lock was held.
Production code treats poison as an internal integrity failure and ends the
process with `std::process::abort()`. It never recovers poisoned state, uses
`unwrap` or `expect`, or raises a new panic on poison. This rule applies to
Rust's lock state. The pin has no lock poisoning. Test code keeps its
existing lock handling.

Production code holds a lock in one place only: the registry of live
contexts in `live.rs` of each C library. Every context in the process
shares it, so the contract that serializes one context's calls does not
cover it.

### Double borrows

A `RefCell` holds state that the C contract already serializes: one call at a time on a context and all its handles.
A borrow that conflicts with a live borrow of the same cell is an internal integrity failure.
`borrow` and `borrow_mut` panic on it, and in the C libraries that panic aborts the process at the `extern "C"` boundary.
Do not recover from it with `try_borrow` or `try_borrow_mut`, and do not catch the panic.
End a borrow before you call a visitor or callback that can reach the same cell.

## Enums and matches

`#[non_exhaustive]` is for public APIs that other teams use (Google Rust style guide).
oxpinyin has no such API, because its Rust interfaces are internal ([interface ruling](constitution.md#external-interfaces)), so no enum or struct is marked `#[non_exhaustive]`.

A match on an oxpinyin enum lists every variant and has no wildcard arm.
Then the compiler flags every in-tree match when a variant is added.
Where only one variant matters, the code uses `if let`, `let ... else` or `matches!`.
A match on literals, ranges, or std or foreign types can use `_`.

## Integer arithmetic

The [source policy](source-policy.md#internal-language-artefacts) decides when the pin's unsigned wrap is reproduced; these are the Rust methods for each case.

- Where the pin's unsigned arithmetic wraps and the result is observable: the matching `wrapping_add`, `wrapping_sub` or `wrapping_mul`.
  A plain operator is not used where input can cause overflow, because it panics in debug builds.
- Where the pin checks and refuses: `checked_*`.
- Where a bound is missing and overflow would be memory-unsafe, under class (b): checked or widened arithmetic.
  This includes sizes, offsets and indexes computed from external input.

Shipped profiles keep `overflow-checks` off.

## Determinism, backends and dependencies

[Constitution](constitution.md) §6 sets the determinism rule.
The [architecture](architecture.md#backend-selection) owns backend selection and [dependencies](architecture.md#dependencies).

## C struct parsing

Packed upstream structures are parsed with explicit byte-offset reads, such as `u32::from_le_bytes`, into owned fields.
No byte slice is cast to a packed struct, and no unaligned pointer read is used.
The reference pattern is `oxpinyin-data/src/chunk_format.rs`.
Its 8-byte header contains a u32 LE length and a u32 XOR checksum over the data section.
The checksum mirrors `memory_chunk.h::get_check_sum`.
The payload follows the header.
The checksum is verified before use.

## Internal API simplicity

The Rust interfaces are internal ([interface ruling](constitution.md#external-interfaces)).
Each interface is written for its callers in the tree.

The code uses concrete types, borrowed parameters and direct implementations.
A generic parameter, a trait, a wrapper, a macro or a configurable builder exists only in one of these conditions:

- The callers in the tree need it, for example two callers that use different types.
- It removes duplication that is in the tree now.

Test-only callers, and duplication that exists only in tests, do not by themselves justify a production abstraction. The exception is a test that is the only adequate witness of a distinct mechanism that the production path cannot exercise.

A named type replaces a Boolean argument when the Boolean hides behaviour at the call site ([C-CUSTOM-TYPE](https://rust-lang.github.io/api-guidelines/type-safety.html#c-custom-type)).
A struct with named fields replaces a tuple when the tuple hides the meaning of its fields.
Comments state contracts, invariants and rationale that the code does not show.
Code is not refactored only for style.

## Tests and style

Fixtures come first.
Portability acceptance uses the platform-free frozen goldens F-A–F-D.

The applicable test kinds are:

- Curated goldens.
- Property tests with `proptest` for totality and determinism.
- Fuzz tests with `cargo-fuzz`, from W1.
- Hard-kill crash-path tests on all operating systems.
- Differentials against frozen captures of pinned libpinyin.

Behaviour is derived from the pin's source as the [source policy](source-policy.md#stage-2) describes; a differential verifies it and never discovers it.
The code passes `cargo fmt --check` and `clippy -D warnings`.
Formatting uses rustfmt's stable defaults; there is no `rustfmt.toml`.
The [verification procedure](runbooks/verification.md) selects the applicable checks.
