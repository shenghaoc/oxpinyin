# Testing infrastructure

The oracle environment, the differential fixtures (F-series) and the
capture-fixture format. How to build and run the oracle is
`../runbooks/oracle.md`; how goldens and pins are refreshed
is `../runbooks/goldens-and-pins.md`.

For cell selection, prebuilt artifacts and the result protocol, see the
[verification procedure](../runbooks/verification.md). The
tooling-lane run records the three-cell table.

## Candidate capture

`fixtures/w4/oracle-candidate-structure.txt` is the single W4 candidate
capture. `crates/oxpinyin-runtime/tests/common/candidate_fixture.rs` reads its
structured rows and its `(input, rank, text)` projection; both retain input
identity, row order, duplicates and the captured pin reference. The
`candidate_surface` test compares oxpinyin with it over the pinned libpinyin-data
package, so run `just fetch-contract-data` before a full test run; a missing package
fails the test. The capture has no regenerator yet
([next pin capture](../runbooks/goldens-and-pins.md#the-next-pin-capture)).

## Test inputs

Tests never skip and are never `#[ignore]`d. Their inputs come from committed
fixtures or the pinned `libpinyin-data` package. Pin behaviour comes from frozen
observations, never a live pin run. A missing input is a test failure.

| Document | Subject |
| --- | --- |
| [`capture-fixtures`](capture-fixtures.md) | F-A and F-C capture fixture freeze |
| [`corpus-tail`](corpus-tail.md) | Corpus tail (W12) |
| [`f1-junk-aware-parse`](f1-junk-aware-parse.md) | F1: split `process_key` / `type_pinyin` accept sets for oracle parity |
| [`f2-unigram-tiebreak-sweep`](f2-unigram-tiebreak-sweep.md) | F2: UNIGRAM_TIEBREAK_SCALE sweep — measured |
| [`f3-bigram-kbest`](f3-bigram-kbest.md) | F3: bigram-in-kbest — negative result |
| [`fixture-adapters`](fixture-adapters.md) | W4 fixture data seam |
| [`oracle-apostrophe-abort`](oracle-apostrophe-abort.md) | Findings — pinned oracle aborts on apostrophe-only input |
| [`oracle-environment`](oracle-environment.md) | Oracle reference freeze |
| [`parity-corpus`](parity-corpus.md) | Findings — W2-T2 parity corpus |
| [`upstream-test-coverage`](upstream-test-coverage.md) | Upstream test coverage ledger |
| [`upstream-test-strategies`](upstream-test-strategies.md) | Upstream IME test-strategy study — libpinyin and libchewing |
