//! Phrase-library load and unload, and the facade total of a failed
//! layout.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

#[cfg(unix)]
use crate::support::directory_reads_as_a_chunk_here;
#[cfg(unix)]
use crate::support::remap_a_directory_library;

use crate::support::init_fixture_at;

use crate::support::{
    DEFAULT_SORT, TempSystemDir, TempUserDir, assert_aborts, cstr, import_batch, lookup,
    lookup_candidate_type_t, pinyin_alloc_instance, pinyin_guess_candidates, pinyin_init,
    pinyin_load_addon_phrase_library, pinyin_load_phrase_library, pinyin_parse_more_full_pinyins,
    pinyin_token_get_n_pronunciation, pinyin_unload_addon_phrase_library, *,
};

/// `load_phrase_library(0)` asserts on the reserved library.
#[test]
fn load_of_the_reserved_library_aborts() {
    assert_aborts(
        "library_loading::load_of_the_reserved_library_aborts",
        || {
            let user = TempUserDir::new("load-library-0");
            let (context, _) = open_at(&user.path);
            unsafe { pinyin_load_phrase_library(context, 0) };
        },
    );
}

/// Pinyin unloads only the GBK library on a live context: libraries 1, 15
/// and 7 answer false, the user library keeps its items, and loading 7
/// answers false.
#[test]
fn pinyin_unloads_only_the_gbk_library() {
    let user = TempUserDir::new("unload-gate");
    let (context, instance) = open_at(&user.path);
    assert_eq!(import_batch(context, 7, &[("龘龘", "da2'da2", 5)]), [true]);
    let token = lookup(instance, "龘龘")[0];
    for library in [1, 15, 7] {
        assert!(
            !unsafe { crate::support::pinyin_unload_phrase_library(context, library) },
            "unload {library}"
        );
    }
    let mut count = UNTOUCHED;
    assert!(unsafe { pinyin_token_get_n_pronunciation(instance, token, &raw mut count) });
    assert_eq!(count, 1);
    assert!(!unsafe { pinyin_load_phrase_library(context, 7) });
    close(context, instance);
}

/// Register row 91 on the reload path: `pinyin_load_phrase_library` of the
/// unloaded GBK library merges its saved `.dbin` (`pinyin.cpp:276`).
#[test]
fn reload_aborts_on_a_log_header_token() {
    crate::support::assert_aborts(
        "library_loading::reload_aborts_on_a_log_header_token",
        || {
            let user_dir = TempUserDir::new("reload-header-token");
            let context = init_fixture_at(&user_dir.path);
            assert!(!context.is_null());
            assert!(unsafe { crate::support::pinyin_unload_phrase_library(context, 2) });
            crate::support::plant_log(
                &user_dir.path,
                "gbk_char.dbin",
                &crate::support::logs::header_token(),
            );
            let _ = unsafe { crate::support::pinyin_load_phrase_library(context, 2) };
        },
    );
}

/// Register row 96 at the reload of an unloaded library.
// Unix only, for the reason `oxpinyin`'s `save_stops_at_the_directory_with_the_pins_files` gives.
#[cfg(unix)]
#[test]
fn reload_aborts_on_a_directory_library() {
    if directory_reads_as_a_chunk_here() {
        crate::support::assert_aborts(
            "library_loading::reload_aborts_on_a_directory_library",
            || {
                let _ = remap_a_directory_library("reload");
            },
        );
    } else {
        assert!(remap_a_directory_library("reload"));
    }
}

/// Register row 96 at the addon load, which maps `art.bin` through the same
/// `MemoryChunk::mmap` (`pinyin.cpp:290`); row 76's `false` elsewhere.
// Unix only, for the reason `oxpinyin`'s `save_stops_at_the_directory_with_the_pins_files` gives.
#[cfg(unix)]
#[test]
fn addon_load_aborts_on_a_directory_library() {
    if directory_reads_as_a_chunk_here() {
        crate::support::assert_aborts(
            "library_loading::addon_load_aborts_on_a_directory_library",
            || {
                let _ = remap_a_directory_library("addon");
            },
        );
    } else {
        assert!(!remap_a_directory_library("addon"));
    }
}

#[test]
fn addon_library_load_is_idempotent_and_surfaces_addon_candidates() {
    let system = TempSystemDir::new("addon-load");
    system.write(
        "interpolation2.text",
        "\\data model interpolation\n\\1-gram\n\\item 1 ok count 1\n",
    );
    let user = TempUserDir::new("addon-load-user");
    let context = unsafe {
        pinyin_init(
            cstr(system.path.to_str().expect("UTF-8 path")).as_ptr(),
            cstr(user.path.to_str().expect("UTF-8 path")).as_ptr(),
        )
    };
    assert!(!context.is_null());
    assert!(unsafe { pinyin_load_addon_phrase_library(context, 4) });
    assert!(
        !unsafe { pinyin_load_addon_phrase_library(context, 4) },
        "second load of the same index is false"
    );
    assert!(
        !unsafe { pinyin_load_addon_phrase_library(context, 15) },
        "missing library is false"
    );
    assert!(
        !unsafe { pinyin_load_addon_phrase_library(context, 16) },
        "out-of-range load answers false, as the pin's own guard does (pinyin.cpp:479-480)"
    );
    // pinyin_unload_addon_phrase_library mirrors the pin (`pinyin.cpp:497-503`):
    // unconditional `true` in range, including for a library that was never
    // loaded, and a reload afterwards must succeed again. The pin's
    // `assert(index < PHRASE_INDEX_LIBRARY_COUNT)` (`:499`) aborts the
    // process;
    // `null_arguments::null_context_unload_addon_phrase_library_aborts_on_index_16`
    // holds it.
    assert!(unsafe { pinyin_unload_addon_phrase_library(context, 4) });
    assert!(
        unsafe { pinyin_unload_addon_phrase_library(context, 4) },
        "unloading an already-unloaded library is still true, as upstream"
    );
    assert!(
        unsafe { pinyin_load_addon_phrase_library(context, 4) },
        "reload after unload succeeds"
    );

    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());
    let input = cstr("erhuang");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        7
    );
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    assert!(
        capture_rows(instance).iter().any(|(kind, text, _)| {
            *kind == lookup_candidate_type_t::ADDON_CANDIDATE && (text == "二簧" || text == "二黄")
        }),
        "loaded art addon must offer 二簧/二黄 for erhuang"
    );

    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}
