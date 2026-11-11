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

Recorded evidence: this directory was the input of the retired drop-in comparison. That recipe was removed on 2026-11-06 UTC with the compat launcher.
On one side, the `gen_binary_files` / `import_interpolation` / `gen_unigram` programs of libpinyin compiled it.
On the other side, the ports of `oxpinyin-utils` under the same names compiled it.
The recipe compared every chunk file byte for byte (tone bits included).
Neither side read the output of the other side.

The `gbf-toned-model` case in
the `frozen::gen_binary_files` module of `crates/oxpinyin-utils/tests/storage_tools.rs` runs `gen_binary_files`
over the same tables. It compares the complete output inventory with the
frozen pin capture, including the toned pinyin-index rows.
