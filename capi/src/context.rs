//! Context lifecycle: `pinyin_init`, `pinyin_fini`, `pinyin_save`.

use std::os::raw::c_char;
use std::ptr;

use crate::ffi::borrow_or_return;
use crate::state::{CapiContext, box_context, context_mut};
use crate::types::PinyinContext;

/// # Safety
///
/// Under the crate-level rules, Strings: both pointers are NULL or live
/// NUL-terminated strings, unchanged for this call.
unsafe fn init_context(systemdir: *const c_char, userdir: *const c_char) -> *mut PinyinContext {
    // The pin keeps `g_strdup` of both arguments and opens them by those
    // bytes, so the names are paths, not text (#587). g_build_filename
    // stops at NULL but drops an empty element (074a2219 pinyin.cpp:335,
    // zhuyin.cpp:279): NULL names no file; "" names cwd/table.conf.
    let Some(system_path) = oxpinyin::input::path::filename(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(systemdir) },
    ) else {
        // `g_build_filename(NULL, "table.conf", NULL)` is `""`: the load's
        // `setlocale` and failed `fopen("")` with its `open  failed.` line,
        // then the init's `load  failed!` (`table_info.cpp:197-202`).
        let _ = oxpinyin::data::table_info::TableConf::load(c"");
        oxpinyin::data::c_stderr::table_conf_load_failed(None);
        return ptr::null_mut();
    };
    // The pin's guards test the user pointer (`pinyin.cpp:1133`, `:2671`):
    // NULL is no user dir, while "" is one — the working directory (#619) —
    // so the two stay apart.
    let user_path = oxpinyin::input::path::filename(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(userdir) },
    );
    match CapiContext::try_new(&system_path, user_path.as_deref()) {
        Ok(context) => {
            let handle = box_context(context);
            crate::live::register(handle);
            handle
        }
        Err(error) => {
            refuse_init(&error, &system_path);
            ptr::null_mut()
        }
    }
}

/// `pinyin_init`'s answer for a failed open, before it returns NULL: the
/// process aborts where the pin dies, and a refusal writes the pin's line or
/// one warning.
fn refuse_init(error: &oxpinyin::facade::OpenFailure, system_path: &std::path::Path) {
    error.refuse_init(
        system_path,
        "pinyin_init",
        |path| oxpinyin::data::c_stderr::table_conf_load_failed(Some(path)),
        crate::ffi::log_warning,
    );
}

/// Create a new pinyin context.
///
/// # C signature
/// ```c
/// pinyin_context_t * pinyin_init(const char * systemdir, const char * userdir);
/// ```
///
/// Open the system tables in `systemdir`. An empty string selects the working directory. A null
/// `systemdir` returns null and writes `open  failed.` and `load  failed!` to stderr. An
/// ordinary required-file load failure also returns null. Release a successful result with
/// `pinyin_fini`.
///
/// Use `userdir` for the user indexes, phrase-library changes, bigram data and `user.conf`. A
/// null pointer selects a transient store. Lookup and import still work, while training and
/// saving return false. An empty string selects the working directory, including for later file
/// operations. Init reads available profile files individually. Success does not guarantee that
/// the directory is writable.
///
/// A missing or incompatible marker can cause init to delete the configured user files before it
/// finishes opening the system data. Init raises the profile open counter and attempts to write
/// the marker before cleanup. The initial option word is `USE_TONE`
/// (`pinyin.cpp:172-217,326-346`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `systemdir` and `userdir` are strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_init(
    systemdir: *const c_char,
    userdir: *const c_char,
) -> *mut PinyinContext {
    // SAFETY: The entry point guarantees Strings: both pointers are NULL or live NUL-terminated
    // strings, unchanged for this call.
    unsafe { init_context(systemdir, userdir) }
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
/// `docs/findings/user-store.md` §6: oxpinyin reproduces the call pattern.
///
/// It does write `user.conf`, as the pin's does: the open counter
/// `pinyin_init` raised is lowered (`counter > 1 ? counter - 1 : 0`) and
/// the marker re-written, saved or not (`pinyin.cpp:1194-1200`) — the
/// user store's fini, run before releasing the public context handle. A process that never
/// calls this leaves the raised counter behind, and the init that reads
/// it past the limit wipes the profile, exactly as upstream's does.
///
/// A null `context` returns immediately.
///
/// # Safety
///
/// The [crate-level rules](crate#safety) for handles and serial calls apply.
/// A non-null `context` must be a live handle returned by [`pinyin_init`].
/// This call takes ownership of the context and ends its lifetime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_fini(context: *mut PinyinContext) {
    // One step: the registry held the address and this call removed it, so
    // of two calls on one context only the first goes on to free it.
    if context.is_null() || !crate::live::unregister(context) {
        return;
    }

    // `mark_version`'s user.conf write (`pinyin.cpp:1200`) runs when the
    // user store is finalised, through `UserTableInfo::save`, which resets
    // LC_NUMERIC to "C" (`table_info.cpp:378`; register row 39).
    // SAFETY: `context` was created by `pinyin_init` via `box_context`
    // (= `Box::into_raw`), the registry held its address and this call
    // removed it, so no other call can free it. The caller transfers
    // ownership back. The C caller serializes this call with all operations
    // on its instances and iterators, so the retained allocation is exclusively
    // borrowed for fini even when other handles still own it.
    unsafe {
        let mut owner = Box::from_raw(context.cast::<crate::state::ContextOwner>());
        crate::state::owner_mut(&mut owner).core.finish();
        drop(owner);
    };
}

// pinyin.cpp:1133.
/// Save user data.
///
/// # C signature
/// ```c
/// bool pinyin_save(pinyin_context_t * context);
/// ```
///
/// Returns false for a null context, a transient store, or a store with no pending save. A dirty
/// save compacts the libraries and writes the profile. A dirty save can also stop and return
/// false when a system library file cannot be mapped again, when the data is too large to write,
/// or when malformed phrase data cannot be encoded for its file. Files written earlier in that
/// save may already have changed: their temporary files stay in the user directory. The session
/// stays dirty, so a later save tries again. A completed save clears the dirty flag and returns
/// true even if a file write, a rename or the marker write failed (`pinyin.cpp:922-1146`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_save(context: *mut PinyinContext) -> bool {
    let ctx = borrow_or_return!(context, context_mut, false);
    // Past both guards the save's `mark_version` writes user.conf through
    // `UserTableInfo::save`, which resets LC_NUMERIC to "C"
    // (`table_info.cpp:378`; register row 39); the two early returns do
    // not reach it.
    let outcome = ctx.core.save_user();
    match outcome {
        outcome @ (oxpinyin::facade::SaveOutcome::NotSaved
        | oxpinyin::facade::SaveOutcome::Saved) => outcome.is_saved(),
    }
}

#[cfg(test)]
mod tests;
