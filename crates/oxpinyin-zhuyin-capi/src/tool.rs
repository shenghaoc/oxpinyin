//! Safe ownership adapter for the pin's nonshipping tests/test_zhuyin.cpp.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use std::ffi::CStr;
use std::ptr;

use crate::types::{GChar, ZhuyinContext, ZhuyinInstance};

/// A GLib-owned sentence returned by the existing C API.
pub struct Text(*mut GChar);

impl Text {
    /// Borrow the sentence, preserving a null C API result.
    #[must_use]
    pub fn as_c_str(&self) -> Option<&CStr> {
        if self.0.is_null() {
            return None;
        }
        // SAFETY: zhuyin_get_sentence returned this owned NUL-terminated
        // allocation, which remains live until this Text is dropped.
        Some(unsafe { CStr::from_ptr(self.0) })
    }
}

impl Drop for Text {
    fn drop(&mut self) {
        // SAFETY: the C facade allocated this string for the caller;
        // this unique owner frees it once. g_free accepts null.
        unsafe { glib_sys::g_free(self.0.cast()) };
    }
}

/// One context and instance, with test_zhuyin's exact teardown sequence.
pub struct Driver {
    context: *mut ZhuyinContext,
    instance: *mut ZhuyinInstance,
}

impl Driver {
    /// Initialize the supplied directories and allocate an instance (:35-38).
    #[must_use]
    pub fn new(system: &CStr, user: &CStr) -> Self {
        let context = unsafe { crate::zhuyin_init(system.as_ptr(), user.as_ptr()) };
        let instance = unsafe { crate::instance::zhuyin_alloc_instance(context) };
        Self { context, instance }
    }

    /// Parse chewing input and guess the sentence (:45-47).
    pub fn parse_guess(&mut self, input: &CStr) {
        unsafe { crate::parse::zhuyin_parse_more_chewings(self.instance, input.as_ptr()) };
        unsafe { crate::zhuyin_guess_sentence(self.instance) };
    }

    /// Allocate the guessed sentence (:49-50).
    pub fn sentence(&mut self) -> Text {
        let mut text = ptr::null_mut();
        unsafe { crate::zhuyin_get_sentence(self.instance, &mut text) };
        Text(text)
    }

    /// Train, reset and save in the driver's order (:55-57).
    pub fn train_reset_save(&mut self) {
        unsafe { crate::zhuyin_train(self.instance) };
        unsafe { crate::instance::zhuyin_reset(self.instance) };
        unsafe { crate::zhuyin_save(self.context) };
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        // tests/test_zhuyin.cpp:60-64 at 074a2219.
        unsafe { crate::instance::zhuyin_free_instance(self.instance) };
        unsafe { crate::config::zhuyin_mask_out(self.context, 0, 0) };
        unsafe { crate::zhuyin_save(self.context) };
        unsafe { crate::zhuyin_fini(self.context) };
    }
}
