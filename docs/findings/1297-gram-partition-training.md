# 1297-gram-partition-training — training over stored grams

Status: **accepted** · 2026-10-29T13:19:09Z (`date -u`) · Refs #1297.
Policy: the 2026-10-29 UTC “No ported external-library internals” ruling
in [Source policy](../source-policy.md), merged in #1402.
The [inventory](glib-libc-inventory.md#executable-copies-of-external-library-algorithms-2026-10-29-utc) identifies
stored gram searches as C and carried pinyin entry searches as D.

## Contract and derivation

The pin is libpinyin 2.11.92 at `074a2219c90feaf962d0d24f034514033ece5f99`.
The source was read from the local libpinyin repository with
`git show 074a2219:<path>`. The working HEAD of that repository is not the pin.

`src/storage/ngram.cpp:153–271` searches stored records before its
get/set/insert/remove/range walks. Its writers normally keep token order.
`merge_single_gram` (`:308–349`) interleaves the two arrays, and does not
sort, clean or combine later duplicates. Thus foreign stored values can
violate the partition requirement of the search predicate.
As the ruling requires, `single_gram::lower_bound_records` replaces the
pin's search with `slice::partition_point` on the existing whole-record
view. The native row codecs keep the physical order. `retrieve_all` and
`mask_out` do not search, and they stay unchanged.

Sorted and predicate-partitioned records keep first-not-less semantics,
including duplicates and misses at both ends. Unpartitioned input gets
the unspecified result of std. The recorded observations name Rust 1.97.1
(`rust-toolchain.toml`) and make no cross-toolchain promise. This is a
recorded library implementation difference, outside classes (a)–(b).
It is not a product ranking change, and it does not change a persistent format.

## Training difference and evidence

`abort-train-unsorted-set-freq-miss-pinyin` and
`abort-train-unsorted-set-freq-miss-zhuyin` train the constrained 你好 guess
against a five-item user sentence-start gram. Relative to 你, its successors
are stored in order -4, -3, -2, +1, -1, each with count one and total five.
`get_freq` misses, `insert_freq` appends, and the pin's final search misses
again (`ngram.cpp:178–205,231–271`). Thus the check of
`user->set_freq(token, freq + seed)` in `phonetic_lookup.h:888–903`
aborts. With Rust's `partition_point`, insertion and the following search
find the inserted item, and training returns true with no warning. In each
facade, the pin dies with SIGABRT and its assertion line, and oxpinyin
answers true. The difference is the same on all three registered cells
(bdb, kc, sound tkrzw).

Both facades reach these searches through user grams for training and editing.
A search that actually misses after insertion still returns the existing
training refusal. oxpinyin does not force a successful std search to
reproduce the miss of another implementation. This record accepts only the training termination/result
difference in these two facade cases. The independent segmentation difference
has its own [record](1297-gram-partition-segmentation.md).

Duplicate, zero-count, short-header and partial-tail cases keep their existing
outcomes. Nothing sorts or cleans a stored gram. C ABI/layout, real GArray/g_free
ownership, lifetime, file formats, data ownership, process-wide effects and
existing refusal conditions are unchanged. The facade total still awaits a ruling.

## Verification

Only the two `abort-train-unsorted-set-freq-miss-*` cases are accepted under
this record. The derivation above sets the pin's result.
`oxpinyin-capi`'s `boundary_tests::training_over_an_unsorted_start_gram_answers_true`
holds the pinyin case. The zhuyin facade trains through the same shared facade
training call and has no separate test.
C's original user tests check that training preflight and apply agree and that
refusal is atomic, without promising a position for unpartitioned search.
The tests of `oxpinyin-core`'s `single_gram` module and `oxpinyin-user`'s
`store_libpinyin::tests::training_preserves_a_partitioned_unsorted_gram_and_its_duplicates`
hold the original broader selection of issues 757 and 785. The bench
runbook documents its unchanged scope and the base/PR/pin comparison.
No raw captures or verification logs are committed.
