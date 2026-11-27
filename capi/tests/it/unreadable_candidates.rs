//! C API regressions for a searched token whose phrase item is absent.

#![allow(unsafe_code)]
#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use oxpinyin_capi::candidates::{
    pinyin_get_candidate, pinyin_get_candidate_string, pinyin_get_n_candidate,
};
use oxpinyin_capi::config::pinyin_unload_phrase_library;
use oxpinyin_capi::context::{pinyin_fini, pinyin_init};
use oxpinyin_capi::instance::{pinyin_alloc_instance, pinyin_free_instance};
use oxpinyin_capi::parse::pinyin_parse_more_full_pinyins;
use oxpinyin_capi::sentence::pinyin_guess_candidates;

use std::ffi::CString;

use std::ptr;

#[path = "../../../tests/support/mod.rs"]
mod chunk_fixture;

mod fixture {
    use oxpinyin::__internal::data::row_format::pinyin_index::{encode_complete_key, encode_item};
    use oxpinyin::__internal::store::{DefaultStore, WriteStore};
    use oxpinyin::facade::ChewingKey;
    use std::{fs, path::Path};

    #[derive(Clone, Copy)]
    pub(super) enum Case {
        Single,
        Pair,
        Mixed,
        MaskedExtension,
        #[cfg(unix)]
        Zero,
        /// [`Case::MaskedExtension`] with a zero facade total.
        #[cfg(unix)]
        MaskedZero,
    }

    pub(super) fn dirs(case: Case) -> (tempfile::TempDir, tempfile::TempDir) {
        let user = tempfile::tempdir().unwrap();
        let total = match case {
            #[cfg(unix)]
            Case::Zero | Case::MaskedZero => 0,
            Case::Single | Case::Pair | Case::Mixed | Case::MaskedExtension => 1,
        };
        let dir = tempfile::tempdir().unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/w3")
            .join(oxpinyin::__internal::DEFAULT_STORE_EXT);
        for name in [
            "table.conf",
            "phrase_index.bin",
            "bigram.db",
            "punct.bin",
            "addon_pinyin_index.bin",
            "addon_phrase_index.bin",
        ] {
            fs::copy(source.join(name), dir.path().join(name)).unwrap();
        }

        let key = ChewingKey::from_pinyin("ni").unwrap();
        write_libraries(dir.path(), case, total, key);
        write_index(dir.path(), case, key);
        (dir, user)
    }

    /// Write the four library chunks into `dir`: `gb_char.bin` with the
    /// case's total and item, the others empty.
    fn write_libraries(dir: &Path, case: Case, total: u32, key: ChewingKey) {
        // Keep the library loaded independently of the indexed slots.
        for (name, total) in [
            ("gb_char.bin", total),
            ("gbk_char.bin", 0),
            ("merged.bin", 0),
            ("opengram.bin", 0),
        ] {
            let mut chunk = super::chunk_fixture::ChunkBuilder::new(total);
            if name == "gb_char.bin" && matches!(case, Case::Mixed) {
                chunk.add(
                    0x11,
                    1,
                    "你",
                    vec![(key.with_tone(3).to_packed().to_le_bytes().to_vec(), 1)],
                );
            }
            fs::write(dir.join(name), chunk.build()).unwrap();
        }
    }

    /// Write the case's rows into the system pinyin index in `dir`.
    fn write_index(dir: &Path, case: Case, key: ChewingKey) {
        #[cfg(unix)]
        let masked = matches!(case, Case::MaskedExtension | Case::MaskedZero);
        #[cfg(not(unix))]
        let masked = matches!(case, Case::MaskedExtension);
        let token = if masked { 0x0200_0001 } else { 0x0100_0010 };
        let mut value = encode_item(token, &[key.with_tone(3)]);
        if matches!(case, Case::Pair | Case::Mixed) {
            value.extend(encode_item(0x0100_0011, &[key.with_tone(3)]));
        }
        let extension = encode_complete_key(&[key, ChewingKey::from_pinyin("hao").unwrap()]);
        let key = encode_complete_key(&[key]);
        let index = DefaultStore::create(&dir.join("pinyin_index.bin")).unwrap();
        index.set_raw(&key, &value).unwrap();
        if masked {
            // Too short for one record: only a decode of this
            // row fails.
            index.set_raw(&extension, &[1, 2, 3]).unwrap();
        }
    }
}

fn check_row(case: fixture::Case, sort: u32) {
    let (system, user) = fixture::dirs(case);
    let user_path = CString::new(user.path().as_os_str().as_encoded_bytes()).unwrap();
    let path = CString::new(system.path().as_os_str().as_encoded_bytes()).unwrap();
    let context = unsafe { pinyin_init(path.as_ptr(), user_path.as_ptr()) };
    assert!(!context.is_null());
    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, c"ni".as_ptr()) },
        2
    );
    assert!(unsafe { pinyin_guess_candidates(instance, 0, sort) });
    let mut count = 0;
    assert!(unsafe { pinyin_get_n_candidate(instance, &raw mut count) });
    assert_eq!(count, u32::from(matches!(case, fixture::Case::Mixed)));
    if count == 1 {
        let mut row = ptr::null_mut();
        assert!(unsafe { pinyin_get_candidate(instance, 0, &raw mut row) });
        let mut row_type = std::mem::MaybeUninit::uninit();
        assert!(unsafe {
            oxpinyin_capi::candidates::pinyin_get_candidate_type(
                instance,
                row,
                row_type.as_mut_ptr(),
            )
        });
        // SAFETY: a successful getter writes the type of the live row.
        let row_type = unsafe { row_type.assume_init() };
        assert_eq!(row_type as i32, 2);
        let mut string = c"sentinel".as_ptr();
        assert!(unsafe { pinyin_get_candidate_string(instance, row, &raw mut string) });
        assert!(!string.is_null());
    }
    unsafe { pinyin_free_instance(instance) };
    unsafe { pinyin_fini(context) };
}

/// omit an unreadable item even when it is the only match.
#[test]
fn a_sole_unreadable_after_cursor_row_is_omitted() {
    check_row(fixture::Case::Single, 0x1f);
}

#[test]
fn two_unreadable_rows_are_omitted() {
    check_row(fixture::Case::Pair, 0x1f);
}

#[test]
fn a_readable_candidate_displaces_an_unreadable_candidate() {
    check_row(fixture::Case::Mixed, 0x1f);
}

/// The widen probe reads no extension row, library mask or not. The pin
/// sets `SEARCH_CONTINUED` once the key's DB read succeeds, before any
/// token or library check (`chewing_large_table2_tkrzwdb.cpp:146-152`,
/// `chewing_large_table2_bdb.cpp:182-187`,
/// `chewing_large_table2_kyotodb.cpp:160-166` at 074a2219); the mask only
/// drops a token at `convert`'s NULL range (`chewing_large_table2.h:107-111`).
/// For `ni` alone the guess searches only `ni` (`pinyin.cpp:2229-2280`), so
/// the malformed `ni'hao` row is never read. The GBK token has no readable item.
#[test]
fn a_library_mask_does_not_read_extension_rows() {
    for unload in [false, true] {
        let (system, user) = fixture::dirs(fixture::Case::MaskedExtension);
        let user_path = CString::new(user.path().as_os_str().as_encoded_bytes()).unwrap();
        let path = CString::new(system.path().as_os_str().as_encoded_bytes()).unwrap();
        let context = unsafe { pinyin_init(path.as_ptr(), user_path.as_ptr()) };
        assert!(!context.is_null());
        if unload {
            assert!(unsafe { pinyin_unload_phrase_library(context, 2) });
        }
        let instance = unsafe { pinyin_alloc_instance(context) };
        assert!(!instance.is_null());
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(instance, c"ni".as_ptr()) },
            2
        );
        assert!(
            unsafe { pinyin_guess_candidates(instance, 0, 0x1f) },
            "unload {unload}"
        );
        let mut count = u32::MAX;
        assert!(unsafe { pinyin_get_n_candidate(instance, &raw mut count) });
        assert_eq!(count, 0, "unload {unload}");
        unsafe { pinyin_free_instance(instance) };
        unsafe { pinyin_fini(context) };
    }
}

/// At a zero facade total the guess dies on any searched row it ranks,
/// and on a row whose item cannot be read (`zero_total_aborts_after_cursor`).
/// A token of an unloaded library is neither: the pin frees that
/// sub-index (`phrase_index.cpp:260-268` at 074a2219), so its row drops
/// out before the ranking and the guess answers an empty list.
#[cfg(unix)]
#[test]
fn a_zero_total_guess_skips_an_unloaded_librarys_token() {
    let (system, user) = fixture::dirs(fixture::Case::MaskedZero);
    let user_path = CString::new(user.path().as_os_str().as_encoded_bytes()).unwrap();
    let path = CString::new(system.path().as_os_str().as_encoded_bytes()).unwrap();
    let context = unsafe { pinyin_init(path.as_ptr(), user_path.as_ptr()) };
    assert!(!context.is_null());
    assert!(unsafe { pinyin_unload_phrase_library(context, 2) });
    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, c"ni".as_ptr()) },
        2
    );
    assert!(unsafe { pinyin_guess_candidates(instance, 0, 0x1f) });
    let mut count = u32::MAX;
    assert!(unsafe { pinyin_get_n_candidate(instance, &raw mut count) });
    assert_eq!(count, 0);
    unsafe { pinyin_free_instance(instance) };
    unsafe { pinyin_fini(context) };
}

#[cfg(unix)]
#[test]
fn zero_total_aborts_after_cursor() {
    crate::support::assert_aborts(
        "unreadable_candidates::zero_total_aborts_after_cursor",
        || {
            check_row(fixture::Case::Zero, 0x1f);
        },
    );
}
