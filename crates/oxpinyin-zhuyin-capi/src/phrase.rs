//! The phrase-result surface: `zhuyin_phrase_segment` and the two getters
//! over `m_phrase_result`.

use std::os::raw::c_char;

use crate::state::{instance_mut, instance_ref};
use crate::types::{GUint, PhraseTokenT, ZhuyinInstance};

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
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    // The pin's first call, `g_utf8_strlen(sentence, -1)` (074a2219
    // zhuyin.cpp:969), made through GLib: on NULL, GLib's own precondition
    // logs its critical (GLib 2.84.4 gutf8.c:240). The pin then dereferences
    // NULL in `g_utf8_to_ucs4`; take the ruled no-input outcome instead
    // (074a2219 phrase_lookup.cpp:119-157,382-428).
    if sentence.is_null() {
        // SAFETY: GLib checks `p != NULL || max == 0` itself, logs and
        // returns 0 without reading `p`.
        unsafe {
            glib_sys::g_utf8_strlen(sentence, -1);
        }
        return match inst.core.session.phrase_segment("") {
            Ok((matched, tokens)) => {
                inst.core.phrase_result = tokens;
                matched
            }
            Err(error) => {
                crate::sentence::refuse_lookup_gram("phrase_segment", &error);
                false
            }
        };
    }
    let Some(text) = oxpinyin_capi_marshal::input::utf8_to_ucs4(
        oxpinyin_capi_marshal::borrow_c_string!(sentence),
    ) else {
        // 074a2219 zhuyin.cpp:973: `g_return_val_if_fail(num_of_chars ==
        // ucs4_len, FALSE)` answers false and leaves the result as it was. Its
        // critical is not reproduced (2026-10-31 UTC ruling,
        // docs/findings/1297-segment-critical.md).
        return false;
    };
    let outcome = match text.as_str() {
        Some(text) => inst.core.session.phrase_segment(text),
        None => inst
            .core
            .session
            .phrase_segment_ucs4(text.stored_words().unwrap_or_default()),
    };
    match outcome {
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

    struct LogHandler {
        default: u32,
        facade: u32,
    }
    impl LogHandler {
        fn new() -> Self {
            LOGS.with(|logs| *logs.borrow_mut() = Some(Vec::new()));
            // SAFETY: The callback lives for the process, has no borrowed
            // user data, and is removed by Drop before the capture ends.
            let default = unsafe {
                glib_sys::g_log_set_handler(
                    std::ptr::null(),
                    glib_sys::G_LOG_LEVEL_MASK,
                    Some(capture_log),
                    std::ptr::null_mut(),
                )
            };
            // SAFETY: Same callback and lifetime; the facade domain is the
            // one `ffi::log_warning` writes.
            let facade = unsafe {
                glib_sys::g_log_set_handler(
                    c"libzhuyin".as_ptr(),
                    glib_sys::G_LOG_LEVEL_MASK,
                    Some(capture_log),
                    std::ptr::null_mut(),
                )
            };
            Self { default, facade }
        }
    }
    impl Drop for LogHandler {
        fn drop(&mut self) {
            // SAFETY: These are the handler ids installed above.
            unsafe {
                glib_sys::g_log_remove_handler(std::ptr::null(), self.default);
                glib_sys::g_log_remove_handler(c"libzhuyin".as_ptr(), self.facade);
            }
            LOGS.with(|logs| *logs.borrow_mut() = None);
        }
    }

    #[test]
    fn read_after_check_emits_one_class_c_warning() {
        let _logs = LogHandler::new();
        let error = oxpinyin_engine::EngineError::ReadAfterCheck { token: 0x0100_0001 };
        assert!(crate::sentence::refuse_lookup_gram(
            "zhuyin_phrase_segment",
            &error
        ));
        LOGS.with(|logs| {
            let logs = logs.borrow();
            let logs = logs.as_ref().expect("capture active");
            assert_eq!(logs.len(), 1);
            assert_eq!(logs[0].0.as_deref(), Some("libzhuyin"));
            assert_eq!(logs[0].1, glib_sys::G_LOG_LEVEL_WARNING);
            assert!(logs[0].2.contains("ngram_kyotodb.cpp:138-139"));
        });
    }

    #[test]
    fn surrogate_segment_matches_pin_length_and_no_critical() {
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

            // Recover through normal lookup, then verify the invalid-byte
            // refusal keeps the prior result and logs nothing (#1297).
            assert!(zhuyin_phrase_segment(instance, c"你好".as_ptr()));
            assert!(!zhuyin_phrase_segment(instance, c"\xff\xfe".as_ptr()));
            assert!(zhuyin_get_n_phrase(instance, &raw mut count));
            assert_eq!(count, 2);
            LOGS.with(|logs| assert!(logs.borrow().as_ref().expect("active capture").is_empty()));
        }
        crate::test_support::close(context, instance);
    }
}
