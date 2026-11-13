# Constitution

oxpinyin implements libpinyin and libzhuyin in native Rust.
It preserves the pin's outward behaviour at the C API and in persistent data.
The [roadmap](../ROADMAP.md) records the stages and their status.

1. **Broad appeal.** A niche feature is not added at a cost to all users.
2. **Binary size.** A change that affects the shipping libraries records their stripped file sizes.
   The PR reports any increase over the base.
   Minor decreases need no report.
   Documentation-only changes need no size build or report.
   There is no pin +10% cap.
   CI reports the sizes for each pull request that can change them.
   See [shipping-library sizes](packaging.md#shipping-library-sizes).
3. **No local AI.** oxpinyin has no client neural inference or LLM inference.
4. **Failure behaviour.** oxpinyin follows the pin's observable failure behaviour.
   Where the pin asserts or aborts, the C adapters and the drop-in tools call `std::process::abort()`, after the pin's preceding work.
   The shared Rust code returns a typed error at the pin's failure point so that they can do this.
   A reachable pin abort is never a `panic!`, `assert!` or `unwrap()`.
   A panic-producing assertion is permitted only where its failure is proven unreachable.
   No panic is introduced where the pin continues or returns an error.
   Error handling is idiomatic Rust; internal Rust APIs do not all need to return `Result`.
   Class (b) rulings stay authoritative.
   The [failure-behaviour rule](findings/compatibility-policy.md#failure-behaviour) gives the details, and the [Rust conventions](rust-conventions.md#panics-and-errors) give the lint expectations.
5. **Unsafe code.** The target allowlist is:
   - oxpinyin-core/data/user/engine/runtime forbid `unsafe`.
   - The bidirectional `oxpinyin-input` boundary permits scoped unsafe modules.
   - `oxpinyin-store` denies `unsafe`, with scoped backend allows.
   - Public C ABI marshalling stays in `oxpinyin-capi` and `oxpinyin-zhuyin-capi`.
   - Every unsafe block has a `// SAFETY:` comment.

   The system/addon mapping boundary is active: data uses the input mapping API and has `#![forbid(unsafe_code)]`.
   Crate-root `#![forbid]` attributes and scoped allows enforce the implemented allowlist.
   `undocumented_unsafe_blocks` requires a safety comment on every unsafe block.
   `missing_safety_doc` requires a `# Safety` section on every public unsafe function or method.
   CI enforces these lints.
6. **Determinism.** On each host, output is a pure function of input, user state and configuration.
   Results that depend on the host or its libc are recorded as platform-dependent native-host behaviour.
   See the [compatibility policy](findings/compatibility-policy.md#native-rust-implementation).
7. **Dependencies.** oxpinyin depends on no other pinyin or IME crate and uses no transpiler dumps.
   Pin-built libpinyin is a test and migration subject; shipping code does not link to it.
8. **Uncertainty.** A doubt goes to the maintainer; it is not settled by improvisation.
9. **Pin-derived implementation.** The core crates are plain, idiomatic Rust.
   oxpinyin implements libpinyin's own code; it never copies another language's library implementation.
   Where the pin calls a library, oxpinyin matches that call's contract.
   At a boundary, it uses the real interface through the established crate: `cxx`, `bindgen`, `libc`, `glib-sys` or `cargo-c`.
   The pin's own code decides what oxpinyin does.
   oxpinyin code is added or removed when it differs.
   Unreleased Rust internals create no independent compatibility obligation.
   The pinned C interfaces, behaviour and persistent formats remain binding.
   In Stages 1 and 2, oxpinyin code may replace what the pin's code does with an idiomatic Rust expression of it.
   It never adds something the pin does not do.
   Code that does something the pin does not do is unnecessary and is removed.
   Divergence starts at Stage 3.
   An expectation about the pin's behaviour derives from cited lines of the pin.
   A Rust-specific safety or tooling expectation follows its own stated contract.
   A test of behaviour that survives a removal is kept, and only its fixtures are adjusted.
   A test that exists only to exercise removed behaviour is removed with it.
   Existing registered exceptions remain in force until each is revisited on its own.

## External interfaces

Rust interfaces are not a compatibility boundary.
The tools are also drop-in replacements for the pin's utilities within the [platform scope](source-policy.md#platform-scope-of-pinned-behaviour).
They preserve the pin's binary names, options, help, messages, inputs and outputs.
Their K-mixture working model uses the pin's backend DBM through `oxpinyin-store`.
The `scripts2/` Python build generators are outside this tool scope.

> The only supported external library interfaces are the libpinyin-compatible and libzhuyin-compatible C APIs and ABIs.
>
> All native Rust interfaces are implementation details. No external Rust consumers are expected or supported, and there is currently no intention to stabilize a native Rust API.
>
> Rust interfaces may be changed incompatibly as required by the implementation. Such changes require no deprecation period, backwards-compatible adapter, preservation of object safety, or accommodation of hypothetical downstream Rust implementations. All affected in-tree callers must be updated in the same change.
>
> The C APIs and ABIs, their observable behavior, ownership and lifetime rules, required diagnostics, and compatible persistent data are governed by the pinned-compatibility policy. Safety, determinism, portability, and performance requirements apply.

A Rust interface change updates all affected in-tree callers in the same change: crates, tools, tests, benches, examples and fuzz targets.
A change to a Rust public item is ordinary internal work and needs no interface-change approval.
This includes changes to traits, associated types, generic parameters, object safety, error types, visibility and module layout.

Rust public visibility, published crate names and doc comments create no stability obligation.
No wrapper, old constructor, defaulted trait method, object-safe shape or `dyn` boundary is retained solely for Rust signature compatibility.
A stable native Rust API requires a separate explicit maintainer ruling.

Oracle inputs, goldens, coefficients, data formats and the C compatibility contract follow the pin; a change to them goes through the [goldens and pins](runbooks/goldens-and-pins.md) procedure.
The SPECs and fixtures bind supported external behaviour and approved compatibility decisions, not Rust signatures, implementation designs or authored test scenarios.
The Rust surface does not justify a divergence from the pin, and it does not excuse skipping verification that the C-facing contract requires.
