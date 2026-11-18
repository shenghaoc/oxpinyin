//! The import and export iterators and the compaction at the end of an
//! import.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{
    drain_phrases, export_of, export_order_fixture, import_batch, init_fixture_at, partial_gram,
    take_exported_row, whole_gram,
};

use crate::support::{
    DEFAULT_SORT, LookupCandidate, PinyinInstance, TempUserDir, assert_aborts, candidate, cstr,
    lookup, open, pinyin_begin_add_phrases, pinyin_begin_get_bigram_phrases,
    pinyin_bigram_iterator_get_next_phrase, pinyin_bigram_iterator_has_next_phrase,
    pinyin_choose_candidate, pinyin_end_add_phrases, pinyin_get_candidate, pinyin_guess_candidates,
    pinyin_guess_sentence, pinyin_iterator_add_phrase, pinyin_parse_full_pinyin, pinyin_save,
    pinyin_token_get_n_pronunciation, pinyin_train, system_dir, write_user_conf, *,
};

use std::ptr;

/// `get_next` on a fresh bigram export iterator of an empty profile
/// asserts.
#[test]
fn bigram_get_next_aborts_on_an_empty_export() {
    assert_aborts(
        "import_export::bigram_get_next_aborts_on_an_empty_export",
        || {
            let user = TempUserDir::new("bigram-get-next");
            let (context, _) = open_at(&user.path);
            let iter = unsafe { pinyin_begin_get_bigram_phrases(context) };
            assert!(!unsafe { pinyin_bigram_iterator_has_next_phrase(iter) });
            let (mut phrase, mut pinyin, mut count) = (ptr::null_mut(), ptr::null_mut(), 0);
            unsafe {
                pinyin_bigram_iterator_get_next_phrase(
                    iter,
                    &raw mut phrase,
                    &raw mut pinyin,
                    &raw mut count,
                )
            };
        },
    );
}

/// A pinyin import parses its reading under CORRECT_ALL | USE_TONE: `egn`
/// becomes the key 0x0500.
#[test]
fn an_import_reading_parses_with_the_corrections() {
    let user = TempUserDir::new("import-egn");
    let (context, instance) = open_at(&user.path);
    assert_eq!(
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        unsafe { import_batch(context, 7, &[("嗯", "egn", 5)]) },
        [true]
    );
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let token = unsafe { lookup(instance, "嗯") }[0];
    let mut count = UNTOUCHED;
    assert!(unsafe { pinyin_token_get_n_pronunciation(instance, token, &raw mut count) });
    assert_eq!(count, 1);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { reading_bytes(instance, token, 0) }, [0x00, 0x05]);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// A system item's reading count is its `guint32` bit pattern in the
/// export, and training adds to it with wrap: ni3 at 2^32 - 76 exports
/// -76, then -7 (#790).
#[test]
fn a_system_reading_count_exports_its_bit_pattern() {
    let user = TempUserDir::new("export-prefix-overflow");
    let (context, instance) = open_at(&user.path);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let token = unsafe { lookup(instance, "你") }[0];
    assert_eq!(token >> 24, 1);
    let key = |text: &str| {
        let mut key = 0_u16;
        let text = cstr(text);
        assert!(unsafe {
            pinyin_parse_full_pinyin(instance, text.as_ptr(), (&raw mut key).cast())
        });
        key
    };
    let readings = [
        (key("ni3"), 0xFFFF_FFB4_u32),
        (key("ni2"), 0),
        (key("na4"), 0),
    ];
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
    // A MODIFY of the item: its readings replaced, its unigram kept.
    let file = std::fs::read(system_dir().join("gb_char.bin")).expect("gb_char.bin");
    let payload = &file[8..];
    let word = |at: usize| u32::from_le_bytes(payload[at..at + 4].try_into().expect("a word"));
    let slot = (word(4) + (token & 0x00FF_FFFF) * 4) as usize;
    let offset = (word(8) + word(slot)) as usize;
    let old = &payload[offset..offset + 10 + usize::from(payload[offset + 1]) * 6];
    let mut new = vec![1, 3];
    new.extend(&old[2..10]);
    for (key, count) in readings {
        new.extend(key.to_le_bytes());
        new.extend(count.to_le_bytes());
    }
    let mut record = 3_u32.to_le_bytes().to_vec();
    record.extend((token & 0x00FF_FFFF).to_le_bytes());
    record.extend(u16::try_from(old.len()).expect("a length").to_le_bytes());
    record.extend(u16::try_from(new.len()).expect("a length").to_le_bytes());
    record.extend(old);
    record.extend(&new);
    write_user_conf(&user.path, &system_dir());
    let total = word(0);
    write_log(
        &user.path,
        "gb_char.dbin",
        &[crate::support::modify_header(0, total, total), record].concat(),
    );
    let (context, instance) = open_at(&user.path);
    let before = [
        ("ni3".to_owned(), -76),
        ("ni2".to_owned(), -1),
        ("na4".to_owned(), -1),
    ];
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { export_rows_of(context, 1, "你") }, before);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "ni") }, 2);
    assert!(unsafe { pinyin_guess_candidates(instance, 0, crate::support::DEFAULT_SORT) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let row = unsafe { row_with(instance, NORMAL, "你") };
    // SAFETY: the row belongs to this live instance and no call has invalidated its snapshot.
    assert!(unsafe { pinyin_choose_candidate(instance, 0, row) } > 0);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { crate::support::pinyin_train(instance, 0) });
    let after = [
        ("ni3".to_owned(), -7),
        ("ni2".to_owned(), -1),
        ("na4".to_owned(), -1),
    ];
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { export_rows_of(context, 1, "你") }, after);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// Saves, then reopens the context on the same user directory.
///
/// # Safety
///
/// `context` is NULL or a live context: `pinyin_init` returned it and `pinyin_fini` has not taken
/// it.
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// `instance` belongs to `context`. The call frees both, so the caller does not use either again.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn save_and_reopen(
    context: *mut crate::support::PinyinContext,
    instance: *mut PinyinInstance,
    user_dir: &std::path::Path,
) -> (*mut crate::support::PinyinContext, *mut PinyinInstance) {
    assert!(unsafe { pinyin_save(context) });
    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
    open(user_dir.to_str().expect("UTF-8 path"))
}

/// S3: an import count other than -1 reaches the item as its `guint32` bit pattern
/// (`pinyin.cpp:523-524`, `:578`, `:601`); the export starts at -1 and writes the count only when
/// it is above 0 (`:703`, `:738-739`), so 0 exports as -1. A second add onto `INT_MAX` passes the
/// overflow guard (`phrase_index.cpp:78`) and stores 0x80000000. Every add answers true, and a
/// save and reopen keep the row. Observations `import-count/0`, `/-2`, `/INT_MIN`, `/INT_MAX`
/// and `/INT_MAX+1`.
#[test]
fn import_counts_reach_the_export_as_the_pin_stores_them() {
    type Row = (&'static str, &'static str, i32);
    let cases: [(&[Row], Row); 5] = [
        (&[("零次", "ling'ci", 0)], ("零次", "ling'ci", -1)),
        (&[("负二", "fu'er", -2)], ("负二", "fu'er", -2)),
        (
            &[("最小", "zui'xiao", i32::MIN)],
            ("最小", "zui'xiao", i32::MIN),
        ),
        (
            &[("最大", "zui'da", i32::MAX)],
            ("最大", "zui'da", i32::MAX),
        ),
        (
            &[("最大", "zui'da", i32::MAX), ("最大", "zui'da", 1)],
            ("最大", "zui'da", i32::MIN),
        ),
    ];
    for (rows, (phrase, reading, count)) in cases {
        let user_dir = TempUserDir::new("import-count");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        assert!(
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { import_batch(context, 7, rows) }
                .iter()
                .all(|&added| added)
        );
        let expected = vec![(phrase.to_owned(), reading.to_owned(), count)];
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        assert_eq!(unsafe { export_of(context, 7) }, expected, "{rows:?}");
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; nothing uses either handle after this call; only this thread calls into
        // them.
        let (context, instance) = unsafe { save_and_reopen(context, instance, &user_dir.path) };
        assert_eq!(
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { export_of(context, 7) },
            expected,
            "{rows:?} after reopen"
        );
        unsafe { crate::support::pinyin_free_instance(instance) };
        unsafe { crate::support::pinyin_fini(context) };
    }
}

/// Drains a bigram export iterator the same way.
///
/// # Safety
///
/// `iter` is NULL or a live bigram export iterator: `pinyin_begin_get_bigram_phrases` returned it
/// and `pinyin_end_get_bigram_phrases` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn drain_bigrams(
    iter: *mut crate::support::BigramExportIterator,
) -> Vec<(String, String, i32)> {
    let mut rows = Vec::new();
    while unsafe { crate::support::pinyin_bigram_iterator_has_next_phrase(iter) } {
        let mut phrase: *mut crate::support::GChar = ptr::null_mut();
        let mut pinyin: *mut crate::support::GChar = ptr::null_mut();
        let mut count: std::os::raw::c_int = 0;
        let more = unsafe {
            crate::support::pinyin_bigram_iterator_get_next_phrase(
                iter,
                &raw mut phrase,
                &raw mut pinyin,
                &raw mut count,
            )
        };
        // SAFETY: `phrase` and `pinyin` are this step's outputs, each NULL or a separate string
        // the library returned to this code, which owns them and frees them only here.
        // `take_exported_row` fails the test if either is NULL; `more` is not the check, since
        // `false` is the expected answer on the last row.
        rows.push(unsafe { take_exported_row(phrase, pinyin, count) });
        // Register row 36: `get_next` answers `has_next` after the row
        // (`pinyin.cpp:896-911`), so `false` on the last one.
        assert_eq!(more, unsafe {
            crate::support::pinyin_bigram_iterator_has_next_phrase(iter)
        });
    }
    rows
}

/// Trains one multi-phrase sentence through choose/guess/train: choosing 你
/// consumes the ni group, the 好 group is guessed at the post-choose cursor
/// (never offset 0), and the (你 → 好) bigram lands at 69.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn train_one_multiphrase_sentence(instance: *mut PinyinInstance) {
    // Train one multi-phrase sentence: (你 → 好) = 69.
    // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls into it.
    let _ = unsafe { candidate(instance, "nihao", 0) };
    let (_, ni_ptr) = {
        // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls into
        // it.
        let index = unsafe { capture_rows(instance) }
            .iter()
            .position(|(_, row, _)| row == "你")
            .expect("你 is offered for ni");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(unsafe {
            pinyin_get_candidate(
                instance,
                u32::try_from(index).expect("small candidate index"),
                &raw mut cand,
            )
        });
        (index, cand)
    };
    let ni_cursor = unsafe { pinyin_choose_candidate(instance, 0, ni_ptr) };
    assert_eq!(ni_cursor, 2, "choosing 你 consumes the ni group");
    // Guess at the post-choose cursor, not offset 0: the window is anchored at
    // the caller offset (`pinyin.cpp:2224`, mirrored by the C2 fix), so the
    // remaining 好 group is offered at offset 2, never at 0 — where the window
    // is the whole `nihao` (你/尼/…, no 好). The frontend passes the choose
    // cursor here (ibus's `key_rest(cursor)` idiom).
    assert!(unsafe {
        pinyin_guess_candidates(
            instance,
            usize::try_from(ni_cursor).expect("non-negative cursor"),
            DEFAULT_SORT,
        )
    });
    let hao_ptr = {
        // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls into
        // it.
        let index = unsafe { capture_rows(instance) }
            .iter()
            .position(|(_, row, _)| row == "好")
            .expect("好 is offered for hao");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(unsafe {
            pinyin_get_candidate(
                instance,
                u32::try_from(index).expect("small candidate index"),
                &raw mut cand,
            )
        });
        cand
    };
    assert!(unsafe { pinyin_choose_candidate(instance, 2, hao_ptr) } > 0);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_train(instance, 0) });
}

/// Register row 85 inside `get_next`: the pin writes the row's phrase,
/// pinyin and count (`pinyin.cpp:904-906`), advances, and only then runs the
/// `has_next` that loads the next predecessor and asserts on its short
/// total (`:910`, `:889`). The out-params live in a page shared with the
/// child, so the writes are read back after the abort.
// Unix only: the out-params live in a page shared through `mmap(MAP_SHARED)` (`assert_aborts_sharing`).
#[cfg(unix)]
#[test]
fn bigram_get_next_writes_its_row_before_the_short_predecessor_aborts() {
    let (first, second, successor) = export_order_fixture();
    let slot = std::mem::size_of::<usize>();
    let sentinel = vec![0xAB; 2 * slot + 4];
    let page = crate::support::assert_aborts_sharing(
        "import_export::bigram_get_next_writes_its_row_before_the_short_predecessor_aborts",
        &sentinel,
        |page| {
            let user_dir = TempUserDir::new("export-row-then-short");
            crate::support::plant_bigram(
                &user_dir.path,
                vec![
                    (first.to_le_bytes().to_vec(), whole_gram(successor)),
                    (second.to_le_bytes().to_vec(), vec![7, 0, 0]),
                ],
            );
            let context = init_fixture_at(&user_dir.path);
            let iter = unsafe { crate::support::pinyin_begin_get_bigram_phrases(context) };
            assert!(unsafe { crate::support::pinyin_bigram_iterator_has_next_phrase(iter) });
            // SAFETY: two pointer slots then a `c_int`, aligned in the page.
            let (phrase, pinyin, count) = unsafe {
                (
                    page.cast::<*mut crate::support::GChar>(),
                    page.add(slot).cast::<*mut crate::support::GChar>(),
                    page.add(2 * slot).cast::<std::os::raw::c_int>(),
                )
            };
            unsafe {
                crate::support::pinyin_bigram_iterator_get_next_phrase(iter, phrase, pinyin, count)
            };
        },
    );
    let pointer = |at: usize| &page[at..at + slot];
    assert!(
        pointer(0) != &sentinel[..slot] && pointer(0).iter().any(|&b| b != 0),
        "phrase written"
    );
    assert!(
        pointer(slot) != &sentinel[..slot] && pointer(slot).iter().any(|&b| b != 0),
        "pinyin written"
    );
    assert_eq!(
        std::os::raw::c_int::from_ne_bytes(
            page[2 * slot..2 * slot + 4].try_into().expect("4 bytes")
        ),
        200
    );
}

/// Register rows 85 and 98 in walk order: the pin's begin reads no gram
/// (`pinyin.cpp:775-787`), so a short predecessor the walk loads before a
/// partial one aborts at `has_next` (`:889`) and the partial one is never
/// read.
#[test]
fn bigram_export_aborts_at_a_short_predecessor_before_a_partial_one() {
    let (first, second, successor) = export_order_fixture();
    crate::support::assert_aborts(
        "import_export::bigram_export_aborts_at_a_short_predecessor_before_a_partial_one",
        || {
            let user_dir = TempUserDir::new("export-short-then-partial");
            crate::support::plant_bigram(
                &user_dir.path,
                vec![
                    (first.to_le_bytes().to_vec(), vec![7, 0, 0]),
                    (second.to_le_bytes().to_vec(), partial_gram(successor)),
                ],
            );
            let context = init_fixture_at(&user_dir.path);
            let iter = unsafe { crate::support::pinyin_begin_get_bigram_phrases(context) };
            assert!(!iter.is_null(), "begin reads no gram");
            let _ = unsafe { crate::support::pinyin_bigram_iterator_has_next_phrase(iter) };
        },
    );
}

/// Row 98 (class (b)) on the export walk: the walk ends with one warning
/// where it loads the partial value, after the rows before it.
#[test]
fn bigram_export_ends_at_a_partial_predecessor() {
    let (first, second, successor) = export_order_fixture();
    let user_dir = TempUserDir::new("export-whole-then-partial");
    crate::support::plant_bigram(
        &user_dir.path,
        vec![
            (first.to_le_bytes().to_vec(), whole_gram(successor)),
            (second.to_le_bytes().to_vec(), partial_gram(successor)),
        ],
    );
    let context = init_fixture_at(&user_dir.path);
    let iter = unsafe { crate::support::pinyin_begin_get_bigram_phrases(context) };
    assert!(!iter.is_null(), "begin reads no gram");
    assert!(unsafe { crate::support::pinyin_bigram_iterator_has_next_phrase(iter) });
    let mut phrase: *mut crate::support::GChar = ptr::null_mut();
    let mut pinyin: *mut crate::support::GChar = ptr::null_mut();
    let mut count: std::os::raw::c_int = 0;
    // The row is written, then the walk meets the partial value.
    assert!(!unsafe {
        crate::support::pinyin_bigram_iterator_get_next_phrase(
            iter,
            &raw mut phrase,
            &raw mut pinyin,
            &raw mut count,
        )
    });
    assert_eq!(count, 200);
    // SAFETY: `phrase` and `pinyin` are this step's outputs, each NULL or a separate string the
    // library returned to this code, which owns them and frees them only here.
    // `take_exported_row` fails the test if either is NULL; the `false` answer above is expected
    // here and is not the check.
    let _row = unsafe { take_exported_row(phrase, pinyin, count) };
    assert!(!unsafe { crate::support::pinyin_bigram_iterator_has_next_phrase(iter) });
    unsafe { crate::support::pinyin_end_get_bigram_phrases(iter) };
    unsafe { crate::support::pinyin_fini(context) };
}

/// Both export handles retain one shared runtime after fini; advancing the
/// cursor re-borrows its storage, including with no surviving instance.
#[test]
fn export_iterator_outlives_context() {
    let user_dir = TempUserDir::new("orphan-export");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let import = unsafe { pinyin_begin_add_phrases(context, 5) };
    assert!(unsafe {
        pinyin_iterator_add_phrase(import, cstr("你好").as_ptr(), cstr("ni'hao").as_ptr(), 7)
    });
    unsafe { pinyin_end_add_phrases(import) };
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    unsafe { train_one_multiphrase_sentence(instance) };
    let phrases = unsafe { crate::support::pinyin_begin_get_phrases(context, 5) };
    let bigrams = unsafe { crate::support::pinyin_begin_get_bigram_phrases(context) };
    assert!(!phrases.is_null());
    assert!(!bigrams.is_null());
    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
    assert_eq!(
        // SAFETY: `phrases` is a live iterator from its `begin` call, not yet ended; only this
        // thread calls into it.
        unsafe { drain_phrases(phrases) },
        vec![("你好".to_owned(), "ni'hao".to_owned(), 7)]
    );
    assert_eq!(
        // SAFETY: `bigrams` is a live iterator from its `begin` call, not yet ended; only this
        // thread calls into it.
        unsafe { drain_bigrams(bigrams) },
        vec![("你好".to_owned(), "ni'hao".to_owned(), 138)]
    );
    unsafe { crate::support::pinyin_end_get_phrases(phrases) };
    unsafe { crate::support::pinyin_end_get_bigram_phrases(bigrams) };
}
