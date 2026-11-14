//! C API regressions for a searched token whose phrase item is absent.
#![allow(unsafe_code)]

use std::ffi::CString;
use std::ptr;

use zhuyin_capi::{
    zhuyin_choose_candidate, zhuyin_fini, zhuyin_get_candidate, zhuyin_get_candidate_string,
    zhuyin_get_n_candidate, zhuyin_init,
};
use zhuyin_capi::{zhuyin_guess_candidates_after_cursor, zhuyin_guess_candidates_before_cursor};

unsafe extern "C" {
    fn zhuyin_alloc_instance(context: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn zhuyin_free_instance(instance: *mut std::ffi::c_void);
    fn zhuyin_parse_more_chewings(
        instance: *mut std::ffi::c_void,
        input: *const std::ffi::c_char,
    ) -> usize;
}

fn check_row(case: fixture::Case, before_cursor: bool) {
    let (system, user) = fixture::dirs(case);
    let user_path = CString::new(user.path().as_os_str().as_encoded_bytes()).unwrap();
    let path = CString::new(system.path().as_os_str().as_encoded_bytes()).unwrap();
    let context = zhuyin_init(path.as_ptr(), user_path.as_ptr());
    assert!(!context.is_null());
    // SAFETY: context is a live C API handle.
    let instance = unsafe { zhuyin_alloc_instance(context.cast()) };
    assert!(!instance.is_null());
    assert_eq!(
        // SAFETY: the instance is live and the input is NUL-terminated.
        unsafe { zhuyin_parse_more_chewings(instance, c"su3".as_ptr()) },
        3
    );
    let instance = instance.cast();
    if before_cursor {
        assert!(zhuyin_guess_candidates_before_cursor(instance, 3));
    } else {
        assert!(zhuyin_guess_candidates_after_cursor(instance, 0));
    }
    let mut count = 0;
    assert!(zhuyin_get_n_candidate(instance, &raw mut count));
    assert_eq!(
        count,
        u32::from(matches!(
            case,
            fixture::Case::Single | fixture::Case::Fallback | fixture::Case::Mixed
        ))
    );
    if count == 1 {
        let mut row = ptr::null_mut();
        assert!(zhuyin_get_candidate(instance, 0, &raw mut row));
        let mut row_type = std::mem::MaybeUninit::uninit();
        assert!(zhuyin_capi::zhuyin_get_candidate_type(
            instance,
            row,
            row_type.as_mut_ptr()
        ));
        // SAFETY: a successful getter writes the type of the live row.
        let row_type = unsafe { row_type.assume_init() };
        assert_eq!(row_type as i32, if before_cursor { 3 } else { 2 });
        let mut string = c"sentinel".as_ptr();
        assert!(zhuyin_get_candidate_string(instance, row, &raw mut string));
        if matches!(case, fixture::Case::Mixed) {
            assert!(!string.is_null());
        } else {
            assert!(string.is_null());
            assert_eq!(
                zhuyin_choose_candidate(instance, 0, row),
                if before_cursor { 0 } else { 3 }
            );
        }
    }
    // SAFETY: the C API instance is live and freed exactly once.
    unsafe { zhuyin_free_instance(instance.cast()) };
    zhuyin_fini(context);
}

#[test]
fn a_sole_unreadable_after_cursor_row_has_a_null_string() {
    check_row(fixture::Case::Single, false);
}

#[test]
fn a_sole_unreadable_before_cursor_row_has_a_null_string() {
    check_row(fixture::Case::Single, true);
}

#[test]
fn two_unreadable_rows_are_omitted() {
    check_row(fixture::Case::Pair, false);
}

#[test]
fn a_sole_unreadable_row_survives_the_user_table_fallback() {
    check_row(fixture::Case::Fallback, false);
}

#[test]
fn a_readable_candidate_displaces_an_unreadable_candidate() {
    check_row(fixture::Case::Mixed, false);
}

#[path = "../../oxpinyin-data/tests/support/mod.rs"]
mod chunk_fixture;

mod fixture {
    use oxpinyin_core::ChewingKey;
    use oxpinyin_data::row_format::pinyin_index::{encode_complete_key, encode_item};
    use oxpinyin_store::{DefaultStore, WriteStore, WriteTxn};
    use std::{fs, path::Path};

    #[derive(Clone, Copy)]
    pub(super) enum Case {
        Single,
        Pair,
        Mixed,
        Fallback,
        #[cfg(unix)]
        Zero,
    }

    pub(super) fn dirs(case: Case) -> (tempfile::TempDir, tempfile::TempDir) {
        let user = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        let total = u32::from(!matches!(case, Case::Zero));
        #[cfg(not(unix))]
        let total = 1_u32;
        let dir = tempfile::tempdir().unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/w3")
            .join(oxpinyin_data::DEFAULT_STORE_EXT);
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
            fs::write(dir.path().join(name), chunk.build()).unwrap();
        }

        let index_path = if matches!(case, Case::Fallback) {
            let conf = oxpinyin_data::table_info::TableConf::load_from_dir(dir.path()).unwrap();
            let versions = oxpinyin_data::user_files::SystemVersions::from_conf(&conf);
            fs::write(
                user.path().join("user.conf"),
                oxpinyin_data::user_files::UserTableInfo::conform_to(&versions).to_text(),
            )
            .unwrap();
            user.path().join("user_pinyin_index.bin")
        } else {
            dir.path().join("pinyin_index.bin")
        };
        let mut value = encode_item(0x0100_0010, &[key.with_tone(3)]);
        if matches!(case, Case::Pair | Case::Mixed) {
            value.extend(encode_item(0x0100_0011, &[key.with_tone(3)]));
        }
        let key = encode_complete_key(&[key]);
        if matches!(case, Case::Fallback) {
            DefaultStore::write_user_index(&index_path, |sink| sink(&key, &value)).unwrap();
        } else {
            let index = DefaultStore::create(&index_path).unwrap();
            index.write(|txn| txn.put_raw(&key, &value)).unwrap();
        }
        (dir, user)
    }
}

#[cfg(unix)]
#[test]
fn zero_total_aborts_after_cursor() {
    use std::os::unix::process::ExitStatusExt;
    const CHILD: &str = "OXPINYIN_UNREADABLE_ZERO_TOTAL_CHILD";
    if std::env::var_os(CHILD).is_some() {
        check_row(fixture::Case::Zero, false);
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "zero_total_aborts_after_cursor"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert_eq!(output.status.signal(), Some(libc::SIGABRT), "{output:?}");
}

#[cfg(unix)]
#[test]
fn zero_total_aborts_before_cursor() {
    use std::os::unix::process::ExitStatusExt;
    const CHILD: &str = "OXPINYIN_UNREADABLE_ZERO_TOTAL_CHILD";
    if std::env::var_os(CHILD).is_some() {
        check_row(fixture::Case::Zero, true);
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "zero_total_aborts_before_cursor"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert_eq!(output.status.signal(), Some(libc::SIGABRT), "{output:?}");
}
