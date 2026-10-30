# oxpinyin improvements

This page covers what oxpinyin does that the pinned libpinyin does not.
The [pinned-behaviour page](pinned-behaviour.md) defines the target.
The [compatibility policy](findings/compatibility-policy.md) defines permitted differences.

## Observable differences

Anything observable outside the established exceptions is a defect to revert.
Follow the policy's decision order:

1. Preserve the C-facing contract.
2. Apply class (a)–(c).
3. Apply the external-I/O call rule or record an internal language/runtime quirk.
4. Obtain a ruling for a product change or use Stage 3.
5. Revert any other difference as a defect.

Justify a new entry under its class or its own discrepancy record.
Do not admit one by analogy.

- **(a) MATH:** platform-dependent floating-point accumulation with a transcendental, not merely a float in the call graph.
  The sentence surface's six remaining rows belong here after the trellis selection rules were ported (row 11, #535).
  Use Rust's standard maths at full speed.
  Record resulting last-bit differences as class (a).
  Do not chase them.
- **(b) MEMORY SAFETY:** the pin has undefined behaviour that Rust structurally prevents reproducing.
- **(c) AVAILABILITY:** the pin asserts or aborts on input supplied by a caller or file.
  Return `false`/`Err` and emit exactly one `g_warning`-level log line per occurrence (#525, #549).
  This class covers aborts, not wrong-but-defined answers.
  It applies uniformly to every verified abort or segfault site.
  Reproduce an unverified site first.
  There is no fourth "tolerate and warn" class.
- **(d) CONSUMER SCOPE:** retired 2026-09-06.

The language/runtime-quirk exception is outside the classes.
It does not change the C ABI, ownership, real `GArray`/`g_free`, on-disk formats or data ownership.
Use the [discrepancy template](findings/language-runtime-discrepancy-template.md).
Register the record by name in the [divergence register](findings/upstream-divergences.md).
Retain a `POLICY-ACCEPTED` differential case.
One record does not admit another by analogy.
Do not reproduce an internal language artefact on valid or malformed input.
No approval is needed for its record.

Use the pin's external I/O call unless source proves a Rust equivalent for every input.
The call rule does not reproduce undefined reads or aborts.
Class (b)/(c) records stand.
Stop for a product-level change or an unruled reversal of a prior ruling.
See the [native Rust policy](findings/compatibility-policy.md#native-rust-implementation).

Registered standing divergences are individual entries under the exception.
The existing example is tkrzw's `SYSTEM_ERROR`/`UNKNOWN_ERROR` collapse through its C API.
The divergence register holds these entries.
Report their notes upstream after the rewrite.

## Internal improvements

Follow the [constitution](constitution.md) for pin-matched failures, class (b)/(c) refusals, unsafe code and determinism.
Keep class (c)'s `false`/`Err` and exactly one warning per occurrence.
No existing ruling is reversed.
Internal Rust APIs do not all need to return `Result`.
The [Rust conventions](rust-conventions.md) define lint denials and narrow justified expectations.
Those lints do not prove behavioural correspondence.

Reduce libpinyin's tight coupling while preserving behaviour.
The additional `oxpinyin-input` crate serves that goal.
Prefer fewer unnecessary dependencies between crates, not fewer crates.
Do not restructure unrelated workspace parts.
Passing a concrete type through its consumers is not a generic-runtime rewrite.
Use type erasure only for runtime heterogeneity, a standard or foreign interface, or a measured trade-off.
Do not retain `dyn` merely to preserve a Rust signature.

Rust interfaces are not a compatibility boundary (maintainer ruling 2026-10-18 UTC).
Only the C APIs and ABIs are supported external library interfaces.
Tools remain drop-in pin-utility replacements.
Change Rust signatures freely while updating every in-tree caller in the same change.
Follow the [interface ruling](constitution.md#external-interfaces).

Do not worsen time and space complexity together.
Trade any regression in one for a gain in the other.
Minimize and justify the regression.

## Performance

Stage 2 permits internal performance, memory and safety work with no observable change.
An implementation can be faster or smaller than the pin by any amount.
The floor is the complexity rule, measured against the pin (axis L, #757).
The [roadmap](../ROADMAP.md) records landed work.

## Stage 3

Stage 3 follows Stage 2.
A departure intentionally changes observable output, files or ABI.
Examples include a new language model, different candidate behaviour or an incompatible data format.
Put each departure behind an opt-in cargo feature, off by default.
Keep the default build bug-for-bug compatible.
Do not claim libpinyin's binary identity for a build that breaks the ABI or data formats.
See the [Stage 3 roadmap](../ROADMAP.md#stage-3--opt-in-departures-after-stage-2).
