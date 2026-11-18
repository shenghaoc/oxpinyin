//! Helpers more than one verification topic uses: the fixture handles, the
//! scratch directories, and the abort and child-process helpers. The
//! exported C functions and types come from the rlib's crate root.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

pub use zhuyin_capi::*;

#[path = "../../src/test_support/abi.rs"]
mod abi;
pub(crate) use abi::*;

#[path = "../../../tests/support/c_harness.rs"]
mod c_harness;
pub(crate) use c_harness::*;

#[path = "../../../tests/support/native_fixture.rs"]
mod native_fixture;
pub(crate) use native_fixture::*;

use std::ffi::c_char;

use std::path::{Path, PathBuf};

use std::ptr;

/// Runs `body` in a child process that exits 0 when it returns, the way
/// [`assert_aborts`] runs one that must abort, and answers the child's
/// stderr. `body` gets a fresh, empty user directory the parent created, so
/// the parent can name it in the expected text. `name` is the calling
/// test's full path as libtest prints it (`module::test`). A child that
/// panics or dies fails the parent's assertion on its exit status.
pub(crate) fn run_in_child(name: &str, body: impl FnOnce(&Path)) -> ChildRun {
    let (mut command, user_dir, scratch) = child_command(name, false, body);
    let output = command.output().expect("re-run the test binary");
    // libtest reports a child's panic on stdout.
    assert!(
        output.status.success(),
        "{name} child failed ({:?}): {}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    ChildRun {
        stderr: output.stderr,
        user_dir,
        _scratch: scratch,
    }
}

/// Each character of `text` as a little-endian UCS-4 word: the key of the
/// user phrase index.
pub(crate) fn ucs4_key(text: &str) -> Vec<u8> {
    text.chars()
        .flat_map(|ch| u32::from(ch).to_le_bytes())
        .collect()
}

/// A seeded pointer out-parameter.
pub(crate) const SEEDED: usize = 0x00AB_CDEF;

/// The GLib level of a critical.
pub(crate) const CRITICAL: u32 = 8;

/// A fresh instance with `text` parsed as full pinyin under `options`.
pub(crate) fn full_pinyin(
    context: *mut ZhuyinContext,
    options: u32,
    text: &str,
) -> *mut ZhuyinInstance {
    assert!(unsafe { zhuyin_set_options(context, options) });
    let instance = unsafe { zhuyin_alloc_instance(context) };
    assert!(!instance.is_null());
    unsafe { zhuyin_parse_more_full_pinyins(instance, cstr(text).as_ptr()) };
    instance
}

/// The sentence getter's answer and text; `None` for an untouched output.
pub(crate) fn sentence(instance: *mut ZhuyinInstance) -> (bool, Option<String>) {
    let mut text = SEEDED as *mut c_char;
    let ret = unsafe { zhuyin_get_sentence(instance, &raw mut text) };
    let text = (text as usize != SEEDED).then(|| take_sentence(text));
    (ret, text)
}

pub(crate) fn candidate_at(instance: *mut ZhuyinInstance, index: u32) -> *mut LookupCandidate {
    let mut candidate: *mut LookupCandidate = ptr::null_mut();
    assert!(unsafe { zhuyin_get_candidate(instance, index, &raw mut candidate) });
    candidate
}

/// The keys of a GArray of two-byte elements as lowercase hex.
pub(crate) fn keys_hex(array: *mut glib_sys::GArray) -> String {
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
pub(crate) fn nth_pronunciation(
    instance: *mut ZhuyinInstance,
    token: u32,
    nth: u32,
) -> (bool, u32, String) {
    // SAFETY: a fresh GArray of two-byte elements, freed below.
    let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
    let ret = unsafe { zhuyin_token_get_nth_pronunciation(instance, token, nth, keys) };
    // SAFETY: the array is live.
    let len = unsafe { (*keys).len };
    let hex = keys_hex(keys);
    // SAFETY: the array was allocated above and is freed once.
    unsafe { glib_sys::g_array_free(keys, 1) };
    (ret, len, hex)
}

/// The phrase of `token`: the answer, the length and the text, the seeds
/// kept where untouched.
pub(crate) fn token_phrase(
    instance: *mut ZhuyinInstance,
    token: u32,
) -> (bool, u32, Option<String>) {
    let mut length = UNTOUCHED;
    let mut text = SEEDED as *mut c_char;
    let ret = unsafe { zhuyin_token_get_phrase(instance, token, &raw mut length, &raw mut text) };
    let text = (text as usize != SEEDED).then(|| take_sentence(text));
    (ret, length, text)
}

/// The library-7 tokens of `text`.
pub(crate) fn user_tokens(instance: *mut ZhuyinInstance, text: &str) -> Vec<u32> {
    tokens_of(instance, text)
        .into_iter()
        .filter(|token| token >> 24 == 7)
        .collect()
}

/// The elements of a zero-terminated GArray of `width`-byte elements; the
/// terminator is checked.
pub(crate) fn elements(array: *mut glib_sys::GArray, width: usize) -> Vec<u32> {
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

/// The text of the candidate at `index`, read through `zhuyin_get_candidate`
/// and `zhuyin_get_candidate_string`.
pub(crate) fn candidate_text(instance: *mut ZhuyinInstance, index: usize) -> String {
    let mut row: *mut LookupCandidate = std::ptr::null_mut();
    assert!(unsafe {
        zhuyin_get_candidate(
            instance,
            u32::try_from(index).expect("small index"),
            &raw mut row,
        )
    });
    let mut text: *const GChar = std::ptr::null();
    assert!(unsafe { zhuyin_get_candidate_string(instance, row, &raw mut text) });
    // SAFETY: a true answer points `text` at the row's borrowed string.
    unsafe { std::ffi::CStr::from_ptr(text) }
        .to_str()
        .expect("candidate text is UTF-8")
        .to_owned()
}

/// Opens `user_dir` over the fixture, answering the raw handle.
pub(crate) fn init_at(user_dir: &std::path::Path) -> *mut crate::support::ZhuyinContext {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir.to_str().expect("UTF-8 path"));
    unsafe { zhuyin_init(system.as_ptr(), user.as_ptr()) }
}

/// One call over a private system directory whose library is a directory
/// (register row 96): init maps `merged.bin` (`zhuyin.cpp:199`), the save and
/// the mask-out map it again (`:589`, `:800`), and the reload of the
/// unloaded library 2 maps `gbk_char.bin` (`:200`).
#[cfg(unix)]
pub(crate) fn map_a_directory_library(site: &str) -> bool {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::support::private_system(scratch.path());
    let user = scratch.path().join("user");
    std::fs::create_dir_all(&user).expect("user dir");
    let system_c = cstr(system.to_str().expect("UTF-8 path"));
    let user_c = cstr(user.to_str().expect("UTF-8 path"));
    if site == "init" {
        crate::support::directory_library(&system, "merged.bin");
        return !unsafe { zhuyin_init(system_c.as_ptr(), user_c.as_ptr()) }.is_null();
    }
    let context = unsafe { zhuyin_init(system_c.as_ptr(), user_c.as_ptr()) };
    assert!(!context.is_null());
    match site {
        "mask" => {
            crate::support::directory_library(&system, "merged.bin");
            unsafe { crate::support::zhuyin_mask_out(context, 0xff00_0000, 0x0400_0000) }
        }
        _ => {
            assert!(unsafe { crate::support::zhuyin_unload_phrase_library(context, 2) });
            crate::support::directory_library(&system, "gbk_char.bin");
            unsafe { crate::support::zhuyin_load_phrase_library(context, 2) }
        }
    }
}
