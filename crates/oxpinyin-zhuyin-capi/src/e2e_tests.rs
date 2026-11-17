//! End-to-end tests: the zhuyin training, import, and dictionary entry
//! points, driven through the C ABI functions themselves — the battery
//! mirroring `oxpinyin-capi`'s e2e suite (the review that found this
//! crate had only a header-identity check). The unit tests call the
//! `#[unsafe(no_mangle)]` symbols directly (they are ordinary crate
//! functions when compiled for testing) and read the counts back
//! through the instance's own store handle — the committed state,
//! exactly as an export would observe it. The system tables are the
//! committed mini fixture (`fixtures/w3`); no model bytes are added by
//! these tests.
//!
//! `su3cl3` is 你好 on the standard chewing keyboard (the `'`-joined
//! session buffer `ni3'hao3`), the same composition the cursor tests in
//! `lib.rs` pin.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use std::os::raw::c_char;

use std::ptr;

use oxpinyin_user::SENTENCE_START;

use crate::candidates::{zhuyin_choose_candidate, zhuyin_train};

use crate::context::{zhuyin_fini, zhuyin_init, zhuyin_save};

use crate::instance::{zhuyin_alloc_instance, zhuyin_reset};

use crate::iterators::{
    zhuyin_begin_add_phrases, zhuyin_end_add_phrases, zhuyin_iterator_add_phrase,
};

use crate::sentence::{zhuyin_get_sentence, zhuyin_guess_sentence};

use crate::test_support::{
    TempUserDir, candidate, close, cstr, open_with_user, system_dir, take_sentence, token_of,
    with_store,
};

/// The choose-then-train seed sequence through the wired zhuyin path:
/// the doubling counts (69, 138, 414, …) with `sentence_start` as the
/// first predecessor, and the fresh-composition reset contract.
#[test]
fn train_records_the_pinned_doubling_sequence() {
    let user_dir = TempUserDir::new("train-doubling");
    let (context, instance) = open_with_user(&user_dir.path);

    let first = candidate(instance, "su3cl3", 0);
    let t1 = token_of(instance, first);
    assert!(unsafe { zhuyin_choose_candidate(instance, 0, first) } > 0);
    assert!(
        !unsafe { zhuyin_train(instance) },
        "a choose without decoded results refuses"
    );
    // The system item's live count, as the chunk shipped it.
    let shipped = with_store(instance, |store| {
        assert_eq!(store.bigram_count(SENTENCE_START, t1).unwrap(), 0);
        store.system_unigram(t1).expect("item")
    });

    // 69 on first selection; the predecessor is sentence_start.
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert!(unsafe { zhuyin_train(instance) });
    with_store(instance, |store| {
        assert_eq!(store.bigram_count(SENTENCE_START, t1).unwrap(), 69);
        assert_eq!(store.bigram_total(SENTENCE_START).unwrap(), 69);
        assert_eq!(store.system_unigram(t1), Some(shipped + 483)); // 69 * 7
    });

    // 138 on reselection (count 207), then 414 (count 621).
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert!(unsafe { zhuyin_train(instance) });
    with_store(instance, |store| {
        assert_eq!(store.bigram_count(SENTENCE_START, t1).unwrap(), 207);
    });
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert!(unsafe { zhuyin_train(instance) });
    with_store(instance, |store| {
        assert_eq!(store.bigram_count(SENTENCE_START, t1).unwrap(), 621);
        assert_eq!(store.system_unigram(t1), Some(shipped + 483 + 966 + 2898));
    });

    // A new composition starts fresh at sentence_start: the explicit
    // reset (the frontend's reset-on-commit contract), then a different
    // row trains from 69 again with no cross-sentence bigram.
    assert!(unsafe { zhuyin_reset(instance) });
    let second = candidate(instance, "su3cl3", 1);
    let t2 = token_of(instance, second);
    assert_ne!(t1, t2, "distinct rows carry distinct tokens");
    assert!(unsafe { zhuyin_choose_candidate(instance, 0, second) } > 0);
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert!(unsafe { zhuyin_train(instance) });
    with_store(instance, |store| {
        assert_eq!(store.bigram_count(SENTENCE_START, t2).unwrap(), 69);
        assert_eq!(store.bigram_count(t1, t2).unwrap(), 0);
    });

    close(context, instance);
}

/// Opens `user_dir` over the fixture, answering the raw handle.
fn init_at(user_dir: &std::path::Path) -> *mut crate::types::ZhuyinContext {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir.to_str().expect("UTF-8 path"));
    unsafe { zhuyin_init(system.as_ptr(), user.as_ptr()) }
}

/// Register row 91's control: `merge` returns at a mismatched MODIFY before
/// `next_record` reads the bad header, and init completes.
#[test]
fn init_completes_when_the_merge_stops_before_a_bad_header() {
    let user_dir = TempUserDir::new("init-stop-before-header");
    let payload = [
        crate::test_support::logs::mismatched_modify(),
        crate::test_support::logs::header_token(),
    ]
    .concat();
    crate::test_support::plant_log(&user_dir.path, "gb_char.dbin", &payload);
    let context = init_at(&user_dir.path);
    assert!(!context.is_null());
    unsafe { zhuyin_fini(context) };
}

/// Row 85's control: a key the mask selects is removed without loading its
/// value (`ngram_bdb.cpp:231-238`), and the mask completes.
#[test]
fn mask_out_of_a_selected_short_key_completes() {
    let user_dir = TempUserDir::new("mask-short-gram-selected");
    crate::test_support::plant_bigram(
        &user_dir.path,
        vec![(1_u32.to_le_bytes().to_vec(), vec![7, 0, 0])],
    );
    let context = init_at(&user_dir.path);
    assert!(!context.is_null());
    assert!(unsafe { crate::config::zhuyin_mask_out(context, 0xffff_ffff, 1) });
    unsafe { zhuyin_fini(context) };
}

// ---------------------------------------------------------------------
// Ports of contract-diff cases whose pin side reads freed or
// heap-dependent memory (undefined behaviour), so no frozen pin
// reference can hold them. Each asserts oxpinyin's defined behaviour;
// the expectations come from the pin's source at 074a2219 and the
// project's rulings, not from running the pin.
// ---------------------------------------------------------------------

/// The `open <user dir>/user.conf failed.` line a fresh user directory
/// makes `zhuyin_init` write: `UserTableInfo::load` prints it when the
/// marker is missing, with no trailing newline (`table_info.cpp:332` at
/// 074a2219).
fn fresh_user_conf_line(user_dir: &std::path::Path) -> String {
    // `g_build_filename` joins with the host's separator.
    format!(
        "open {}{}user.conf failed.",
        user_dir.display(),
        std::path::MAIN_SEPARATOR
    )
}

/// GLib log records the child's default handler collected:
/// `(domain, level, message)`.
static GLIB_LOG: std::sync::Mutex<Vec<(String, u32, String)>> = std::sync::Mutex::new(Vec::new());

/// Collects every GLib log record instead of printing it, so the child's
/// stderr holds only the library's raw `fprintf` lines.
extern "C" fn collect_glib_log(
    domain: *const c_char,
    level: glib_sys::GLogLevelFlags,
    message: *const c_char,
    _data: glib_sys::gpointer,
) {
    let text = |ptr: *const c_char| {
        if ptr.is_null() {
            String::new()
        } else {
            // SAFETY: GLib hands NUL-terminated strings that outlive the call.
            unsafe { std::ffi::CStr::from_ptr(ptr) }
                .to_string_lossy()
                .into_owned()
        }
    };
    GLIB_LOG
        .lock()
        .expect("log lock")
        .push((text(domain), level, text(message)));
}

/// `#919`, `table-conf-919-short-row-merged-zhuyin`: the five-word row
/// `default GB_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin` plus
/// the next line's first word `SYSTEM_FILE` is one valid six-word row
/// (`fscanf("%255s ... %255s\n")` skips white space across newlines and
/// takes whole words, `table_info.cpp:254-258`), so GB_DICTIONARY is
/// re-pointed at the GBK files; the trailing `default RESERVED NULL NULL
/// NULL` is five words, `6 != num` drops it (`:260-261`). The library and
/// table then disagree and the pin reads a NULL item (SIGSEGV, class (b)),
/// so only oxpinyin's answer is defined. The session is the case's: look
/// up, learn a sentence, import a phrase, save, finalise.
///
/// libzhuyin writes and renames each library's log in turn
/// (`zhuyin.cpp:560-640`), so the second write over the shared
/// `gbk_char.dbin` finds its own tmp file and no `rename ... failed.`
/// line follows (the pinyin twin's save writes all logs first,
/// `pinyin.cpp:925-1090`); the only raw stderr line is the init's
/// missing-marker line (`table_info.cpp:332`).
#[test]
fn table_conf_919_short_row_merged() {
    let run =
        crate::test_support::run_in_child("e2e_tests::table_conf_919_short_row_merged", |user| {
            let system = crate::test_support::private_system(user.parent().expect("scratch"));
            let conf = system.join("table.conf");
            let stock = std::fs::read_to_string(&conf).expect("stock table.conf");
            std::fs::remove_file(&conf).expect("unlink the link");
            std::fs::write(
                &conf,
                stock
                    + "default GB_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin\n"
                    + "SYSTEM_FILE default RESERVED NULL NULL NULL\n",
            )
            .expect("crafted table.conf");
            // SAFETY: the handler is a plain function and `user_data` is unused.
            unsafe {
                glib_sys::g_log_set_default_handler(Some(collect_glib_log), ptr::null_mut());
            }
            let system_c = cstr(system.to_str().expect("UTF-8 path"));
            let user_c = cstr(user.to_str().expect("UTF-8 path"));
            let context = unsafe { zhuyin_init(system_c.as_ptr(), user_c.as_ptr()) };
            assert!(!context.is_null());
            for text in ["ni3hao3", "yi4shu4"] {
                let inst = unsafe { zhuyin_alloc_instance(context) };
                unsafe { crate::parse::zhuyin_parse_more_full_pinyins(inst, cstr(text).as_ptr()) };
                unsafe { zhuyin_guess_sentence(inst) };
                let mut sentence: *mut c_char = ptr::null_mut();
                unsafe { zhuyin_get_sentence(inst, &raw mut sentence) };
                if !sentence.is_null() {
                    take_sentence(sentence);
                }
                assert!(
                    unsafe { crate::sentence::zhuyin_guess_candidates_after_cursor(inst, 0) },
                    "{text}"
                );
                // The original GB table still points to 你 at slot 4645.
                // GB_DICTIONARY now reads that slot from gbk_char.bin.
                // SAFETY: inst is live and the guess just populated its snapshot.
                let snapshot = unsafe { crate::state::instance_ref(inst) };
                assert!(
                    snapshot.state.candidates.iter().all(|candidate| {
                        candidate.token.map(|token| token.value()) != Some(0x0100_1225)
                    }),
                    "{text}"
                );
            }
            let inst = unsafe { zhuyin_alloc_instance(context) };
            unsafe { crate::parse::zhuyin_parse_more_full_pinyins(inst, cstr("ni3hao3").as_ptr()) };
            unsafe { zhuyin_guess_sentence(inst) };
            let train = unsafe { zhuyin_train(inst) };
            let it = unsafe { zhuyin_begin_add_phrases(context, 7) };
            let add = unsafe {
                zhuyin_iterator_add_phrase(
                    it,
                    cstr("测试词").as_ptr(),
                    cstr("ㄘㄜˋ ㄕˋ ㄘˊ").as_ptr(),
                    1,
                )
            };
            unsafe { zhuyin_end_add_phrases(it) };
            let save = unsafe { zhuyin_save(context) };
            unsafe { zhuyin_fini(context) };
            assert!(train && add && save, "train {train} add {add} save {save}");
            let mut sizes = std::collections::BTreeMap::new();
            let mut marker = String::new();
            for entry in std::fs::read_dir(user).expect("user dir") {
                let entry = entry.expect("entry");
                let name = entry.file_name().to_string_lossy().into_owned();
                let bytes = std::fs::read(entry.path()).expect("read");
                if name == "user.conf" {
                    marker = String::from_utf8_lossy(&bytes).into_owned();
                } else {
                    sizes.insert(name, bytes.len());
                }
            }
            let present: Vec<&str> = sizes.keys().map(String::as_str).collect();
            assert_eq!(
                present,
                [
                    "addon.bin",
                    "gbk_char.dbin",
                    "merged.dbin",
                    "network.bin",
                    "opengram.dbin",
                    "user.bin",
                    "user_bigram.db",
                    "user_phrase_index.bin",
                    "user_pinyin_index.bin",
                ]
            );
            for (name, size) in [
                ("addon.bin", 27),
                ("gbk_char.dbin", 26),
                ("merged.dbin", 26),
                ("network.bin", 27),
                ("opengram.dbin", 26),
                ("user.bin", 71),
            ] {
                assert_eq!(sizes[name], size, "{name}");
            }
            // user.conf is written in text mode: CRLF on Windows.
            let newline = oxpinyin_input::TEXT_NEWLINE;
            assert!(
                marker.starts_with(&format!(
                    "binary format version:7{newline}model data version:14{newline}"
                )) && marker.ends_with(&format!("{newline}open counter:0{newline}")),
                "{marker}"
            );
            // Row 106 applies to a live item lookup failure here too.
            assert!(GLIB_LOG.lock().expect("log lock").is_empty());
        });
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        fresh_user_conf_line(&run.user_dir)
    );
}
