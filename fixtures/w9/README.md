# W9 training fixtures

`segmenter-han.txt` is a **synthetic**, hand-written Han-text sample used
to differential-test the Rust segmenter against libpinyin `ngseg`. It is
not a training corpus and is not derived from Wikipedia or any other
third-party dump.

`segmenter-ngseg.txt` is the stdout of pin-built `ngseg` on that sample,
using the pinned system model (`table.conf` λ = 0.312699). The Rust
segmenter must emit the same bytes.

`counter-ngram.manifest` pins the value-level result of the W9-T2
`pinyin-counter` over `segmenter-ngseg.txt`: the unigram/bigram counts and
an FNV-1a 64-bit checksum of the full `Counts::dump()`. The full dump is
~1.5 MB of integer counts (the phrase index's token-id space), so only the
checksum and shape are tracked; the live differential test reconstructs the
full value map from pin-built `gen_binary_files` → `gen_unigram` →
`gen_ngram` → `export_interpolation` when those binaries are available.

`interpolation2.manifest` pins the value-level result of the W9-T4a
`pinyin-emitter` over the same T1→T2 chain: the unigram/bigram record
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

`corpus-sample.txt` is the `pinyin-corpus` output for that fixture (with
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
