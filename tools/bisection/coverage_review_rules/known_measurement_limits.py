"""Contract cases whose instrumented measurement depends on undefined behaviour in the pin.

user-gram-duplicate-export and user-gram-unsorted-export export planted user
bigram rows through pinyin_bigram_iterator_has_next_phrase, which hands an
unterminated GPtrArray to g_strjoinv (src/pinyin.cpp:862 at pin 074a2219;
compatibility-policy register row 1, class (b)). Whether that read past the
allocation faults depends on heap layout. The -O0 --coverage measurement
build faulted in every first pass on kc and tkrzw but only in some fatal-case
replays. The installed -O2 oracle faults too: the kc cell on a cloud host
provisioned with tools/cloud/setup.sh (main 1a7c739), while bdb and tkrzw
completed and matched there. contract-diff.py holds such a fault to the class
(b) shape (strjoinv_over_read). A delta ledger therefore never credits these
cases and always lists them as measurement limits, so --check cannot flip on
heap layout.
"""

from coverage_reviews import measurement_limit

measurement_limit(
    ("user-gram-duplicate-export", "user-gram-unsorted-export"),
    "the bigram export iterator's g_strjoinv over-read (src/pinyin.cpp:862, compatibility-policy register row 1) faults in the -O0 --coverage build depending on heap layout",
)
