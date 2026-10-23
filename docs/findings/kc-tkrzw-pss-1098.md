# KC and tkrzw Pss attribution (#1098)

No current Pss, heap or performance result is available.

## Comparison-stage extension

At source `747b00cd`, only KC/tkrzw profile provisioning was supported.
Confirmation and attribution were refused because final-profile comparison
handled BDB containers only. This extension resolves that source limitation
before measurements: each peer backend's user indexes and user bigram are
compared by all logical records, including empty values. KC loads snapshots;
tkrzw opens TreeDBM indexes and HashDBM bigrams read-only. Unknown files
remain byte comparisons. Existing pinyin-index padding and chunk-header
timestamp rules remain the only normalization. BDB behavior is preserved.

Peer readers run as a mode of the existing isolated contract worker, so
backend native libraries do not remain mapped in the controller between
samples. Runtime checks cover each actual copied object before native,
fixture-population and comparison launches. The tkrzw reader loads the exact
certified runtime path; errors refuse a comparison rather than producing
an empty or partial database result. No new driver or dependency is added.

The source basis was read with
`git -C /home/sheng/repos/libpinyin show 074a2219:PATH`:
`src/storage/ngram_kyotodb.cpp:54-64,82-91` loads/saves the user bigram as
a snapshot; `src/storage/ngram_tkrzwdb.cpp:48-81` uses a HashDBM temporary;
`src/storage/phrase_large_table3_tkrzwdb.cpp:83-118` uses TreeDBM;
`src/storage/chewing_large_table2_tkrzwdb.cpp:92-106` loads TreeDBM records.
These are source-derived storage requirements, not performance results.

The existing worker and readers already provide the relevant primitives:
`teardown.py::mapping_group` and `read_memory` read mapping classes and
checkpoint totals from the same smaps observation; the teardown worker
pauses at M0 and M1. `tools/measure-function-order.py::teardown_memory`
reports total Pss from a later smaps_rollup read. Its text-Pss measurement
uses the earlier smaps read. These are different sampling definitions;
report any rollup-versus-mapping-sum difference explicitly rather than
silently substituting one for the other.

## References

- [Issue #1098](https://github.com/shenghaoc/oxpinyin/issues/1098).
- [BDB attribution method](init-attribution-2026-10-22.md).
- [Backend shipping policies and historical controls](backend-function-order-1076.md).
- [Section 7.5](tooling-consolidation-python-just.md#75-metrics-and-statistics).
