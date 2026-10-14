"""Contract cases whose instrumented measurement depends on undefined behaviour in the pin.

user-gram-duplicate-export and user-gram-unsorted-export export planted user
bigram rows through pinyin_bigram_iterator_has_next_phrase, which hands an
unterminated GPtrArray to g_strjoinv (src/pinyin.cpp:862 at pin 074a2219;
compatibility-policy register row 1, class (b)). Whether that read past the
allocation faults depends on heap layout: the installed -O2 oracle has not
faulted and matches Rust, while the -O0 --coverage measurement build faulted
in every first pass on kc and tkrzw but only in some fatal-case replays. A
delta ledger therefore never credits these cases and always lists them as
measurement limits, so --check cannot flip on heap layout.
"""

from coverage_reviews import measurement_limit

measurement_limit(
    ("user-gram-duplicate-export", "user-gram-unsorted-export"),
    "the bigram export iterator's g_strjoinv over-read (src/pinyin.cpp:862, compatibility-policy register row 1) faults in the -O0 --coverage build depending on heap layout",
)
