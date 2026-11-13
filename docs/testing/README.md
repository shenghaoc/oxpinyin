# Testing infrastructure

This directory records the differential fixtures (F-series) and the capture-fixture format.
To build and run the oracle, see [the oracle procedure](../runbooks/oracle.md).
To refresh goldens and pins, see [goldens and pins](../runbooks/goldens-and-pins.md).

For cell selection, prebuilt artifacts and the result protocol, see the
[verification procedure](../runbooks/verification.md).

## Where tests belong

A behaviour is tested where it is implemented. A higher layer tests only the decisions and connections that it adds.
A test exists for a failure mechanism that no other test protects; different inputs or execution layers alone do not justify one.
A new regression uses reduced input and explains any substantial fixture or runtime cost. It extends existing cases where that keeps the test clear, and a live comparison or a compact expectation replaces bulk observations where it suffices.

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
[candidate-construction §8](../findings/candidate-construction.md#8-the-frozen-construction-contract).

A change to the capture's inputs is a deliberate, reviewed act. It moves the `candidate_surface`
counts and needs a new pin capture, so it goes in its own commit with the edited capture and
the reason for the change.

## Test inputs

Tests never skip and are never `#[ignore]`d. Their inputs come from committed
fixtures or the pinned `libpinyin-data` package. Pin behaviour comes from frozen
observations, never from a live pin run. A missing input is a test failure.

| Document | Subject |
| --- | --- |
| [`capture-fixtures`](capture-fixtures.md) | F-A and F-C capture fixture freeze |
| [`fixture-adapters`](fixture-adapters.md) | W4 fixture data seam |
| [`upstream-test-coverage`](upstream-test-coverage.md) | Upstream test coverage ledger and fuzz harnesses |
