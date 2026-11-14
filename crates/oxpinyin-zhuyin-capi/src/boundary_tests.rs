//! C-boundary decisions of the zhuyin facade on the w3 mini fixture: the
//! return values, out-parameters, aborts and stderr lines of the pin at
//! 074a2219 that no lower-layer test holds. An abort test asserts the abort
//! status only
//! (`docs/findings/upstream-divergences.md#reproduced-library-aborts-write-no-assertion-line`:
//! oxpinyin writes no assertion line).

use std::os::raw::c_char;
use std::path::Path;
use std::ptr;

use crate::candidates::{
    zhuyin_get_candidate, zhuyin_get_candidate_string, zhuyin_get_candidate_type,
    zhuyin_get_n_candidate, zhuyin_train,
};
use crate::config::{
    zhuyin_load_phrase_library, zhuyin_mask_out, zhuyin_set_chewing_scheme, zhuyin_set_options,
    zhuyin_unload_phrase_library,
};
use crate::context::{zhuyin_fini, zhuyin_init, zhuyin_save};
use crate::cursor::{
    zhuyin_get_left_zhuyin_offset, zhuyin_get_right_zhuyin_offset, zhuyin_get_zhuyin_key,
    zhuyin_get_zhuyin_key_rest, zhuyin_get_zhuyin_key_rest_length,
    zhuyin_get_zhuyin_key_rest_positions,
};
use crate::dict::{
    zhuyin_token_add_unigram_frequency, zhuyin_token_get_n_pronunciation,
    zhuyin_token_get_nth_pronunciation, zhuyin_token_get_phrase,
};
use crate::instance::{zhuyin_alloc_instance, zhuyin_free_instance};
use crate::keys::{zhuyin_get_pinyin_string, zhuyin_get_zhuyin_string, zhuyin_parse_chewing};
use crate::parse::{
    zhuyin_get_parsed_input_length, zhuyin_in_chewing_keyboard, zhuyin_parse_more_chewings,
    zhuyin_parse_more_full_pinyins,
};
use crate::phrase::{zhuyin_get_n_phrase, zhuyin_get_phrase_token, zhuyin_phrase_segment};
use crate::sentence::{
    zhuyin_get_character_offset, zhuyin_get_sentence, zhuyin_guess_candidates_after_cursor,
    zhuyin_guess_candidates_before_cursor, zhuyin_guess_sentence,
};
use crate::test_support::{
    LogCapture, TempUserDir, abort_in_child, assert_aborts, close, craft_user_index, cstr, import,
    init_on, open, open_with_user, plant_bigram, private_system, run_in_child, system_dir,
    take_sentence, tokens_of, ucs4_key, write_user_conf,
};
use crate::types::{ChewingKey, ChewingKeyRest, LookupCandidate, ZhuyinContext, ZhuyinInstance};

/// The out-parameter seed that shows an untouched output.
const UNTOUCHED: u32 = 0x00AB_CDEF;
/// A seeded pointer out-parameter.
const SEEDED: usize = 0x00AB_CDEF;
/// `PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE | IS_PINYIN`.
const SEPARATOR_OPTIONS: u32 = 0x18a;
/// The GLib level of a critical.
const CRITICAL: u32 = 8;
/// The GLib level of a warning.
const WARNING: u32 = 16;

/// A fresh instance on `context` with `keys` parsed as chewing keystrokes.
fn chewing(context: *mut ZhuyinContext, keys: &str) -> *mut ZhuyinInstance {
    let instance = zhuyin_alloc_instance(context);
    assert!(!instance.is_null());
    zhuyin_parse_more_chewings(instance, cstr(keys).as_ptr());
    instance
}

/// A fresh instance with `text` parsed as full pinyin under `options`.
fn full_pinyin(context: *mut ZhuyinContext, options: u32, text: &str) -> *mut ZhuyinInstance {
    assert!(zhuyin_set_options(context, options));
    let instance = zhuyin_alloc_instance(context);
    assert!(!instance.is_null());
    zhuyin_parse_more_full_pinyins(instance, cstr(text).as_ptr());
    instance
}

/// The sentence getter's answer and text; `None` for an untouched output.
fn sentence(instance: *mut ZhuyinInstance) -> (bool, Option<String>) {
    let mut text = SEEDED as *mut c_char;
    let ret = zhuyin_get_sentence(instance, &raw mut text);
    let text = (text as usize != SEEDED).then(|| take_sentence(text));
    (ret, text)
}

/// The candidate count of the current list.
fn n_candidate(instance: *mut ZhuyinInstance) -> (bool, u32) {
    let mut count = UNTOUCHED;
    let ret = zhuyin_get_n_candidate(instance, &raw mut count);
    (ret, count)
}

fn candidate_at(instance: *mut ZhuyinInstance, index: u32) -> *mut LookupCandidate {
    let mut candidate: *mut LookupCandidate = ptr::null_mut();
    assert!(zhuyin_get_candidate(instance, index, &raw mut candidate));
    candidate
}

/// The keys of a GArray of two-byte elements as lowercase hex.
fn keys_hex(array: *mut glib_sys::GArray) -> String {
    // SAFETY: a live GArray of two-byte elements with `len` of them.
    unsafe {
        let view = &*array;
        (0..view.len as usize)
            .flat_map(|index| {
                view.data
                    .cast::<u16>()
                    .add(index)
                    .read_unaligned()
                    .to_le_bytes()
            })
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

/// The `nth` pronunciation of `token`: the answer, the key count and the keys.
fn nth_pronunciation(instance: *mut ZhuyinInstance, token: u32, nth: u32) -> (bool, u32, String) {
    // SAFETY: a fresh GArray of two-byte elements, freed below.
    let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
    let ret = zhuyin_token_get_nth_pronunciation(instance, token, nth, keys);
    // SAFETY: the array is live.
    let len = unsafe { (*keys).len };
    let hex = keys_hex(keys);
    // SAFETY: the array was allocated above and is freed once.
    unsafe { glib_sys::g_array_free(keys, 1) };
    (ret, len, hex)
}

/// The phrase of `token`: the answer, the length and the text, the seeds
/// kept where untouched.
fn token_phrase(instance: *mut ZhuyinInstance, token: u32) -> (bool, u32, Option<String>) {
    let mut length = UNTOUCHED;
    let mut text = SEEDED as *mut c_char;
    let ret = zhuyin_token_get_phrase(instance, token, &raw mut length, &raw mut text);
    let text = (text as usize != SEEDED).then(|| take_sentence(text));
    (ret, length, text)
}

fn n_pronunciation(instance: *mut ZhuyinInstance, token: u32) -> (bool, u32) {
    let mut count = UNTOUCHED;
    let ret = zhuyin_token_get_n_pronunciation(instance, token, &raw mut count);
    (ret, count)
}

/// The library-7 tokens of `text`.
fn user_tokens(instance: *mut ZhuyinInstance, text: &str) -> Vec<u32> {
    tokens_of(instance, text)
        .into_iter()
        .filter(|token| token >> 24 == 7)
        .collect()
}

/// Opens and closes `user_dir` once, so that the profile exists.
fn first_init(user_dir: &Path) {
    let context = init_on(&system_dir(), Some(user_dir));
    assert!(!context.is_null());
    zhuyin_fini(context);
}

/// The two string getters refuse the zero key that a separator column holds
/// and render a real key (`zhuyin.cpp:1732-1760`).
#[test]
fn string_getters_refuse_the_zero_key_and_render_a_real_key() {
    let _slots = crate::test_support::key_slot_guard();
    let (context, instance) = open();
    assert!(zhuyin_set_options(context, 0x1aa));
    zhuyin_parse_more_full_pinyins(instance, cstr("xi1'").as_ptr());
    assert!(zhuyin_guess_sentence(instance));
    let mut key: *mut ChewingKey = ptr::null_mut();
    assert!(zhuyin_get_zhuyin_key(instance, 3, &raw mut key));
    // SAFETY: the getter answered a live key slot.
    assert_eq!(unsafe { *key }.packed, 0);
    for getter in [zhuyin_get_zhuyin_string, zhuyin_get_pinyin_string] {
        let mut text = SEEDED as *mut c_char;
        assert!(!getter(instance, key, &raw mut text));
        assert!(text.is_null());
    }
    assert!(zhuyin_get_zhuyin_key(instance, 0, &raw mut key));
    let mut text: *mut c_char = ptr::null_mut();
    assert!(zhuyin_get_pinyin_string(instance, key, &raw mut text));
    assert_eq!(take_sentence(text), "xi1");

    let mut key = ChewingKey::ZERO;
    assert!(zhuyin_parse_chewing(
        instance,
        cstr("su3").as_ptr(),
        &raw mut key
    ));
    let mut text: *mut c_char = ptr::null_mut();
    assert!(zhuyin_get_zhuyin_string(
        instance,
        &raw mut key,
        &raw mut text
    ));
    assert_eq!(take_sentence(text), "ㄋㄧˇ");
    let mut text: *mut c_char = ptr::null_mut();
    assert!(zhuyin_get_pinyin_string(
        instance,
        &raw mut key,
        &raw mut text
    ));
    assert_eq!(take_sentence(text), "ni3");
    close(context, instance);
}

/// An empty `merged.bin` refuses the init: the mmap line goes to stderr and
/// libzhuyin logs one warning (the pin crashes here, SIGSEGV).
#[test]
fn an_empty_library_refuses_the_init_with_the_mmap_line() {
    let run = run_in_child(
        "boundary_tests::an_empty_library_refuses_the_init_with_the_mmap_line",
        |user| {
            let system = private_system(user.parent().expect("scratch"));
            std::fs::remove_file(system.join("merged.bin")).expect("unlink the library");
            std::fs::write(system.join("merged.bin"), b"").expect("an empty library");
            let logs = LogCapture::new();
            assert!(init_on(&system, Some(user)).is_null());
            assert_eq!(logs.take(), [(Some("libzhuyin".to_owned()), WARNING)]);
        },
    );
    let system = run.user_dir.parent().expect("scratch").join("system");
    let line = format!(
        "mmap {} failed!{}",
        system.join("merged.bin").display(),
        if cfg!(windows) { "\r\n" } else { "\n" }
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.ends_with(&line), "{stderr}");
}

/// A user phrase added again after its library reloads unsaved aborts.
#[test]
fn adding_a_user_phrase_again_after_a_reload_aborts() {
    assert_aborts(
        "boundary_tests::adding_a_user_phrase_again_after_a_reload_aborts",
        || {
            let user_dir = TempUserDir::new("reload-reuse");
            let (context, _instance) = open_with_user(&user_dir.path);
            let reading = "ㄏㄠˇ ㄋㄧˇ".as_bytes();
            assert!(import(context, 7, "好你", reading, 50));
            assert!(zhuyin_unload_phrase_library(context, 7));
            assert!(zhuyin_load_phrase_library(context, 7));
            import(context, 7, "好你", reading, 50);
        },
    );
}

/// The sentence getter resolves each kept token through the phrase index
/// when the caller reads it (`convert_to_utf8`, `lookup/lookup.cpp:37-48`
/// at 074a2219), and the candidate getter returns its saved string
/// (`zhuyin.cpp:1865-1870`). After a reload drops an unsaved phrase, its
/// token does not resolve: `get_phrase_item` fails and leaves the default
/// item of length 0 (`lookup.cpp:35,43`, `storage/phrase_index.h:81-84`),
/// so the token adds "". The next import reuses the token
/// (`zhuyin.cpp:474-478`, register row 104), and the kept sentence then
/// reads the new phrase with no new guess.
#[test]
fn a_kept_sentence_reads_its_tokens_through_the_live_phrase_index() {
    let user_dir = TempUserDir::new("sentence-token-reuse");
    let (context, spare) = open_with_user(&user_dir.path);
    // The count makes the 1-best row A's token alone, not the two system
    // phrases 好 and 你, which also read "好你". The read after the reload
    // tells the two rows apart.
    assert!(import(context, 7, "好你", "ㄏㄠˇ ㄋㄧˇ".as_bytes(), 10_000));
    let instance = chewing(context, "cl3su3");
    let user_tokens = |text| {
        tokens_of(instance, text)
            .into_iter()
            .filter(|token| token >> 24 == 7)
            .collect::<Vec<_>>()
    };
    let [token] = user_tokens("好你")[..] else {
        panic!("one user token for 好你");
    };
    assert!(zhuyin_guess_sentence(instance));
    assert!(zhuyin_guess_candidates_after_cursor(instance, 0));
    let candidate = candidate_at(instance, 0);
    let candidate_text = || {
        let mut text: *const c_char = ptr::null();
        assert!(zhuyin_get_candidate_string(
            instance,
            candidate,
            &raw mut text
        ));
        // SAFETY: the getter answered the candidate's NUL-terminated text,
        // which the instance owns until its candidates are rebuilt.
        unsafe { std::ffi::CStr::from_ptr(text) }
            .to_str()
            .expect("UTF-8")
            .to_owned()
    };
    assert_eq!(
        token_phrase(instance, token),
        (true, 2, Some("好你".into()))
    );
    assert_eq!(sentence(instance), (true, Some("好你".into())));
    assert_eq!(candidate_text(), "好你");

    assert!(zhuyin_unload_phrase_library(context, 7));
    assert!(zhuyin_load_phrase_library(context, 7));
    assert_eq!(token_phrase(instance, token), (false, UNTOUCHED, None));
    assert_eq!(sentence(instance), (true, Some(String::new())));

    assert!(import(context, 7, "你好", "ㄋㄧˇ ㄏㄠˇ".as_bytes(), 50));
    assert_eq!(user_tokens("你好"), [token]);
    assert_eq!(
        token_phrase(instance, token),
        (true, 2, Some("你好".into()))
    );
    assert_eq!(sentence(instance), (true, Some("你好".into())));
    assert_eq!(candidate_text(), "好你");
    zhuyin_free_instance(instance);
    close(context, spare);
}

/// A save that the file-size limit cuts after the chunk's length word aborts
/// on the checksum word (`memory_chunk.h:547`). Unix only: `RLIMIT_FSIZE`.
#[cfg(unix)]
#[test]
fn a_save_cut_after_the_chunk_length_word_aborts() {
    let run = abort_in_child(
        "boundary_tests::a_save_cut_after_the_chunk_length_word_aborts",
        |user| {
            let (context, instance) = open_with_user(user);
            zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr());
            assert!(zhuyin_guess_sentence(instance));
            assert!(zhuyin_train(instance));
            let limit = libc::rlimit {
                rlim_cur: 4,
                rlim_max: 4,
            };
            // SAFETY: ignores SIGXFSZ and lowers this child's own file-size limit.
            unsafe {
                libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
                assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &raw const limit), 0);
            }
            zhuyin_save(context);
        },
    );
    let tmp = run.user_dir.join("gb_char.dbin.tmp");
    assert_eq!(std::fs::metadata(&tmp).expect("the cut tmp file").len(), 4);
}

/// Each init refusal writes the pin's two stderr lines and answers NULL; an
/// init with a NULL user directory opens with the one `open  failed.` line
/// and no marker line.
#[test]
fn init_refusals_write_the_open_and_load_lines() {
    let run = run_in_child(
        "boundary_tests::init_refusals_write_the_open_and_load_lines",
        |user| {
            let context = init_on(&system_dir(), None);
            assert!(!context.is_null());
            zhuyin_fini(context);
            let user_c = cstr(user.to_str().expect("UTF-8 path"));
            assert!(zhuyin_init(ptr::null(), user_c.as_ptr()).is_null());
            assert!(init_on(Path::new("/nonexistent-system-dir"), Some(user)).is_null());
            let system = private_system(user.parent().expect("scratch"));
            std::fs::remove_file(system.join("table.conf")).expect("unlink table.conf");
            assert!(init_on(&system, Some(user)).is_null());
        },
    );
    let nl = if cfg!(windows) { "\r\n" } else { "\n" };
    let conf = run
        .user_dir
        .parent()
        .expect("scratch")
        .join("system")
        .join("table.conf");
    let conf = conf.display();
    let expected = format!(
        "open  failed.open  failed.{nl}load  failed!{nl}\
         open /nonexistent-system-dir/table.conf failed.{nl}\
         load /nonexistent-system-dir/table.conf failed!{nl}\
         open {conf} failed.{nl}load {conf} failed!{nl}"
    );
    assert_eq!(String::from_utf8_lossy(&run.stderr), expected);
}

/// Library 0 is the reserved row, neither `SYSTEM_FILE` nor `USER_FILE`:
/// the load asserts (`zhuyin.cpp:372`).
#[test]
fn loading_library_zero_aborts() {
    assert_aborts("boundary_tests::loading_library_zero_aborts", || {
        let (context, _instance) = open();
        zhuyin_load_phrase_library(context, 0);
    });
}

/// A token of a library with no sub-index answers false and writes 0 to the
/// count (`dict.rs`, `phrase_index.cpp:180-205`).
#[test]
fn pronunciation_count_without_a_sub_index_writes_zero() {
    let (context, instance) = open();
    assert_eq!(n_pronunciation(instance, 0x0f00_0001), (false, 0));
    close(context, instance);
}

/// libzhuyin's import parses chewing (`zhuyin.cpp:515-523`): ㄉㄣ, a reading
/// no typed input spells, stores key 0x1484 and survives a reopen.
#[test]
fn an_imported_den_reading_stores_its_key() {
    let user_dir = TempUserDir::new("den");
    let (context, instance) = open_with_user(&user_dir.path);
    assert!(import(context, 7, "扽", "ㄉㄣ".as_bytes(), 5));
    let tokens = user_tokens(instance, "扽");
    assert_eq!(tokens, [0x0700_0001]);
    assert_eq!(
        nth_pronunciation(instance, tokens[0], 0),
        (true, 1, "8414".to_owned())
    );
    assert!(zhuyin_save(context));
    close(context, instance);
    let (context, instance) = open_with_user(&user_dir.path);
    assert_eq!(
        nth_pronunciation(instance, tokens[0], 0),
        (true, 1, "8414".to_owned())
    );
    close(context, instance);
}

/// At 074a2219 the USER_FILE load installs the whole offset and content
/// sections (phrase_index.cpp:335-362). A bad offset fails only the item
/// read (phrase_index.cpp:180-205); neighbouring items remain readable.
#[test]
fn a_bad_user_item_offset_leaves_neighbouring_items_readable() {
    use oxpinyin_data::chunk_format::build_memory_chunk;
    use oxpinyin_data::chunk_write::{ChunkItem, build_chunk};

    let user = TempUserDir::new("user-file-bad-item");
    write_user_conf(&user.path, &system_dir());
    let rows = [(1, "你", 0x302b_u16), (2, "好", 0x30a7), (3, "吗", 0x5083)];
    let items = rows.map(|(slot, phrase, key)| {
        (
            slot,
            ChunkItem {
                phrase: phrase.chars().map(u32::from).collect(),
                unigram: 17,
                prons: vec![(vec![key], 5)],
            },
        )
    });
    let chunk = build_chunk(&items).expect("three-item user chunk");
    let mut payload = chunk[8..].to_vec();
    let offsets_start = u32::from_le_bytes(payload[4..8].try_into().expect("offset header"));
    let slot_two = offsets_start as usize + 2 * 4;
    payload[slot_two..slot_two + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    std::fs::write(
        user.path.join("user.bin"),
        build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write user.bin");

    let (context, instance) = open_with_user(&user.path);
    for (slot, expected, key) in [rows[0], rows[2]] {
        let token = 0x0700_0000 | slot;
        assert_eq!(
            token_phrase(instance, token),
            (true, 1, Some(expected.to_owned()))
        );
        assert_eq!(n_pronunciation(instance, token), (true, 1));
        let [low, high] = key.to_le_bytes();
        assert_eq!(
            nth_pronunciation(instance, token, 0),
            (true, 1, format!("{low:02x}{high:02x}"))
        );
    }
    assert_eq!(
        token_phrase(instance, 0x0700_0002),
        (false, UNTOUCHED, None)
    );
    assert_eq!(n_pronunciation(instance, 0x0700_0002), (false, 0));
    assert_eq!(
        nth_pronunciation(instance, 0x0700_0002, 0),
        (false, 0, String::new())
    );
    close(context, instance);
}

/// A stored key word 0x8000 reads back verbatim (`phrase_index.cpp:33-50`).
#[test]
fn a_stored_key_word_reads_back_verbatim() {
    let user_dir = TempUserDir::new("key-8000");
    let (context, instance) = open_with_user(&user_dir.path);
    assert!(import(context, 7, "你", "ㄋㄧˇ".as_bytes(), 5));
    let token = user_tokens(instance, "你")[0];
    assert_eq!(
        nth_pronunciation(instance, token, 0),
        (true, 1, "2b30".to_owned())
    );
    assert!(zhuyin_save(context));
    close(context, instance);
    let path = user_dir.path.join("user.bin");
    let mut payload = std::fs::read(&path).expect("user.bin")[8..].to_vec();
    let text = ucs4_key("你");
    let at = (0..=payload.len() - text.len())
        .find(|&at| payload[at..at + text.len()] == text[..])
        .expect("the imported item")
        + text.len();
    assert_eq!(payload[at..at + 2], [0x2b, 0x30]);
    payload[at..at + 2].copy_from_slice(&0x8000_u16.to_le_bytes());
    std::fs::write(
        &path,
        oxpinyin_data::chunk_format::build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write user.bin");
    let (context, instance) = open_with_user(&user_dir.path);
    assert_eq!(
        token_phrase(instance, token),
        (true, 1, Some("你".to_owned()))
    );
    assert_eq!(
        nth_pronunciation(instance, token, 0),
        (true, 1, "0080".to_owned())
    );
    close(context, instance);
}

/// A user pinyin index whose crafted keys are the first key and its 17-fold
/// repeat: the user index rows of a profile reopened under a conforming
/// `user.conf`.
fn overlong_index_profile(user_dir: &Path) {
    first_init(user_dir);
    let first = vec![0_u8, 1];
    craft_user_index(
        &user_dir.join("user_pinyin_index.bin"),
        &[(first.clone(), Vec::new()), (first.repeat(17), Vec::new())],
    );
    write_user_conf(user_dir, &system_dir());
}

/// mask_out walks the user index and asserts on a key longer than 16 words
/// (register row 84).
#[test]
fn mask_out_over_an_overlong_user_index_key_aborts() {
    assert_aborts(
        "boundary_tests::mask_out_over_an_overlong_user_index_key_aborts",
        || {
            let user_dir = TempUserDir::new("overlong-mask");
            overlong_index_profile(&user_dir.path);
            let (context, _instance) = open_with_user(&user_dir.path);
            zhuyin_mask_out(context, 0xFFFF_FFFF, 0);
        },
    );
}

/// The cursor guesses never read the overlong user index key: both answer
/// true, with no abort.
#[test]
fn cursor_guesses_ignore_an_overlong_user_index_key() {
    let user_dir = TempUserDir::new("overlong-guess");
    overlong_index_profile(&user_dir.path);
    let (context, instance) = open_with_user(&user_dir.path);
    assert_eq!(
        zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()),
        6
    );
    assert!(zhuyin_guess_candidates_after_cursor(instance, 0));
    assert!(zhuyin_guess_candidates_before_cursor(instance, 6));
    close(context, instance);
}

/// Replaces the user phrase index with `你好 -> value`, reopens and imports
/// 你好 into library 7: the pin asserts at `zhuyin.cpp:440` or `:454`.
fn add_after_index(user_dir: &Path, value: &[u8]) {
    craft_user_index(
        &user_dir.join("user_phrase_index.bin"),
        &[(ucs4_key("你好"), value.to_vec())],
    );
    write_user_conf(user_dir, &system_dir());
    let (context, _instance) = open_with_user(user_dir);
    import(context, 7, "你好", "ㄋㄧˇ ㄏㄠˇ".as_bytes(), 1);
}

/// Two library-7 tokens for one phrase key (register row 93).
#[test]
fn adding_a_phrase_with_two_library_tokens_aborts() {
    assert_aborts(
        "boundary_tests::adding_a_phrase_with_two_library_tokens_aborts",
        || {
            let user_dir = TempUserDir::new("duplicate-token");
            first_init(&user_dir.path);
            let value = [0x0700_0001_u32.to_le_bytes(), 0x0700_0002_u32.to_le_bytes()].concat();
            add_after_index(&user_dir.path, &value);
        },
    );
}

/// The phrase key points at the token of another phrase of the same length
/// (register row 104).
#[test]
fn adding_a_phrase_whose_index_token_has_other_text_aborts() {
    assert_aborts(
        "boundary_tests::adding_a_phrase_whose_index_token_has_other_text_aborts",
        || {
            let user_dir = TempUserDir::new("text-mismatch");
            let (context, instance) = open_with_user(&user_dir.path);
            assert!(import(context, 7, "你們", "ㄋㄧˇ ㄇㄣˊ".as_bytes(), 1));
            assert!(zhuyin_save(context));
            let token = user_tokens(instance, "你們")[0];
            close(context, instance);
            add_after_index(&user_dir.path, &token.to_le_bytes());
        },
    );
}

/// A user bigram key that is not a four-byte token aborts the mask walk.
#[test]
fn mask_out_over_a_non_token_bigram_key_aborts() {
    assert_aborts(
        "boundary_tests::mask_out_over_a_non_token_bigram_key_aborts",
        || {
            let user_dir = TempUserDir::new("bigram-key");
            plant_bigram(&user_dir.path, vec![(vec![0, 1], vec![7, 0, 0, 0])]);
            let (context, _instance) = open_with_user(&user_dir.path);
            zhuyin_mask_out(context, 0xFFFF_FFFF, 0);
        },
    );
}

/// A user gram whose item survives the mask with a zero token aborts the walk.
#[test]
fn mask_out_over_a_residual_bigram_gram_aborts() {
    assert_aborts(
        "boundary_tests::mask_out_over_a_residual_bigram_gram_aborts",
        || {
            let user_dir = TempUserDir::new("bigram-residual");
            let value = [&[7_u8, 0, 0, 0][..], &[0; 8]].concat();
            plant_bigram(&user_dir.path, vec![(vec![1, 0, 0, 0], value)]);
            let (context, _instance) = open_with_user(&user_dir.path);
            zhuyin_mask_out(context, 0xFFFF_FFFF, 0);
        },
    );
}

/// A mask that refuses a partial user gram leaves the import working.
#[test]
fn an_import_after_a_refused_bigram_mask_still_adds() {
    let user_dir = TempUserDir::new("partial-mask");
    plant_bigram(
        &user_dir.path,
        vec![(vec![1, 0, 0, 0], vec![0, 0, 0, 0, 1])],
    );
    let (context, instance) = open_with_user(&user_dir.path);
    let reading = "ㄋㄧˇ ㄋㄧˇ ㄋㄧˇ ㄋㄧˇ".as_bytes();
    assert!(import(context, 1, "你你你你", reading, 5));
    assert!(!zhuyin_mask_out(context, 0xFFFF_FFFF, 0));
    assert!(import(context, 1, "你你你你", reading, 4));
    close(context, instance);
}

/// A parse that places no key refuses the guess: an empty reparse, and a
/// lone separator, which keeps the earlier sentence and clears the list
/// (`phonetic_lookup.h:743-748`, `zhuyin.cpp:902-907`).
#[test]
fn a_keyless_parse_refuses_the_guess_and_keeps_the_sentence() {
    let (context, _) = open();
    assert!(zhuyin_set_options(context, 1 << 5));
    let instance = chewing(context, "su3cl3");
    assert_eq!(zhuyin_get_parsed_input_length(instance), 6);
    assert!(zhuyin_guess_sentence(instance));
    assert_eq!(zhuyin_parse_more_chewings(instance, cstr("").as_ptr()), 0);
    assert!(!zhuyin_guess_sentence(instance));
    close(context, instance);

    let (context, instance) = open();
    zhuyin_parse_more_full_pinyins(instance, cstr("ni3hao3").as_ptr());
    assert!(zhuyin_guess_sentence(instance));
    assert_eq!(
        zhuyin_parse_more_full_pinyins(instance, cstr("'").as_ptr()),
        1
    );
    assert!(!zhuyin_guess_sentence(instance));
    assert_eq!(sentence(instance), (true, Some("你好".to_owned())));
    assert_eq!(n_candidate(instance), (true, 0));
    zhuyin_free_instance(instance);

    let instance = zhuyin_alloc_instance(context);
    assert_eq!(
        zhuyin_parse_more_full_pinyins(instance, cstr("'").as_ptr()),
        1
    );
    assert!(!zhuyin_guess_sentence(instance));
    assert_eq!(sentence(instance), (false, None));
    close(context, instance);
}

/// `'ni'hao` under the separator options, guessed: column 3 holds the zero
/// key that `_check_offset` asserts on (`zhuyin.cpp:1453`).
fn separated(context: *mut ZhuyinContext) -> *mut ZhuyinInstance {
    let instance = full_pinyin(context, SEPARATOR_OPTIONS, "'ni'hao");
    assert_eq!(zhuyin_get_parsed_input_length(instance), 7);
    zhuyin_guess_sentence(instance);
    instance
}

/// The left move from just past a separator asserts (`zhuyin.cpp:1998`).
#[test]
fn left_offset_just_past_a_separator_aborts() {
    assert_aborts(
        "boundary_tests::left_offset_just_past_a_separator_aborts",
        || {
            let (context, _) = open();
            let mut out = 0;
            zhuyin_get_left_zhuyin_offset(separated(context), 4, &raw mut out);
        },
    );
}

/// The right move from just past a separator asserts (`zhuyin.cpp:2030`).
#[test]
fn right_offset_just_past_a_separator_aborts() {
    assert_aborts(
        "boundary_tests::right_offset_just_past_a_separator_aborts",
        || {
            let (context, _) = open();
            let mut out = 0;
            zhuyin_get_right_zhuyin_offset(separated(context), 4, &raw mut out);
        },
    );
}

/// The key rest just past a separator asserts (`zhuyin.cpp:1920`).
#[test]
fn key_rest_just_past_a_separator_aborts() {
    assert_aborts(
        "boundary_tests::key_rest_just_past_a_separator_aborts",
        || {
            let (context, _) = open();
            let mut rest: *mut ChewingKeyRest = ptr::null_mut();
            zhuyin_get_zhuyin_key_rest(separated(context), 4, &raw mut rest);
        },
    );
}

/// The character offset walk on a leading separator asserts
/// (`zhuyin.cpp:2110`).
#[test]
fn character_offset_on_a_leading_separator_aborts() {
    assert_aborts(
        "boundary_tests::character_offset_on_a_leading_separator_aborts",
        || {
            let (context, _) = open();
            let instance = full_pinyin(context, SEPARATOR_OPTIONS, "'ni");
            zhuyin_guess_sentence(instance);
            let mut out = 0;
            zhuyin_get_character_offset(instance, cstr("你").as_ptr(), 0, &raw mut out);
        },
    );
}

/// The zhuyin facade's own parsers stop at the first invalid byte.
#[test]
fn parses_stop_at_an_invalid_byte() {
    let (context, _) = open();
    for (parse, text, consumed) in [
        (
            zhuyin_parse_more_full_pinyins
                as extern "C" fn(*mut ZhuyinInstance, *const c_char) -> usize,
            &b"ni3\xffhao3"[..],
            3,
        ),
        (zhuyin_parse_more_full_pinyins, b"ni3hao3\xff", 7),
        (zhuyin_parse_more_full_pinyins, b"\xffni3", 0),
        (zhuyin_parse_more_chewings, b"su\xff3", 0),
        (zhuyin_parse_more_chewings, b"su3\xff", 3),
        (zhuyin_parse_more_chewings, b"\xffsu3", 0),
    ] {
        let instance = zhuyin_alloc_instance(context);
        let input = std::ffi::CString::new(text).expect("no interior NUL");
        assert_eq!(parse(instance, input.as_ptr()), consumed, "{text:?}");
        zhuyin_free_instance(instance);
    }
    zhuyin_fini(context);
}

/// The import reads a chewing reading directly: an invalid byte in a later
/// segment keeps the earlier key and adds, and one in the only or the last
/// whole segment refuses the reading.
#[test]
fn the_import_reads_a_chewing_reading_up_to_an_invalid_byte() {
    let user_dir = TempUserDir::new("direct-reading");
    let (context, instance) = open_with_user(&user_dir.path);
    let before = tokens_of(instance, "甲");
    assert!(import(
        context,
        7,
        "甲",
        &["ㄉㄚ ".as_bytes(), b"\xff"].concat(),
        5
    ));
    assert_eq!(
        tokens_of(instance, "甲"),
        [before, vec![0x0700_0001]].concat()
    );
    let whole = "ㄋㄧˇ ㄏㄠˇ".as_bytes();
    assert!(!import(context, 7, "你好", &[whole, b"\xff"].concat(), 1));
    assert!(!import(
        context,
        7,
        "你好",
        &["ㄋㄧˇ".as_bytes(), b"\xff", "ㄏㄠˇ".as_bytes()].concat(),
        1
    ));
    assert!(import(context, 7, "你好", whole, 1));
    close(context, instance);
}

/// The values a GLib clear callback saw.
static CLEARED: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

extern "C" fn clear_element(element: glib_sys::gpointer) {
    // SAFETY: GLib hands the callback one live element; the arrays here hold
    // four-byte or two-byte elements, and both start with their low bytes.
    let low = unsafe { element.cast::<u16>().read_unaligned() };
    CLEARED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(u32::from(low));
}

fn take_cleared() -> Vec<u32> {
    std::mem::take(
        &mut *CLEARED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

/// The elements of a zero-terminated GArray of `width`-byte elements; the
/// terminator is checked.
fn elements(array: *mut glib_sys::GArray, width: usize) -> Vec<u32> {
    // SAFETY: a live zero-terminated GArray of `width`-byte elements.
    unsafe {
        let view = &*array;
        let read = |index: usize| {
            if width == 4 {
                view.data.cast::<u32>().add(index).read_unaligned()
            } else {
                u32::from(view.data.cast::<u16>().add(index).read_unaligned())
            }
        };
        assert_eq!(read(view.len as usize), 0, "the zero terminator");
        (0..view.len as usize).map(read).collect()
    }
}

/// A caller's zero-terminated, cleared GArray keeps its metadata: the
/// facade's fill runs the clear callback on the seed, and a refusal clears
/// the array through it (`garray.c`, #924).
#[test]
fn caller_arrays_keep_their_metadata_and_clear_through_the_callback() {
    let (context, instance) = open();
    let mut token = 0;
    for width in [4_u32, 2] {
        // SAFETY: a fresh zero-terminated, cleared GArray of `width`-byte elements.
        let array = unsafe { glib_sys::g_array_new(1, 1, width) };
        // SAFETY: the callback reads the low two bytes of one element.
        unsafe { glib_sys::g_array_set_clear_func(array, Some(clear_element)) };
        let seed: u32 = 0x1234;
        // SAFETY: one element of `width` bytes from `seed`.
        unsafe { glib_sys::g_array_append_vals(array, (&raw const seed).cast(), 1) };
        take_cleared();
        let ret = if width == 4 {
            crate::dict::zhuyin_lookup_tokens(instance, cstr("你好").as_ptr(), array)
        } else {
            zhuyin_token_get_nth_pronunciation(instance, token, 0, array)
        };
        assert!(ret);
        assert_eq!(take_cleared(), [0x1234]);
        let filled = elements(array, width as usize);
        assert!(!filled.is_empty());
        if width == 4 {
            token = filled[0];
        }
        let refused = if width == 4 {
            crate::dict::zhuyin_lookup_tokens(instance, cstr("not-a-phrase").as_ptr(), array)
        } else {
            zhuyin_token_get_nth_pronunciation(instance, 0xFFFF_FFFF, 0, array)
        };
        assert!(!refused);
        assert!(elements(array, width as usize).is_empty());
        let low: Vec<u32> = filled.iter().map(|value| value & 0xFFFF).collect();
        assert_eq!(take_cleared(), low);
        // SAFETY: the array is freed once, with its data.
        unsafe { glib_sys::g_array_free(array, 1) };
    }
    close(context, instance);
}

/// A NULL key array makes GLib log a critical for each array call: two for
/// a known token (`g_array_set_size`, `g_array_append_vals`), one for an
/// unknown token.
#[test]
fn a_null_pronunciation_array_logs_the_glib_criticals() {
    let user = TempUserDir::new("garray-null-pronunciation");
    let (context, instance) = open_with_user(&user.path);
    let token = tokens_of(instance, "你好")[0];
    assert!(import(context, 7, "你好", "ㄋㄧˇ ㄏㄠˇ".as_bytes(), 1));
    let user_token = tokens_of(instance, "你好")
        .into_iter()
        .find(|token| token >> 24 == 7)
        .expect("the imported USER_FILE item");
    let logs = LogCapture::new();
    let glib = (Some("GLib".to_owned()), CRITICAL);
    for token in [token, user_token] {
        assert!(zhuyin_token_get_nth_pronunciation(
            instance,
            token,
            0,
            ptr::null_mut()
        ));
        let records = logs.take_with_messages();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].0, glib);
        assert!(records[0].1.contains("g_array_set_size"));
        assert_eq!(records[1].0, glib);
        assert!(records[1].1.contains("g_array_append_vals"));
    }
    assert!(!zhuyin_token_get_nth_pronunciation(
        instance,
        0xFFFF_FFFF,
        0,
        ptr::null_mut()
    ));
    let records = logs.take_with_messages();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].0, glib);
    assert!(records[0].1.contains("g_array_set_size"));
    drop(logs);
    close(context, instance);
}

/// Replaces the `database format:` token of a `table.conf` or `user.conf`
/// text with `LMDB`.
fn unknown_format(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.starts_with("database format:") {
                "database format:LMDB".to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// An unknown `database format:` in `table.conf` aborts the init
/// (`table_info.cpp:132`) before any user file is written.
#[test]
fn an_unknown_table_database_format_aborts_before_any_user_file() {
    let run = abort_in_child(
        "boundary_tests::an_unknown_table_database_format_aborts_before_any_user_file",
        |user| {
            let system = private_system(user.parent().expect("scratch"));
            let conf = system.join("table.conf");
            let text = std::fs::read_to_string(&conf).expect("table.conf");
            std::fs::remove_file(&conf).expect("unlink table.conf");
            std::fs::write(&conf, unknown_format(&text)).expect("table.conf");
            init_on(&system, Some(user));
        },
    );
    let left: Vec<_> = std::fs::read_dir(&run.user_dir)
        .expect("user dir")
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// An unknown `database format:` in the user's `user.conf` aborts the init
/// (`zhuyin.cpp:126-132`), and the file stays as it was.
#[test]
fn an_unknown_user_database_format_aborts_and_keeps_user_conf() {
    let run = abort_in_child(
        "boundary_tests::an_unknown_user_database_format_aborts_and_keeps_user_conf",
        |user| {
            write_user_conf(user, &system_dir());
            let path = user.join("user.conf");
            let text = std::fs::read_to_string(&path).expect("user.conf");
            std::fs::write(&path, unknown_format(&text)).expect("user.conf");
            init_on(&system_dir(), Some(user));
        },
    );
    let text = std::fs::read_to_string(run.user_dir.join("user.conf")).expect("user.conf");
    assert!(text.contains("database format:LMDB\n"), "{text}");
    assert!(text.ends_with("open counter:0\n"), "{text}");
}

/// A stale `user_phrase_index.bin.tmp` directory: the save renames it over
/// an absent or empty-directory index; over a live file the rename fails, the
/// tmp stays and the live file is kept, with one stderr line (#542). On
/// Windows `g_rename` cannot replace a directory with a directory, so the
/// empty-directory round keeps its tmp too.
#[test]
fn a_stale_index_tmp_directory_is_renamed_unless_a_live_file_holds_the_name() {
    let run = run_in_child(
        "boundary_tests::a_stale_index_tmp_directory_is_renamed_unless_a_live_file_holds_the_name",
        |root| {
            let marker: &[u8] = b"unchanged temporary directory";
            let empty_directory_moves = cfg!(not(windows));
            for (round, (destination, moved)) in [
                ("absent", true),
                ("empty-directory", empty_directory_moves),
                ("live-file", false),
            ]
            .into_iter()
            .enumerate()
            {
                let user = root.join(format!("round-{round}"));
                std::fs::create_dir(&user).expect("a fresh user directory");
                let (context, instance) = open_with_user(&user);
                let train = |instance| {
                    zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr());
                    assert!(zhuyin_guess_sentence(instance));
                    assert!(zhuyin_train(instance));
                };
                train(instance);
                let live = user.join("user_phrase_index.bin");
                let mut before = None;
                if destination == "live-file" {
                    assert!(zhuyin_save(context));
                    before = Some(std::fs::read(&live).expect("the live index"));
                    train(instance);
                } else if destination == "empty-directory" {
                    std::fs::create_dir(&live).expect("an empty destination directory");
                }
                let tmp = user.join("user_phrase_index.bin.tmp");
                std::fs::create_dir(&tmp).expect("the stale temporary directory");
                std::fs::write(tmp.join("stale-marker"), marker).expect("the stale marker");
                assert!(zhuyin_save(context), "{destination}");
                assert_eq!(tmp.exists(), !moved, "{destination}");
                assert_eq!(
                    std::fs::read(live.join("stale-marker")).ok().as_deref(),
                    moved.then_some(marker),
                    "{destination}"
                );
                if let Some(before) = before {
                    assert_eq!(std::fs::read(&live).expect("the live index"), before);
                }
                close(context, instance);
            }
        },
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    let renames = stderr.matches("user_phrase_index.bin failed.").count();
    assert_eq!(renames, if cfg!(windows) { 2 } else { 1 }, "{stderr}");
}

/// The left move past the matrix asserts in its column read
/// (`phonetic_key_matrix.h:103`).
#[test]
fn left_offset_past_the_matrix_aborts() {
    assert_aborts("boundary_tests::left_offset_past_the_matrix_aborts", || {
        let (context, _) = open();
        let mut out = 0;
        zhuyin_get_left_zhuyin_offset(chewing(context, "su3cl3"), 99, &raw mut out);
    });
}

/// The right move past the matrix asserts in its column read.
#[test]
fn right_offset_past_the_matrix_aborts() {
    assert_aborts(
        "boundary_tests::right_offset_past_the_matrix_aborts",
        || {
            let (context, _) = open();
            let mut out = 0;
            zhuyin_get_right_zhuyin_offset(chewing(context, "su3cl3"), 99, &raw mut out);
        },
    );
}

/// The key rest on an empty matrix asserts after `matrix.size() - 1` wraps
/// (`zhuyin.cpp:1914-1917`).
#[test]
fn key_rest_on_an_empty_matrix_aborts() {
    assert_aborts("boundary_tests::key_rest_on_an_empty_matrix_aborts", || {
        let (_context, instance) = open();
        let mut rest: *mut ChewingKeyRest = ptr::null_mut();
        zhuyin_get_zhuyin_key_rest(instance, 0, &raw mut rest);
    });
}

/// The after-cursor guess answers at the reserved slot and asserts one past
/// it (`zhuyin.cpp:1453`).
#[test]
fn after_cursor_guess_one_past_the_reserved_slot_aborts() {
    assert_aborts(
        "boundary_tests::after_cursor_guess_one_past_the_reserved_slot_aborts",
        || {
            let (context, _) = open();
            let instance = chewing(context, "su3cl3");
            assert!(zhuyin_guess_candidates_after_cursor(instance, 6));
            zhuyin_guess_candidates_after_cursor(instance, 7);
        },
    );
}

/// su3cl3 with the facade's unigram total wrapped to zero through
/// `zhuyin_token_add_unigram_frequency`.
fn zero_total(context: *mut ZhuyinContext) -> *mut ZhuyinInstance {
    let instance = chewing(context, "su3cl3");
    // SAFETY: `instance` is live; the reference ends in this block.
    let total = unsafe { crate::state::instance_ref(instance) }
        .core
        .lm
        .amplified_total();
    let total = u32::try_from(total).expect("a guint32 total");
    assert!(zhuyin_token_add_unigram_frequency(
        instance,
        0x0100_0001,
        total.wrapping_neg()
    ));
    instance
}

/// A zero facade total asserts while the after-cursor guess ranks a row
/// (`zhuyin.cpp:1261`, register rows 61 and 66).
#[test]
fn a_zero_facade_total_aborts_the_after_cursor_ranking() {
    assert_aborts(
        "boundary_tests::a_zero_facade_total_aborts_the_after_cursor_ranking",
        || {
            let (context, _) = open();
            zhuyin_guess_candidates_after_cursor(zero_total(context), 0);
        },
    );
}

/// Before offset 0 the before-cursor guess searches no span, so a zero
/// total ranks nothing: true with no row.
#[test]
fn a_zero_facade_total_before_offset_zero_ranks_nothing() {
    let (context, _) = open();
    let instance = zero_total(context);
    assert!(zhuyin_guess_candidates_before_cursor(instance, 0));
    assert_eq!(n_candidate(instance), (true, 0));
    close(context, instance);
}

/// The sentence getter answers from the n-best results alone: false with the
/// output untouched on a fresh and on a parsed instance (`zhuyin.cpp:988`).
#[test]
fn get_sentence_before_a_guess_leaves_the_output() {
    let (context, instance) = open();
    assert_eq!(sentence(instance), (false, None));
    zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr());
    assert_eq!(sentence(instance), (false, None));
    close(context, instance);
}

/// A character walk that fails stores the length it reached, 0
/// (`zhuyin.cpp:2192`).
#[test]
fn a_failed_character_walk_writes_zero() {
    let (context, _) = open();
    let instance = chewing(context, "su3cl3");
    let mut length = UNTOUCHED as usize;
    assert!(!zhuyin_get_character_offset(
        instance,
        cstr("啊").as_ptr(),
        3,
        &raw mut length
    ));
    assert_eq!(length, 0);
    close(context, instance);
}

/// The token getters (`zhuyin.cpp:1793-1826`): the nth pronunciation answers
/// true for any nth of a known token, and false with an emptied array for an
/// unknown one; the phrase getter fills each non-NULL output on a hit and
/// leaves both on a miss; the count is 0 on a miss.
#[test]
fn token_getters_answer_the_pins_ranges_and_outputs() {
    let (context, instance) = open();
    let token = tokens_of(instance, "你好")[0];
    let (_, count) = n_pronunciation(instance, token);
    assert_eq!(count, 1);
    let first = nth_pronunciation(instance, token, 0);
    assert_eq!((first.0, first.1), (true, 2));
    for nth in [count, count + 1, u32::MAX] {
        assert!(nth_pronunciation(instance, token, nth).0, "nth {nth}");
    }
    // SAFETY: a fresh GArray of two-byte elements, seeded with one key.
    let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
    let seed: u16 = 0x1234;
    // SAFETY: one two-byte element from a live local.
    unsafe { glib_sys::g_array_append_vals(keys, (&raw const seed).cast(), 1) };
    assert!(!zhuyin_token_get_nth_pronunciation(
        instance,
        0xFFFF_FFFF,
        0,
        keys
    ));
    // SAFETY: the array is live; it is freed once.
    unsafe {
        assert_eq!((*keys).len, 0);
        glib_sys::g_array_free(keys, 1);
    }

    assert_eq!(
        token_phrase(instance, token),
        (true, 2, Some("你好".to_owned()))
    );
    let mut length = UNTOUCHED;
    assert!(zhuyin_token_get_phrase(
        instance,
        token,
        &raw mut length,
        ptr::null_mut()
    ));
    assert_eq!(length, 2);
    let mut text: *mut c_char = ptr::null_mut();
    assert!(zhuyin_token_get_phrase(
        instance,
        token,
        ptr::null_mut(),
        &raw mut text
    ));
    assert_eq!(take_sentence(text), "你好");
    assert!(zhuyin_token_get_phrase(
        instance,
        token,
        ptr::null_mut(),
        ptr::null_mut()
    ));
    for miss in [0xFFFF_FFFF, 0x0DEA_DBEE, 0x0100_0000] {
        assert_eq!(
            token_phrase(instance, miss),
            (false, UNTOUCHED, None),
            "{miss:#x}"
        );
        assert!(!zhuyin_token_get_phrase(
            instance,
            miss,
            ptr::null_mut(),
            ptr::null_mut()
        ));
        assert_eq!(n_pronunciation(instance, miss), (false, 0), "{miss:#x}");
    }
    close(context, instance);
}

/// The key getters hand out the pin's function-local statics
/// (`zhuyin.cpp:1898`, `:1921`): one slot for every instance, which outlives
/// the instance. The slots are process-wide, so the test runs alone in a child.
#[test]
fn key_slots_are_process_wide_statics() {
    run_in_child("boundary_tests::key_slots_are_process_wide_statics", |_| {
        let (context, _) = open();
        let (first, second) = (chewing(context, "su3"), chewing(context, "cl3"));
        let mut keys: [*mut ChewingKey; 2] = [ptr::null_mut(); 2];
        let mut rests: [*mut ChewingKeyRest; 2] = [ptr::null_mut(); 2];
        assert!(zhuyin_get_zhuyin_key(first, 0, &raw mut keys[0]));
        assert!(zhuyin_get_zhuyin_key(second, 0, &raw mut keys[1]));
        assert!(zhuyin_get_zhuyin_key_rest(first, 0, &raw mut rests[0]));
        assert!(zhuyin_get_zhuyin_key_rest(second, 0, &raw mut rests[1]));
        assert_eq!(keys[0], keys[1]);
        assert_eq!(rests[0], rests[1]);
        // SAFETY: the slots are statics, readable for the process's life.
        let read = || unsafe { ((*keys[0]).packed, *rests[0].cast::<[u8; 4]>()) };
        assert_eq!(read(), (0x3286, [0, 0, 3, 0]));
        zhuyin_free_instance(first);
        assert_eq!(read(), (0x3286, [0, 0, 3, 0]));
    });
}

/// Unloading the user library hides its token from the token getters, and
/// the load shows it again.
#[test]
fn an_unloaded_user_library_hides_its_token_until_the_load() {
    let user_dir = TempUserDir::new("token-unload");
    let (context, instance) = open_with_user(&user_dir.path);
    assert!(import(context, 7, "你好", "ㄋㄧˇ ㄏㄠˇ".as_bytes(), 1));
    let tokens = user_tokens(instance, "你好");
    assert_eq!(tokens.len(), 1);
    let token = tokens[0];
    let shown = |instance| {
        assert_eq!(n_pronunciation(instance, token), (true, 1));
        assert_eq!(
            nth_pronunciation(instance, token, 0),
            (true, 2, "2b308632".to_owned())
        );
        assert_eq!(
            token_phrase(instance, token),
            (true, 2, Some("你好".to_owned()))
        );
    };
    shown(instance);
    assert!(zhuyin_save(context));
    assert!(zhuyin_unload_phrase_library(context, 7));
    assert_eq!(n_pronunciation(instance, token), (false, 0));
    assert_eq!(
        nth_pronunciation(instance, token, 0),
        (false, 0, String::new())
    );
    assert_eq!(token_phrase(instance, token), (false, UNTOUCHED, None));
    assert!(zhuyin_load_phrase_library(context, 7));
    shown(instance);
    close(context, instance);
}

/// The chewing scheme setter accepts schemes 6, 8 and 9; the loader refuses
/// indexes 16 and 255.
#[test]
fn chewing_schemes_and_library_bounds_follow_the_pin() {
    let (context, instance) = open();
    for scheme in [6, 8, 9] {
        assert!(
            zhuyin_set_chewing_scheme(context, scheme),
            "scheme {scheme}"
        );
    }
    assert!(!zhuyin_load_phrase_library(context, 16));
    assert!(!zhuyin_load_phrase_library(context, 255));
    close(context, instance);
}

/// The NULL arguments that the pin dereferences answer the declared value
/// (register row 41, class (b); `docs/findings/upstream-divergences.md`,
/// "NULL pointer arguments: the pin dereferences, oxpinyin null-guards"). The
/// NULL strings after defined prior work keep the earlier state
/// ("NULL string conversion after defined prior work (#1279)").
#[test]
fn null_arguments_answer_the_declared_value() {
    let _slots = crate::test_support::key_slot_guard();
    let user_dir = TempUserDir::new("null-arguments");
    let (context, instance) = open_with_user(&user_dir.path);
    let su3 = cstr("su3");
    let mut key = ChewingKey::ZERO;

    let iter = crate::iterators::zhuyin_begin_add_phrases(context, 16);
    assert!(!iter.is_null());
    let reading = cstr("ㄋㄧˇ ㄏㄠˇ");
    assert!(!crate::iterators::zhuyin_iterator_add_phrase(
        iter,
        cstr("你好").as_ptr(),
        reading.as_ptr(),
        1
    ));
    crate::iterators::zhuyin_end_add_phrases(iter);
    let iter = crate::iterators::zhuyin_begin_add_phrases(context, 7);
    assert!(!crate::iterators::zhuyin_iterator_add_phrase(
        iter,
        ptr::null(),
        reading.as_ptr(),
        5
    ));
    crate::iterators::zhuyin_end_add_phrases(iter);

    assert!(!zhuyin_parse_chewing(
        ptr::null_mut(),
        su3.as_ptr(),
        &raw mut key
    ));
    let mut seeded = ChewingKey { packed: 0xCDEF };
    assert!(!zhuyin_parse_chewing(
        instance,
        ptr::null(),
        &raw mut seeded
    ));
    assert_eq!(seeded.packed, 0xCDEF);
    assert!(!zhuyin_parse_chewing(
        instance,
        su3.as_ptr(),
        ptr::null_mut()
    ));
    let mut symbols = SEEDED as *mut *mut c_char;
    assert!(!zhuyin_in_chewing_keyboard(
        ptr::null_mut(),
        b's' as c_char,
        &raw mut symbols
    ));
    assert!(zhuyin_in_chewing_keyboard(
        instance,
        b's' as c_char,
        ptr::null_mut()
    ));

    assert!(zhuyin_parse_chewing(instance, su3.as_ptr(), &raw mut key));
    let mut text = SEEDED as *mut c_char;
    assert!(!zhuyin_get_pinyin_string(
        ptr::null_mut(),
        &raw mut key,
        &raw mut text
    ));
    assert!(!zhuyin_get_zhuyin_string(
        instance,
        ptr::null_mut(),
        &raw mut text
    ));
    assert!(!zhuyin_get_zhuyin_string(
        instance,
        &raw mut key,
        ptr::null_mut()
    ));

    let (mut begin, mut end) = (0_u16, 0_u16);
    assert!(!zhuyin_get_zhuyin_key_rest_positions(
        instance,
        ptr::null_mut(),
        &raw mut begin,
        &raw mut end
    ));
    assert_eq!(
        zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()),
        6
    );
    let mut rest: *mut ChewingKeyRest = ptr::null_mut();
    assert!(zhuyin_get_zhuyin_key_rest(instance, 0, &raw mut rest) && !rest.is_null());
    let mut length = 0_u16;
    assert!(!zhuyin_get_zhuyin_key_rest_length(
        instance,
        ptr::null_mut(),
        &raw mut length
    ));
    assert!(zhuyin_get_zhuyin_key_rest_length(
        instance,
        rest,
        ptr::null_mut()
    ));

    assert!(zhuyin_guess_candidates_after_cursor(instance, 0));
    let candidate = candidate_at(instance, 0);
    let mut kind = crate::types::lookup_candidate_type_t::ZOMBIE_CANDIDATE;
    assert!(!zhuyin_get_candidate_type(
        instance,
        ptr::null_mut(),
        &raw mut kind
    ));
    assert!(zhuyin_get_candidate_type(
        instance,
        candidate,
        ptr::null_mut()
    ));
    let mut borrowed = SEEDED as *const c_char;
    assert!(!zhuyin_get_candidate_string(
        instance,
        ptr::null_mut(),
        &raw mut borrowed
    ));
    assert!(zhuyin_get_candidate_string(
        instance,
        candidate,
        ptr::null_mut()
    ));

    assert_eq!(zhuyin_parse_more_chewings(instance, ptr::null()), 0);
    assert_eq!(zhuyin_get_parsed_input_length(instance), 0);

    let mut count = 0;
    assert!(!zhuyin_get_n_phrase(ptr::null_mut(), &raw mut count));
    assert!(zhuyin_get_n_phrase(instance, ptr::null_mut()));
    let mut slot = 0;
    assert!(!zhuyin_get_phrase_token(ptr::null_mut(), 0, &raw mut slot));
    assert!(!zhuyin_get_phrase_token(instance, 0, ptr::null_mut()));

    assert!(zhuyin_phrase_segment(instance, cstr("你好").as_ptr()));
    let logs = LogCapture::new();
    assert!(zhuyin_phrase_segment(instance, ptr::null()));
    assert_eq!(logs.take(), [(Some("GLib".to_owned()), CRITICAL)]);
    drop(logs);
    let mut count = UNTOUCHED;
    assert!(zhuyin_get_n_phrase(instance, &raw mut count));
    assert_eq!(count, 0);
    close(context, instance);
}

/// The keyboard symbols of one key: the answer and the symbols, `None` for
/// a NULL vector. The vector is freed.
fn keyboard(instance: *mut ZhuyinInstance, key: u8) -> (bool, Option<Vec<String>>) {
    let mut symbols: *mut *mut c_char = ptr::null_mut();
    #[allow(clippy::cast_possible_wrap)]
    let ret = zhuyin_in_chewing_keyboard(instance, key as c_char, &raw mut symbols);
    if symbols.is_null() {
        return (ret, None);
    }
    let mut out = Vec::new();
    // SAFETY: a non-NULL vector is NULL-terminated and the caller frees it.
    unsafe {
        let mut at = 0;
        while !(*symbols.add(at)).is_null() {
            out.push(
                std::ffi::CStr::from_ptr(*symbols.add(at))
                    .to_str()
                    .expect("UTF-8")
                    .to_owned(),
            );
            at += 1;
        }
        glib_sys::g_strfreev(symbols);
    }
    (ret, Some(out))
}

/// `zhuyin_in_chewing_keyboard` maps a key by the scheme and the tone option
/// at the pin.
#[test]
fn the_chewing_keyboard_maps_keys_by_scheme_and_options() {
    let (context, instance) = open();
    let one = |symbol: &str| (true, Some(vec![symbol.to_owned()]));
    assert_eq!(keyboard(instance, b's'), one("ㄋ"));
    assert_eq!(keyboard(instance, b'3'), one("ˇ"));
    assert_eq!(keyboard(instance, b'1'), one("ㄅ"));
    for key in [b'!', 0x80, b'~'] {
        assert_eq!(keyboard(instance, key), (false, None), "{key:#x}");
    }
    assert!(zhuyin_set_options(context, 0));
    assert_eq!(keyboard(instance, b'3'), (false, None));
    close(context, instance);
    let (context, instance) = open();
    assert!(zhuyin_set_chewing_scheme(context, 2));
    assert_eq!(
        keyboard(instance, b's'),
        (true, Some(vec!["ㄙ".to_owned(), "˙".to_owned()]))
    );
    close(context, instance);
}

/// `zhuyin_parse_chewing` writes the key only for one whole syllable.
#[test]
fn parse_chewing_writes_the_key_only_on_success() {
    let (context, instance) = open();
    for (text, ret, packed) in [
        ("su3", true, 0x302B),
        ("cl3", true, 0x3286),
        ("su", false, 0xCDEF),
        ("su3cl3", false, 0xCDEF),
        ("", false, 0xCDEF),
    ] {
        let mut key = ChewingKey { packed: 0xCDEF };
        assert_eq!(
            zhuyin_parse_chewing(instance, cstr(text).as_ptr(), &raw mut key),
            ret,
            "{text}"
        );
        assert_eq!(key.packed, packed, "{text}");
    }
    close(context, instance);
}

// ── Native DBM copies ──────────────────────────────────────────────────────

/// The regular files of `dir` by name, with their bytes.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
fn files_of(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .expect("read the directory")
        .map(|entry| {
            let entry = entry.expect("an entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            (name, std::fs::read(entry.path()).expect("read a file"))
        })
        .collect()
}

/// Trains one sentence on `context`, so that the next save writes.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
fn train_once(context: *mut ZhuyinContext) {
    let instance = zhuyin_alloc_instance(context);
    assert!(!instance.is_null());
    zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr());
    assert!(zhuyin_guess_sentence(instance));
    assert!(zhuyin_train(instance));
    zhuyin_free_instance(instance);
}

/// The pin's copies assert on a failed destination write
/// (`tkrzwdb_utils.h:65`, `bdb_utils.h:61`). `zhuyin_save` renames each
/// file after its copy (`zhuyin.cpp:646-691`), so the save stops with the
/// earlier files renamed, the failed file still staged and its saved file
/// unchanged, and `user.conf` not marked again.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
#[test]
fn save_aborts_where_a_copy_fails_a_destination_write() {
    let run = abort_in_child(
        "boundary_tests::save_aborts_where_a_copy_fails_a_destination_write",
        |user| {
            plant_bigram(
                user,
                vec![
                    (0x0100_0001_u32.to_le_bytes().to_vec(), vec![0; 4]),
                    (0x0100_0002_u32.to_le_bytes().to_vec(), vec![0; 4]),
                ],
            );
            let (context, _instance) = open_with_user(user);
            train_once(context);
            assert!(zhuyin_save(context));
            let before = user.parent().expect("the child's scratch").join("before");
            std::fs::create_dir(&before).expect("the before directory");
            for (name, bytes) in files_of(user) {
                std::fs::write(before.join(name), bytes).expect("keep a file");
            }
            train_once(context);
            // The first copy writes one record and fails the next.
            oxpinyin_store::copy_faults::fail_destination_write_after(1);
            zhuyin_save(context);
        },
    );
    let before = files_of(&run.user_dir.parent().expect("scratch").join("before"));
    let after = files_of(&run.user_dir);
    assert_eq!(after.get("user.conf"), before.get("user.conf"));
    let staged: Vec<&String> = after.keys().filter(|name| name.ends_with(".tmp")).collect();
    assert!(!staged.is_empty(), "the copy left no staged file");
    for name in staged {
        let saved = name.strip_suffix(".tmp").expect("a staged name");
        assert_eq!(after.get(saved), before.get(saved), "{saved} was replaced");
    }
}
