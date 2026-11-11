# W9 training fixtures

This directory holds the inputs and goldens of the segmentation utility tests.
No model bytes are tracked here.

- `segmenter-han.txt` is a **synthetic**, hand-written Han-text sample. The tests use it
  to compare the `ngseg` utility with libpinyin `ngseg`. It is
  not a training corpus. It does not come from Wikipedia or any other
  third-party dump.
- `segmenter-spseg-w3.txt` is the fewest-words `spseg` reproduction over the
  same `segmenter-han.txt`. It is produced against the **committed W3
  phrase_index** (`fixtures/w3/<backend>/phrase_index.bin`), not against the full
  system export. The W3 table is a real, committed system table, but it is a
  *subset* of the shipped dictionary. Some single Han characters therefore segment
  as unknown (`0 …`) runs. That table is committed, so the `spseg`
  differential (`crates/oxpinyin-utils/tests/segmentation.rs`) runs unconditionally on CI.
- `mergeseq-input.txt` is a small hand-built segmented stream over the W3
  vocabulary: `中`(16786821) + `国`(16780275), a separator, then a
  pass-through `你好`(16802309). It is chosen so that the merge to `中国`(16817937) is
  exercised.
