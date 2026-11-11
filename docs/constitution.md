# Constitution

oxpinyin implements libpinyin and libzhuyin in native Rust.
It preserves the pin's outward behaviour at the C API and in persistent data.
The [roadmap](../ROADMAP.md) records the stages and their status.

1. **Broad appeal.** Do not add a niche feature at a cost to all users.
2. **Binary size (ruling 2026-10-18 UTC).** Record stripped shipping-library file sizes for changes that affect shipping libraries.
   Report increases over the base in the PR.
   Minor decreases need no report.
   Documentation-only changes need no size build or report.
   There is no pin +10% cap.
   CI reports the sizes for each pull request that can change them.
   See [shipping-library sizes](packaging.md#shipping-library-sizes).
3. **No local AI.** Do not add client neural inference or LLM inference.
4. **Failure behaviour.** Follow the pin's observable failure behaviour.
   Where the pin asserts or aborts, the C adapters and the drop-in tools call `std::process::abort()`, after the pin's preceding work.
   The shared Rust code returns a typed error at the pin's failure point so that they can do this.
   Never implement a reachable pin abort with `panic!`, `assert!` or `unwrap()`.
   Permit a panic-producing assertion only where its failure is proven unreachable.
   Do not introduce a panic where the pin continues or returns an error.
   Use idiomatic Rust error handling; internal Rust APIs do not all need to return `Result`.
   Class (b) rulings stay authoritative.
   The [failure-behaviour rule](findings/compatibility-policy.md#failure-behaviour) gives the details, and the [Rust conventions](rust-conventions.md#panics-and-errors) give the lint expectations.
5. **Unsafe code.** Use the following target allowlist:
   - Forbid `unsafe` in oxpinyin-core/data/user/engine/runtime.
   - Permit scoped unsafe modules in the bidirectional `oxpinyin-input` boundary.
   - Deny `unsafe` in `oxpinyin-store`, with scoped backend allows.
   - Keep public C ABI marshalling in `oxpinyin-capi` and `oxpinyin-zhuyin-capi`.
   - Give every unsafe block a `// SAFETY:` comment.

   The system/addon mapping boundary is active: data uses the input mapping API and has `#![forbid(unsafe_code)]`.
   Crate-root `#![forbid]` attributes and scoped allows enforce the implemented allowlist.
   `undocumented_unsafe_blocks` requires a safety comment on every unsafe block.
   `missing_safety_doc` requires a `# Safety` section on every public unsafe function or method.
   CI enforces these lints.
6. **Determinism.** On each host, output is a pure function of input, user state and configuration.
   Record results that depend on the host or its libc as platform-dependent native-host behaviour.
   See the [compatibility policy, #1303](findings/compatibility-policy.md#native-rust-implementation).
7. **Dependencies.** Do not depend on other pinyin or IME crates.
   Do not use transpiler dumps.
   Pin-built libpinyin is a test and migration subject.
   Do not link shipping code to it.
8. **Uncertainty.** Stop when you are in doubt.
   Do not improvise.

## External interfaces

Maintainer ruling 2026-10-18 UTC.

Rust interfaces are not a compatibility boundary.
The 2026-10-28 UTC ruling also makes the tools drop-in replacements for the pin's utilities.
Preserve their binary names, options, help, messages, inputs and outputs.
Their K-mixture working model uses the pin's backend DBM through `oxpinyin-store`.
The `scripts2/` Python build generators are outside this tool scope.

> The only supported external library interfaces are the libpinyin-compatible and libzhuyin-compatible C APIs and ABIs.
>
> All native Rust interfaces are implementation details. No external Rust consumers are expected or supported, and there is currently no intention to stabilize a native Rust API.
>
> Rust interfaces may be changed incompatibly as required by the implementation. Such changes require no deprecation period, backwards-compatible adapter, preservation of object safety, or accommodation of hypothetical downstream Rust implementations. All affected in-tree callers must be updated in the same change.
>
> This ruling supersedes earlier Rust-interface stability promises, interface freezes, and restrictions imposed solely for Rust source compatibility.
>
> The C APIs and ABIs, their observable behavior, ownership and lifetime rules, required diagnostics, and compatible persistent data remain governed by the existing pinned-compatibility policy. Safety, determinism, portability, and performance requirements remain in force.

Update all affected in-tree callers in the same change: crates, tools, tests, benches, examples and fuzz targets.
A change to a Rust public item is ordinary internal work and does not require an interface-change STOP.
This includes changes to traits, associated types, generic parameters, object safety, error types, visibility and module layout.

Rust public visibility, published crate names and doc comments create no stability obligation.
Historical promises that a Rust interface is frozen or unchanged create no obligation either.
Do not retain a wrapper, old constructor, defaulted trait method, object-safe shape or `dyn` boundary solely for Rust signature compatibility.
A stable native Rust API requires a separate explicit maintainer ruling.

Keep frozen oracle inputs, goldens, coefficients, behavioural SPECs, data formats and the C compatibility contract frozen.
Only freezes of Rust signatures are retired by this ruling.
Do not use the Rust surface to justify a divergence from the pin.
Do not skip verification that the C-facing contract requires.
