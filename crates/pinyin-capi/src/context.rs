//! Context lifecycle: `pinyin_init`, `pinyin_fini`, `pinyin_save`.

use std::os::raw::c_char;
use std::ptr;

use crate::ffi::{cstr_to_string, ffi_catch};
use crate::state::{CapiContext, box_context, context_mut};
use crate::types::PinyinContext;

/// Create a new pinyin context.
///
/// # C signature
/// ```c
/// pinyin_context_t * pinyin_init(const char * systemdir, const char * userdir);
/// ```
///
/// Opens the system dictionary and language model tables from `systemdir`.
/// Returns NULL when `systemdir` is empty or any table fails to open.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_init(
    systemdir: *const c_char,
    userdir: *const c_char,
) -> *mut PinyinContext {
    ffi_catch(ptr::null_mut(), || {
        // SAFETY: Both pointers are C strings from the caller (null OK).
        let system_dir = unsafe { cstr_to_string(systemdir) };
        let user_dir = unsafe { cstr_to_string(userdir) };
        match CapiContext::new(&system_dir, &user_dir) {
            Some(ctx) => box_context(ctx),
            None => ptr::null_mut(),
        }
    })
}

/// Finalize and free a pinyin context.
///
/// # C signature
/// ```c
/// void pinyin_fini(pinyin_context_t * context);
/// ```
///
/// Deliberately does **not** save — upstream's teardown has no flush
/// (`PYLibPinyin.cc:43-50` destroys the timer, removes the timeout source,
/// and calls only `pinyin_fini`; `focusOut` at `PYPPinyinEngine.cc:496`
/// saves nothing either). The shutdown decision is recorded in
/// `docs/findings/user-store.md` §6: pinyin-rs reproduces the call pattern,
/// and the upstream sub-timer data-loss window does not exist here because
/// every training update is a durable redb commit.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_fini(context: *mut PinyinContext) {
    if context.is_null() {
        return;
    }
    ffi_catch((), || {
        // SAFETY: `context` was created by `pinyin_init` via `box_context`
        // (= `Box::into_raw`). The caller transfers ownership back.
        unsafe {
            drop(Box::from_raw(context.cast::<CapiContext>()));
        }
    });
}

/// Save user data.
///
/// # C signature
/// ```c
/// bool pinyin_save(pinyin_context_t * context);
/// ```
///
/// The §4 semantics: `false` when there is no user directory (upstream
/// `pinyin.cpp:1133`) or nothing changed since the last save (`:1136` — the
/// unmodified deliberate no-op); `true` after a dirty save. The save
/// compacts the redb store and clears `m_modified`; durability itself is
/// redb's per-commit guarantee, so training writes are crash-safe before
/// any save is issued (`docs/findings/user-store.md` §4).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_save(context: *mut PinyinContext) -> bool {
    if context.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `context` is non-null and was produced by `pinyin_init`;
        // the unique borrow lasts only for the save call.
        let ctx = unsafe { context_mut(context) };
        ctx.save_user()
    })
}
