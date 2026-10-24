//! The phrase-result surface: `zhuyin_phrase_segment` and the two getters
//! over `m_phrase_result`.

use std::os::raw::c_char;

use crate::state::{instance_mut, instance_ref};
use crate::types::{GUint, PhraseTokenT, ZhuyinInstance};

/// Text accepted by the pin's UCS4 gate, or an unmatchable UCS4 span.
enum SegmentText {
    Unicode(String),
    Unmatched { prefix: String, length: usize },
}

/// 074a2219 pinyin.cpp:1450-1454 / zhuyin.cpp:969-973 compare the
/// byte-walking `g_utf8_strlen` with `g_utf8_to_ucs4`'s written length.
/// GLib 2.89.4-2 gutf8.c uses `g_utf8_get_char_extended`, not the scalar
/// validity check in `g_utf8_get_char_validated`: surrogates and values
/// above U+10FFFF pass, while malformed, partial and overlong bytes fail.
/// Successful conversion walks the same complete sequences as strlen.
/// A failed conversion leaves length zero, unequal to any nonempty input.
fn segment_text(sentence: *const c_char) -> Option<SegmentText> {
    // SAFETY: The entry point checked for NULL; its C contract requires
    // a live NUL-terminated string for the duration of the call.
    let bytes = unsafe { std::ffi::CStr::from_ptr(sentence) };
    let invalid = match bytes.to_str() {
        Ok(text) => return Some(SegmentText::Unicode(text.to_owned())),
        Err(invalid) => invalid,
    };
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
    // The first Rust-invalid sequence is a GLib-accepted non-scalar.
    // Process only its scalar prefix before publishing the failed result.
    let prefix = std::str::from_utf8(&bytes.to_bytes()[..invalid.valid_up_to()])
        .ok()?
        .to_owned();
    Some(SegmentText::Unmatched {
        prefix,
        length: usize::try_from(length).ok()?,
    })
}

/// Segment an arbitrary sentence string into phrase tokens.
///
/// # C signature
/// ```c
/// bool zhuyin_phrase_segment(zhuyin_instance_t * instance,
///                            const char * sentence);
/// ```
///
/// Upstream gates on GLib UCS4 conversion length
/// (`g_return_val_if_fail`, `zhuyin.cpp:973`) and stores the best phrase path into the instance's result array.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_phrase_segment(
    instance: *mut ZhuyinInstance,
    sentence: *const c_char,
) -> bool {
    if instance.is_null() || sentence.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    let Some(text) = segment_text(sentence) else {
        // 074a2219 zhuyin.cpp:973: same default-domain GLib critical as the
        // pinyin twin, with this facade's C++ G_STRFUNC spelling.
        // SAFETY: the function and expression are NUL-terminated constants;
        // GLib accepts NULL for its default log domain.
        unsafe {
            glib_sys::g_return_if_fail_warning(
                std::ptr::null(),
                c"bool zhuyin_phrase_segment(zhuyin_instance_t*, const char*)".as_ptr(),
                c"num_of_chars == ucs4_len".as_ptr(),
            );
        }
        return false;
    };
    let text = match text {
        SegmentText::Unicode(text) => text,
        SegmentText::Unmatched { prefix, length } => {
            // Row 85: the prefix's reached gram is read before final_step.
            // Refuse through the ordinary path and preserve the old result.
            if let Err(error) = inst.core.session.check_phrase_prefix(&prefix) {
                crate::sentence::refuse_lookup_gram("zhuyin_phrase_segment", &error);
                return false;
            }
            inst.core.phrase_result = vec![oxpinyin_core::PhraseToken::new(0); length];
            return false;
        }
    };
    match inst.core.session.phrase_segment(&text) {
        Ok((matched, tokens)) => {
            inst.core.phrase_result = tokens;
            matched
        }
        Err(error) => {
            crate::sentence::refuse_lookup_gram("zhuyin_phrase_segment", &error);
            false
        }
    }
}

/// Get the number of phrase tokens in the phrase result.
///
/// # C signature
/// ```c
/// bool zhuyin_get_n_phrase(zhuyin_instance_t * instance, guint * num);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_n_phrase(instance: *mut ZhuyinInstance, num: *mut GUint) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
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
/// bool zhuyin_get_phrase_token(zhuyin_instance_t * instance,
///                              guint index, phrase_token_t * token);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_phrase_token(
    instance: *mut ZhuyinInstance,
    index: GUint,
    token: *mut PhraseTokenT,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
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
        let (context, instance) = crate::test_support::open();
        let _logs = LogHandler::new();
        for bytes in [
            b"\xed\xa0\x80nihao".as_slice(),
            b"ni\xed\xa0\x80hao".as_slice(),
            b"nihao\xed\xa0\x80".as_slice(),
        ] {
            let sentence = CString::new(bytes).expect("no interior NUL");
            assert!(!zhuyin_phrase_segment(instance, sentence.as_ptr()));
            let mut count = 0xABCDEF;
            assert!(zhuyin_get_n_phrase(instance, &raw mut count));
            assert_eq!(count, 6);
            for index in 0..count {
                let mut token = 0xABCDEF;
                assert!(zhuyin_get_phrase_token(instance, index, &raw mut token));
                assert_eq!(token, 0);
            }
            let mut token = 0xABCDEF;
            assert!(!zhuyin_get_phrase_token(instance, count, &raw mut token));
            assert_eq!(token, 0);
            LOGS.with(|logs| assert!(logs.borrow().as_ref().expect("active capture").is_empty()));

            // Recover through normal lookup, then verify #739's exact
            // invalid-byte critical and preservation of the prior result.
            assert!(zhuyin_phrase_segment(instance, c"你好".as_ptr()));
            assert!(!zhuyin_phrase_segment(instance, c"\xff\xfe".as_ptr()));
            assert!(zhuyin_get_n_phrase(instance, &raw mut count));
            assert_eq!(count, 2);
            LOGS.with(|logs| assert_eq!(
                std::mem::take(logs.borrow_mut().as_mut().expect("active capture")),
                vec![(None, glib_sys::G_LOG_LEVEL_CRITICAL,
                    "bool zhuyin_phrase_segment(zhuyin_instance_t*, const char*): assertion 'num_of_chars == ucs4_len' failed".to_owned())],
            ));
        }
        crate::test_support::close(context, instance);
    }
}
