"""Reviewed bodies in src/storage/chewing_large_table2.h at pin 074a2219; no name-match mapping."""

from coverage_reviews import review, C, E, R, U, P, D

review("src/storage/chewing_large_table2.h", [52, 55], [(C, "pub fn prefix_keys_match(")],
       "The comparator object's only state is prefix length. Rust takes it from the query slice length, with no comparator allocation; destruction's reset-to-zero has no subsequent observer. The prefix predicate uses that length directly.", "ABSENT_INLINE")

review("src/storage/chewing_large_table2.h", [59, 66], [(C, "pub fn prefix_keys_match("), (C, "pub(crate) fn walk_suggestions(")],
       "Prefix comparison is folded into the prefix slice/record predicate and encoded-key range walk. Rust retains initial matching, zero middle/final wildcard and zero-tone wildcard; the numeric less-than callback and its equal_range object have no standalone counterpart.", "ABSENT_INLINE")

review("src/storage/chewing_large_table2.h", [74, 86], [(R, "pub fn decode_items(")],
       "Length-specialized entry ownership is replaced by decoded Vec<PinyinIndexItem> rows. The Rust decoder sizes each row by the runtime phrase length and owns its keys; vector ownership releases the rows. No array of sixteen MemoryChunk-backed entry objects is allocated.", "ABSENT_STD")

review("src/storage/chewing_large_table2.h", [91], [(C, "pub(crate) fn search("), (D, "fn fill_lookup(")],
       "The pin filters records by keys, drops unloaded-library tokens and coalesces consecutive tokens into ranges. Rust filters records in search, resolves loaded libraries in fill_lookup and hands individual entries to the decoder. Range compression is unnecessary because the engine consumes token entries rather than numeric range buffers.")

review("src/storage/chewing_large_table2.h", [136], [(C, "pub(crate) fn search(")],
       "Both choose incomplete versus tone-zeroed complete index keys, retrieve the corresponding record group and apply the original query's wildcard/tone filter. Rust decodes L at runtime instead of selecting an entry template.")

review("src/storage/chewing_large_table2.h", [155, 182], [(C, "pub(crate) fn walk_suggestions("), (D, "pub fn checked_extension_tokens(")],
       "The pin finds longer-key groups by the encoded prefix and retains matching records whose library is loaded. Rust walks the same key prefix, decodes the row length, and checked_extension_tokens filters the query prefix and visible token libraries. Template-specific record pointers are replaced by decoded records.")

review("src/storage/chewing_large_table2.h", [208], [(U, "pub fn add_phrase_in("), (E, "pub fn pinyin_index_entries("), (P, "&pinyin_index_entries(&chewing_rows),")],
       "Insertion's meaningful state is the indexed (token, pronunciation) membership. add_phrase_in creates indexed readings only on the new-item path; persistence materializes those memberships in both complete/incomplete keyspaces, sorts records by exact keys then token, and creates proper-prefix markers. The old mutable byte-array insertion step is folded into this state/materialization pair.")

review("src/storage/chewing_large_table2.h", [233], [(U, "pub fn remove_user_phrase("), (P, "&pinyin_index_entries(&chewing_rows),")],
       "Removal validates that every reading was indexed, then removes pronunciation memberships. Persistence rebuilds the raw pinyin index from surviving indexed readings. The source's per-entry token lookup/removal is part of this operation rather than a new independent Rust method.")

review("src/storage/chewing_large_table2.h", [268], [(U, "pub fn mask_out("), (P, "&pinyin_index_entries(&chewing_rows),")],
       "Both remove tokens satisfying (token & mask)==value. Rust's user-store mask removes matching phrase/pronunciation state and persistence emits index records for survivors. This cites the actual mutation and runtime writer, rather than matching a datagen-only function name.")
