//! The PyO3 surface: `oxpinyin._native`.
//!
//! Everything here is a thin, Python-shaped wrapper over [`crate::runtime`]
//! and the public `oxpinyin-engine` session API. No engine logic lives in
//! this file — the invariant the parity tests enforce is that this layer
//! only translates representations (Rust structs ↔ Python objects) and
//! errors (`EngineError` → exceptions), never behaviour.
//!
//! Safety policy: no explicit `unsafe` appears anywhere in this crate.
//! Panics cannot cross into CPython by construction — PyO3 catches unwinding
//! at the boundary and raises `pyo3_runtime.PanicException` — and the
//! oxpinyin crates are panic-free on any input (constitution §4). Expensive
//! decodes run under `Python::detach`, with the session parked behind an
//! internal mutex so a released GIL can never expose it to two threads.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use pyo3::create_exception;
use pyo3::exceptions::{PyFileNotFoundError, PyIndexError, PyOSError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyType;

use oxpinyin_data::{DictError, InterpolationError, LmError};
use oxpinyin_engine::{
    CandidateKind, EmptyConfigSource, EngineError, KeyOutcome, Preedit, Selection,
};

use oxpinyin_engine::{CandidateList, Session};
use oxpinyin_runtime::{OpenError, Runtime, RuntimeDict, RuntimeLm};

use crate::lock::locked;

create_exception!(
    _native,
    OxpinyinError,
    pyo3::exceptions::PyRuntimeError,
    "A runtime failure inside the oxpinyin engine (backend, scoring or decode)."
);

/// Workspace version surfaced as `oxpinyin.__version__`.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Translates an open failure into the Python exception hierarchy:
/// missing paths/models → `FileNotFoundError`, unreadable ones → `OSError`,
/// corrupt data content → `ValueError`.
fn open_error(error: OpenError) -> PyErr {
    match &error {
        OpenError::Missing(_) | OpenError::ModelMissing(_) => {
            PyFileNotFoundError::new_err(error.to_string())
        }
        OpenError::Io(..) => PyOSError::new_err(error.to_string()),
        OpenError::Dict(DictError::Parse(_)) | OpenError::Lm(LmError::Parse(_)) => {
            PyValueError::new_err(error.to_string())
        }
        OpenError::Interpolation(
            InterpolationError::Parse { .. } | InterpolationError::MissingOneGram,
        ) => PyValueError::new_err(error.to_string()),
        OpenError::Dict(_) | OpenError::Lm(_) | OpenError::Interpolation(_) => {
            OxpinyinError::new_err(error.to_string())
        }
        // `OpenError` is #[non_exhaustive]; a future variant is a runtime
        // failure until this layer grows an explicit exception for it.
        _ => OxpinyinError::new_err(error.to_string()),
    }
}

/// Translates a session failure: stale indexes → `IndexError`, bad offsets →
/// `ValueError`, everything else (backend/decode) → [`OxpinyinError`].
fn engine_error(error: &EngineError) -> PyErr {
    match error {
        EngineError::CandidateIndexOutOfRange { index, len } => {
            PyIndexError::new_err(format!("candidate index {index} is out of range 0..{len}"))
        }
        EngineError::LookupOffsetPastSeparator { .. }
        | EngineError::LookupOffsetOutOfRange { .. }
        | EngineError::SelectionAnchorBeforeComposition { .. } => {
            PyValueError::new_err(error.to_string())
        }
        _ => OxpinyinError::new_err(error.to_string()),
    }
}

/// The refusal from [`crate::lock`], in Python terms.
///
/// One policy, stated once in that module and applied at every entry point
/// — mutating and reading alike: a lock poisoned by a panicking operation
/// is refused, never recovered. `#[pyclass(frozen)]` leaves no way to
/// rebuild the session in place, so a refused engine stays refused and the
/// caller opens a new one.
fn lock_error() -> PyErr {
    OxpinyinError::new_err("engine lock poisoned by a failed operation")
}

/// One opened oxpinyin engine over converted system data.
///
/// Create with a system data directory holding ``pinyin_index.redb``,
/// ``phrase_index.redb`` and ``bigram.redb``; pass ``user_dir`` to enable
/// learning. Prefer :pykeyword:`with`, or call :meth:`close`. Safe to share
/// across threads: calls serialize on an internal lock and run with the GIL
/// released.
#[pyclass(frozen)]
pub struct Engine {
    inner: Arc<Mutex<EngineInner>>,
}

impl Engine {
    fn open_with(system_dir: PathBuf, user_dir: Option<PathBuf>, fixtures: bool) -> PyResult<Self> {
        let runtime = if fixtures {
            Runtime::open_fixtures(&system_dir, user_dir.as_deref())
        } else {
            Runtime::open(&system_dir, user_dir.as_deref())
        }
        .map_err(open_error)?;
        let user = runtime.user_store();
        // Defaults-only configuration: the pinned upstream values, exactly
        // as before the shared-runtime extraction.
        let session = runtime
            .new_session(&EmptyConfigSource)
            .map_err(|error| engine_error(&error))?;
        Ok(Self {
            inner: Arc::new(Mutex::new(EngineInner { session, user })),
        })
    }

    /// Runs `f` with the session locked and the GIL released.
    ///
    /// The result must be GIL-free owned data; Python objects are built
    /// after re-acquiring the interpreter lock.
    fn with_session<T>(
        &self,
        py: Python<'_>,
        f: impl FnOnce(&mut EngineInner) -> Result<T, EngineError> + Send,
    ) -> PyResult<T>
    where
        T: Send,
    {
        let inner = Arc::clone(&self.inner);
        // `None` is `lock::locked`'s refusal: a `PyErr` cannot be built with
        // the interpreter detached, so the refusal is carried out as an
        // absent result and turned into one here.
        let outcome = py.detach(move || {
            let mut guard = locked(&inner).ok()?;
            Some(f(&mut guard))
        });
        outcome
            .ok_or_else(lock_error)?
            .map_err(|error| engine_error(&error))
    }

    fn guard(&self) -> Result<MutexGuard<'_, EngineInner>, PyErr> {
        locked(&self.inner).map_err(|_| lock_error())
    }
}

#[pymethods]
impl Engine {
    /// Opens a production engine over `system_dir`.
    ///
    /// Requires `interpolation2.text` next to the tables (the real-unigram
    /// model the pinned ranking uses); `user_dir`, when given, holds
    /// ``user_store.redb`` and enables learning.
    #[new]
    #[pyo3(signature = (system_dir, user_dir=None))]
    fn new(system_dir: PathBuf, user_dir: Option<PathBuf>) -> PyResult<Self> {
        Self::open_with(system_dir, user_dir, false)
    }

    /// Opens fixture-mode semantics like the repository's committed mini
    /// tables: when no unigram model file is present, flat counts derive
    /// from the phrase index instead. Development and tests only.
    #[classmethod]
    #[pyo3(signature = (system_dir, user_dir=None))]
    fn from_fixture_dir(
        _cls: &Bound<'_, PyType>,
        system_dir: PathBuf,
        user_dir: Option<PathBuf>,
    ) -> PyResult<Self> {
        Self::open_with(system_dir, user_dir, true)
    }

    /// Feeds one pinyin batch string and returns its candidates.
    ///
    /// This resets the composition first, so consecutive calls are
    /// independent queries — the issue-#181 workflow. Characters outside
    /// ``a-z`` and ``'`` are filtered away exactly as the native API does;
    /// input past 4096 bytes stops extending.
    fn lookup(&self, py: Python<'_>, text: &str) -> PyResult<Vec<PyCandidate>> {
        let owned = text.to_owned();
        self.with_session(py, move |inner| {
            inner.session.reset();
            inner.session.type_pinyin(&owned)?;
            Ok(snapshot(inner.session.candidates()))
        })
    }

    /// Types `text` onto the current composition without resetting.
    fn type_pinyin(&self, py: Python<'_>, text: String) -> PyResult<bool> {
        self.with_session(py, move |inner| {
            Ok(matches!(
                inner.session.type_pinyin(&text)?,
                KeyOutcome::Consumed | KeyOutcome::Commit(_)
            ))
        })
    }

    /// Chooses candidate `index` and advances (or completes) the
    /// composition.
    ///
    /// Returns ``"continued"`` while input remains, ``"completed"`` once
    /// everything is consumed.
    fn select(&self, py: Python<'_>, index: usize) -> PyResult<&'static str> {
        self.with_session(py, move |inner| {
            // `Selection` is #[non_exhaustive]: a future variant degrades to
            // `"unknown"` rather than inventing semantics for it.
            Ok(match inner.session.select(index)? {
                Selection::Continued => "continued",
                Selection::Completed => "completed",
                _ => "unknown",
            })
        })
    }

    /// Finishes the composition and returns its text.
    fn commit(&self, py: Python<'_>) -> PyResult<String> {
        self.with_session(py, |inner| inner.session.commit())
    }

    /// Discards composition and selection state.
    fn reset(&self, py: Python<'_>) -> PyResult<()> {
        self.with_session(py, |inner| {
            inner.session.reset();
            Ok(())
        })
    }

    /// Runs the n-best sentence decode for the current composition.
    ///
    /// Returns whether a lookup ran at all; rows land at the head of
    /// :attr:`candidates` and in :attr:`sentences`.
    fn guess_sentence(&self, py: Python<'_>) -> PyResult<bool> {
        self.with_session(py, |inner| inner.session.guess_sentence())
    }

    /// The decoded text of n-best row `index`, or ``None``.
    fn sentence(&self, index: usize) -> PyResult<Option<String>> {
        let guard = self.guard()?;
        let index = u8::try_from(index)
            .map_err(|_| PyValueError::new_err("sentence row index exceeds 255"))?;
        Ok(guard.session.sentence_text(index).map(str::to_owned))
    }

    /// Trains the recorded history/sentence through the user store.
    ///
    /// Mirrors the native ``pinyin_train``; refuses without a user dir.
    fn train(&self, py: Python<'_>) -> PyResult<()> {
        self.with_session(py, |inner| {
            let Some(mut user) = inner.user.clone() else {
                return Err(EngineError::UserModel(
                    "no user directory was configured for learning".to_owned(),
                ));
            };
            inner.session.train(&mut user)
        })
    }

    /// Persists user learning when anything changed; True when saved.
    fn save(&self, py: Python<'_>) -> PyResult<bool> {
        self.with_session(py, |inner| {
            let Some(mut user) = inner.user.clone() else {
                return Ok(false);
            };
            Ok(user.save().unwrap_or(false))
        })
    }

    /// Releases the underlying table handles early. Subsequent use of a
    /// closed engine simply reopens nothing: calls keep working against the
    /// shared handles until every reference drops.
    fn close(&self) {}

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    #[allow(clippy::needless_pass_by_value)]
    #[pyo3(signature = (_exc_type=None, _exc=None, _tb=None))]
    fn __exit__(
        &self,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc: Option<&Bound<'_, PyAny>>,
        _tb: Option<&Bound<'_, PyAny>>,
    ) {
    }

    /// The raw input typed so far (post-filtering).
    #[getter]
    fn input(&self) -> PyResult<String> {
        Ok(self.guard()?.session.raw_input().to_owned())
    }

    /// Whether a composition is in progress.
    #[getter]
    fn composing(&self) -> PyResult<bool> {
        Ok(self.guard()?.session.is_composing())
    }

    /// Bytes of raw input already consumed by selections.
    #[getter]
    fn composition_offset(&self) -> PyResult<usize> {
        Ok(self.guard()?.session.composition_offset())
    }

    /// Filtered parse length of the whole raw buffer.
    #[getter]
    fn parsed_len(&self) -> PyResult<usize> {
        Ok(self.guard()?.session.full_parsed_len())
    }

    /// What a shell should display: selected text plus the raw remainder.
    #[getter]
    fn preedit(&self) -> PyResult<String> {
        let preedit: Preedit = self.guard()?.session.preedit();
        Ok(preedit.text().to_owned())
    }

    /// The current candidates, best first.
    #[getter]
    fn candidates(&self) -> PyResult<Vec<PyCandidate>> {
        let guard = self.guard()?;
        Ok(snapshot(guard.session.candidates()))
    }

    /// Candidates anchored at byte `offset` of the raw input, mirroring the
    /// per-offset window the C ABI builds; does not disturb engine state.
    fn candidates_at(&self, offset: usize) -> PyResult<Vec<PyCandidate>> {
        let mut guard = self.guard()?;
        let window = guard
            .session
            .candidates_at(offset)
            .map_err(|error| engine_error(&error))?;
        Ok(snapshot(&window))
    }

    /// Available sentence rows after :meth:`guess_sentence`, best first.
    #[getter]
    fn sentences(&self) -> PyResult<Vec<String>> {
        let guard = self.guard()?;
        let mut rows = Vec::new();
        for index in 0..=u8::MAX {
            let Some(text) = guard.session.sentence_text(index) else {
                break;
            };
            rows.push(text.to_owned());
        }
        Ok(rows)
    }
}

type SharedSession = Session<RuntimeDict, RuntimeLm>;

/// State guarded by [`Engine`]'s mutex.
struct EngineInner {
    session: SharedSession,
    user: Option<oxpinyin_user::UserStore>,
}

/// Where a candidate came from, rendered for Python. `CandidateKind` is
/// `#[non_exhaustive]`; a future kind degrades to `"other"` rather than
/// inventing semantics for it.
fn kind_label(kind: CandidateKind) -> &'static str {
    match kind {
        CandidateKind::Phrase => "phrase",
        CandidateKind::Addon => "addon",
        CandidateKind::Sentence => "sentence",
        CandidateKind::Fallback => "fallback",
        _ => "other",
    }
}

/// A conversion offer from the candidate list.
#[pyclass(frozen, name = "Candidate")]
pub struct PyCandidate {
    /// The Chinese text this candidate would insert.
    #[pyo3(get)]
    pub text: String,
    /// One of ``"phrase"``, ``"addon"``, ``"sentence"``, ``"fallback"``.
    #[pyo3(get)]
    pub kind: &'static str,
    /// Pinyin keys absorbed by this candidate.
    #[pyo3(get)]
    pub consumed_keys: usize,
    /// Raw-input bytes absorbed by this candidate.
    #[pyo3(get)]
    pub consumed_bytes: usize,
    /// Decoder cost that ranked this candidate; opaque — trust list order.
    #[pyo3(get)]
    pub cost: i64,
    /// Tail rank when this is an n-best sentence row, else 0.
    #[pyo3(get)]
    pub nbest_index: u8,
}

#[pymethods]
impl PyCandidate {
    fn __repr__(&self) -> String {
        format!(
            "Candidate(text={:?}, kind={:?}, consumed_bytes={})",
            self.text, self.kind, self.consumed_bytes
        )
    }

    fn __str__(&self) -> &str {
        &self.text
    }
}

/// Copies one native candidate into its Python representation.
fn convert(candidate: &oxpinyin_engine::Candidate) -> PyCandidate {
    PyCandidate {
        text: candidate.text().to_owned(),
        kind: kind_label(candidate.kind()),
        consumed_keys: candidate.consumed_keys(),
        consumed_bytes: candidate.consumed_bytes(),
        cost: candidate.cost(),
        nbest_index: candidate.nbest_index(),
    }
}

/// Copies an ordered list window into Python representations.
fn snapshot(list: &CandidateList) -> Vec<PyCandidate> {
    list.iter().map(convert).collect()
}

/// The `oxpinyin._native` extension module.
#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Engine>()?;
    module.add_class::<PyCandidate>()?;
    module.add("OxpinyinError", module.py().get_type::<OxpinyinError>())?;
    module.add("__version__", VERSION)?;
    Ok(())
}
