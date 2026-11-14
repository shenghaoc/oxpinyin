//! C API regressions for a searched token whose phrase item is absent.

#![allow(unsafe_code)]

use std::ffi::{CString, c_void};

unsafe extern "C" {
    fn pinyin_clear_constraint(instance: *mut c_void, offset: usize) -> bool;
}
use std::ptr;

use pinyin_capi::{
    pinyin_alloc_instance, pinyin_free_instance, pinyin_guess_candidates,
    pinyin_parse_more_full_pinyins,
};
use pinyin_capi::{
    pinyin_choose_candidate, pinyin_fini, pinyin_get_candidate, pinyin_get_candidate_string,
    pinyin_get_n_candidate, pinyin_init,
};

fn check_row(case: fixture::Case, sort: u32) {
    let (system, user) = fixture::dirs(case);
    let user_path = CString::new(user.path().as_os_str().as_encoded_bytes()).unwrap();
    let path = CString::new(system.path().as_os_str().as_encoded_bytes()).unwrap();
    let context = pinyin_init(path.as_ptr(), user_path.as_ptr());
    assert!(!context.is_null());
    let instance = pinyin_alloc_instance(context);
    assert!(!instance.is_null());
    assert_eq!(pinyin_parse_more_full_pinyins(instance, c"ni".as_ptr()), 2);
    assert!(pinyin_guess_candidates(instance, 0, sort));
    let mut count = 0;
    assert!(pinyin_get_n_candidate(instance, &raw mut count));
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
        assert!(pinyin_get_candidate(instance, 0, &raw mut row));
        let mut row_type = std::mem::MaybeUninit::uninit();
        assert!(pinyin_capi::pinyin_get_candidate_type(
            instance,
            row,
            row_type.as_mut_ptr()
        ));
        // SAFETY: a successful getter writes the type of the live row.
        let row_type = unsafe { row_type.assume_init() };
        assert_eq!(row_type as i32, 2);
        let mut string = c"sentinel".as_ptr();
        assert!(pinyin_get_candidate_string(instance, row, &raw mut string));
        if matches!(case, fixture::Case::Mixed) {
            assert!(!string.is_null());
        } else {
            assert!(string.is_null());
            assert_eq!(
                pinyin_choose_candidate(instance, 0, row),
                if sort & 1 == 1 { 1 } else { 2 }
            );
            if sort & 1 == 0 {
                // SAFETY: the instance is live.
                let cleared = unsafe { pinyin_clear_constraint(instance.cast(), 0) };
                assert!(!cleared);
            }
        }
    }
    if matches!(case, fixture::Case::ChooseToZero) {
        // The missing item's choose increments the total by 483, wrapping
        // it to zero. Ranking the readable hao row must now abort.
        assert_eq!(pinyin_parse_more_full_pinyins(instance, c"hao".as_ptr()), 3);
        assert!(pinyin_guess_candidates(instance, 0, sort));
    }
    pinyin_free_instance(instance);
    pinyin_fini(context);
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
        #[cfg_attr(not(unix), allow(dead_code))]
        ChooseToZero,
        Fallback,
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
            index
                .write(|txn| {
                    txn.put_raw(&key, &value)?;
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
        (dir, user)
    }
}

#[cfg(unix)]
#[test]
fn choosing_the_unreadable_row_adds_483_to_the_facade_total() {
    use std::os::unix::process::ExitStatusExt;

    const CHILD: &str = "OXPINYIN_CHOOSE_TO_ZERO_CHILD";
    if std::env::var_os(CHILD).is_some() {
        check_row(fixture::Case::ChooseToZero, 0x1f);
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "choosing_the_unreadable_row_adds_483_to_the_facade_total",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert_eq!(output.status.signal(), Some(libc::SIGABRT), "{output:?}");
}

#[cfg(unix)]
#[test]
fn zero_total_aborts_after_cursor() {
    use std::os::unix::process::ExitStatusExt;
    const CHILD: &str = "OXPINYIN_UNREADABLE_ZERO_TOTAL_CHILD";
    if std::env::var_os(CHILD).is_some() {
        check_row(fixture::Case::Zero, 0x1f);
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "zero_total_aborts_after_cursor"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert_eq!(output.status.signal(), Some(libc::SIGABRT), "{output:?}");
}
