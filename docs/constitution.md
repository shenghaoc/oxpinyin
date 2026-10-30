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
   See the [size policy](perf/ci-perf-size-gate-proposal-2026-09-09.md).
3. **No local AI.** Do not add client neural inference or LLM inference.
4. **Panics and error handling.** Follow the pin's observable failure behaviour.
   Permit a Rust panic only where the pin asserts or aborts on the corresponding condition.
   An existing compatibility ruling can require a different outcome.
   Do not introduce a panic where the pin continues or returns an error.
   Use idiomatic Rust error handling.
   Internal Rust APIs do not all need to return `Result`.
   Use ordinary operations and assertions only when their possible failure paths satisfy this rule.
   Existing class (b)/(c) rulings remain authoritative.
5. **Unsafe code.** Use the following target allowlist:
   - Forbid `unsafe` in oxpinyin-core/data/user/engine/runtime.
   - Permit scoped unsafe modules in the bidirectional `oxpinyin-input` boundary.
   - Deny `unsafe` in `oxpinyin-store`, with scoped backend allows.
   - Keep public C ABI marshalling in capi/oracle.
   - Give every unsafe block a `// SAFETY:` comment.

   Until the boundary is implemented, data retains its existing `deny` and mmap exception.
   Crate-root `#![forbid]` attributes and scoped allows enforce the implemented allowlist.
   `undocumented_unsafe_blocks` requires a safety comment on every unsafe block.
   `missing_safety_doc` requires a `# Safety` section on every public unsafe function or method.
   CI enforces these lints.
   This description supplies context; the compiler and lint checks enforce the rule.
6. **Determinism.** On each host, output is a pure function of input, user state and configuration.
   Record results that depend on the host or its libc as platform-dependent native-host behaviour.
   See the [compatibility policy, #1303](findings/compatibility-policy.md#native-rust-implementation).
7. **Dependencies.** Do not depend on other pinyin or IME crates.
   Do not use transpiler dumps.
   Pin-built libpinyin is a test and migration subject.
   Do not link shipping code to it.
8. **Uncertainty.** Stop when in doubt.
   Do not improvise.

Constitution §4 amended by maintainer decision 2026-10-29 UTC; pin:
libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`.

Existing class (c) refusals still return `false`/`Err` and emit exactly one warning per occurrence.
The [panic amendment](findings/compatibility-policy.md#amendment--pin-matched-panics-and-error-handling) states the scoped lint expectations and their limits.
See also [safety-profile Layer 3](safety/oxpinyin-safety-profile.md).

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

Update all affected in-tree callers in the same change.
This includes crates, tools, tests, benches, examples and fuzz targets.
A change to a Rust public item is ordinary internal work.
This includes traits, associated types, generic parameters, object safety, error types, visibility and module layout.
It does not require an interface-change STOP.

Rust public visibility, published crate names and doc comments create no stability obligation.
Historical promises that a Rust interface is frozen or unchanged create no obligation either.
Do not retain a wrapper, old constructor, defaulted trait method, object-safe shape or `dyn` boundary solely for Rust signature compatibility.
A stable native Rust API requires a separate explicit maintainer ruling.

Keep frozen oracle inputs, goldens, coefficients, behavioural SPECs, data formats and the C compatibility contract frozen.
Only freezes of Rust signatures are retired by this ruling.
Do not use the Rust surface to justify a divergence from the pin.
Do not skip verification that the C-facing contract requires.
