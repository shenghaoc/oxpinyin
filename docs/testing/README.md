# Testing infrastructure

The corpus pipeline, the oracle environment, the differential fixtures
(F-series) and the capture-fixture format. How to build and run the
oracle is `../runbooks/oracle.md`; how goldens and pins are refreshed
is `../runbooks/goldens-and-pins.md`.

Tests that need inputs CI never has (model20, the export, pin-built
tools, opencc) are `#[ignore]`d with the input named in the reason; run
them with `--include-ignored` and they fail, never skip, on a missing
input (AGENTS.md, "Tests that need inputs CI never has").

| Document | Subject |
| --- | --- |
| [`capture-fixtures`](capture-fixtures.md) | F-A and F-C capture fixture freeze |
| [`corpus-pipeline`](corpus-pipeline.md) | Corpus pipeline — zhwiki dump → ngseg raw text (W9-T4b) |
| [`corpus-tail`](corpus-tail.md) | Corpus tail (W12) |
| [`f1-junk-aware-parse`](f1-junk-aware-parse.md) | F1: split `process_key` / `type_pinyin` accept sets for oracle parity |
| [`f2-unigram-tiebreak-sweep`](f2-unigram-tiebreak-sweep.md) | F2: UNIGRAM_TIEBREAK_SCALE sweep — measured |
| [`f3-bigram-kbest`](f3-bigram-kbest.md) | F3: bigram-in-kbest — negative result |
| [`fixture-adapters`](fixture-adapters.md) | W4 fixture data seam |
| [`oracle-apostrophe-abort`](oracle-apostrophe-abort.md) | Findings — pinned oracle aborts on apostrophe-only input |
| [`oracle-bisect-differential-abort`](oracle-bisect-differential-abort.md) | Findings — pinned oracle aborts under bisect's differential mode |
| [`oracle-environment`](oracle-environment.md) | Oracle reference freeze |
| [`oracle-ffi-seam`](oracle-ffi-seam.md) | Findings — W2-T1 oracle FFI seam |
| [`parity-corpus`](parity-corpus.md) | Findings — W2-T2 parity corpus |
| [`upstream-test-coverage`](upstream-test-coverage.md) | Upstream test coverage ledger |
| [`upstream-test-strategies`](upstream-test-strategies.md) | Upstream IME test-strategy study — libpinyin and libchewing |
