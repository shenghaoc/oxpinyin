# 1297-gram-partition-segmentation — phrase segmentation over stored grams

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

## Segmentation difference and evidence

`user-gram-unsorted-phrase-segment` plants a sentence-start gram with total
2³¹ + 2 and stored (token, count) pairs (0x10059f5, 2³¹), (0x10059fc, 1),
(0x10059f4, 1), then segments 今天天气很好.
The pin's stored-order interleave and `get_freq` walk
(`ngram.cpp:231–250,308–349`, `phrase_lookup.cpp:268–290`) miss the boosted
今天 record. The Rust `partition_point` search finds it. The pin selects
今天天气 as one phrase; Rust selects 今天 and 天气 separately. Both segmenters
return true. All three registered cells (bdb, kc, sound tkrzw) show this
difference. The final 很 and 好 tokens are unchanged.

Both facades reach gram search through merged system/user grams for
sentence guessing, dynamic adjustment and segmentation. This record
accepts only the phrase-selection difference in the named segmentation
case. The independent training termination/result difference has its own
[record](1297-gram-partition-training.md).

`user-gram-unsorted-guess`, `user-gram-unsorted-dynamic-adjust`,
`user-gram-unsorted-range-boundary` and `system-gram-unsorted-guess` stay
MATCH. Duplicate, zero-count, short-header and partial-tail cases also
keep their existing outcomes. Nothing sorts or cleans a stored gram.
C ABI/layout, real GArray/g_free ownership, lifetime, file formats,
data ownership, process-wide effects and refusal conditions are unchanged.
The facade total still awaits a ruling.

## Verification

Only `user-gram-unsorted-phrase-segment` is accepted under this record.
The derivation above sets the pin's result. The `oxpinyin-capi` test `pinned_data_tests::the_segmenter_reads_an_unsorted_user_gram_as_stored` holds oxpinyin's side.
C's original core tests cover sorted duplicates, empty/short headers,
both-end misses, all partial-tail lengths and stored-order preservation.
The tests of `oxpinyin-core`'s `single_gram` module and `oxpinyin-capi`'s
`e2e_tests::user_gram_unsorted_export_follows_the_stored_order` and
`e2e_tests::user_gram_duplicate_export_lists_every_stored_item` hold the
original broader selection of issues 757 and 785. The bench runbook documents
its unchanged scope and the base/PR/pin comparison.
No raw captures or verification logs are committed.
