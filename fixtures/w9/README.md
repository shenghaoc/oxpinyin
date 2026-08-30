# W9 training fixtures

`segmenter-han.txt` is a **synthetic**, hand-written Han-text sample used
to differential-test the Rust segmenter against libpinyin `ngseg`. It is
not a training corpus and is not derived from Wikipedia or any other
third-party dump.

`segmenter-ngseg.txt` is the stdout of pin-built `ngseg` on that sample,
using the pinned system model (`table.conf` λ = 0.312699). The Rust
segmenter must emit the same bytes.

`segmenter-spseg-w3.txt` is the fewest-words `spseg` reproduction over the
same `segmenter-han.txt`, but produced against the **committed W3
phrase_index** (`fixtures/w3/phrase_index.<backend>`) rather than the full
system export. The W3 table is a real, committed system table but a
*subset* of the shipped dictionary, so some single Han characters segment
as unknown (`0 …`) runs. Because that table is committed, the `spseg`
differential (`tests/spseg_mergeseq.rs`) runs unconditionally on CI; the
live cross-check against pin-built `spseg` runs only when `PINYIN_SPSEG`
and the full oracle data dir are set.

`mergeseq-input.txt` is a small hand-built segmented stream over the W3
vocabulary — `中`(16786821) + `国`(16780275), a separator, then a
pass-through `你好`(16802309) — chosen so the merge to `中国`(16817937) is
exercised. `segmenter-mergeseq-w3.txt` is the `mergeseq` reproduction over
it (again against the committed W3 phrase_index). The live cross-check
against pin-built `mergeseq` (`PINYIN_MERGESEQ`) uses the full-dict
`segmenter-ngseg.txt` as its shared input instead.

`counter-ngram.manifest` pins the value-level result of the W9-T2
`oxpinyin-counter` over `segmenter-ngseg.txt`: the unigram/bigram counts and
an FNV-1a 64-bit checksum of the full `Counts::dump()`. The full dump is
~1.5 MB of integer counts (the phrase index's token-id space), so only the
checksum and shape are tracked; the live differential test reconstructs the
full value map from pin-built `gen_binary_files` → `gen_unigram` →
`gen_ngram` → `export_interpolation` when those binaries are available.

`interpolation2.manifest` pins the value-level result of the W9-T4a
`oxpinyin-emitter` over the same T1→T2 chain: the unigram/bigram record
counts and an FNV-1a 64-bit checksum of the emitted `interpolation2.text`
(header, `\item` lines with phrase text, both n-gram sections, no λ).
The live differential reconstructs the pin-built
`export_interpolation` dump when those binaries are available.

`corpus-xml.xml` (W9-T4b) is a tiny XML-export fixture: three verbatim
`<page>` blocks (`裸茎金腰`, `昌国街道`, `苇河镇`) from a real
zh.wikipedia.org export, all-simplified so CI can run the cleaner
without an `opencc` binary. It is real markup — infobox templates,
`<ref>`s, tables, file links — deliberately, because synthetic text
would not exercise the stripper.

`corpus-sample.txt` is the `oxpinyin-corpus` output for that fixture (with
the opencc `t2s` conversion applied — the identity on all-simplified
input). It is the committed raw-text sample: tier-1 well-formedness
invariants run on it, and tier-2 feeds it to T1 and to pin `ngseg` with
zero glue.

**Attribution:** `corpus-xml.xml` and `corpus-sample.txt` are derived
from zh.wikipedia.org article text, licensed **CC BY-SA 4.0**
(https://creativecommons.org/licenses/by-sa/4.0/), © the respective
Wikipedia contributors. Insubstantial excerpts (≈6 KB / ≈0.8 KB) kept
solely for tests; no dump and no corpus are committed. Reuse of these
files is under the same licence.

No model bytes are tracked here. The segmenter reads the fetched
`interpolation2.text` cache and the migrate-export `bigram.redb` /
`phrase_index.redb`; the counter reads `phrase_index.redb` (for the
gen_unigram freq-1 floor token set) and `segmenter-ngseg.txt`; the
emitter reads those T2 counts plus the same phrase index (for the
phrase-text columns, including `<start>` for `sentence_start`).
