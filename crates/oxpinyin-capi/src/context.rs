//! Context lifecycle: `pinyin_init`, `pinyin_fini`, `pinyin_save`.

use std::os::raw::c_char;
use std::ptr;

use crate::ffi::{cstr_to_owned_lossy, ffi_catch};
use crate::state::{CapiContext, UnigramSource, box_context, context_mut};
// Only the harness-gated fixture hooks below take a shared context ref; the
// shipped build (--no-default-features) does not compile them.
#[cfg(not(feature = "shipped"))]
use crate::state::context_ref;
use crate::types::PinyinContext;

fn init_context(
    systemdir: *const c_char,
    userdir: *const c_char,
    unigram_source: UnigramSource,
) -> *mut PinyinContext {
    ffi_catch(ptr::null_mut(), || {
        // SAFETY: Both pointers are C strings from the caller (null OK).
        let system_dir = cstr_to_owned_lossy(systemdir);
        let user_dir = cstr_to_owned_lossy(userdir);
        let context = match unigram_source {
            UnigramSource::RealOnly => CapiContext::new(&system_dir, &user_dir),
            UnigramSource::FlatExportForFixtures => {
                CapiContext::new_for_fixtures(&system_dir, &user_dir)
            }
        };
        match context {
            Some(ctx) => box_context(ctx),
            None => ptr::null_mut(),
        }
    })
}

/// Create a new pinyin context.
///
/// # C signature
/// ```c
/// pinyin_context_t * pinyin_init(const char * systemdir, const char * userdir);
/// ```
///
/// Opens the system dictionary and language model tables from `systemdir`.
/// Returns NULL when `systemdir` is empty, any table fails to open, or the
/// system dir has no parsable `interpolation2.text` real-unigram model.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_init(
    systemdir: *const c_char,
    userdir: *const c_char,
) -> *mut PinyinContext {
    init_context(systemdir, userdir, UnigramSource::RealOnly)
}

/// Fixture constructor for tools and Rust tests that drive the W3 mini
/// tables (no `interpolation2.text`).
///
/// Not in `pinyin.h` and not part of the W8 51-symbol surface. C tools
/// `dlsym` this name; the public [`pinyin_init`] path never takes the
/// flat-export fallback.
/// Outside the consumer union: compiled out of the shipped artifact
/// (`--features shipped`) so it exports exactly the union, per exception (d)
/// of `docs/findings/compatibility-policy.md`.
#[cfg(not(feature = "shipped"))]
#[unsafe(no_mangle)]
#[must_use]
pub extern "C" fn oxpinyin_init_for_fixtures(
    systemdir: *const c_char,
    userdir: *const c_char,
) -> *mut PinyinContext {
    init_context(systemdir, userdir, UnigramSource::FlatExportForFixtures)
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
pub(crate) fn save_context(context: *mut PinyinContext) -> bool {
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
    fn public_init_refuses_a_system_dir_without_real_unigrams() {
        let system = TempSystemDir::new("no-interpolation");
        let user = TempUserDir::new("no-interpolation-user");
        let context = pinyin_init(
            cstr(system.path.to_str().expect("UTF-8 path")).as_ptr(),
            cstr(user.path.to_str().expect("UTF-8 path")).as_ptr(),
        );
        assert!(
            context.is_null(),
            "missing interpolation2.text must fail init"
        );
    }

    #[test]
    fn public_init_refuses_an_unparsable_interpolation2_file() {
        let system = TempSystemDir::new("bad-interpolation");
        system.write("interpolation2.text", "not an interpolation model\n");
        let user = TempUserDir::new("bad-interpolation-user");
        let context = pinyin_init(
            cstr(system.path.to_str().expect("UTF-8 path")).as_ptr(),
            cstr(user.path.to_str().expect("UTF-8 path")).as_ptr(),
        );
        assert!(
            context.is_null(),
            "present-but-unparsable interpolation2.text must fail init"
        );
    }

    #[test]
    fn public_init_loads_a_parsable_interpolation2_file() {
        let system = TempSystemDir::new("good-interpolation");
        system.write(
            "interpolation2.text",
            "\\data model interpolation\n\\1-gram\n\\item 1 ok count 1\n",
        );
        let user = TempUserDir::new("good-interpolation-user");
        let context = pinyin_init(
            cstr(system.path.to_str().expect("UTF-8 path")).as_ptr(),
            cstr(user.path.to_str().expect("UTF-8 path")).as_ptr(),
        );
        assert!(!context.is_null(), "parsable model file must open");
        crate::context::pinyin_fini(context);
    }
}
