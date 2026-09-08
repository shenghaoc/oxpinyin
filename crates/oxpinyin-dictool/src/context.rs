//! RAII owner for a user-import context core.

use std::path::Path;

use oxpinyin_facade::{ContextCore, PINYIN_DEFAULT_OPTION_WORD};
use oxpinyin_user::UserStore;

/// A user-store-only [`ContextCore`] for the §9 import/export/save trio.
///
/// The same context the C ABI's migration entry points build — a user
/// store and the pinyin option word, no decode model — held as safe Rust
/// end to end: no C handles, nothing to `pinyin_fini`.
pub struct UserImportContext {
    core: ContextCore,
}

impl UserImportContext {
    /// Opens (creating when absent) the user store under `user_dir`.
    ///
    /// `None` mirrors the old `open_user_import_context` null: an empty
    /// or unopenable directory.
    pub(crate) fn open(user_dir: &Path) -> Option<Self> {
        let dir = user_dir.to_str()?;
        Some(Self {
            core: ContextCore::new_user_only(dir, PINYIN_DEFAULT_OPTION_WORD)?,
        })
    }

    /// The shared context core (export materialization, gated save).
    pub(crate) const fn core(&self) -> &ContextCore {
        &self.core
    }

    /// The mutable core, for the store-writing import path.
    pub(crate) fn core_mut(&mut self) -> &mut ContextCore {
        &mut self.core
    }

    /// The context's user store, mutably, for the add batch.
    pub(crate) fn user(&mut self) -> Option<&mut UserStore> {
        self.core.user.as_mut()
    }
}
