//! Context lifecycle: `zhuyin_init`, `zhuyin_fini`, `zhuyin_save`.

use std::os::raw::c_char;
use std::ptr;

use crate::state::{CapiContext, box_context, context_mut};
use crate::types::ZhuyinContext;

/// Create a new zhuyin context.
///
/// # C signature
/// ```c
/// zhuyin_context_t * zhuyin_init(const char * systemdir, const char * userdir);
/// ```
///
/// Opens the system dictionary and language model tables from `systemdir`.
/// An empty `systemdir` string opens data from the current working directory.
/// A NULL `systemdir` returns NULL and writes the pin's raw stderr lines
/// `open  failed.` and `load  failed!`, without a GLib warning. Required-file
/// failures also return NULL; diagnostics follow the failing load path.
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
    // The pin's guards test the user pointer (`zhuyin.cpp:548`, `:1697`):
    // NULL is no user dir, while "" is one — the working directory (#619) —
    // so the two stay apart.
    let user_path =
        oxpinyin_capi_marshal::input::filename(oxpinyin_capi_marshal::borrow_c_string!(userdir));
    match CapiContext::try_open(&system_path, user_path.as_deref()) {
        Ok(ctx) => {
            for warning in ctx.core.load_warnings() {
                crate::ffi::log_warning(warning);
            }
            let handle = box_context(ctx);
            crate::live::register(handle);
            handle
        }
        Err(error) => {
            // Register row 44, `table_info.cpp:132` via `:353-354`: the
            // pin's `check_format` loads `user.conf` first and aborts on a
            // `database format:` token it does not know, before it writes
            // or unlinks anything (`zhuyin.cpp:126-132`); the open stopped
            // at the same point.
            if error.unknown_database_format() {
                std::process::abort();
            }
            if let Some(conf_error) = error.table_conf_error()
                && table_conf_aborts(conf_error, &system_path)
            {
                std::process::abort();
            }
            if error.chunk_header_read() {
                // Class (c), `memory_chunk.h:493`: a system library is a
                // directory whose reported size passes the header check and
                // the pin's `MemoryChunk::mmap` asserts on reading it.
                crate::ffi::log_warning(
                    "zhuyin_init: a system library is a directory the chunk header cannot be \
                     read from (upstream asserts, memory_chunk.h:493)",
                );
            } else if error.corrupt_log_header() {
                // Class (c), `phrase_index_logger.h:202`: a system
                // library's user .dbin carries a non-null-token
                // MODIFY_HEADER and the pin's `next_record` assert dies
                // during init's library load.
                crate::ffi::log_warning(
                    "zhuyin_init: a system library's user .dbin carries a non-null-token \
                     MODIFY_HEADER (upstream asserts, phrase_index_logger.h:202)",
                );
            } else if let Some(conf_error) = error.table_conf_error() {
                // A `table.conf` the pin refuses is `false` and one raw
                // line (after the load's own `open %s failed.` when it
                // could not be opened, #545); one it dies on without an
                // assert or `abort()` (class (b): the missing source format
                // line, a row without a chunk where the directory does not
                // read as one) is one warning (#694, #525).
                match conf_error.warning() {
                    Some(text) => crate::ffi::log_warning(text),
                    None => oxpinyin_data::c_stderr::table_conf_load_failed(Some(&system_path)),
                }
            } else {
                crate::ffi::log_warning(&format!(
                    "zhuyin_init: {error} (systemdir {system_path:?})"
                ));
            }
            ptr::null_mut()
        }
    }
}

/// Whether the pin dies of an `abort()` or `assert` on this `table.conf`
/// refusal (register row 79): `table_info.cpp:119`, `:132`, `:142`, `:156`
/// and `:175` (`abort()`) and `:276` inside `SystemTableInfo2::load`, with
/// nothing written; `zhuyin.cpp:330` and `memory_chunk.h:434` after
/// `check_format` unlinked a non-conforming profile's files
/// (`zhuyin.cpp:134-159`; libzhuyin writes no `user.conf` there), which the
/// open has unlinked too; and `memory_chunk.h:493` for a row without a
/// system chunk where the system directory reads as one. The missing
/// source-format line (row 80) and the chunkless row on a directory that
/// does not read as a chunk are class (b) and keep their warning.
fn table_conf_aborts(
    error: oxpinyin_data::table_info::TableConfError,
    system_path: &std::path::Path,
) -> bool {
    use oxpinyin_data::table_info::{TableConfError, TableConfSite};
    match error {
        TableConfError::Abort(TableConfSite::NullSystemFile) => {
            oxpinyin_facade::null_chunk_asserts(system_path)
        }
        TableConfError::Abort(_) => true,
        TableConfError::Unopenable
        | TableConfError::Header
        | TableConfError::UninitialisedSourceFormat => false,
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
/// A second call on a context that is already finalised (or was never
/// handed out by `zhuyin_init`) returns at once: the pin deletes the freed
/// context's members again (`zhuyin.cpp:741-757`), a double free that kills
/// it with SIGSEGV (register row 105, class (b)).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_fini(context: *mut ZhuyinContext) {
    // One step: the registry held the address and this call removed it, so
    // of two calls on one context only the first goes on to free it.
    if context.is_null() || !crate::live::unregister(context) {
        return;
    }

    // SAFETY: `context` was created by `zhuyin_init` via `box_context`
    // (= `Box::into_raw`), the registry held its address and this call
    // removed it, so no other call can free it. The caller transfers
    // ownership back.
    unsafe {
        let ctx = Box::from_raw(context.cast::<CapiContext>());
        drop(ctx);
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
    // Past both guards the save's `mark_version` writes user.conf through
    // `UserTableInfo::save`, which resets LC_NUMERIC to "C"
    // (`table_info.cpp:378`; register row 39); the two early returns do
    // not reach it.
    let outcome = ctx.save_user();
    match outcome {
        // Register row 77, `memory_chunk.h:543`/`:547`: a chunk file's
        // header write failed and the pin `assert`s there. The save stopped
        // at the same point: every earlier library's `.tmp` written and
        // renamed into place (libzhuyin renames each as soon as it is
        // written, `zhuyin.cpp:610-650`), this one's created with the words
        // that were written and not renamed, no user index or bigram file
        // and no `user.conf`. The abort skips every destructor, so nothing
        // else is written either.
        oxpinyin_facade::SaveOutcome::ChunkWriteFailed(_) => std::process::abort(),
        // Class (c), `memory_chunk.h:493`: the save maps every loaded system
        // library again and one is a directory.
        oxpinyin_facade::SaveOutcome::ChunkHeaderRead => {
            crate::ffi::log_warning(
                "zhuyin_save: assertion 'ret_len == sizeof(length)' failed (memory_chunk.h:493)",
            );
            false
        }
        outcome => outcome.is_saved(),
    }
}
