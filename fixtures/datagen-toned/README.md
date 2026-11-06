# Toned mini model

An eighteen-file model directory in the pinned model20 layout —
seventeen `.table` files plus `interpolation2.text`, the same inventory
`tools/dev/model.py` (`EXPECTED_MODEL_FILES`) checks, small enough to read by eye and carrying what the pinned model
does not: **tone digits**.

`gb_char.table` stores four readings of one syllable under one
tone-zeroed pinyin-index key (`ba`, `ba1`, `ba3`, `ba4`), two
pronunciations of one phrase item that differ only by tone
(`ni3'hao3` / `ni'hao`, both token 16777224), a vowel-initial toned
syllable (`a1`), and `lv4`; `art.table` does the same for an addon
library (`er4'huang2` / `er'huang`). `interpolation2.text` carries
`\1-gram` counts for some tokens only (the others take `gen_unigram`'s
bare `+1`), a `<start>` bigram, and a two-token row.

It was the input of `just compat drop-in` (removed 2026-11-06 UTC with the compat launcher): libpinyin's own
`gen_binary_files` / `import_interpolation` / `gen_unigram` compile it on
one side, `oxpinyin-utils`' ports under the same names on the other, and
the recipe compares every chunk file byte for byte (tone bits included).
Neither side reads the other's output. The `gbf-toned-model` case in
the `frozen::gen_binary_files` module of `crates/oxpinyin-utils/tests/storage_tools.rs` runs `gen_binary_files`
over the same tables and compares the complete output inventory with the
frozen pin capture, the toned pinyin-index rows included.
