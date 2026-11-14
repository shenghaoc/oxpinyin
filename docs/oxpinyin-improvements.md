# oxpinyin improvements

This page covers what oxpinyin does that the pinned libpinyin does not.
The [pinned-behaviour page](pinned-behaviour.md) defines the target.
The [compatibility policy](findings/compatibility-policy.md) defines permitted differences.

## Observable differences

Every observable difference outside the established exceptions is a defect to revert.
The compatibility policy's [decision order](findings/compatibility-policy.md#decision-order) decides each one.
A new entry is justified under its class or its own [divergence register](findings/upstream-divergences.md) entry.
None is admitted by analogy.

See the policy's [exception classes](findings/compatibility-policy.md#exception-classes),
[standing rulings](findings/compatibility-policy.md#standing-maintainer-rulings)
and the [divergence register](findings/upstream-divergences.md).

## Internal improvements

The [constitution](constitution.md) governs pin-matched failures, class (b), unsafe code and determinism.
The [failure-behaviour rule](findings/compatibility-policy.md#failure-behaviour) gives the abort and error-handling details.
The [Rust conventions](rust-conventions.md) define lint denials and narrow justified expectations.
Those lints do not prove behavioural correspondence.

oxpinyin reduces libpinyin's tight coupling and preserves behaviour.
The additional `oxpinyin-input` crate serves that goal.
The aim is fewer unnecessary dependencies between crates, not fewer crates.
Unrelated workspace parts are not restructured.

Passing a concrete type through its consumers is not a generic-runtime rewrite.
Type erasure is used only for runtime heterogeneity, a standard or foreign interface, or a measured trade-off.
`dyn` is not retained merely to preserve a Rust signature.

The [interface ruling](constitution.md#external-interfaces) makes Rust interfaces internal.
Rust signatures change freely, and the same change updates every in-tree caller.
The supported external interfaces are the C APIs and ABIs; the tools stay drop-in replacements for the pin's utilities.

## Performance

Stage 2 code may replace how existing required work is represented or performed.
It adds no unapproved capability, independently maintained parallel product model or observable behaviour; the compatibility policy governs permitted differences.
Optional retained complexity, such as a cache, must justify itself in memory, execution time or binary size.
Existing registered exceptions remain in force until each is revisited on its own.
An implementation can be faster or smaller than the pin by any amount within that limit.
The complexity rule still applies ([performance standard](source-policy.md#performance-standard)).
Measurement is the exception ([controlling rule](source-policy.md#controlling-rule)).
The [roadmap](../ROADMAP.md) lists the open internal improvements.

## Stage 3

Stage 3 follows Stage 2.
A Stage 3 departure adds an unapproved capability or independently maintained parallel product model, or intentionally changes observable output, files or the ABI beyond what the compatibility policy permits.
Examples include a new language model, different candidate behaviour or an incompatible data format.
The [source policy](source-policy.md#stage-3) states what a departure requires: an opt-in cargo feature, off by default, a default build that stays bug-for-bug compatible, and no claim of libpinyin's binary identity for a build that breaks the ABI or data formats.
See the [Stage 3 roadmap](../ROADMAP.md#stage-3-after-stage-2).
