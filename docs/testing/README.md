# Testing infrastructure

This directory records the differential fixtures (F-series) and the capture-fixture format.
To build and run the oracle, see [the oracle procedure](../runbooks/oracle.md).
To refresh goldens and pins, see [goldens and pins](../runbooks/goldens-and-pins.md).

For cell selection, prebuilt artifacts and the result protocol, see the
[verification procedure](../runbooks/verification.md).

## Where tests belong

oxpinyin has two kinds of test.

A module test checks the behaviour and invariants of its own crate, normally
through the crate's production entries. It lives in the module's sibling
`tests.rs`, declared as `#[cfg(test)] mod tests;`, and may use the crate's
private items. It calls a private helper directly only when the production entry
cannot reach the case, reaches it only at a high cost, or when it compares
retained speed code with its plain path. A separate test target stays only where
a compilation boundary is needed: `oxpinyin-capi-marshal`'s macro tests expand
`unsafe` code that the library's `forbid(unsafe_code)` rejects.

A verification test checks behaviour at an interface that a consumer uses: an
exported C function of `oxpinyin-capi` or `oxpinyin-zhuyin-capi`, or the
arguments, standard streams, exit status and files of a utility or of the REPL.
It lives in the crate's one test target, `tests/it/main.rs`, with one module for
each topic, and uses nothing else. A helper that more than one topic uses lives
in `tests/it/support.rs`. An abort test names itself by its full path.

A behaviour is tested where it is implemented. A higher crate tests only the
decisions and connections that it adds. A test exists for a failure mechanism
that no other test protects; failure ordering and persisted file state are
mechanisms too. Different inputs or layers alone do not justify a test, and
neither does a sibling entry point alone (another utility, REPL mode, parser
scheme or C facade); its own dispatch, conversion, error propagation or
observable effects do. A test is redundant only when another test's assertions
protect the same required behaviour. One death test covers a shared abort path
when the decisions that trigger it and the effects visible before it are
checked elsewhere; a route whose abort is the only witness of its effects keeps
its own.

Every expected result has a basis: a frozen pin observation, cited pin lines, a
row of the divergence register, or a stated Rust contract. Tests prefer real
input and environmental failures. A minimal non-shipping seam stays only where
it is the sole adequate witness of a distinct production failure mechanism, and
no test bypasses an internal invariant to make a state that production cannot
reach; specified handling of malformed external input stays testable. The REPL
has one session per mode, plus tests of the decisions it makes itself. The
crates have no doc tests (`doctest = false`).

A Python tooling test protects a consequential observable contract: a result,
an exit status, argument transport or a file effect. A test that only repeats
the calls the code makes is not kept. For non-shipping tooling, retain tests for
failures that could silently invalidate results, cause damage or leak resources,
require expensive recovery, or compromise automated consumers; failures reliably
detected before consequential work during ordinary manual use need no dedicated
test.

## Candidate capture

`fixtures/w4/oracle-candidate-structure.txt` is the single W4 candidate
capture. `crates/oxpinyin-runtime/tests/common/candidate_fixture.rs` reads its
structured rows and its `(input, rank, text)` projection. Both keep the input
identity, row order, duplicates and the captured pin reference.

The `candidate_surface` test compares oxpinyin with the capture over the pinned libpinyin-data
package. `just fetch-contract-data` runs before a full test run; a missing package
fails the test. The capture has no regenerator yet
([next pin capture](../runbooks/goldens-and-pins.md#the-next-pin-capture)).
The construction that it checks is
[candidate-construction §8](../findings/candidate-construction.md#8-the-construction-contract).

A change to the capture's inputs is a deliberate, reviewed act. It moves the `candidate_surface`
counts and needs a new pin capture, so it goes in its own commit with the edited capture and
the reason for the change.

## Test inputs

Tests never skip and are never `#[ignore]`d. Their inputs come from committed
fixtures or the pinned `libpinyin-data` package. Pin behaviour comes from frozen
observations, never from a live pin run. A missing input is a test failure.

A new regression uses reduced input and explains any substantial fixture or runtime cost. It extends existing cases where that keeps the test clear, and a live comparison or a compact expectation replaces bulk observations where it suffices.

| Document | Subject |
| --- | --- |
| [`capture-fixtures`](capture-fixtures.md) | F-A and F-C capture fixture freeze |
| [`fixture-adapters`](fixture-adapters.md) | W4 fixture data seam |
| [`upstream-test-coverage`](upstream-test-coverage.md) | Upstream test coverage ledger and fuzz harnesses |
