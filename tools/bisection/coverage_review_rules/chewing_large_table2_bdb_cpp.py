"""Reviewed bodies in src/storage/chewing_large_table2_bdb.cpp at pin 074a2219; no name-match mapping."""

from coverage_reviews import review, C, E, R, U, P, D

review("src/storage/chewing_large_table2_bdb.cpp", [30], [(C, "pub(crate) fn walk_suggestions("), (C, "pub fn prefix_upper_bound(")],
       "The pin stops when the next DBM key is no longer strictly longer with an equal packed prefix. Rust bounds the raw key walk by prefix_upper_bound and its caller excludes same-length rows; this preserves the longer encoded-key predicate.")

review("src/storage/chewing_large_table2_bdb.cpp", [165, 196], [(C, "pub(crate) fn search(")],
       "Both perform a point read of the encoded index and filter records by the query. Runtime phrase_length plus decode_items replaces the 1..16 switch and template body; key existence remains a separate continuation probe.")

review("src/storage/chewing_large_table2_bdb.cpp", [232, 251, 550], [(C, "pub(crate) fn walk_suggestions("), (D, "pub fn checked_extension_tokens(")],
       "Both read longer records under an existing prefix key, filter by the prefix and library visibility, and expose tokens. Rust checks overlong keys before decoding and returns the named gate to the facade, where the registered abort seam is handled.")

review("src/storage/chewing_large_table2_bdb.cpp", [291, 361], [(U, "pub fn add_phrase_in("), (E, "pub fn pinyin_index_entries("), (P, "&with_kept_keys(pinyin_index_entries(&chewing_rows), &kept.pinyin),")],
       "The pin loads one DBM value, inserts an exact key/token record, writes it back and creates missing proper-prefix markers. Rust records indexed reading membership during import and its runtime persistence writer groups, orders and emits the same keyspaces plus markers. The template's fixed L is a runtime slice length.")

review("src/storage/chewing_large_table2_bdb.cpp", [398, 435], [(U, "pub fn remove_user_phrase("), (P, "&with_kept_keys(pinyin_index_entries(&chewing_rows), &kept.pinyin),")],
       "The pin removes an exact key/token record from one DBM value. Rust checks indexed readings during candidate removal, deletes their authoritative memberships, then the runtime writer excludes them from the serialized index.")

review("src/storage/chewing_large_table2_bdb.cpp", [471], [(U, "pub fn mask_out("), (P, "&with_kept_keys(pinyin_index_entries(&chewing_rows), &kept.pinyin),")],
       "The pin walks every value and erases masked tokens. Rust masks authoritative user phrase/reading rows and serializes only surviving index memberships; the overlong-key gate is checked before mutation. This is a structural counterpart, not a proof of identical malformed-file side effects.")
