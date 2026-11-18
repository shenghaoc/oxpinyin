#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::context::pinyin_save;
use crate::test_support::drain_phrases;
#[cfg(unix)]
use crate::test_support::take_exported;

use crate::test_support::with_store;

use crate::iterators::{
    pinyin_begin_add_phrases, pinyin_begin_get_phrases, pinyin_end_add_phrases,
    pinyin_end_get_phrases, pinyin_iterator_add_phrase, pinyin_iterator_has_next_phrase,
};

use crate::test_support::{TempUserDir, cstr, open};

#[cfg(unix)]
use oxpinyin::user::USER_DICTIONARY;

use std::ptr;

/// Exports the user dictionary after planting a reading one tone past
/// `MAX_KEY_TONE` behind a spellable one; the aborting read writes its
/// out-params into `page` (phrase, pinyin, then count).
// Unix only: the out-params live in a page shared through `mmap(MAP_SHARED)` (`assert_aborts_sharing`).
#[cfg(unix)]
fn export_an_unspellable_reading(page: *mut u8) {
    let user_dir = TempUserDir::new("export-unspellable");
    let (context, _instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    {
        // SAFETY: `context` is a live `pinyin_init` handle, borrowed
        // uniquely for this block.
        let ctx = unsafe { crate::state::context_mut(context) };
        let store = ctx.core.user_store_mut();
        let syllable = |text: &str| {
            oxpinyin::SyllableKey::from_text(text)
                .expect("syllable")
                .index()
        };
        let ni = oxpinyin::user::toned_key(syllable("ni"), 0).expect("key");
        store
            .add_phrase_in(USER_DICTIONARY, &"你".into(), &[ni], Some(5))
            .expect("a reading the engine spells");
        // One past `MAX_KEY_TONE`: the add checks lengths only.
        let past_the_tones =
            oxpinyin::user::toned_key(syllable("hao"), oxpinyin::user::MAX_KEY_TONE).expect("key")
                + (1 << oxpinyin::user::KEY_TONE_SHIFT);
        store
            .add_phrase_in(USER_DICTIONARY, &"好".into(), &[past_the_tones], Some(3))
            .expect("stored unchecked");
    }
    let iter = unsafe { crate::iterators::pinyin_begin_get_phrases(context, 7) };
    assert!(!iter.is_null());
    let mut phrase: *mut crate::types::GChar = ptr::null_mut();
    let mut pinyin: *mut crate::types::GChar = ptr::null_mut();
    let mut count: std::os::raw::c_int = 0;
    assert!(unsafe {
        crate::iterators::pinyin_iterator_get_next_phrase(
            iter,
            &raw mut phrase,
            &raw mut pinyin,
            &raw mut count,
        )
    });
    assert_eq!(
        (take_exported(phrase), take_exported(pinyin), count),
        ("你".to_owned(), "ni".to_owned(), 5)
    );
    // The probe selected the second item: it has a pronunciation.
    assert!(unsafe { crate::iterators::pinyin_iterator_has_next_phrase(iter) });
    // The read of the second item aborts.
    let slot = std::mem::size_of::<usize>();
    // SAFETY: the page holds two pointer slots then a `c_int`, each aligned
    // (the mapping is page-aligned).
    let (phrase_out, pinyin_out, count_out) = unsafe {
        (
            page.cast::<*mut crate::types::GChar>(),
            page.add(slot).cast::<*mut crate::types::GChar>(),
            page.add(2 * slot).cast::<std::os::raw::c_int>(),
        )
    };
    unsafe {
        crate::iterators::pinyin_iterator_get_next_phrase(iter, phrase_out, pinyin_out, count_out)
    };
}

/// Register row 62: a `USER_FILE` reading the probe counts
/// (`pinyin.cpp:682`) but `get_pinyin_string` cannot spell (`:728` →
/// `chewing_key.cpp:48`) is read, not skipped, and the pin's assert aborts
/// the process. No recorded contract diff harness fixture can plant the reading in the
/// pin's sub-index, so this death test holds the site.
// Unix only: the out-params live in a page shared through `mmap(MAP_SHARED)` (`assert_aborts_sharing`).
#[cfg(unix)]
#[test]
fn export_iterator_aborts_on_a_reading_it_cannot_spell() {
    // The out-params live in a page shared with the child: `pinyin.cpp:703`
    // writes NULL, NULL and -1 before the assert, over these sentinels.
    let slot = std::mem::size_of::<usize>();
    let page = crate::test_support::assert_aborts_sharing(
        "iterators::tests::export_iterator_aborts_on_a_reading_it_cannot_spell",
        &vec![0xAB; 2 * slot + 4],
        export_an_unspellable_reading,
    );
    assert!(
        page[..2 * slot].iter().all(|&byte| byte == 0),
        "NULL phrase and pinyin"
    );
    assert_eq!(
        std::os::raw::c_int::from_ne_bytes(
            page[2 * slot..2 * slot + 4].try_into().expect("4 bytes")
        ),
        -1
    );
}

#[test]
fn import_iterators_add_per_phrase_and_arm_modified_at_end() {
    let user_dir = TempUserDir::new("import");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // Null-safety first: add/end tolerate null exactly like the export
    // quartet. `begin` with a NULL context is covered by
    // `null_context_import_handle_is_returned_and_freed`: the pin returns an
    // iterator there (`pinyin.cpp:506-512`).
    assert!(!unsafe {
        pinyin_iterator_add_phrase(
            ptr::null_mut(),
            cstr("你好").as_ptr(),
            cstr("nihao").as_ptr(),
            1,
        )
    });
    unsafe { pinyin_end_add_phrases(ptr::null_mut()) };

    let iter = unsafe { pinyin_begin_add_phrases(context, 7) };
    assert!(!iter.is_null());

    // Default count -1 is 5; a same-reading re-add accumulates 5+7=12 —
    // upstream `_add_phrase` / `PhraseItem::add_pronunciation`, not an
    // add-or-update. Trailing unparsed pinyin bytes are ignored, exactly
    // like `FullPinyinParser2`'s longest-prefix final step.
    assert!(unsafe {
        pinyin_iterator_add_phrase(iter, cstr("你好").as_ptr(), cstr("nihao").as_ptr(), -1)
    });
    assert!(unsafe {
        pinyin_iterator_add_phrase(iter, cstr("你好").as_ptr(), cstr("nihao").as_ptr(), 7)
    });
    assert!(unsafe {
        pinyin_iterator_add_phrase(iter, cstr("世界").as_ptr(), cstr("shijieXYZ").as_ptr(), 3)
    });

    // Bad pinyin (no complete keys, incomplete-only, empty) reports false
    // and writes nothing for that call.
    assert!(!unsafe {
        pinyin_iterator_add_phrase(iter, cstr("你好").as_ptr(), cstr("n").as_ptr(), 1)
    });
    assert!(!unsafe {
        pinyin_iterator_add_phrase(iter, cstr("你好").as_ptr(), cstr("!!!").as_ptr(), 1)
    });
    assert!(!unsafe {
        pinyin_iterator_add_phrase(iter, cstr("你好").as_ptr(), cstr("").as_ptr(), 1)
    });

    // Adds are per-phrase durable commits, but upstream's m_modified
    // set-site is _end_add_phrases (pinyin.cpp:658) — a save before end is
    // the unmodified no-op.
    assert!(!unsafe { pinyin_save(context) });
    assert!(!with_store(instance, |store| store.is_modified()));

    unsafe { pinyin_end_add_phrases(iter) };

    // End armed the shared dirty flag; the next save compacts and clears it.
    assert!(with_store(instance, |store| store.is_modified()));
    assert!(unsafe { pinyin_save(context) });
    assert!(!with_store(instance, |store| store.is_modified()));

    let export = unsafe { pinyin_begin_get_phrases(context, 7) };
    assert_eq!(
        drain_phrases(export),
        vec![
            ("你好".to_owned(), "ni'hao".to_owned(), 12),
            ("世界".to_owned(), "shi'jie".to_owned(), 3),
        ]
    );
    unsafe { pinyin_end_get_phrases(export) };

    // A loaded system index accepts imports through its sparse overlay
    // (074a2219 pinyin.cpp:585-608).
    let system = unsafe { pinyin_begin_add_phrases(context, 1) };
    assert!(!system.is_null());
    assert!(unsafe {
        pinyin_iterator_add_phrase(system, cstr("测试").as_ptr(), cstr("ce'shi").as_ptr(), 1)
    });
    unsafe { pinyin_end_add_phrases(system) };

    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

#[test]
fn empty_import_batch_arms_modified_and_saves() {
    let user_dir = TempUserDir::new("import-empty");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let iter = unsafe { pinyin_begin_add_phrases(context, 7) };
    assert!(!iter.is_null());
    unsafe { pinyin_end_add_phrases(iter) };

    // Upstream sets m_modified unconditionally at end (:657-658), even with
    // no add calls, so the next save is a write cycle, not a no-op.
    assert!(with_store(instance, |store| store.is_modified()));
    assert!(unsafe { pinyin_save(context) });
    assert!(!with_store(instance, |store| store.is_modified()));

    let export = unsafe { pinyin_begin_get_phrases(context, 7) };
    assert!(!unsafe { pinyin_iterator_has_next_phrase(export) });
    unsafe { pinyin_end_get_phrases(export) };

    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// An import reading parses past long separator runs, between its keys and
/// after them, to its two keys.
#[test]
fn an_import_reading_parses_past_long_separator_runs() {
    let user = TempUserDir::new("import-separator-runs");
    let (context, instance) = open(user.path.to_str().unwrap());
    let iter = unsafe { crate::iterators::pinyin_begin_add_phrases(context, 7) };
    for reading in [
        [&b"ni3"[..], &[b'\''; 40_000], b"hao3"].concat(),
        [&b"ni3'hao3"[..], &[b'\''; 70_000]].concat(),
    ] {
        let reading = std::ffi::CString::new(reading).expect("no NUL");
        assert!(unsafe {
            crate::iterators::pinyin_iterator_add_phrase(
                iter,
                c"你好".as_ptr(),
                reading.as_ptr(),
                1,
            )
        });
    }
    unsafe { crate::iterators::pinyin_end_add_phrases(iter) };
    assert_eq!(
        crate::test_support::export_of(context, 7),
        [("你好".to_owned(), "ni3'hao3".to_owned(), 2)]
    );
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}
