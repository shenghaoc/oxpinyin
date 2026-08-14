//! User data persistence: `pinyin_remember_user_input`.

use std::os::raw::{c_char, c_int};

use crate::types::PinyinInstance;

/// Remember a user-provided phrase with its current pinyin context.
///
/// # C signature
/// ```c
/// bool pinyin_remember_user_input(pinyin_instance_t * instance,
///                                 const char * phrase,
///                                 gint count);
/// ```
///
/// `count` of -1 means use the default value.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_remember_user_input(
    instance: *mut PinyinInstance,
    _phrase: *const c_char,
    _count: c_int,
) -> bool {
    if instance.is_null() {
        return false;
    }
    // STUB: T4 will implement.
    false
}
