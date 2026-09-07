# Testing infrastructure

Testing infrastructure: the corpus pipeline, oracle environment,
differential fixtures (F-series), and the capture-fixtures format. The
existing `upstream-test-coverage.md` documents upstream test strategy
imports from libpinyin and libchewing.

Tests that need inputs CI never has (model20, the export, pin-built
tools, opencc) are `#[ignore]`d with the input named in the reason; run
them with `--include-ignored` and they fail, never skip, on a missing
input (AGENTS.md, "Tests that need inputs CI never has").
