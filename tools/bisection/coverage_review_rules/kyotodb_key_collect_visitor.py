"""The kc cell's two global `KeyCollectVisitor` classes at pin 074a2219.

src/storage/ngram_kyotodb.cpp:163 and src/storage/punct_table_kyotodb.cpp:163
each define a distinct global class named `KeyCollectVisitor` (both derive
from Kyoto Cabinet's `DB::Visitor`, each collecting keys into a borrowed
GArray). Their inline constructors share one USR and one mangled symbol, so
the compiler graph gives the punct-table constructor the ngram constructor's
export witness. That witness is the collision, not a call: the punct-table
visitor is constructed only by `PunctTable::get_all_items`
(punct_table_kyotodb.cpp:186), which no export path reaches and which no
differential executed, while the ngram constructor is a separate
signature-keyed row credited by executed MATCH. Reported as a pin finding
(one-definition-rule collision in the Kyoto Cabinet cell); the kc delta
ledger applies this rule.
"""

from coverage_reviews import identity_review

identity_review(
    "c:@S@KeyCollectVisitor@F@KeyCollectVisitor#*$@S@_GArray#", [],
    "Not constructible through the kc C exports. This USR names the inline constructor of the punct-table KeyCollectVisitor (punct_table_kyotodb.cpp:163-170), whose only construction site is PunctTable::get_all_items (punct_table_kyotodb.cpp:186): no export path reaches that method in any cell (the bdb ledger carries no punct_table_bdb.cpp get_all_items row either) and no differential executed it. The witness path through pinyin_begin_get_bigram_phrases belongs to the other global class of the same name, ngram_kyotodb.cpp:163-184, whose constructor is the separate signature-keyed row at ngram_kyotodb.cpp:167 with executed MATCH credit; libclang assigns both constructors one USR because the pin declares two distinct global classes with one name in two translation units. Every direct caller and function-address use of the punct-table visitor in the configured 23 TUs was checked; missing counters are not the proof.",
    status="ABSENT_KC_EXPORTS", cell="kc",
)
