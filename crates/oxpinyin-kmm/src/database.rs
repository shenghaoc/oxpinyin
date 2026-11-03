//! The pin's `KMixtureModelBigram` handle with the pin's ignored-return-value
//! call sites made explicit.
//!
//! The pin's tools attach a container and then call
//! `get_magic_header`, `get_all_items`, `load`, `store`, `get_array_header`,
//! `set_array_header`, `remove` and `set_magic_header`, almost always
//! ignoring the answer. The object `attach` leaves and its answer are kept
//! apart ([`KmmBigram::attach`]): a failed attach may still leave an object
//! that the later calls reach, or none, and then the same calls answer
//! `false` without touching a file. The `*_checked` methods are the pin's
//! `check_result(…)` sites, which abort on a false answer in the reference
//! build.

use std::ffi::CStr;
use std::path::PathBuf;

use oxpinyin_store::{AttachMode, StoreError};

use crate::records::{KMixtureModelArrayHeader, KMixtureModelMagicHeader, KmmBigram, KmmGram};

/// `filename` as a Rust path: on Unix the C string's bytes are the path,
/// elsewhere the narrow name is UTF-8.
#[must_use]
pub fn open_path(filename: &CStr) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        Some(std::ffi::OsStr::from_bytes(filename.to_bytes()).into())
    }
    #[cfg(not(unix))]
    {
        filename.to_str().ok().map(Into::into)
    }
}

/// One `KMixtureModelBigram` after its `attach`.
pub struct Database {
    inner: Option<KmmBigram>,
}

impl Database {
    /// `attach(filename, flags)`. A NULL or unrepresentable name leaves no
    /// object, as the pin's `if (!dbfile) return false;` does.
    #[must_use]
    pub fn attach(filename: Option<&CStr>, mode: AttachMode) -> Self {
        let path = filename.and_then(open_path);
        Self {
            inner: Some(attached(KmmBigram::attach(path.as_deref(), mode))),
        }
    }

    /// `attach(filename, ATTACH_READWRITE | ATTACH_CREATE)`.
    #[must_use]
    pub fn open_read_write(filename: Option<&CStr>) -> Self {
        Self::attach(filename, AttachMode::ReadWriteCreate)
    }

    /// `attach(filename, ATTACH_READONLY)`.
    #[must_use]
    pub fn open_read_only(filename: Option<&CStr>) -> Self {
        Self::attach(filename, AttachMode::ReadOnly)
    }

    /// `attach`'s answer.
    #[must_use]
    pub fn attached(&self) -> bool {
        self.inner.as_ref().is_some_and(KmmBigram::attached)
    }

    /// Whether `attach` left an object (`m_db != NULL`).
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.inner.as_ref().is_some_and(KmmBigram::is_open)
    }

    /// `get_magic_header`, preserving the backend's missing/size assertions.
    #[must_use]
    pub fn get_magic_header(&self) -> Option<KMixtureModelMagicHeader> {
        self.inner.as_ref()?.get_magic_header().ok().flatten()
    }

    /// `check_result(get_magic_header(…))`.
    #[must_use]
    pub fn magic_header_checked(&self) -> KMixtureModelMagicHeader {
        match self.get_magic_header() {
            Some(header) => header,
            None => abort(),
        }
    }

    /// `set_magic_header`; the ignored-answer form.
    pub fn set_magic_header(&self, header: &KMixtureModelMagicHeader) {
        if let Some(inner) = self.inner.as_ref() {
            let _ = inner.set_magic_header(header);
        }
    }

    /// `check_result(set_magic_header(…))`.
    pub fn set_magic_header_checked(&self, header: &KMixtureModelMagicHeader) {
        let stored = self
            .inner
            .as_ref()
            .is_some_and(|inner| inner.set_magic_header(header).unwrap_or(false));
        if !stored {
            abort();
        }
    }

    /// `get_all_items`: every token key in the backend's native order. A
    /// closed handle answers an empty array, as the pin's cleared `GArray`.
    #[must_use]
    pub fn get_all_items(&self) -> Vec<u32> {
        self.inner
            .as_ref()
            .and_then(KmmBigram::get_all_items)
            .unwrap_or_default()
    }

    /// `load`; `None` when the token has no row.
    #[must_use]
    pub fn load(&self, token: u32) -> Option<KmmGram> {
        self.inner.as_ref()?.load(token).ok().flatten()
    }

    /// `check_result(load(…))`: the caller knows the row exists.
    #[must_use]
    pub fn load_checked(&self, token: u32) -> KmmGram {
        match self.load(token) {
            Some(gram) => gram,
            None => abort(),
        }
    }

    /// `store`; the ignored-answer form.
    pub fn store(&self, token: u32, gram: &KmmGram) {
        if let Some(inner) = self.inner.as_ref() {
            let _ = inner.store(token, gram);
        }
    }

    /// `check_result(store(…))`.
    pub fn store_checked(&self, token: u32, gram: &KmmGram) {
        let stored = self
            .inner
            .as_ref()
            .is_some_and(|inner| inner.store(token, gram).unwrap_or(false));
        if !stored {
            abort();
        }
    }

    /// `get_array_header`; `None` when the token has no row, and a short
    /// record aborts through the layer's assert.
    #[must_use]
    pub fn get_array_header(&self, token: u32) -> Option<KMixtureModelArrayHeader> {
        self.inner.as_ref()?.get_array_header(token).ok().flatten()
    }

    /// `check_result(get_array_header(…))`.
    #[must_use]
    pub fn array_header_checked(&self, token: u32) -> KMixtureModelArrayHeader {
        match self.get_array_header(token) {
            Some(header) => header,
            None => abort(),
        }
    }

    /// `set_array_header`; the ignored-answer form.
    pub fn set_array_header(&self, token: u32, header: &KMixtureModelArrayHeader) {
        if let Some(inner) = self.inner.as_ref() {
            let _ = inner.set_array_header(token, header);
        }
    }

    /// `check_result(set_array_header(…))`.
    pub fn set_array_header_checked(&self, token: u32, header: &KMixtureModelArrayHeader) {
        let stored = self
            .inner
            .as_ref()
            .is_some_and(|inner| inner.set_array_header(token, header).unwrap_or(false));
        if !stored {
            abort();
        }
    }

    /// `remove`; the ignored-answer form.
    pub fn remove(&self, token: u32) {
        if let Some(inner) = self.inner.as_ref() {
            let _ = inner.remove(token);
        }
    }

    /// `check_result(remove(…))`.
    pub fn remove_checked(&self, token: u32) {
        let removed = self
            .inner
            .as_ref()
            .is_some_and(|inner| inner.remove(token).unwrap_or(false));
        if !removed {
            abort();
        }
    }
}

/// The object an attach leaves. A failed native open is `Ok` on every
/// backend: an object with a false answer, or none
/// (`FlexibleBigram::attach`, `HashAttach`). An error is a failure before
/// the open, such as an unsupported library, an allocation failure or a
/// rejected path, with no pin counterpart: the utility reports it and exits
/// before it creates or writes anything, never as a missing object.
fn attached(result: Result<KmmBigram, StoreError>) -> KmmBigram {
    result.unwrap_or_else(|error| oxpinyin_input::tool::exit_on_store_error(&error))
}

/// The pin's abort at a `check_result` or `assert` site; the macro's text is
/// not reproduced (POLICY-ACCEPTED, `eio-tool-check-messages.md`).
fn abort() -> ! {
    std::process::abort()
}

#[cfg(test)]
mod tests {
    use super::attached;
    use oxpinyin_store::StoreError;

    /// Set in the child: route a store error instead of the test body.
    const CHILD: &str = "OXPINYIN_KMM_ATTACH_ERROR_CHILD";

    #[test]
    fn an_attach_error_is_never_a_missing_object() {
        if std::env::var_os(CHILD).is_some() {
            let _ = attached(Err(StoreError::Backend("unsupported library 6.2".into())));
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "database::tests::an_attach_error_is_never_a_missing_object",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .output()
            .expect("child test");
        assert_eq!(output.status.code(), Some(1), "the utility exits");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("unsupported library 6.2"), "{stderr}");
    }
}
