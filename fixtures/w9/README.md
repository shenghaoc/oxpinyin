# W9-T1 segmenter fixture

`segmenter-han.txt` is a **synthetic**, hand-written Han-text sample used
to differential-test the Rust segmenter against libpinyin `ngseg`. It is
not a training corpus and is not derived from Wikipedia or any other
third-party dump.

`segmenter-ngseg.txt` is the stdout of pin-built `ngseg` on that sample,
using the pinned system model (`table.conf` λ = 0.312699). The Rust
segmenter must emit the same bytes.

No model bytes are tracked here. The segmenter reads the fetched
`interpolation2.text` cache and the migrate-export `bigram.redb` /
`phrase_index.redb`.
