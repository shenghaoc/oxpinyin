"""Reviewed bodies in src/storage/pinyin_phrase3.h at pin 074a2219; no name-match mapping."""

from coverage_reviews import review, C, E, R, U, P, D

review("src/storage/pinyin_phrase3.h", [33, 201, 210], [(E, "fn exact_compare2(")],
       "The value-array ordering is initials across all syllables, then middle/final pairs, then tones. Rust uses the same ordered field passes before sorting records by token. The templated wrapper and less-than sign test are folded into sort_by; this review applies to each length-specific signature of the same body.")

review("src/storage/pinyin_phrase3.h", [68, 73, 96, 108], [(C, "pub fn prefix_keys_match(")],
       "The lookup needs comparison==0: initial equality; middle/final equality unless either pair is zero; tone equality unless either tone is zero. Rust spells those exact three predicates in the record filter. Numeric negative/positive results are not computed: the grouped DBM read/filter replaces the array equal_range, so this is an inlined matching operation, not a claim that the numeric comparator is reproduced.", "ABSENT_INLINE")

review("src/storage/pinyin_phrase3.h", [146], [(C, "pub fn contains_incomplete(")],
       "Both identify a syllable whose middle and final are zero. The pin's assertion that its tone is also zero is a separate availability seam; this rule accounts for the incomplete-keyspace selection operation.")

review("src/storage/pinyin_phrase3.h", [160], [(R, "pub fn encode_complete_key(")],
       "Both preserve initial/middle/final for every syllable and zero tone before using the packed two-byte keys as the complete DBM key.")

review("src/storage/pinyin_phrase3.h", [170], [(R, "pub fn encode_incomplete_key(")],
       "Both retain only each syllable's initial and zero middle/final/tone to build the incomplete DBM key.")

review("src/storage/pinyin_phrase3.h", [186], [(C, "pub fn index_key("), (R, "pub fn decode_items(")],
       "The temporary query item's null token/zero-initialized key array has no owning Rust object. index_key builds query bytes directly; decode_items constructs stored token/key records with their actual fields. No temporary token participates in the query.", "ABSENT_INLINE")

review("src/storage/pinyin_phrase3.h", [191], [(R, "pub fn encode_item("), (R, "pub fn decode_items(")],
       "The pin copies a token and L ChewingKey elements into an item. The Rust record carries token plus owned keys; encode_item/decode_items preserve token offset 0, key offset 4, two-byte packed keys and the C++ stride for each L.")

review("src/storage/pinyin_phrase3.h", [217, 226], [(C, "pub(crate) fn search("), (C, "pub fn keys_match(")],
       "The pin's item wrappers feed wildcard comparison/sign tests to equal_range. Rust selects the same grouped DBM value and filters its decoded records by equality predicates; there is no wildcard ordering callback or array binary-search wrapper.", "ABSENT_INLINE")
