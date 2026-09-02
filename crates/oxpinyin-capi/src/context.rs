//! Context lifecycle: `pinyin_init`, `pinyin_fini`, `pinyin_save`.

use std::os::raw::c_char;
use std::ptr;

use crate::ffi::{cstr_to_owned_lossy, ffi_catch};
use crate::state::{CapiContext, box_context, context_mut};
// Only the harness-gated fixture hooks below take a shared context ref; the
// shipped build (--features shipped) does not compile them.
#[cfg(not(feature = "shipped"))]
use crate::state::context_ref;
use crate::types::PinyinContext;

fn init_context(systemdir: *const c_char, userdir: *const c_char) -> *mut PinyinContext {
    ffi_catch(ptr::null_mut(), || {
        // SAFETY: Both pointers are C strings from the caller (null OK).
        let system_path = cstr_to_owned_lossy(systemdir);
        let user_path = cstr_to_owned_lossy(userdir);
        CapiContext::new(&system_path, &user_path).map_or(ptr::null_mut(), box_context)
    })
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
/// Returns NULL when `systemdir` is empty or a required file fails to
/// open.
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
/// Not in `pinyin.h` and not part of the W8 51-symbol surface. Outside the
/// consumer union: compiled out of the shipped artifact
/// (`--features shipped`) so it exports exactly the union, per exception (d)
/// of `docs/findings/compatibility-policy.md`.
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
/// Outside the consumer union: compiled out of the shipped artifact
/// (`--features shipped`) so it exports exactly the union, per exception (d)
/// of `docs/findings/compatibility-policy.md`.
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
    ffi_catch(false, || {
        // SAFETY: `context` is non-null and was produced by `pinyin_init`.
        let ctx = unsafe { context_ref(context) };
        let Some(mut user) = ctx.user_store() else {
            return false;
        };
        let prev_text = cstr_to_owned_lossy(prev);
        let cur_text = cstr_to_owned_lossy(cur);
        let Some(prev_tok) = user.token_for_phrase(&prev_text).ok().flatten() else {
            return false;
        };
        let Some(cur_tok) = user.token_for_phrase(&cur_text).ok().flatten() else {
            return false;
        };
        user.set_bigram_count(prev_tok, cur_tok, count).is_ok()
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
/// `docs/findings/user-store.md` §6: oxpinyin reproduces the call pattern,
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
pub fn save_context(context: *mut PinyinContext) -> bool {
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

/// Save user data.
///
/// # C signature
/// ```c
/// bool pinyin_save(pinyin_context_t * context);
/// ```
///
/// Body and §4 semantics: [`save_context`].
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
