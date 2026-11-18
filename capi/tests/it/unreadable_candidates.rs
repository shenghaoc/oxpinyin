//! C API regressions for a searched token whose phrase item is absent.

#![allow(unsafe_code)]
#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use pinyin_capi::{
    pinyin_alloc_instance, pinyin_choose_candidate, pinyin_fini, pinyin_free_instance,
    pinyin_get_candidate, pinyin_get_candidate_string, pinyin_get_n_candidate,
    pinyin_guess_candidates, pinyin_init, pinyin_parse_more_full_pinyins,
    pinyin_unload_phrase_library,
};

use std::ffi::CString;

use std::ptr;

#[path = "../../../tests/support/mod.rs"]
mod chunk_fixture;

mod fixture {
    use oxpinyin::ChewingKey;
    use oxpinyin::data::row_format::pinyin_index::{encode_complete_key, encode_item};
    use oxpinyin::store::{DefaultStore, WriteStore, WriteTxn};
    use std::{fs, path::Path};

    #[derive(Clone, Copy)]
    pub(super) enum Case {
        Single,
        Pair,
        Mixed,
        #[cfg_attr(not(unix), allow(dead_code))]
        ChooseToZero,
        Fallback,
        MaskedExtension,
        #[cfg(unix)]
        Zero,
    }

    pub(super) fn dirs(case: Case) -> (tempfile::TempDir, tempfile::TempDir) {
        let user = tempfile::tempdir().unwrap();
        let total = match case {
            Case::ChooseToZero => u32::MAX - 482,
            #[cfg(unix)]
            Case::Zero => 0,
            _ => 1,
        };
        let dir = tempfile::tempdir().unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/w3")
            .join(oxpinyin::data::DEFAULT_STORE_EXT);
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
        write_index(dir.path(), user.path(), case, key);
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
            if name == "gb_char.bin" && matches!(case, Case::Mixed | Case::ChooseToZero) {
                let reading = if matches!(case, Case::Mixed) {
                    key
                } else {
                    ChewingKey::from_pinyin("hao").unwrap()
                };
                chunk.add(
                    0x11,
                    1,
                    "你",
                    vec![(reading.with_tone(3).to_packed().to_le_bytes().to_vec(), 1)],
                );
            }
            fs::write(dir.join(name), chunk.build()).unwrap();
        }
    }

    /// Write the case's pinyin index: the user index in `user` for
    /// `Fallback`, else the system index in `dir` with the case's rows.
    fn write_index(dir: &Path, user: &Path, case: Case, key: ChewingKey) {
        let index_path = if matches!(case, Case::Fallback) {
            let conf = oxpinyin::data::table_info::TableConf::load_from_dir(dir).unwrap();
            let versions = oxpinyin::data::user_files::SystemVersions::from_conf(&conf);
            fs::write(
                user.join("user.conf"),
                oxpinyin::data::user_files::UserTableInfo::conform_to(&versions).to_text(),
            )
            .unwrap();
            user.join("user_pinyin_index.bin")
        } else {
            dir.join("pinyin_index.bin")
        };
        let token = if matches!(case, Case::MaskedExtension) {
            0x0200_0001
        } else {
            0x0100_0010
        };
        let mut value = encode_item(token, &[key.with_tone(3)]);
        if matches!(case, Case::Pair | Case::Mixed) {
            value.extend(encode_item(0x0100_0011, &[key.with_tone(3)]));
        }
        let extension = encode_complete_key(&[key, ChewingKey::from_pinyin("hao").unwrap()]);
        let key = encode_complete_key(&[key]);
        if matches!(case, Case::Fallback) {
            DefaultStore::write_user_index(&index_path, |sink| sink(&key, &value)).unwrap();
        } else {
            let index = DefaultStore::create(&index_path).unwrap();
            index
                .write(|txn| {
                    txn.put_raw(&key, &value)?;
                    if matches!(case, Case::MaskedExtension) {
                        // Too short for one record: only a decode of this
                        // row fails.
                        txn.put_raw(&extension, &[1, 2, 3])?;
                    }
                    if matches!(case, Case::ChooseToZero) {
                        let hao = ChewingKey::from_pinyin("hao").unwrap();
                        txn.put_raw(
                            &encode_complete_key(&[hao]),
                            &encode_item(0x0100_0011, &[hao.with_tone(3)]),
                        )?;
                    }
                    Ok(())
                })
                .unwrap();
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
    assert_eq!(
        count,
        u32::from(matches!(
            case,
            fixture::Case::Single
                | fixture::Case::Fallback
                | fixture::Case::Mixed
                | fixture::Case::ChooseToZero
        ))
    );
    if count == 1 {
        let mut row = ptr::null_mut();
        assert!(unsafe { pinyin_get_candidate(instance, 0, &raw mut row) });
        let mut row_type = std::mem::MaybeUninit::uninit();
        assert!(unsafe {
            pinyin_capi::pinyin_get_candidate_type(instance, row, row_type.as_mut_ptr())
        });
        // SAFETY: a successful getter writes the type of the live row.
        let row_type = unsafe { row_type.assume_init() };
        assert_eq!(row_type as i32, 2);
        let mut string = c"sentinel".as_ptr();
        assert!(unsafe { pinyin_get_candidate_string(instance, row, &raw mut string) });
        if matches!(case, fixture::Case::Mixed) {
            assert!(!string.is_null());
        } else {
            assert!(string.is_null());
            assert_eq!(
                unsafe { pinyin_choose_candidate(instance, 0, row) },
                if sort & 1 == 1 { 1 } else { 2 }
            );
            if sort & 1 == 0 {
                let cleared = unsafe { crate::support::pinyin_clear_constraint(instance, 0) };
                assert!(!cleared);
            }
        }
    }
    if matches!(case, fixture::Case::ChooseToZero) {
        // The missing item's choose increments the total by 483, wrapping
        // it to zero. Ranking the readable hao row must now abort.
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(instance, c"hao".as_ptr()) },
            3
        );
        assert!(unsafe { pinyin_guess_candidates(instance, 0, sort) });
    }
    unsafe { pinyin_free_instance(instance) };
    unsafe { pinyin_fini(context) };
}

#[test]
fn a_sole_unreadable_after_cursor_row_has_a_null_string() {
    check_row(fixture::Case::Single, 0x1f);
}

#[test]
fn two_unreadable_rows_are_omitted() {
    check_row(fixture::Case::Pair, 0x1f);
}

#[test]
fn a_sole_unreadable_row_survives_the_user_table_fallback() {
    check_row(fixture::Case::Fallback, 0x1f);
}

#[test]
fn choosing_a_sole_unreadable_row_reports_the_span_and_clears_the_constraint() {
    check_row(fixture::Case::Single, 0x1e);
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
/// the malformed `ni'hao` row is never read. Unloaded, the GBK token is
/// hidden; loaded, it is row 106's sole row with a NULL string.
#[test]
fn a_library_mask_does_not_read_extension_rows() {
    for (unload, expected) in [(false, 1), (true, 0)] {
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
        assert_eq!(count, expected, "unload {unload}");
        if count == 1 {
            let mut row = ptr::null_mut();
            assert!(unsafe { pinyin_get_candidate(instance, 0, &raw mut row) });
            let mut string = c"sentinel".as_ptr();
            assert!(unsafe { pinyin_get_candidate_string(instance, row, &raw mut string) });
            assert!(string.is_null());
        }
        unsafe { pinyin_free_instance(instance) };
        unsafe { pinyin_fini(context) };
    }
}

#[cfg(unix)]
#[test]
fn choosing_the_unreadable_row_adds_483_to_the_facade_total() {
    crate::support::assert_aborts(
        "unreadable_candidates::choosing_the_unreadable_row_adds_483_to_the_facade_total",
        || {
            check_row(fixture::Case::ChooseToZero, 0x1f);
        },
    );
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
