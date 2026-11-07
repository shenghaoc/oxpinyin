# W9 training fixtures

`segmenter-han.txt` is a **synthetic**, hand-written Han-text sample used
to test the `ngseg` utility against libpinyin `ngseg`. It is
not a training corpus and is not derived from Wikipedia or any other
third-party dump.

`segmenter-ngseg.txt` is the stdout of pin-built `ngseg` on that sample,
using the pinned system model (`table.conf` λ = 0.312699). The utility
must emit the same bytes.

`segmenter-spseg-w3.txt` is the fewest-words `spseg` reproduction over the
same `segmenter-han.txt`, but produced against the **committed W3
phrase_index** (`fixtures/w3/phrase_index.<backend>`) rather than the full
system export. The W3 table is a real, committed system table but a
*subset* of the shipped dictionary, so some single Han characters segment
as unknown (`0 …`) runs. Because that table is committed, the `spseg`
differential (`crates/oxpinyin-utils/tests/segmentation.rs`) runs unconditionally on CI.

`mergeseq-input.txt` is a small hand-built segmented stream over the W3
vocabulary — `中`(16786821) + `国`(16780275), a separator, then a
pass-through `你好`(16802309) — chosen so the merge to `中国`(16817937) is
exercised. `segmenter-mergeseq-w3.txt` is the `mergeseq` reproduction over
it (again against the committed W3 phrase_index).

No model bytes are tracked here.
