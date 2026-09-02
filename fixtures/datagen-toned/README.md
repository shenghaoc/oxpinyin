# Toned mini model

A seventeen-file model directory in the pinned model20 layout (the same
inventory `oxpinyin-testsupport::model_cache::EXPECTED_MODEL_FILES`
checks), small enough to read by eye and carrying what the pinned model
does not: **tone digits**.

`gb_char.table` stores four readings of one syllable under one
tone-zeroed pinyin-index key (`ba`, `ba1`, `ba3`, `ba4`), two
pronunciations of one phrase item that differ only by tone
(`ni3'hao3` / `ni'hao`, both token 16777224), a vowel-initial toned
syllable (`a1`), and `lv4`; `art.table` does the same for an addon
library (`er4'huang2` / `er'huang`). `interpolation2.text` carries
`\1-gram` counts for some tokens only (the others take `gen_unigram`'s
bare `+1`), a `<start>` bigram, and a two-token row.

It is the input of `tools/datagen/libpinyin-drop-in-differential.sh`:
libpinyin's own `gen_binary_files` / `import_interpolation` /
`gen_unigram` compile it on one side, `oxpinyin-datagen` on the other,
and `crates/oxpinyin-datagen/tests/libpinyin_parity.rs` compares the
results field by field — every chunk file byte-exact (tone bits included)
and every DBM row. Neither side reads the other's output.
