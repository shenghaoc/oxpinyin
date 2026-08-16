//! RAII owner for a user-import [`PinyinContext`].

use std::path::Path;

use pinyin_capi::{PinyinContext, close_user_import_context, open_user_import_context};

/// A [`PinyinContext`] that always calls `pinyin_fini` on drop.
pub(crate) struct UserImportContext(*mut PinyinContext);

impl UserImportContext {
    pub(crate) fn open(user_dir: &Path) -> Option<Self> {
        let ptr = open_user_import_context(user_dir);
        if ptr.is_null() { None } else { Some(Self(ptr)) }
    }

    pub(crate) fn as_ptr(&self) -> *mut PinyinContext {
        self.0
    }
}

impl Drop for UserImportContext {
    fn drop(&mut self) {
        if !self.0.is_null() {
            close_user_import_context(self.0);
            self.0 = std::ptr::null_mut();
        }
    }
}
