//! Context lifecycle: `pinyin_init`, `pinyin_fini`, `pinyin_save`.

use std::os::raw::c_char;
use std::ptr;

use crate::state::{CapiContext, box_context, context_mut};
// Only the harness-gated fixture hooks below take a shared context ref; the
// shipped build (--features shipped) does not compile them.
#[cfg(not(feature = "shipped"))]
use crate::state::context_ref;
use crate::types::PinyinContext;

fn init_context(systemdir: *const c_char, userdir: *const c_char) -> *mut PinyinContext {
    // The pin's first step is `SystemTableInfo2::load` on table.conf
    // (`pinyin.cpp:337`), which resets the process's LC_NUMERIC to "C"
    // before it can fail (`table_info.cpp:197`; register row 39), so an
    // empty or missing system dir leaves the same locale behind as a
    // successful init.
    crate::locale::pin_table_info_locale();
    // The pin keeps `g_strdup` of both arguments and opens them by those
    // bytes, so the names are paths, not text (#587). g_build_filename
    // stops at NULL but drops an empty element (074a2219 pinyin.cpp:335,
    // zhuyin.cpp:279): NULL names no file; "" names cwd/table.conf.
    let Some(system_path) =
        oxpinyin_capi_marshal::input::filename(oxpinyin_capi_marshal::borrow_c_string!(systemdir))
    else {
        oxpinyin_data::pin_stderr::emit(&[b"open  failed.\nload  failed!\n"]);
        return ptr::null_mut();
    };
    // The pin's guards test the user pointer (`pinyin.cpp:1133`, `:2671`):
    // NULL is no user dir, while "" is one — the working directory (#619) —
    // so the two stay apart.
    let user_path =
        oxpinyin_capi_marshal::input::filename(oxpinyin_capi_marshal::borrow_c_string!(userdir));
    match CapiContext::try_new(&system_path, user_path.as_deref()) {
        Ok(context) => {
            log_layout_faults(&context);
            for warning in context.core.load_warnings() {
                crate::ffi::log_warning(warning);
            }
            let handle = box_context(context);
            crate::live::register(handle);
            handle
        }
        Err(error) => {
            // The user marker's class-(c) refusal has its own fixed line
            // (`OpenFailure::unknown_database_format`); every other
            // failure keeps the descriptive one.
            if error.unknown_database_format() {
                crate::ffi::log_warning(oxpinyin_facade::UNKNOWN_DATABASE_FORMAT_WARNING);
            } else if error.chunk_header_read() {
                // Class (c), `memory_chunk.h:493`: a system library is a
                // directory whose reported size passes the header check and
                // the pin's `MemoryChunk::mmap` asserts on reading it.
                crate::ffi::log_warning(
                    "pinyin_init: a system library is a directory the chunk header cannot be \
                     read from (upstream asserts, memory_chunk.h:493)",
                );
            } else if error.corrupt_log_header() {
                // Class (c), `phrase_index_logger.h:202`: a system
                // library's user .dbin carries a non-null-token
                // MODIFY_HEADER and the pin's `next_record` assert dies
                // during init's library load.
                crate::ffi::log_warning(
                    "pinyin_init: a system library's user .dbin carries a non-null-token \
                     MODIFY_HEADER (upstream asserts, phrase_index_logger.h:202)",
                );
            } else if let Some(conf_error) = error.table_conf_error() {
                // A `table.conf` the pin refuses is `false` and one raw
                // line; one it dies on is one warning (#694, #525).
                match conf_error.warning() {
                    Some(text) => crate::ffi::log_warning(text),
                    None => oxpinyin_data::pin_stderr::report_rejected_table_conf(&system_path),
                }
            } else if oxpinyin_data::pin_stderr::report_unopenable_table_conf(&system_path) {
                // The pin stops at its first step, the `table.conf` it
                // cannot open, and says so in two raw lines (#545); that is
                // the whole report.
            } else {
                crate::ffi::log_warning(&format!(
                    "pinyin_init: {error} (systemdir {system_path:?})"
                ));
            }
            ptr::null_mut()
        }
    }
}

/// Logs the pin's `SubPhraseIndex::load` criticals the preceding operation
/// recorded, in the order the pin emits them.
pub(crate) fn log_layout_faults(ctx: &CapiContext) {
    for fault in ctx.take_layout_faults() {
        crate::ffi::log_critical(&fault.critical_message());
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

/// Test-tool constructor kept for the Rust suites and C tools that
/// `dlsym` this name to open the committed `fixtures/w3` mini data set.
/// It is `pinyin_init` under another name: the mini set is a real
/// (small) data directory with real counts, so there is no separate
/// fixture mode any more.
///
/// Not in `pinyin.h`: compiled out of the shipped artifact
/// (`--features shipped`) so it exports exactly the pin's own set.
#[cfg(not(feature = "shipped"))]
#[unsafe(no_mangle)]
#[must_use]
pub extern "C" fn oxpinyin_init_for_fixtures(
    systemdir: *const c_char,
    userdir: *const c_char,
) -> *mut PinyinContext {
    init_context(systemdir, userdir)
}

/// Test-only: overwrite a user-bigram successor count by phrase text.
///
/// Not in `pinyin.h`. Public `pinyin_train` first-seeds 69 (`23 * 3`), so
/// the prediction filter edge (`pinyin.cpp:2311`, `:2349-2350`) cannot be
/// reached through the C ABI. Looks up `prev` and `cur` in the user
/// phrase index.
/// Not in `pinyin.h`: compiled out of the shipped artifact
/// (`--features shipped`) so it exports exactly the pin's own set.
#[cfg(not(feature = "shipped"))]
#[unsafe(no_mangle)]
pub extern "C" fn oxpinyin_test_set_user_bigram(
    context: *mut PinyinContext,
    prev: *const c_char,
    cur: *const c_char,
    count: u64,
) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `pinyin_init`.
    let ctx = unsafe { context_ref(context) };
    let Some(mut user) = ctx.user_store() else {
        return false;
    };
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
/// user store's fini, run as the context drops. A process that never
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

    // `mark_version`'s user.conf write (`pinyin.cpp:1200`) runs on every
    // fini and resets LC_NUMERIC to "C" (`table_info.cpp:378`; register
    // row 39).
    crate::locale::pin_table_info_locale();
    // SAFETY: `context` was created by `pinyin_init` via `box_context`
    // (= `Box::into_raw`), the registry held its address and this call
    // removed it, so no other call can free it. The caller transfers
    // ownership back.
    unsafe {
        let ctx = Box::from_raw(context.cast::<CapiContext>());
        // The pin logs its layout criticals at the load, not at fini; a
        // fault an entry did not log is dropped rather than repeated here.
        let _ = ctx.take_layout_faults();
        drop(ctx);
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
/// compacts the store and clears `m_modified`; durability itself is
/// the backend's per-commit guarantee, so training writes are crash-safe before
/// any save is issued (`docs/findings/user-store.md` §4).
pub fn save_context(context: *mut PinyinContext) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `pinyin_init`;
    // the unique borrow lasts only for the save call.
    let ctx = unsafe { context_mut(context) };
    // Past both guards (`pinyin.cpp:1133-1137`) the pin's `mark_version`
    // writes user.conf and resets LC_NUMERIC to "C" (`:1143`,
    // `table_info.cpp:378`; register row 39); the two early returns do
    // not reach it.
    if ctx
        .core
        .user
        .as_ref()
        .is_some_and(|user| user.has_user_directory() && user.is_modified())
    {
        crate::locale::pin_table_info_locale();
    }
    let outcome = ctx.save_user(&mut |fault| crate::ffi::log_critical(&fault.critical_message()));
    match outcome {
        // Class (c), `memory_chunk.h:543`/`:547`: a chunk file's header
        // write failed and the pin `assert`s there. The save fails and
        // logs one line, as every class-(c) site does.
        oxpinyin_facade::SaveOutcome::ChunkWriteFailed(field) => {
            crate::ffi::log_warning(&format!(
                "pinyin_save: assertion '{}' failed",
                field.assert_expression()
            ));
            false
        }
        // Class (c), `memory_chunk.h:493`: the save maps every loaded system
        // library again and one is a directory.
        oxpinyin_facade::SaveOutcome::ChunkHeaderRead => {
            crate::ffi::log_warning(
                "pinyin_save: assertion 'ret_len == sizeof(length)' failed (memory_chunk.h:493)",
            );
            false
        }
        outcome => outcome.is_saved(),
    }
}

/// Save user data.
///
/// # C signature
/// ```c
/// bool pinyin_save(pinyin_context_t * context);
/// ```
///
/// Body and §4 semantics: `save_context`.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_save(context: *mut PinyinContext) -> bool {
    save_context(context)
}

#[cfg(test)]
mod tests {
    use super::pinyin_init;
    use crate::test_support::{TempSystemDir, TempUserDir, cstr};

    #[test]
    fn public_init_opens_a_system_data_directory() {
        let system = TempSystemDir::new("opens");
        let user = TempUserDir::new("opens-user");
        let context = pinyin_init(
            cstr(system.path.to_str().expect("UTF-8 path")).as_ptr(),
            cstr(user.path.to_str().expect("UTF-8 path")).as_ptr(),
        );
        assert!(!context.is_null(), "the fixture data directory must open");
        crate::context::pinyin_fini(context);
    }

    #[test]
    fn public_init_refuses_a_directory_without_the_dbms() {
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
        assert!(context.is_null(), "a missing pinyin index must fail init");
    }
}
