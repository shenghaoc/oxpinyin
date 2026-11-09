# oxpinyin improvements

This page covers what oxpinyin does that the pinned libpinyin does not.
The [pinned-behaviour page](pinned-behaviour.md) defines the target.
The [compatibility policy](findings/compatibility-policy.md) defines permitted differences.

## Observable differences

Every observable difference outside the established exceptions is a defect to revert.
The compatibility policy's [decision order](findings/compatibility-policy.md#decision-order) decides each one.
Justify a new entry under its class or its own discrepancy record.
Do not admit one by analogy.

See the policy's [exception classes](findings/compatibility-policy.md#exception-classes),
[standing rulings](findings/compatibility-policy.md#standing-maintainer-rulings)
and the [divergence register](findings/upstream-divergences.md).

## Internal improvements

Follow the [constitution](constitution.md) for pin-matched failures, class (b), unsafe code and determinism.
The [failure-behaviour rule](findings/compatibility-policy.md#failure-behaviour) gives the abort and error-handling details.
The [Rust conventions](rust-conventions.md) define lint denials and narrow justified expectations.
Those lints do not prove behavioural correspondence.

Reduce libpinyin's tight coupling and preserve behaviour.
The additional `oxpinyin-input` crate serves that goal.
Prefer fewer unnecessary dependencies between crates, not fewer crates.
Do not restructure unrelated workspace parts.

Passing a concrete type through its consumers is not a generic-runtime rewrite.
Use type erasure only for runtime heterogeneity, a standard or foreign interface, or a measured trade-off.
Do not retain `dyn` merely to preserve a Rust signature.

The [interface ruling](constitution.md#external-interfaces) (maintainer ruling 2026-10-18 UTC) makes Rust interfaces internal.
Change Rust signatures freely, and update every in-tree caller in the same change.
The supported external interfaces are the C APIs and ABIs; the tools stay drop-in replacements for the pin's utilities.

## Performance

Stage 2 permits internal performance, memory and safety work with no observable change.
An implementation can be faster or smaller than the pin by any amount.
The floor is the complexity rule, derived from the source and compared with the pin ([performance standard](source-policy.md#performance-standard)).
Measurement is the exception ([controlling rule](source-policy.md#controlling-rule)).
The [roadmap](../ROADMAP.md) lists the open internal improvements.

## Stage 3

Stage 3 follows Stage 2.
A Stage 3 departure intentionally changes observable output, files or the ABI.
Examples include a new language model, different candidate behaviour or an incompatible data format.
The [source policy](source-policy.md#stage-3) states what a departure requires: an opt-in cargo feature, off by default, a default build that stays bug-for-bug compatible, and no claim of libpinyin's binary identity for a build that breaks the ABI or data formats.
See the [Stage 3 roadmap](../ROADMAP.md#stage-3-after-stage-2).
