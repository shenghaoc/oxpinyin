//! C API regressions for a searched token whose phrase item is absent.
#![allow(unsafe_code)]
#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use std::ffi::CString;

use std::ptr;

use oxzhuyin_capi::{
    zhuyin_fini, zhuyin_get_candidate, zhuyin_get_candidate_string, zhuyin_get_n_candidate,
    zhuyin_guess_candidates_after_cursor, zhuyin_guess_candidates_before_cursor, zhuyin_init,
};

use crate::support::{zhuyin_alloc_instance, zhuyin_free_instance, zhuyin_parse_more_chewings};

fn check_row(case: fixture::Case, before_cursor: bool) {
    let (system, user) = fixture::dirs(case);
    let user_path = CString::new(user.path().as_os_str().as_encoded_bytes()).unwrap();
    let path = CString::new(system.path().as_os_str().as_encoded_bytes()).unwrap();
    let context = unsafe { zhuyin_init(path.as_ptr(), user_path.as_ptr()) };
    assert!(!context.is_null());
    let instance = unsafe { zhuyin_alloc_instance(context.cast()) };
    assert!(!instance.is_null());
    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(instance, c"su3".as_ptr()) },
        3
    );
    let instance = instance.cast();
    if before_cursor {
        assert!(unsafe { zhuyin_guess_candidates_before_cursor(instance, 3) });
    } else {
        assert!(unsafe { zhuyin_guess_candidates_after_cursor(instance, 0) });
    }
    let mut count = 0;
    assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut count) });
    assert_eq!(count, u32::from(matches!(case, fixture::Case::Mixed)));
    if count == 1 {
        let mut row = ptr::null_mut();
        assert!(unsafe { zhuyin_get_candidate(instance, 0, &raw mut row) });
        let mut row_type = std::mem::MaybeUninit::uninit();
        assert!(unsafe {
            oxzhuyin_capi::zhuyin_get_candidate_type(instance, row, row_type.as_mut_ptr())
        });
        // SAFETY: a successful getter writes the type of the live row.
        let row_type = unsafe { row_type.assume_init() };
        assert_eq!(row_type as i32, if before_cursor { 3 } else { 2 });
        let mut string = c"sentinel".as_ptr();
        assert!(unsafe { zhuyin_get_candidate_string(instance, row, &raw mut string) });
        assert!(!string.is_null());
    }
    unsafe { zhuyin_free_instance(instance.cast()) };
    unsafe { zhuyin_fini(context) };
}

#[path = "../../../tests/support/mod.rs"]
mod chunk_fixture;

#[path = "../../../tests/support/unreadable_fixture.rs"]
mod fixture;

/// POLICY-ACCEPTED (row 106): both cursor paths omit an item removed by a journal.
#[test]
fn a_journal_remove_omits_the_row_on_both_cursor_paths() {
    use crate::support::{
        init_on, libraries_system, modify_header, plant_log, system_dir, zhuyin_get_candidate_type,
        zhuyin_parse_more_full_pinyins,
    };
    let scratch = tempfile::tempdir().unwrap();
    let total = 1000_u32;
    let system = libraries_system(
        &system_dir(),
        scratch.path(),
        &[
            (1, "gb_char.bin", &[(&[0x7532], &[0x1081], 100)], total),
            (2, "gbk_char.bin", &[], 0),
            (3, "opengram.bin", &[], 0),
            (4, "merged.bin", &[], 0),
        ],
    );
    // Token 0x01000001 as `libraries_system` stores it: the length and
    // reading count, the unigram, the text, the key and the reading's count.
    let item = [
        &[1_u8, 1][..],
        &100_u32.to_le_bytes(),
        &0x7532_u32.to_le_bytes(),
        &0x1081_u16.to_le_bytes(),
        &10_u32.to_le_bytes(),
    ]
    .concat();
    let remove = [
        &2_u32.to_le_bytes()[..],
        &0x0100_0001_u32.to_le_bytes(),
        &u16::try_from(item.len()).unwrap().to_le_bytes(),
        &item,
    ]
    .concat();
    for removed in [false, true] {
        for before_cursor in [false, true] {
            let user = tempfile::tempdir().unwrap();
            let log = if removed { &remove[..] } else { &[] };
            plant_log(
                user.path(),
                "gb_char.dbin",
                &[&modify_header(0, total, total)[..], log].concat(),
            );
            let context = init_on(&system, Some(user.path()));
            assert!(!context.is_null());
            let instance = unsafe { zhuyin_alloc_instance(context) };
            assert_eq!(
                unsafe { zhuyin_parse_more_full_pinyins(instance, c"ba1".as_ptr()) },
                3
            );
            if before_cursor {
                assert!(unsafe { zhuyin_guess_candidates_before_cursor(instance, 3) });
            } else {
                assert!(unsafe { zhuyin_guess_candidates_after_cursor(instance, 0) });
            }
            let mut count = 0;
            assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut count) });
            assert_eq!(
                count,
                u32::from(!removed),
                "removed {removed}, before {before_cursor}"
            );
            if removed {
                unsafe { zhuyin_free_instance(instance) };
                unsafe { zhuyin_fini(context) };
                continue;
            }
            let mut row = ptr::null_mut();
            assert!(unsafe { zhuyin_get_candidate(instance, 0, &raw mut row) });
            let mut row_type = std::mem::MaybeUninit::uninit();
            assert!(unsafe { zhuyin_get_candidate_type(instance, row, row_type.as_mut_ptr()) });
            // SAFETY: a successful getter writes the type of the live row.
            let row_type = unsafe { row_type.assume_init() };
            assert_eq!(row_type as i32, if before_cursor { 3 } else { 2 });
            let mut string = c"sentinel".as_ptr();
            assert!(unsafe { zhuyin_get_candidate_string(instance, row, &raw mut string) });
            assert!(!string.is_null());
            // SAFETY: the successful getter wrote a live, NUL-terminated string.
            let text = unsafe { std::ffi::CStr::from_ptr(string) }
                .to_str()
                .unwrap();
            assert_eq!(text, "甲");
            unsafe { zhuyin_free_instance(instance) };
            unsafe { zhuyin_fini(context) };
        }
    }
}

/// POLICY-ACCEPTED (row 106).
#[test]
fn a_sole_unreadable_after_cursor_row_is_omitted() {
    check_row(fixture::Case::Single, false);
}

/// POLICY-ACCEPTED (row 106).
#[test]
fn a_sole_unreadable_before_cursor_row_is_omitted() {
    check_row(fixture::Case::Single, true);
}

#[test]
fn two_unreadable_rows_are_omitted() {
    check_row(fixture::Case::Pair, false);
}

#[test]
fn a_readable_candidate_displaces_an_unreadable_candidate() {
    check_row(fixture::Case::Mixed, false);
}

#[cfg(unix)]
#[test]
fn zero_total_aborts_after_cursor() {
    let _ = crate::support::assert_aborts(
        "unreadable_candidates::zero_total_aborts_after_cursor",
        || {
            check_row(fixture::Case::Zero, false);
        },
    );
}

#[cfg(unix)]
#[test]
fn zero_total_aborts_before_cursor() {
    let _ = crate::support::assert_aborts(
        "unreadable_candidates::zero_total_aborts_before_cursor",
        || {
            check_row(fixture::Case::Zero, true);
        },
    );
}

use crate::support::zhuyin_unload_phrase_library;
/// POLICY-ACCEPTED (row 106).
#[test]
fn a_sole_unreadable_row_is_omitted_after_the_user_table_fallback() {
    check_row(fixture::Case::Fallback, false);
}

/// The zhuyin twin of the pinyin facade's
/// `a_library_mask_does_not_read_extension_rows`. `zhuyin_unload_phrase_library`
/// unloads any library but TSI (`zhuyin.cpp:379-390` at 074a2219), and the
/// after-cursor loop searches `ni` alone for one syllable
/// (`zhuyin.cpp:1496-1513`), whose `SEARCH_CONTINUED` comes from the key's
/// DB read before any token or library check
/// (`chewing_large_table2_tkrzwdb.cpp:146-152`,
/// `chewing_large_table2_bdb.cpp:182-187`,
/// `chewing_large_table2_kyotodb.cpp:160-166`). The malformed `ni'hao` row
/// is never read.
#[test]
fn a_library_mask_does_not_read_extension_rows() {
    for unload in [false, true] {
        let (system, user) = fixture::dirs(fixture::Case::MaskedExtension);
        let user_path = CString::new(user.path().as_os_str().as_encoded_bytes()).unwrap();
        let path = CString::new(system.path().as_os_str().as_encoded_bytes()).unwrap();
        let context = unsafe { zhuyin_init(path.as_ptr(), user_path.as_ptr()) };
        assert!(!context.is_null());
        if unload {
            // SAFETY: context is a live C API handle.
            assert!(unsafe { zhuyin_unload_phrase_library(context.cast(), 2) });
        }
        // SAFETY: context is a live C API handle.
        let instance = unsafe { zhuyin_alloc_instance(context.cast()) };
        assert!(!instance.is_null());
        assert_eq!(
            // SAFETY: the instance is live and the input is NUL-terminated.
            unsafe { zhuyin_parse_more_chewings(instance, c"su3".as_ptr()) },
            3
        );
        let instance = instance.cast();
        assert!(
            unsafe { zhuyin_guess_candidates_after_cursor(instance, 0) },
            "unload {unload}"
        );
        let mut count = u32::MAX;
        assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut count) });
        assert_eq!(count, 0, "unload {unload}");
        // SAFETY: the C API instance is live and freed exactly once.
        unsafe { zhuyin_free_instance(instance.cast()) };
        unsafe { zhuyin_fini(context) };
    }
}
