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
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    // GLib 2.84.4 gutf8.c:240 logs before the following UCS-4 NULL
    // dereference. Preserve that defined diagnostic, then take the ruled
    // no-input outcome (074a2219 phrase_lookup.cpp:119-157,382-428).
    if sentence.is_null() {
        // SAFETY: All three arguments are static NUL-terminated strings.
        unsafe {
            glib_sys::g_return_if_fail_warning(
                c"GLib".as_ptr(),
                c"g_utf8_strlen".as_ptr(),
                c"p != NULL || max == 0".as_ptr(),
            );
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
            crate::sentence::refuse_lookup_gram("pinyin_phrase_segment", &error);
            false
        }
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
                    c"libpinyin".as_ptr(),
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
                glib_sys::g_log_remove_handler(c"libpinyin".as_ptr(), self.facade);
            }
            LOGS.with(|logs| *logs.borrow_mut() = None);
        }
    }

    #[test]
    fn a_layout_critical_is_the_pins_return_if_fail_warning() {
        let _logs = LogHandler::new();
        crate::ffi::return_if_fail_warning(
            oxpinyin_data::phrase_library::SUB_PHRASE_LOAD_SIGNATURE,
            "index_three <= end",
        );
        LOGS.with(|logs| {
            let logs = logs.borrow();
            let logs = logs.as_ref().expect("capture active");
            assert_eq!(logs.len(), 1);
            assert_eq!(logs[0].0, None);
            assert_eq!(logs[0].1, glib_sys::G_LOG_LEVEL_CRITICAL);
            assert_eq!(
                logs[0].2,
                "bool pinyin::SubPhraseIndex::load(pinyin::MemoryChunk*, table_offset_t, \
                 table_offset_t): assertion 'index_three <= end' failed"
            );
        });
    }

    #[test]
    fn read_after_check_emits_one_class_c_warning() {
        let _logs = LogHandler::new();
        let error = oxpinyin_engine::EngineError::ReadAfterCheck { token: 0x0100_0001 };
        assert!(crate::sentence::refuse_lookup_gram(
            "pinyin_phrase_segment",
            &error
        ));
        LOGS.with(|logs| {
            let logs = logs.borrow();
            let logs = logs.as_ref().expect("capture active");
            assert_eq!(logs.len(), 1);
            assert_eq!(logs[0].0.as_deref(), Some("libpinyin"));
            assert_eq!(logs[0].1, glib_sys::G_LOG_LEVEL_WARNING);
            assert!(logs[0].2.contains("ngram_kyotodb.cpp:138-139"));
        });
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
