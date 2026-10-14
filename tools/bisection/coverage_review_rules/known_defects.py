"""Known, tracked parity defects at pin 074a2219 that the delta ledgers must not credit.

#785: on Kyoto Cabinet and Tkrzw the pin's user-index `mask_out` visits every
record through one reused table entry, and `MemoryChunk::set_content` never
shrinks a chunk, so a record shorter than the one visited before it is written
back with the earlier record's tail (whole tokens, later returned by lookups).
oxpinyin writes the record's own bytes and does not reproduce the leak yet. The
issue784-*/issue762-* contract cases are deliberately narrowed around it on kc
and tkrzw (index key set only; the first character's prefix lookup skipped), so
their MATCH verdicts say nothing about these bodies' values. Berkeley DB is
unaffected (set_chunk per record), so the bdb ledger carries no such row.
"""

from coverage_reviews import known_defect

for file, line in (("src/storage/phrase_large_table3_kyotodb.cpp", 361),
                   ("src/storage/chewing_large_table2_kyotodb.cpp", 459),
                   ("src/storage/phrase_large_table3_tkrzwdb.cpp", 288),
                   ("src/storage/chewing_large_table2_tkrzwdb.cpp", 429)):
    known_defect(file, [line], "#785", ("contract/issue784-", "contract/issue762-"),
                 "the mask-out callback masks each visited record inside a reused table entry whose chunk never shrinks, so a record shorter than its predecessor is written back with the predecessor's tail; oxpinyin writes the record's own bytes and does not reproduce this yet. The issue784-*/issue762-* cases compare the index key set only and skip the first character's prefix lookup on this cell, so they are excluded from the evidence shown; the remaining executed records show the differentials reach this body, not that its values match.")
