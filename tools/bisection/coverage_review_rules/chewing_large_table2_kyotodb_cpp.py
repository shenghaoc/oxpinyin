"""Reviewed bodies in src/storage/chewing_large_table2_kyotodb.cpp at pin 074a2219; no name-match mapping.

Same operations as the Berkeley DB file's reviewed bodies, on the Kyoto
Cabinet tree (check/get/set point operations, a jump/step cursor and a
writable record visitor); the kc delta ledger applies them to the rows that
the kc differentials do not execute.
"""

from coverage_reviews import review, C, E, R, U, P, D

review("src/storage/chewing_large_table2_kyotodb.cpp", [34], [(C, "pub(crate) fn walk_suggestions("), (C, "pub fn prefix_upper_bound(")],
       "The pin stops when the cursor key is no longer strictly longer with an equal packed prefix (the shorter application key is compared against the cursor key for its own length). Rust bounds the raw key walk by prefix_upper_bound and its caller excludes same-length rows; this preserves the longer encoded-key predicate.")

review("src/storage/chewing_large_table2_kyotodb.cpp", [149, 181], [(C, "pub(crate) fn search(")],
       "Both perform a point read of the encoded index (check then get on the Kyoto Cabinet tree) and filter records by the query. Runtime phrase_length plus decode_items replaces the 1..16 switch and template body; key existence remains a separate continuation probe.")

review("src/storage/chewing_large_table2_kyotodb.cpp", [217, 238, 524], [(C, "pub(crate) fn walk_suggestions("), (D, "pub fn checked_extension_tokens(")],
       "Both read longer records under an existing prefix key (a cursor positioned by jump and advanced by step), filter by the prefix and library visibility, and expose tokens. Rust checks overlong keys before decoding and returns the named gate to the facade, where the registered abort seam is handled.")

review("src/storage/chewing_large_table2_kyotodb.cpp", [278, 340], [(U, "pub fn add_phrase_in("), (E, "pub fn pinyin_index_entries("), (P, "&with_kept_keys(pinyin_index_entries(&chewing_rows), &kept.pinyin),")],
       "The pin checks one DBM key, inserts an exact key/token record (a fresh entry for a missing key, otherwise get, add_index and set), and creates missing proper-prefix markers with empty values. Rust records indexed reading membership during import and its runtime persistence writer groups, orders and emits the same keyspaces plus markers. The template's fixed L is a runtime slice length.")

review("src/storage/chewing_large_table2_kyotodb.cpp", [376, 411], [(U, "pub fn remove_user_phrase("), (P, "&with_kept_keys(pinyin_index_entries(&chewing_rows), &kept.pinyin),")],
       "The pin removes an exact key/token record from one DBM value (check, get, remove_index, set). Rust checks indexed readings during candidate removal, deletes their authoritative memberships, then the runtime writer excludes them from the serialized index.")

review("src/storage/chewing_large_table2_kyotodb.cpp", [514], [(U, "pub fn mask_out("), (P, "&with_kept_keys(pinyin_index_entries(&chewing_rows), &kept.pinyin),")],
       "The pin iterates every record with a writable Kyoto Cabinet visitor whose visit_full rewrites the value after erasing masked tokens, then synchronizes. Rust masks authoritative user phrase/reading rows and serializes only surviving index memberships; the overlong-key gate is checked before mutation. This is a structural counterpart, not a proof of identical malformed-file side effects.")
