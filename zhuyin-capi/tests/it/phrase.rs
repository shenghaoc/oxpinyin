#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{LOGS, capture_segment_log};

use crate::support::*;

use std::ffi::CString;

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
                Some(capture_segment_log),
                std::ptr::null_mut(),
            )
        };
        // SAFETY: Same callback and lifetime; the facade domain is the
        // one `ffi::log_warning` writes.
        let facade = unsafe {
            glib_sys::g_log_set_handler(
                c"libzhuyin".as_ptr(),
                glib_sys::G_LOG_LEVEL_MASK,
                Some(capture_segment_log),
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
fn surrogate_segment_matches_pin_length_and_no_critical() {
    let (context, instance) = crate::support::open();
    let _logs = LogHandler::new();
    for bytes in [
        b"\xed\xa0\x80nihao".as_slice(),
        b"ni\xed\xa0\x80hao".as_slice(),
        b"nihao\xed\xa0\x80".as_slice(),
    ] {
        let sentence = CString::new(bytes).expect("no interior NUL");
        assert!(!unsafe { zhuyin_phrase_segment(instance, sentence.as_ptr()) });
        let mut count = 0xABCDEF;
        assert!(unsafe { zhuyin_get_n_phrase(instance, &raw mut count) });
        assert_eq!(count, 6);
        for index in 0..count {
            let mut token = 0xABCDEF;
            assert!(unsafe { zhuyin_get_phrase_token(instance, index, &raw mut token) });
            assert_eq!(token, 0);
        }
        let mut token = 0xABCDEF;
        assert!(!unsafe { zhuyin_get_phrase_token(instance, count, &raw mut token) });
        assert_eq!(token, 0);
        LOGS.with(|logs| assert!(logs.borrow().as_ref().expect("active capture").is_empty()));

        // Recover through normal lookup, then verify the invalid-byte
        // refusal keeps the prior result and logs nothing (#1297).
        assert!(unsafe { zhuyin_phrase_segment(instance, c"你好".as_ptr()) });
        assert!(!unsafe { zhuyin_phrase_segment(instance, c"\xff\xfe".as_ptr()) });
        assert!(unsafe { zhuyin_get_n_phrase(instance, &raw mut count) });
        assert_eq!(count, 2);
        LOGS.with(|logs| assert!(logs.borrow().as_ref().expect("active capture").is_empty()));
    }
    crate::support::close(context, instance);
}
