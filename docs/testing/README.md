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
retained speed code with its plain path. The Pinyin C crate tests its output allocations in `ffi/tests.rs` with the production helpers and their matching deallocators.
A separate test target stays only where a compilation boundary is needed.

A verification test checks behaviour at an interface that a consumer uses: an
exported C function of `oxpinyin-capi` or `oxzhuyin-capi`, or the
arguments, standard streams, exit status and files of a utility or of the REPL.
It lives in the crate's one test target, `tests/it/main.rs`, with one module for
each topic, and uses nothing else.

On Linux, `just abi` compiles a C++ consumer for each public header and links
it against the corresponding shared library. The same command runs the
non-shipping Rust test in `tools/abi`, which loads both libraries and checks
allocation ownership under AddressSanitizer with leak detection enabled.

A helper that more than one topic uses lives
in `tests/it/support.rs`. An abort test names itself by its full path.

A test helper has one definition. In each C crate, `src/test_support.rs` and
`tests/it/support.rs` include the same shared files by `#[path]`:
`src/test_support/abi.rs` for that crate's C-function helpers, and
`tests/support/c_harness.rs` and `tests/support/native_fixture.rs` for the
helpers that both C crates use. A test build binds the crate's own name with
`extern crate self`, so a shared file names the exported functions the same
way in both targets.

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

The project tests product behaviour. Internal development tools have no automated test suite.

## Candidate capture

`fixtures/w4/oracle-candidate-structure.txt` is the single candidate
capture. `tests/common/candidate_fixture.rs` reads its
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
| [`fixture-adapters`](fixture-adapters.md) | Fixture data seam |
| [`upstream-test-coverage`](upstream-test-coverage.md) | Upstream test coverage ledger and fuzz harnesses |
