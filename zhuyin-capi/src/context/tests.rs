#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

#[cfg(unix)]
use crate::test_support::directory_reads_as_a_chunk_here;

#[cfg(unix)]
use crate::context::zhuyin_init;

#[cfg(unix)]
use crate::context::zhuyin_save;

#[cfg(unix)]
use crate::test_support::cstr;

/// One call over a private system directory whose library is a directory: init maps `merged.bin` (`zhuyin.cpp:199`), the save and
/// the mask-out map it again (`:589`, `:800`), and the reload of the
/// unloaded library 2 maps `gbk_char.bin` (`:200`).
#[cfg(unix)]
fn map_a_directory_library(site: &str) -> bool {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::test_support::private_system(scratch.path());
    let user = scratch.path().join("user");
    std::fs::create_dir_all(&user).expect("user dir");
    let system_c = cstr(system.to_str().expect("UTF-8 path"));
    let user_c = cstr(user.to_str().expect("UTF-8 path"));
    if site == "init" {
        crate::test_support::directory_library(&system, "merged.bin");
        return !unsafe { zhuyin_init(system_c.as_ptr(), user_c.as_ptr()) }.is_null();
    }
    let context = unsafe { zhuyin_init(system_c.as_ptr(), user_c.as_ptr()) };
    assert!(!context.is_null());
    match site {
        "save" => {
            // SAFETY: `context` is a live `zhuyin_init` handle, borrowed
            // uniquely for this statement.
            let ctx = unsafe { crate::state::context_mut(context) };
            ctx.core.user_store_mut().mark_modified();
            crate::test_support::directory_library(&system, "merged.bin");
            unsafe { zhuyin_save(context) }
        }
        "mask" => {
            crate::test_support::directory_library(&system, "merged.bin");
            unsafe { crate::config::zhuyin_mask_out(context, 0xff00_0000, 0x0400_0000) }
        }
        _ => {
            assert!(unsafe { crate::config::zhuyin_unload_phrase_library(context, 2) });
            crate::test_support::directory_library(&system, "gbk_char.bin");
            unsafe { crate::config::zhuyin_load_phrase_library(context, 2) }
        }
    }
}

/// The save aborts when the directory reads as a short chunk and completes elsewhere.
// Unix only: a directory opens as a file, so the pin's `MemoryChunk::mmap` judges it by its
// size (`directory_reads_as_a_chunk`). Windows refuses the open, and oxpinyin skips the row
// without a word there (`oxpinyin`, `phrase_libraries.rs`), which no pin observation
// covers.
#[cfg(unix)]
#[test]
fn save_aborts_on_a_directory_library() {
    if directory_reads_as_a_chunk_here() {
        crate::test_support::assert_aborts(
            "context::tests::save_aborts_on_a_directory_library",
            || {
                let _ = map_a_directory_library("save");
            },
        );
    } else {
        assert!(map_a_directory_library("save"));
    }
}

#[test]
fn the_short_table_row_sequence_writes_no_glib_log() {
    use crate::test_support::{LogCapture, TempUserDir, cstr, short_row_system};
    let scratch = tempfile::tempdir().unwrap();
    let system = short_row_system(scratch.path());
    let user = TempUserDir::new("short-row-log");
    let logs = LogCapture::new();
    let context = crate::test_support::init_on(&system, Some(&user.path));
    assert!(!context.is_null());
    for text in ["ni3hao3", "yi4shu4"] {
        let instance = unsafe { crate::instance::zhuyin_alloc_instance(context) };
        let _ =
            unsafe { crate::parse::zhuyin_parse_more_full_pinyins(instance, cstr(text).as_ptr()) };
        unsafe { crate::sentence::zhuyin_guess_sentence(instance) };
        let mut sentence = std::ptr::null_mut();
        unsafe { crate::sentence::zhuyin_get_sentence(instance, &raw mut sentence) };
        if !sentence.is_null() {
            // SAFETY: `sentence` is the string the library just returned; this code owns it and
            // frees it only here.
            unsafe { crate::test_support::take_sentence(sentence) };
        }
        assert!(unsafe { crate::sentence::zhuyin_guess_candidates_after_cursor(instance, 0) });
        unsafe { crate::instance::zhuyin_free_instance(instance) };
    }
    let instance = unsafe { crate::instance::zhuyin_alloc_instance(context) };
    let _ = unsafe { crate::parse::zhuyin_parse_more_full_pinyins(instance, c"ni3hao3".as_ptr()) };
    unsafe { crate::sentence::zhuyin_guess_sentence(instance) };
    assert!(unsafe { crate::candidates::zhuyin_train(instance) });
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    assert!(unsafe {
        crate::test_support::import(context, 7, "测试词", "ㄘㄜˋ ㄕˋ ㄘˊ".as_bytes(), 1)
    });
    assert!(unsafe { crate::context::zhuyin_save(context) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { crate::test_support::close(context, instance) };
    assert!(logs.take().is_empty());
}
