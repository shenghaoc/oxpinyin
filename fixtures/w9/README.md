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

No model bytes are tracked here. The segmenter reads the fetched
`interpolation2.text` cache and the migrate-export `bigram.redb` /
`phrase_index.redb`; the counter reads `phrase_index.redb` (for the
gen_unigram freq-1 floor token set) and `segmenter-ngseg.txt`; the
emitter reads those T2 counts plus the same phrase index (for the
phrase-text columns, including `<start>` for `sentence_start`).
