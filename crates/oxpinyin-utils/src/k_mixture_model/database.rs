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

use super::records::{KMixtureModelArrayHeader, KMixtureModelMagicHeader, KmmBigram, KmmGram};

/// `filename` as a Rust path: on Unix the C string's bytes are the path;
/// Windows takes its UTF-8 and refuses bytes that are not UTF-8.
#[must_use]
pub fn open_path(filename: &CStr) -> Option<PathBuf> {
    oxpinyin_input::path::path_from_bytes_checked(filename.to_bytes())
}

/// A single gram that [`Database::load`] found.
pub enum Loaded {
    /// A whole single gram.
    Gram(KmmGram),
    /// A stored value shorter than the array header. The pin's `load`
    /// answers true for it, and the pin then reads outside the record
    /// wherever it uses the gram (class (b),
    /// `upstream-divergences.md#flexiblebigram-malformed-record-lengths`).
    Short,
}

/// The tools' store-error exit for a short single gram that the pin reads
/// outside of (class (b), `upstream-divergences.md#flexiblebigram-malformed-record-lengths`): one
/// stderr line, then status 1. The pin's work before that read is done.
pub fn exit_on_short_single_gram() -> ! {
    oxpinyin_input::tool::exit_on_store_error(&StoreError::ShortArrayHeader)
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

    /// `load`; `None` where the pin's `load` answers false: no object, no
    /// row or a failed read (`flexible_ngram_bdb.h:169-181`,
    /// `flexible_ngram_kyotodb.h:196-205`, `flexible_ngram_tkrzwdb.h:
    /// 204-214`). A value shorter than the array header is
    /// [`Loaded::Short`], since the pin's `load` succeeds on it. A value
    /// with a partial item tail is also `None`.
    /// Kyoto Cabinet's body read after a good size check aborts where the
    /// pin's `check_result` does (`flexible_ngram_kyotodb.h:209-210`).
    #[must_use]
    pub fn load(&self, token: u32) -> Option<Loaded> {
        match self.inner.as_ref()?.load(token) {
            Ok(gram) => gram.map(Loaded::Gram),
            Err(StoreError::ShortArrayHeader) => Some(Loaded::Short),
            // A failed record copy is a pin assert too (`tkrzwdb_utils.h:65`,
            // `:72`; `bdb_utils.h:61`, `:67`); the flexible bigram does not
            // copy, so it never reaches here.
            Err(StoreError::ReadAfterCheck | StoreError::FatalCopy) => abort(),
            Err(
                StoreError::Io(_)
                | StoreError::Backend(_)
                | StoreError::ReadOnly
                | StoreError::InvalidInput(_),
            ) => None,
        }
    }

    /// `check_result(load(…))` where the caller then uses the gram. A
    /// missing row aborts at the `check_result`. A short row takes the
    /// store-error exit ([`exit_on_short_single_gram`]), where the pin reads
    /// outside it.
    #[must_use]
    pub fn load_checked(&self, token: u32) -> KmmGram {
        match self.load(token) {
            Some(Loaded::Gram(gram)) => gram,
            Some(Loaded::Short) => exit_on_short_single_gram(),
            None => abort(),
        }
    }

    /// The pin's `store` of the gram it just loaded from `source`, which
    /// writes the loaded bytes unchanged (`merge_k_mixture_model.cpp:
    /// 142-146`); the ignored-answer form.
    pub fn copy_single_gram(&self, token: u32, source: &Self) {
        if let (Some(inner), Some(source)) = (self.inner.as_ref(), source.inner.as_ref()) {
            let _ = inner.copy_single_gram(token, source);
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

    /// `get_array_header`; `None` when there is no object, the token has no
    /// row or the read fails. Those are the pin's `false` (`!m_db`, then
    /// `flexible_ngram_bdb.h:385`, `flexible_ngram_kyotodb.h:367` and
    /// `flexible_ngram_tkrzwdb.h:382`). A record shorter than the array
    /// header aborts where the pin's assert does (`flexible_ngram_bdb.h:388`,
    /// `flexible_ngram_kyotodb.h:371`, `flexible_ngram_tkrzwdb.h:388`).
    #[must_use]
    pub fn get_array_header(&self, token: u32) -> Option<KMixtureModelArrayHeader> {
        match self.inner.as_ref()?.get_array_header(token) {
            Ok(header) => header,
            Err(StoreError::ShortArrayHeader | StoreError::FatalCopy) => abort(),
            Err(
                StoreError::Io(_)
                | StoreError::Backend(_)
                | StoreError::ReadOnly
                | StoreError::InvalidInput(_)
                | StoreError::ReadAfterCheck,
            ) => None,
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
/// not reproduced (POLICY-ACCEPTED,
/// `upstream-divergences.md#utility-check-macros-write-no-message`).
fn abort() -> ! {
    std::process::abort()
}

#[cfg(test)]
pub(crate) mod tests;
