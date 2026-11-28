# Toned mini model

This directory is an eighteen-file model directory in the pinned model20 layout:
seventeen `.table` files plus `interpolation2.text`. That is the same inventory that
`tools/dev/model.py` (`EXPECTED_MODEL_FILES`) checks. The model is small enough to read by eye, and it carries what the pinned model
does not carry: **tone digits**.

`gb_char.table` stores:

- four readings of one syllable under one
  tone-zeroed pinyin-index key (`ba`, `ba1`, `ba3`, `ba4`);
- two
  pronunciations of one phrase item that differ only by tone
  (`ni3'hao3` / `ni'hao`, both token 16777224);
- a vowel-initial toned
  syllable (`a1`);
- `lv4`.

`art.table` does the same for an addon
library (`er4'huang2` / `er'huang`). `interpolation2.text` carries
`\1-gram` counts for some tokens only (the others take the
bare `+1` of `gen_unigram`), a `<start>` bigram, and a two-token row.

The `gbf-toned-model` case in
the `frozen::gen_binary_files` module of `utils/tests/it/storage_tools.rs` runs `gen_binary_files`
over these tables. It compares the complete output inventory with the
frozen pin capture, including the toned pinyin-index rows.
