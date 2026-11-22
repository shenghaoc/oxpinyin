//! Safe ownership adapter for the pin's nonshipping tests/test_zhuyin.cpp.

use std::ffi::CStr;
use std::ptr;

use oxzhuyin_capi::types::{GChar, ZhuyinContext, ZhuyinInstance};

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
        // SAFETY: system and user are live, NUL-terminated C strings.
        let context =
            unsafe { oxzhuyin_capi::context::zhuyin_init(system.as_ptr(), user.as_ptr()) };
        // SAFETY: the context is the current init result. Allocation accepts NULL.
        let instance = unsafe { oxzhuyin_capi::instance::zhuyin_alloc_instance(context) };
        Self { context, instance }
    }

    /// Parse chewing input and guess the sentence (:45-47).
    pub fn parse_guess(&mut self, input: &CStr) {
        // SAFETY: the driver owns the handles, serializes calls, and keeps the arguments live.
        unsafe { oxzhuyin_capi::parse::zhuyin_parse_more_chewings(self.instance, input.as_ptr()) };
        // SAFETY: the driver owns the handles and serializes these calls.
        unsafe { oxzhuyin_capi::sentence::zhuyin_guess_sentence(self.instance) };
    }

    /// Allocate the guessed sentence (:49-50).
    pub fn sentence(&mut self) -> Text {
        let mut text = ptr::null_mut();
        // SAFETY: the instance is owned by the driver. The string output slot is writable.
        unsafe { oxzhuyin_capi::sentence::zhuyin_get_sentence(self.instance, &mut text) };
        Text(text)
    }

    /// Train, reset and save in the driver's order (:55-57).
    pub fn train_reset_save(&mut self) {
        // SAFETY: the driver owns the handles, serializes calls, and keeps the arguments live.
        unsafe { oxzhuyin_capi::candidates::zhuyin_train(self.instance) };
        // SAFETY: the driver owns the handles and serializes these calls.
        unsafe { oxzhuyin_capi::instance::zhuyin_reset(self.instance) };
        // SAFETY: the driver owns the handles and serializes these calls.
        unsafe { oxzhuyin_capi::context::zhuyin_save(self.context) };
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        // tests/test_zhuyin.cpp:60-64 at 074a2219.
        // SAFETY: this driver releases its own handle once, with calls serialized.
        unsafe { oxzhuyin_capi::instance::zhuyin_free_instance(self.instance) };
        // SAFETY: the driver owns the handles and serializes these calls.
        unsafe { oxzhuyin_capi::config::zhuyin_mask_out(self.context, 0, 0) };
        // SAFETY: the driver still owns the context during this save.
        unsafe { oxzhuyin_capi::context::zhuyin_save(self.context) };
        // SAFETY: the driver releases its own context once, after freeing its instance.
        unsafe { oxzhuyin_capi::context::zhuyin_fini(self.context) };
    }
}
