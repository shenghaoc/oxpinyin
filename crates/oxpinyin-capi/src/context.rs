//! Context lifecycle: `pinyin_init`, `pinyin_fini`, `pinyin_save`.

use std::os::raw::c_char;
use std::ptr;

use crate::state::{CapiContext, box_context, context_mut};
use crate::types::PinyinContext;

fn init_context(systemdir: *const c_char, userdir: *const c_char) -> *mut PinyinContext {
    // The pin keeps `g_strdup` of both arguments and opens them by those
    // bytes, so the names are paths, not text (#587). g_build_filename
    // stops at NULL but drops an empty element (074a2219 pinyin.cpp:335,
    // zhuyin.cpp:279): NULL names no file; "" names cwd/table.conf.
    let Some(system_path) =
        oxpinyin_capi_marshal::input::filename(oxpinyin_capi_marshal::borrow_c_string!(systemdir))
    else {
        // `g_build_filename(NULL, "table.conf", NULL)` is `""`: the load's
        // `setlocale` and failed `fopen("")` with its `open  failed.` line,
        // then the init's `load  failed!` (`table_info.cpp:197-202`).
        let _ = oxpinyin_data::table_info::TableConf::load(c"");
        oxpinyin_data::c_stderr::table_conf_load_failed(None);
        return ptr::null_mut();
    };
    // The pin's guards test the user pointer (`pinyin.cpp:1133`, `:2671`):
    // NULL is no user dir, while "" is one — the working directory (#619) —
    // so the two stay apart.
    let user_path =
        oxpinyin_capi_marshal::input::filename(oxpinyin_capi_marshal::borrow_c_string!(userdir));
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
fn refuse_init(error: &oxpinyin_facade::OpenFailure, system_path: &std::path::Path) {
    use oxpinyin_facade::InitRefusal;
    match error.init_refusal(system_path) {
        InitRefusal::Abort => std::process::abort(),
        InitRefusal::TableConfLoadFailed => {
            oxpinyin_data::c_stderr::table_conf_load_failed(Some(system_path));
        }
        InitRefusal::TableConfWarning(message) => crate::ffi::log_warning(message),
        InitRefusal::OpenWarning => {
            crate::ffi::log_warning(&format!("pinyin_init: {error} (systemdir {system_path:?})"));
        }
    }
}

/// Create a new pinyin context.
///
/// # C signature
/// ```c
/// pinyin_context_t * pinyin_init(const char * systemdir, const char * userdir);
/// ```
///
/// Opens the system data directory from `systemdir` the way libpinyin
/// does — the pinyin and phrase DBMs, the per-library chunk files,
/// `bigram.db`, `punct.bin`, the addon DBM pair, λ from `table.conf`.
/// An empty `systemdir` string opens data from the current working directory.
/// A NULL `systemdir` returns NULL and writes the pin's raw stderr lines
/// `open  failed.` and `load  failed!`, without a GLib warning. Required-file
/// failures also return NULL; diagnostics follow the failing load path.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_init(
    systemdir: *const c_char,
    userdir: *const c_char,
) -> *mut PinyinContext {
    init_context(systemdir, userdir)
}

/// Test-only: overwrite a user-bigram successor count by phrase text.
///
/// Public `pinyin_train` first-seeds 69 (`23 * 3`), so
/// the prediction filter edge (`pinyin.cpp:2311`, `:2349-2350`) cannot be
/// reached through the C ABI. Looks up `prev` and `cur` in the user
/// phrase index.
#[cfg(test)]
pub(crate) fn oxpinyin_test_set_user_bigram(
    context: *mut PinyinContext,
    prev: *const c_char,
    cur: *const c_char,
    count: u64,
) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `pinyin_init`.
    let ctx = unsafe { context_mut(context) };
    let user = ctx.core.user_store_mut();
    let prev_text =
        oxpinyin_capi_marshal::input::scalar_text(oxpinyin_capi_marshal::borrow_c_string!(prev))
            .unwrap_or_default();
    let cur_text =
        oxpinyin_capi_marshal::input::scalar_text(oxpinyin_capi_marshal::borrow_c_string!(cur))
            .unwrap_or_default();
    let Some(prev_tok) = user.token_for_phrase(&prev_text).ok().flatten() else {
        return false;
    };
    let Some(cur_tok) = user.token_for_phrase(&cur_text).ok().flatten() else {
        return false;
    };
    user.set_bigram_count(prev_tok, cur_tok, count).is_ok()
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
/// A second call on a context that is already finalised (or was never
/// handed out by `pinyin_init`) returns at once: the pin reads and deletes
/// the freed context again (`pinyin.cpp:1194-1222`), a double free that
/// kills it with SIGSEGV (register row 105, class (b)).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_fini(context: *mut PinyinContext) {
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
/// compacts the store and clears `m_modified`. Durability is save-gated, as
/// at the pin: training reaches the user files only here, and a crash before
/// the save loses it (`docs/findings/user-store.md` §6).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_save(context: *mut PinyinContext) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `pinyin_init`;
    // the unique borrow lasts only for the save call.
    let ctx = unsafe { context_mut(context) };
    // Past both guards the save's `mark_version` writes user.conf through
    // `UserTableInfo::save`, which resets LC_NUMERIC to "C"
    // (`table_info.cpp:378`; register row 39); the two early returns do
    // not reach it.
    let outcome = ctx.core.save_user();
    match outcome {
        // Register row 77, `memory_chunk.h:543`/`:547`: a chunk file's
        // header write failed and the pin `assert`s there. The save stopped
        // at the same point: every earlier library's `.tmp` written, this
        // one's created with the words that were written, no rename and no
        // `user.conf`. The abort skips every destructor, so nothing else is
        // written either.
        oxpinyin_facade::SaveOutcome::ChunkWriteFailed(_) => std::process::abort(),
        // Register row 96, `memory_chunk.h:493`: the save's library loop
        // maps a loaded system library again and it is a directory. The
        // save stopped there with the earlier libraries' `.tmp` files
        // written and none renamed (`pinyin.cpp:940-1020`).
        oxpinyin_facade::SaveOutcome::ChunkHeaderRead => std::process::abort(),
        // `copy_tkrzwdb` / `copy_bdb` assert on a failed destination write
        // or source walk (`tkrzwdb_utils.h:65`, `:72`; `bdb_utils.h:61`,
        // `:67`). The save stopped in that copy, before its rename and the
        // marker.
        oxpinyin_facade::SaveOutcome::FatalCopy => std::process::abort(),
        outcome
        @ (oxpinyin_facade::SaveOutcome::NotSaved | oxpinyin_facade::SaveOutcome::Saved) => {
            outcome.is_saved()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::pinyin_init;
    use crate::test_support::{TempSystemDir, TempUserDir, cstr};

    /// The session could not be built once the library loop had handed it
    /// the system libraries (`OpenError::UserSession`): the pin dies at each
    /// counterpart. No input makes those steps fail, so the open's answer
    /// is handed to the init's refusal directly.
    #[test]
    fn a_user_session_fault_aborts_the_init() {
        let stderr = crate::test_support::assert_aborts(
            "context::tests::a_user_session_fault_aborts_the_init",
            || {
                let error = oxpinyin_facade::OpenFailure::Runtime(
                    oxpinyin_runtime::OpenError::UserSession(
                        oxpinyin_user::UserStoreError::SessionSeed("injected".into()),
                    ),
                );
                super::refuse_init(&error, std::path::Path::new("system"));
            },
        );
        // No Rust panic text or other line: the pin writes none here but
        // glibc's assert line, the register's policy-accepted missing line.
        assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
    }

    #[test]
    fn public_init_missing_pinyin_index_follows_backend_attach() {
        let system = TempSystemDir::new("no-dbms");
        for entry in std::fs::read_dir(&system.path).expect("dir") {
            let path = entry.expect("entry").path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with("pinyin_index") {
                std::fs::remove_file(&path).expect("remove");
            }
        }
        let user = TempUserDir::new("no-dbms-user");
        let context = pinyin_init(
            cstr(system.path.to_str().expect("UTF-8 path")).as_ptr(),
            cstr(user.path.to_str().expect("UTF-8 path")).as_ptr(),
        );
        // Every backend keeps the failed native object: the pin ignores the
        // attach's answer (pinyin.cpp:357 at 074a2219) and fini closes it.
        assert!(!context.is_null(), "the pin ignores a failed system attach");
        crate::context::pinyin_fini(context);
    }
}
