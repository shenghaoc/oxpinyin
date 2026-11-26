//! Context creation, saving, and destruction.
//!
//! [`zhuyin_init`] opens the system and user data. [`zhuyin_save`] saves
//! changed user data. [`zhuyin_fini`] ends the context's lifetime.

use std::os::raw::c_char;
use std::ptr;

use crate::ffi::borrow_or_return;
use crate::state::{CapiContext, box_context, context_mut};
use crate::types::ZhuyinContext;

// zhuyin.cpp:273, zhuyin_parser2.cpp:178,602.
/// Create a new zhuyin context.
///
/// # C signature
/// ```c
/// zhuyin_context_t * zhuyin_init(const char * systemdir, const char * userdir);
/// ```
///
/// Open the system tables in `systemdir`. An empty string selects the working directory. A null
/// `systemdir` returns null and writes `open  failed.` and `load  failed!` to stderr. An
/// ordinary required-file load failure also returns null. Release a successful result with
/// `zhuyin_fini`.
///
/// Use `userdir` for the user indexes, phrase-library changes, bigram data and `user.conf`. A
/// null pointer selects a transient store. Lookup and import still work, while training and
/// saving return false. An empty string selects the working directory, including for later file
/// operations. Init reads available profile files individually. Success does not guarantee that
/// the directory is writable.
///
/// A missing or incompatible marker can cause init to delete the configured user files before it
/// finishes opening the system data. Init checks the marker without raising its open counter or
/// rewriting it. The initial option word is `USE_TONE | FORCE_TONE`
/// (`zhuyin.cpp:126-161,270-290`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `systemdir` and `userdir` are strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_init(
    systemdir: *const c_char,
    userdir: *const c_char,
) -> *mut ZhuyinContext {
    // The pin keeps `g_strdup` of both arguments and opens them by those
    // bytes, so the names are paths, not text (#587). g_build_filename
    // stops at NULL but drops an empty element (074a2219 pinyin.cpp:335,
    // zhuyin.cpp:279): NULL names no file; "" names cwd/table.conf.
    let Some(system_path) = oxpinyin::input::filename(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(systemdir) },
    ) else {
        // `g_build_filename(NULL, "table.conf", NULL)` is `""`: the load's
        // `setlocale` and failed `fopen("")` with its `open  failed.` line,
        // then the init's `load  failed!` (`table_info.cpp:197-202`).
        let _ = oxpinyin::data::TableConf::load(c"");
        oxpinyin::data::table_conf_load_failed(None);
        return ptr::null_mut();
    };
    // The pin's guards test the user pointer (`zhuyin.cpp:548`, `:1697`):
    // NULL is no user dir, while "" is one — the working directory (#619) —
    // so the two stay apart.
    let user_path = oxpinyin::input::filename(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(userdir) },
    );
    match CapiContext::try_open(&system_path, user_path.as_deref()) {
        Ok(ctx) => {
            let handle = box_context(ctx);
            crate::live::register(handle);
            handle
        }
        Err(error) => {
            // The process aborts where the pin dies, and a refusal writes
            // the pin's line or one warning.
            error.refuse_init(
                &system_path,
                "zhuyin_init",
                |path| oxpinyin::data::table_conf_load_failed(Some(path)),
                crate::ffi::log_warning,
            );
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
///
/// A null `context` returns immediately.
///
/// # Safety
///
/// The [crate-level rules](crate#safety) for handles and serial calls apply.
/// A non-null `context` must be a live handle returned by [`zhuyin_init`].
/// This call takes ownership of the context and ends its lifetime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_fini(context: *mut ZhuyinContext) {
    // One step: the registry held the address and this call removed it, so
    // of two calls on one context only the first goes on to free it.
    if context.is_null() || !crate::live::unregister(context) {
        return;
    }

    // SAFETY: `zhuyin_init` boxed this owner and registered its address.
    // This call removed the address and takes back the box exactly once.
    let mut owner = unsafe { Box::from_raw(context.cast::<crate::state::ContextOwner>()) };
    // SAFETY: The caller serializes fini with every operation on the context,
    // its instances and its iterators, so no borrow of the retained context overlaps.
    unsafe { crate::state::owner_mut(&mut owner) }.core.finish();
    drop(owner);
}

/// Save user data.
///
/// # C signature
/// ```c
/// bool zhuyin_save(zhuyin_context_t * context);
/// ```
///
/// Returns false for a null context, a transient store, or a store with no pending save. A dirty
/// save compacts the libraries and writes the profile. A dirty save can also stop and return
/// false when a system library file cannot be mapped again, when the data is too large to write,
/// or when malformed phrase data cannot be encoded for its file. Files written earlier in that
/// save may already have changed: each library file is renamed into place as soon as it is
/// written. The session stays dirty, so a later save tries again. A completed save clears the
/// dirty flag and returns true even if a file write, a rename or the marker write failed
/// (`zhuyin.cpp:547-698`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_save(context: *mut ZhuyinContext) -> bool {
    let ctx = borrow_or_return!(context, context_mut, false);
    // Past both guards the save's `mark_version` writes user.conf through
    // `UserTableInfo::save`, which resets LC_NUMERIC to "C"
    // (`table_info.cpp:378`). The two early returns do not reach it.
    let outcome = ctx.core.save_user();
    match outcome {
        outcome @ (oxpinyin::facade::SaveOutcome::NotSaved
        | oxpinyin::facade::SaveOutcome::Saved) => outcome.is_saved(),
    }
}

#[cfg(test)]
mod tests;
