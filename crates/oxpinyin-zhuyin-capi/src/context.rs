//! Context lifecycle: `zhuyin_init`, `zhuyin_fini`, `zhuyin_save`.

use std::os::raw::c_char;
use std::ptr;

use crate::ffi::cstr_to_path;
use crate::state::{CapiContext, box_context, context_mut};
use crate::types::ZhuyinContext;
use oxpinyin_user::UserStore;

/// Create a new zhuyin context.
///
/// # C signature
/// ```c
/// zhuyin_context_t * zhuyin_init(const char * systemdir, const char * userdir);
/// ```
///
/// Opens the system dictionary and language model tables from `systemdir`.
/// Returns NULL when `systemdir` is empty, any table fails to open, or the
/// system dir has no parsable `interpolation2.text` real-unigram model.
/// The reason is logged through `GLib` at warning level under the
/// `libzhuyin` domain; the return value is unchanged.
///
/// **Divergence note (the pin seeds `USE_TONE | FORCE_TONE`).** The zhuyin
/// facade's context defaults to `m_options = USE_TONE | FORCE_TONE`
/// (`zhuyin.cpp:273` at 0c5e80e1 and at the 074a2219 pin), unlike `pinyin_init`, which seeds
/// only `PINYIN_INCOMPLETE`. The zhuyin parser honours `FORCE_TONE` nested
/// inside `USE_TONE` for the Simple and CP26 keyboards
/// (`zhuyin_parser2.cpp:178,602`) and unconditionally for Discrete
/// (`:373,:387`) — the same law the shared [`oxpinyin_core::ZhuyinParser`]
/// implements. If that law cannot be reproduced exactly for a keyboard, the
/// gap is registered under the existing `FORCE_TONE on double-pinyin/zhuyin
/// schemes` divergence class rather than silently absorbed.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_init(
    systemdir: *const c_char,
    userdir: *const c_char,
) -> *mut ZhuyinContext {
    // The pin's first step is `SystemTableInfo2::load` on table.conf
    // (`zhuyin.cpp:281`), which resets the process's LC_NUMERIC to "C"
    // before it can fail (`table_info.cpp:197`; register row 39), so an
    // empty or missing system dir leaves the same locale behind as a
    // successful init.
    crate::locale::pin_table_info_locale();
    // The pin keeps `g_strdup` of both arguments and opens them by those
    // bytes, so the names are paths, not text (#587). A NULL system dir
    // reads as the empty one the pin refuses first.
    let system_path = cstr_to_path(systemdir).unwrap_or_default();
    // The pin's guards test the user pointer (`zhuyin.cpp:548`, `:1697`):
    // NULL is no user dir, while "" is one — the working directory (#619) —
    // so the two stay apart.
    let user_path = cstr_to_path(userdir);
    match CapiContext::try_open(&system_path, user_path.as_deref()) {
        Ok(ctx) => {
            let handle = box_context(ctx);
            crate::live::register(handle);
            handle
        }
        Err(error) => {
            // The user marker's class-(c) refusal has its own fixed line
            // (`OpenFailure::unknown_database_format`); every other
            // failure keeps the descriptive one.
            if error.unknown_database_format() {
                crate::ffi::log_warning(oxpinyin_facade::UNKNOWN_DATABASE_FORMAT_WARNING);
            } else if crate::ffi::report_unopenable_table_conf(&system_path) {
                // The pin stops at its first step, the `table.conf` it
                // cannot open, and says so in two raw lines (#545); that is
                // the whole report.
            } else {
                crate::ffi::log_warning(&format!(
                    "zhuyin_init: {error} (systemdir {system_path:?})"
                ));
            }
            ptr::null_mut()
        }
    }
}

/// Finalize and free a zhuyin context.
///
/// # C signature
/// ```c
/// void zhuyin_fini(zhuyin_context_t * context);
/// ```
///
/// Deliberately does **not** save — upstream's teardown has no flush —
/// and writes no `user.conf` either: libzhuyin keeps no open counter of
/// its own (`zhuyin_init` only reads the marker, `zhuyin_fini` writes
/// nothing, `zhuyin.cpp:126-162`, `:741-757`).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_fini(context: *mut ZhuyinContext) {
    if context.is_null() {
        return;
    }

    crate::live::unregister(context);
    // SAFETY: `context` was created by `zhuyin_init` via `box_context`
    // (= `Box::into_raw`). The caller transfers ownership back.
    unsafe {
        drop(Box::from_raw(context.cast::<CapiContext>()));
    };
}

/// Save user data.
///
/// # C signature
/// ```c
/// bool zhuyin_save(zhuyin_context_t * context);
/// ```
///
/// The §4 semantics: `false` when there is no user directory or nothing
/// changed since the last save; `true` after a dirty save.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_save(context: *mut ZhuyinContext) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`;
    // the unique borrow lasts only for the save call.
    let ctx = unsafe { context_mut(context) };
    // Past both guards (`zhuyin.cpp:548-552`) the pin's `mark_version`
    // writes user.conf and resets LC_NUMERIC to "C" (`:695`,
    // `table_info.cpp:378`; register row 39); the two early returns do
    // not reach it.
    if ctx.core.user.as_ref().is_some_and(UserStore::is_modified) {
        crate::locale::pin_table_info_locale();
    }
    ctx.save_user()
}
