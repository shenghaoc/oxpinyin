# eio-pinyin-index-padding — the pin's uninitialised padding in pinyin-index items

- **Status:** accepted · 2026-11-01 UTC · PR #1500
- **Disposition:** POLICY-ACCEPTED (never MATCH)
- **Policy limb:** class (b), memory safety. The pin writes the bytes of a
  stack object that its constructor never set, so the on-disk bytes are an
  indeterminate read, not a value the pin's program text defines. It is
  not (a): no arithmetic produces them. It is not (c): the files exist and
  open on both sides. A language-neutral requirement on the index would
  name the token and the keys of each item. It would say nothing about the
  bytes between the last key of one item and the next item, because no reader
  of the format gives them a meaning.
- **Pin:** libpinyin 2.11.92 at 074a2219; tree read: a clone at that
  commit (`git cat-file -p 074a2219:<path>`); facade: pinyin (the zhuyin
  tables use the same item type); backend: all three.
- **Pin derivation:**
  - `src/storage/pinyin_phrase3.h:181-196`: `struct PinyinIndexItem2<N>`
    is `phrase_token_t m_token; ChewingKey m_keys[N];`. The constructor
    used by the index writer (`:191-195`) does `memmove` into `m_keys` and
    assigns `m_token`, and nothing else. `sizeof(ChewingKey)` is 2 and
    `sizeof(phrase_token_t)` is 4. Thus for an odd `N`, the size of the struct
    rounds up to a multiple of 4, and its last two bytes are tail padding
    that no member covers (`N` = 1: 6 bytes of members in an 8-byte
    object; `N` = 3: 10 in 12; and so on).
  - `src/storage/chewing_large_table2.h:208-232`
    (`ChewingTableEntry<N>::add_index`): `const IndexItem add_elem(keys,
    token)` is a stack object, and `m_chunk.insert_content(offset,
    &add_elem, sizeof(IndexItem))` (`:228-230`) copies the whole object,
    padding included, into the entry's chunk.
  - The `add_index_internal` of each backend then stores the chunk as the
    DBM value: `chewing_large_table2_bdb.cpp:311-316` (a new
    entry, `new_entry.m_chunk.begin()` / `.size()` in the `DBT`) and
    `:351` (an existing entry), `chewing_large_table2_kyotodb.cpp:
    295-299` and `:325`, `chewing_large_table2_tkrzwdb.cpp:276-279` and
    `:308`.
  - Every reader of the value (`ChewingTableEntry<N>::search`,
    `:136-180`; `prefix_compare_with_tones` / `phrase_exact_compare2`,
    `:59-71` and `pinyin_phrase3.h:201-207`) reads `m_keys` and `m_token`
    only. No pin code reads the padding.
- **Rust counterpart:**
  `crates/oxpinyin-data/src/row_format.rs:207-214`
  (`row_format::pinyin_index::encode_item`). The record buffer is
  `vec![0; item2_stride(keys.len())]`. The function writes the token and the
  packed keys into it, and the tail stays zero. The index writer of
  `gen_binary_files` reaches it through
  `crates/oxpinyin-utils/src/storage/gen_binary_files.rs:172-200`
  (`add_index_space`). The user store reaches it through
  `crates/oxpinyin-user/src/persistence.rs`.
- **Forward trace:** `gen_binary_files` → `generate_binary_files` →
  `load_chewing` (`ChewingLargeTable2::load_text`) → `add_index` for both
  key spaces → `add_index_internal` → `ChewingTableEntry::add_index` →
  the DBM `put`. The observable is the value bytes of
  `pinyin_index.bin` and `addon_pinyin_index.bin`. No exit status,
  stream or other file changes.
- **Backward trace:** these values have two producers only:
  - `ChewingTableEntry<N>::add_index`, which every `add_index_internal` of the
    three backends calls;
  - in the user tables, the same template through
    `ChewingLargeTable2::add_index` from the persistence of the user store.

  `remove_index` and `mask_out` (`chewing_large_table2.h:
  233-290`) move whole items with `remove_content` and keep their bytes.
  There is no other writer: `import_interpolation`, `gen_unigram` and the
  training utilities do not open the pinyin index for writing. Both
  facades use one `ChewingLargeTable2`. The three backends differ only in
  the container call.
- **Input partition:**
  - Valid supported input: any `.table` row whose spelling has an odd
    number of syllables (`N` = 1, 3, 5, …). Every item of those keys
    has two padding bytes. Rows with an even number of syllables
    have none, and compare byte for byte.
  - Malformed / host-dependent input: none. The bytes depend on what the
    pin's stack held before the construction of `add_elem`. That depends on
    the compiler, the call history and the host, not on the input.
  - Other: an empty table writes no item.
- **Difference:** in `gbf-toned-model` on the bdb cell, 3 of the 190
  pinyin-index records differ from the subject's in their padding bytes
  only. Every token and key of every record is identical. The pin's bytes
  there were `b7ac` and similar values; the subject's are `0000`. By chance,
  the pin's padding is zero in the other 187 records and in every record
  of the `gbf-mini-fixture` case. Thus those records compare equal.
- **Why accepted:** the pin's bytes are an indeterminate read of the padding
  of a stack object (C++ [basic.indet]). They are not a value that the program
  defines, and they cannot be reproduced on purpose. No reader interprets
  them. The Rust writer zeroes the buffer that it builds, which is the only
  defined choice.
- **Contract preserved:** the C ABI, ownership and lifetimes are not
  involved. The on-disk layout is unchanged: the same key bytes, the same
  item stride, and the same token and key fields in the same order. A
  libpinyin build of the same DBM reads an index that the subject wrote, and a
  subject reads an index that the pin wrote. The runtime, user-directory and
  drop-in gates exercise this. No diagnostic or process-wide effect differs.
- **Affected inputs:** every odd-syllable item of a pinyin index that the pin
  writes. The pin's stack decides whether the bytes differ from zero on a
  given run, so a case may compare equal by chance. Controls:
  `phrase_index.bin`, `punct.bin` and the `addon_phrase_index.bin` of the
  same case compare identical record for record, and all seventeen chunk
  files byte for byte.
- **Tests:** tool-diff case `gbf-toned-model` (`gen_binary_files.json`)
  is POLICY-ACCEPTED on bdb, kc and sound tkrzw with complete logical
  key/value rows after masking only the documented item padding.
  `gbf-mini-fixture` and `kmm-estimate-good` are MATCH on each cell.
  The `user-key-spelling/pinyin-egn` and `user-key-spelling/pinyin-nuen`
  contract controls also MATCH on all three cells (recorded evidence). `oxpinyin-capi`'s
  `boundary_tests::an_import_reading_parses_with_the_corrections` holds the
  `egn` control.
- **Prior rulings:** none reversed.
- **Evidence:** recorded evidence (2026-11-01 UTC, bdb): the case's pin
  and subject `pinyin_index.bin`, read with the retired `raw_records`
  record reader and compared field by field at the item
  stride: 190 records, 3 differing in tail padding only, 0 field
  mismatches. The capture is not committed.
- **Conclusion:** accepted under the maintainer's #1500 instruction to mask only these tail padding bytes.

## Comparator

`mask_pinyin_index_row` in `crates/oxpinyin-utils/tests/common/mod.rs` keeps every key and value. It
masks only the two tail bytes of each odd-N item. N is half the DBM
key length, and each item stride is `(4 + 2*N + 3) & ~3`. Empty continuation
rows stay present. Invalid key or item lengths fail. The helper drops no token, spelling
key, record order or value length. `rows_digest` in the same file sorts whole DBM rows to
exclude container metadata. It does not reorder items within a value.

The comparator is O(B) time and space in the helper's B output bytes.

The selected case reports POLICY-ACCEPTED when complete observations match
after this narrowly defined mask. It reports DIFFER for every remaining mismatch.
The tests do these things:

- They distinguish token, packed-key and item-order mutations.
- They verify odd and even lengths and empty continuation rows.
- They reject truncated items.

Recorded evidence: the focused harness test caught the named mutation
`pinyin-index-keys-only`, and `gbf-toned-model` (DIFFER) caught
`pinyin-index-value-token-zero`. The helper opens a DBM as a tree
first, because tkrzw exposes internal tree pages to a hash-first open.
