//! The pin's `KMixtureModelBigram` handle with the pin's ignored-return-value
//! call sites made explicit.
//!
//! The pin's tools attach a container and then call
//! `get_magic_header`, `get_all_items`, `load`, `store`, `get_array_header`,
//! `set_array_header`, `remove` and `set_magic_header`, almost always
//! ignoring the answer. A failed attach leaves `m_db` NULL and the same
//! calls answer `false` without touching a file; this type keeps that shape
//! as an `Option`, and the `*_checked` methods are the pin's
//! `check_result(…)` sites, which abort on a false answer in the reference
//! build.

use std::ffi::CStr;
use std::path::PathBuf;

use crate::records::{
    KMixtureModelArrayHeader, KMixtureModelMagicHeader, KmmBigram, KmmGram,
};

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

/// One attached `KMixtureModelBigram`, or the pin's NULL handle.
pub struct Database {
    inner: Option<KmmBigram>,
}

impl Database {
    /// `attach(filename, ATTACH_READWRITE | ATTACH_CREATE)`. A NULL name, or
    /// an attach the layer refuses, is the pin's closed handle.
    #[must_use]
    pub fn open_read_write(filename: Option<&CStr>) -> Self {
        let inner = filename
            .and_then(open_path)
            .and_then(|path| KmmBigram::open(&path).ok());
        Self { inner }
    }

    /// `attach(filename, ATTACH_READONLY)`.
    #[must_use]
    pub fn open_read_only(filename: Option<&CStr>) -> Self {
        let inner = filename
            .and_then(open_path)
            .and_then(|path| KmmBigram::open_read_only(&path).ok());
        Self { inner }
    }

    /// Whether a handle is attached (`m_db != NULL`).
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.inner.is_some()
    }

    /// `get_magic_header`; `None` on a missing or short record.
    #[must_use]
    pub fn get_magic_header(&self) -> Option<KMixtureModelMagicHeader> {
        let inner = self.inner.as_ref()?;
        match inner.get_magic_header() {
            Ok(header) => header,
            Err(_) => abort(),
        }
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
        match self.inner.as_ref()?.load(token) {
            Ok(gram) => gram,
            Err(_) => abort(),
        }
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
        let inner = self.inner.as_ref()?;
        match inner.get_array_header(token) {
            Ok(header) => header,
            Err(_) => abort(),
        }
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

/// The pin's abort at a `check_result` or `assert` site; the macro's text is
/// not reproduced (POLICY-ACCEPTED, `eio-tool-check-messages.md`).
fn abort() -> ! {
    std::process::abort()
}
