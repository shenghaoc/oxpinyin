"""Implicit destructors of the kc/tkrzw record-callback objects at pin 074a2219.

Kyoto Cabinet's `DB::Visitor` and Tkrzw's `DBM::RecordProcessor` are
polymorphic bases; the pin's mask-out and key-collecting subclasses are
stack objects holding only borrowed pointers and scalars, so their
compiler-generated destructors release nothing. GCC emits the complete ABI
destructor bodies but public gcov output omits artificial-function records
(gcov.cc:1764-1766 at GCC 16.2.0, as the bdb ledger's synthetic identities
document), so these bodies can never earn execution credit. Reviewed by
class declaration site; the kc and tkrzw delta ledgers apply these rules.
"""

from coverage_reviews import review, U, P

FACADE = "crates/oxpinyin-facade/src/context.rs"
EXPORT = "crates/oxpinyin-facade/src/export_rows.rs"

for file, line in (("src/storage/chewing_large_table2_kyotodb.cpp", 447),
                   ("src/storage/chewing_large_table2_tkrzwdb.cpp", 419)):
    review(file, [line], [(U, "pub fn mask_out("), (P, "&with_kept_keys(pinyin_index_entries(&chewing_rows), &kept.pinyin),")],
           "The mask-out callback object holds the borrowed entry array and the mask/value scalars; its implicit destructor releases nothing. Rust masks the authoritative user phrase/reading rows in place and serializes the surviving index memberships without a visitor object, so no destructor exists. GCC's complete object ABI destructor symbol exists while public gcov output omits its artificial-function record; this source review resolves the identity and supplies no inferred execution credit.",
           "ABSENT_INLINE")

for file, line in (("src/storage/phrase_large_table3_kyotodb.cpp", 349),
                   ("src/storage/phrase_large_table3_tkrzwdb.cpp", 277)):
    review(file, [line], [(U, "pub fn mask_out("), (FACADE, "pub fn mask_out(")],
           "The phrase-table mask-out callback object holds the reusable table entry and the mask/value scalars; its implicit destructor releases nothing. Rust masks the user phrase rows through the store's own walk and the facade's outcome, with no callback object, so no destructor exists. GCC's complete object ABI destructor symbol exists while public gcov output omits its artificial-function record; this source review resolves the identity and supplies no inferred execution credit.",
           "ABSENT_INLINE")

for file, line in (("src/storage/ngram_kyotodb.cpp", 163),
                   ("src/storage/ngram_tkrzwdb.cpp", 143)):
    review(file, [line], [(U, "pub fn export_bigrams("), (EXPORT, "pub fn export_bigram_rows(")],
           "The key-collecting callback object holds only the borrowed GArray that get_all_items fills; its implicit destructor releases nothing. Rust's bigram export reads the store's rows into an owned Vec and the facade renders the rows, with no callback object, so no destructor exists. GCC's complete object ABI destructor symbol exists while public gcov output omits its artificial-function record; this source review resolves the identity and supplies no inferred execution credit.",
           "ABSENT_INLINE")
