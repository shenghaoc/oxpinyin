"""Reviewed bodies in src/storage/chewing_large_table2.cpp at pin 074a2219; no name-match mapping."""

from coverage_reviews import review, C, E, R, U, P, D

review("src/storage/chewing_large_table2.cpp", [26, 68], [(C, "pub(crate) fn new(dbm:"), (R, "pub fn decode_items(")],
       "The sixteen reusable length-specialized entry buffers are absent. Rust keeps only an owned DBM handle and allocates decoded records for the queried runtime length; Vec and DBM ownership provide their lifetimes. This removes entry initialization/destruction, without declaring any lookup behavior dead.", "ABSENT_STD")

review("src/storage/chewing_large_table2.cpp", [161], [(C, "pub(crate) fn search(")],
       "Both select initial-only versus complete encoded keyspace and query using the original keys; Rust's runtime-length decoder replaces the 1..16 template switch.")

review("src/storage/chewing_large_table2.cpp", [179], [(U, "pub fn add_phrase_in("), (E, "pub fn pinyin_index_entries(")],
       "Both add the same reading/token to initial-only and complete keyspaces. Runtime indexed-membership state is serialized through pinyin_index_entries into both spaces; the field-level mutation is in add_phrase_in.")

review("src/storage/chewing_large_table2.cpp", [200], [(U, "pub fn remove_user_phrase("), (P, "&with_kept_keys(pinyin_index_entries(&chewing_rows), &kept.pinyin),")],
       "Both remove each indexed reading from the user pinyin index; Rust validates indexed memberships and removes the reading rows before the runtime index writer emits survivors.")
