//! The phrase-result surface: `pinyin_phrase_segment` and the two
//! getters over `m_phrase_result` (`pinyin.cpp:3269-3287`).
//!
//! The segmenter is the engine's span DP (`oxpinyin_engine::Session::
//! phrase_segment`, the `PhraseLookup::get_best_match` port); this
//! module only stores its result on the instance and hands out reads.
//! The result shape is the pin's: a character-length array with each
//! phrase's token at its span's start position and `null_token` between
//! phrases — and, on a failed match, the fully sized all-null array
//! (`PhraseLookup::final_step` sizes and null-fills before its
//! empty-last-step `false`, `phrase_lookup.cpp:382-428`). `pinyin_reset`
//! clears the array (`pinyin.cpp:2699`); nothing else removes from it.

use std::os::raw::c_char;

use crate::state::{instance_mut, instance_ref};
use crate::types::{GUint, PhraseTokenT, PinyinInstance};

/// Text accepted by the pin's UCS4 gate, or an unmatchable UCS4 span.
enum SegmentText {
    Unicode(String),
    Unmatched(usize),
}

/// 074a2219 pinyin.cpp:1450-1454 / zhuyin.cpp:969-973 compare the
/// byte-walking `g_utf8_strlen` with `g_utf8_to_ucs4`'s written length.
/// GLib 2.90.0 gutf8.c uses `g_utf8_get_char_extended`, not the scalar
/// validity check in `g_utf8_get_char_validated`: surrogates and values
/// above U+10FFFF pass, while malformed, partial and overlong bytes fail.
/// Successful conversion walks the same complete sequences as strlen.
/// A failed conversion leaves length zero, unequal to any nonempty input.
fn segment_text(sentence: *const c_char) -> Option<SegmentText> {
    // SAFETY: The entry point checked for NULL; its C contract requires
    // a live NUL-terminated string for the duration of the call.
    let bytes = unsafe { std::ffi::CStr::from_ptr(sentence) };
    if let Ok(text) = bytes.to_str() {
        return Some(SegmentText::Unicode(text.to_owned()));
    }
    let mut length = 0;
    // SAFETY: `sentence` is the live NUL-terminated string above; length
    // is writable. NULL items_read rejects a trailing partial sequence;
    // NULL error suppresses conversion errors, as at the pin.
    let ucs4 = unsafe {
        glib_sys::g_utf8_to_ucs4(
            sentence,
            -1,
            std::ptr::null_mut(),
            &raw mut length,
            std::ptr::null_mut(),
        )
    };
    if ucs4.is_null() {
        return None;
    }
    // SAFETY: Successful conversion returns a GLib allocation, freed once.
    unsafe { glib_sys::g_free(ucs4.cast()) };
    // Rust-invalid bytes accepted by this converter contain a non-scalar
    // UCS4 value. No Unicode dictionary phrase can span it, so final_step
    // returns false with a character-sized all-null result (:382-397).
    usize::try_from(length).ok().map(SegmentText::Unmatched)
}

/// Segment an arbitrary sentence string into phrase tokens.
///
/// # C signature
/// ```c
/// bool pinyin_phrase_segment(pinyin_instance_t * instance,
///                            const char * sentence);
/// ```
///
/// Upstream gates on GLib UCS4 conversion length
/// (`g_return_val_if_fail(num_of_chars == ucs4_len, FALSE)`, `pinyin.cpp:1454`) and stores the best phrase
/// path into the instance's result array — including on a failed match,
/// where the array stays fully sized and all-null.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_phrase_segment(
    instance: *mut PinyinInstance,
    sentence: *const c_char,
) -> bool {
    if instance.is_null() || sentence.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    let Some(text) = segment_text(sentence) else {
        // 074a2219 pinyin.cpp:1454: the GLib precondition uses the default
        // domain, critical level and C++ G_STRFUNC spelling, exactly once.
        // SAFETY: the function and expression are NUL-terminated constants;
        // GLib accepts NULL for its default log domain.
        unsafe {
            glib_sys::g_return_if_fail_warning(
                std::ptr::null(),
                c"bool pinyin_phrase_segment(pinyin_instance_t*, const char*)".as_ptr(),
                c"num_of_chars == ucs4_len".as_ptr(),
            );
        }
        return false;
    };
    let text = match text {
        SegmentText::Unicode(text) => text,
        SegmentText::Unmatched(length) => {
            inst.core.phrase_result = vec![oxpinyin_core::PhraseToken::new(0); length];
            return false;
        }
    };
    match inst.core.session.phrase_segment(&text) {
        Ok((matched, tokens)) => {
            inst.core.phrase_result = tokens;
            matched
        }
        Err(_) => false,
    }
}

/// Get the number of phrase tokens in the phrase result.
///
/// # C signature
/// ```c
/// bool pinyin_get_n_phrase(pinyin_instance_t * instance, guint * num);
/// ```
///
/// Upstream answers `true` unconditionally over whatever
/// `pinyin_phrase_segment` last stored (`pinyin.cpp:3269-3274`); without
/// a prior segment call the count is 0.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_n_phrase(instance: *mut PinyinInstance, num: *mut GUint) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    let count = inst.core.phrase_result.len();
    if !num.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *num = GUint::try_from(count).unwrap_or(GUint::MAX);
        }
    }
    true
}

/// Get the phrase token at an index of the phrase result.
///
/// # C signature
/// ```c
/// bool pinyin_get_phrase_token(pinyin_instance_t * instance,
///                              guint index,
///                              phrase_token_t * token);
/// ```
///
/// The out-param is zeroed before the bounds check — a `false` still
/// delivers `null_token` (`pinyin.cpp:3276-3287`).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_phrase_token(
    instance: *mut PinyinInstance,
    index: GUint,
    token: *mut PhraseTokenT,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    if !token.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *token = crate::types::null_token;
        }
    }
    if index as usize >= inst.core.phrase_result.len() {
        return false;
    }
    if !token.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *token = inst.core.phrase_result[index as usize].value();
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::ffi::{CStr, CString};
    use std::os::raw::c_void;

    type Log = (Option<String>, glib_sys::GLogLevelFlags, String);
    thread_local! {
        static LOGS: RefCell<Option<Vec<Log>>> = const { RefCell::new(None) };
    }

    unsafe extern "C" fn capture_log(
        domain: *const c_char,
        level: glib_sys::GLogLevelFlags,
        message: *const c_char,
        data: *mut c_void,
    ) {
        let captured = LOGS.with(|logs| {
            let mut logs = logs.borrow_mut();
            let Some(logs) = logs.as_mut() else {
                return false;
            };
            let domain = (!domain.is_null()).then(|| {
                // SAFETY: GLib supplies a live NUL-terminated domain;
                // the default domain was excluded by the NULL check.
                unsafe { CStr::from_ptr(domain) }
                    .to_string_lossy()
                    .into_owned()
            });
            // SAFETY: GLib's log message is live and NUL-terminated.
            let message = unsafe { CStr::from_ptr(message) }
                .to_string_lossy()
                .into_owned();
            logs.push((domain, level, message));
            true
        });
        if !captured {
            // SAFETY: Forward GLib's unchanged arguments for other test
            // threads, whose thread-local capture is inactive.
            unsafe { glib_sys::g_log_default_handler(domain, level, message, data) };
        }
    }

    struct LogHandler(u32);
    impl LogHandler {
        fn new() -> Self {
            LOGS.with(|logs| *logs.borrow_mut() = Some(Vec::new()));
            // SAFETY: The callback lives for the process, has no borrowed
            // user data, and is removed by Drop before the capture ends.
            Self(unsafe {
                glib_sys::g_log_set_handler(
                    std::ptr::null(),
                    glib_sys::G_LOG_LEVEL_MASK,
                    Some(capture_log),
                    std::ptr::null_mut(),
                )
            })
        }
    }
    impl Drop for LogHandler {
        fn drop(&mut self) {
            // SAFETY: This is the handler id for the default domain above.
            unsafe { glib_sys::g_log_remove_handler(std::ptr::null(), self.0) };
            LOGS.with(|logs| *logs.borrow_mut() = None);
        }
    }

    #[test]
    fn surrogate_segment_matches_pin_length_and_diagnostics() {
        let user = crate::test_support::TempUserDir::new("surrogate-segment");
        let (context, instance) =
            crate::test_support::open(user.path.to_str().expect("UTF-8 path"));
        let _logs = LogHandler::new();
        for bytes in [
            b"\xed\xa0\x80nihao".as_slice(),
            b"ni\xed\xa0\x80hao".as_slice(),
            b"nihao\xed\xa0\x80".as_slice(),
        ] {
            let sentence = CString::new(bytes).expect("no interior NUL");
            assert!(!pinyin_phrase_segment(instance, sentence.as_ptr()));
            let mut count = 0xABCDEF;
            assert!(pinyin_get_n_phrase(instance, &raw mut count));
            assert_eq!(count, 6);
            for index in 0..count {
                let mut token = 0xABCDEF;
                assert!(pinyin_get_phrase_token(instance, index, &raw mut token));
                assert_eq!(token, 0);
            }
            let mut token = 0xABCDEF;
            assert!(!pinyin_get_phrase_token(instance, count, &raw mut token));
            assert_eq!(token, 0);
            LOGS.with(|logs| assert!(logs.borrow().as_ref().expect("active capture").is_empty()));

            // Recover through normal lookup, then verify #739's exact
            // invalid-byte critical and preservation of the prior result.
            assert!(pinyin_phrase_segment(instance, c"你好".as_ptr()));
            assert!(!pinyin_phrase_segment(instance, c"\xff\xfe".as_ptr()));
            assert!(pinyin_get_n_phrase(instance, &raw mut count));
            assert_eq!(count, 2);
            LOGS.with(|logs| assert_eq!(
                std::mem::take(logs.borrow_mut().as_mut().expect("active capture")),
                vec![(None, glib_sys::G_LOG_LEVEL_CRITICAL,
                    "bool pinyin_phrase_segment(pinyin_instance_t*, const char*): assertion 'num_of_chars == ucs4_len' failed".to_owned())],
            ));
        }
        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }
}
