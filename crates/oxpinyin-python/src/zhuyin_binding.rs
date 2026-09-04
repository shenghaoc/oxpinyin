//! The PyO3 surface: `oxpinyin._native.zhuyin`.
//!
//! The same invariant as [`crate::binding`]: no engine logic lives in this
//! file — every function is a pure representation/error translation over
//! [`crate::zhuyin::ZhuyinFacade`] (Rust structs ↔ Python objects,
//! `EngineError` → exceptions), never behaviour. The facade owns the zhuyin
//! law; this layer only shapes it for Python, reusing the crate's shared
//! translators (`open_error`, `engine_error`, `with_locked`, `kind_label`)
//! so the two engines report identical failures identically.
//!
//! Safety policy: no explicit `unsafe` appears anywhere in this crate (see
//! [`crate::binding`] for the boundary argument). Expensive decodes run
//! under `Python::detach` through [`with_locked`](crate::binding::with_locked),
//! with the facade parked behind an internal mutex so a released GIL can
//! never expose it to two threads.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyModule, PyType};

use oxpinyin_core::ChewingKey;
use oxpinyin_engine::{Config, EngineError};
use oxpinyin_runtime::Runtime;

use crate::binding::{engine_error, kind_label, open_error, with_locked};
use crate::zhuyin::{
    ZhuyinCandidate as FacadeCandidate, ZhuyinFacade, chewing_scheme_from_value,
    chewing_scheme_value, dvorak_scheme_message, full_scheme_from_value, full_scheme_value,
    in_keyboard_arity_message, unknown_chewing_scheme_message, unknown_full_scheme_message,
};

/// The `oxpinyin._native.zhuyin` extension submodule.
///
/// Built with [`PyModule::new`] and attached via `add_submodule` (the
/// `#[pymodule] fn` form would additionally export a dead `PyInit_zhuyin`
/// entry point). The `sys.modules` insert is what makes
/// `from oxpinyin._native.zhuyin import Engine` resolve: `add_submodule`
/// alone only sets the parent attribute.
pub(crate) fn init_submodule(py: Python<'_>) -> PyResult<Bound<'_, PyModule>> {
    let module = PyModule::new(py, "oxpinyin._native.zhuyin")?;
    module.add_class::<ZhuyinEngine>()?;
    module.add_class::<ZhuyinCandidate>()?;
    module.add_class::<PyChewingKey>()?;
    py.import("sys")?
        .getattr("modules")?
        .set_item("oxpinyin._native.zhuyin", &module)?;
    Ok(module)
}

/// One opened zhuyin engine over libpinyin system data.
///
/// Create with a system data directory in the compiled-in backend's layout;
/// pass ``user_dir`` to enable learning. Usable as a :pykeyword:`with`
/// block, though nothing needs releasing — see :meth:`close`.
///
/// The chewing facade over the same session assembly `Engine` uses:
/// keystrokes arrive through a chewing keyboard (bopomofo) or full pinyin,
/// parsed by the pinned scheme tables into the decoder. Seeded like
/// `zhuyin_init`: the `USE_TONE | FORCE_TONE` option word, the Standard
/// keyboard, the Hanyu full-pinyin scheme.
///
/// Shareable across threads one call at a time: every call takes an internal
/// lock, so a single call is atomic, but a *sequence* of calls is not — the
/// lock is released between them, and another thread's call can land in the
/// gap. Code that needs several members to agree must hold its own lock
/// around the sequence, or give each thread its own engine. Decoding calls
/// release the GIL while they run.
#[pyclass(frozen, name = "Engine")]
pub struct ZhuyinEngine {
    inner: Arc<Mutex<ZhuyinFacade>>,
}

impl ZhuyinEngine {
    fn open_with(system_dir: PathBuf, user_dir: Option<PathBuf>) -> PyResult<Self> {
        let runtime = Runtime::open(&system_dir, user_dir.as_deref()).map_err(open_error)?;
        // The zhuyin facade's layered configuration: the pinned upstream
        // defaults, exactly as `oxpinyin-zhuyin-capi` constructs them.
        let session = runtime
            .new_session(&Config::default())
            .map_err(|error| engine_error(&error))?;
        Ok(Self {
            inner: Arc::new(Mutex::new(ZhuyinFacade::wrap(&runtime, session))),
        })
    }
}

#[pymethods]
impl ZhuyinEngine {
    /// Opens a production engine over `system_dir`.
    ///
    /// `user_dir`, when given, holds ``user_store.<ext>`` (the compiled-in
    /// backend's format) and enables learning.
    #[new]
    #[pyo3(signature = (system_dir, user_dir=None))]
    fn new(system_dir: PathBuf, user_dir: Option<PathBuf>) -> PyResult<Self> {
        Self::open_with(system_dir, user_dir)
    }

    /// Opens the engine from a system data directory. Identical to
    /// `new()` — the mini fixture set is a real (small) data directory
    /// now. Kept for symmetry with `oxpinyin.Engine.from_fixture_dir`.
    #[classmethod]
    #[pyo3(signature = (system_dir, user_dir=None))]
    fn from_fixture_dir(
        _cls: &Bound<'_, PyType>,
        system_dir: PathBuf,
        user_dir: Option<PathBuf>,
    ) -> PyResult<Self> {
        Self::open_with(system_dir, user_dir)
    }

    /// Feeds one chewing batch string and returns its candidates.
    ///
    /// This resets the composition first, so consecutive calls are
    /// independent queries — the issue-#181 workflow in chewing
    /// coordinates. `text` holds the keyboard's keystrokes (Standard: `s`
    /// is ㄋ, `u` is ㄧ, …); tones ride the keyboard's tone keys.
    fn lookup_chewing(&self, py: Python<'_>, text: &str) -> PyResult<Vec<ZhuyinCandidate>> {
        let owned = text.to_owned();
        with_locked(py, &self.inner, move |facade| {
            facade.reset();
            let _ = facade.parse_chewing(&owned);
            let _ = facade.guess_candidates(0, false);
            Ok(snapshot(facade.candidates()))
        })
    }

    /// Feeds one full-pinyin batch string and returns its candidates.
    ///
    /// Same reset-first shape as :meth:`lookup_chewing`, through the live
    /// full-pinyin scheme (Hanyu by default; Luoma and SecondaryZhuyin
    /// parse through their pinned indexes).
    fn lookup_full_pinyin(&self, py: Python<'_>, text: &str) -> PyResult<Vec<ZhuyinCandidate>> {
        let owned = text.to_owned();
        with_locked(py, &self.inner, move |facade| {
            facade.reset();
            let _ = facade.parse_full_pinyin(&owned);
            let _ = facade.guess_candidates(0, false);
            Ok(snapshot(facade.candidates()))
        })
    }

    /// Batch-parses chewing keystrokes onto the current composition without
    /// resetting — the incremental `zhuyin_parse_more_chewings` shape.
    /// Returns the input bytes consumed.
    fn parse_chewing(&self, py: Python<'_>, text: String) -> PyResult<usize> {
        with_locked(py, &self.inner, move |facade| {
            Ok(facade.parse_chewing(&text))
        })
    }

    /// Batch-parses full pinyin onto the current composition without
    /// resetting — the `zhuyin_parse_more_full_pinyins` shape. Returns the
    /// input bytes consumed.
    fn parse_full_pinyin(&self, py: Python<'_>, text: String) -> PyResult<usize> {
        with_locked(py, &self.inner, move |facade| {
            Ok(facade.parse_full_pinyin(&text))
        })
    }

    /// Rebuilds the candidate snapshot at byte `offset` of the original
    /// input and returns whether the lookup ran.
    ///
    /// `offset` is in original keystroke coordinates. With
    /// `before_cursor` false (the default) the window holds spans starting
    /// at the offset; with true, spans ending at it. Read the rows through
    /// :attr:`candidates`.
    #[pyo3(signature = (offset=0, before_cursor=false))]
    fn guess_candidates(
        &self,
        py: Python<'_>,
        offset: usize,
        before_cursor: bool,
    ) -> PyResult<bool> {
        with_locked(py, &self.inner, move |facade| {
            Ok(facade.guess_candidates(offset, before_cursor))
        })
    }

    /// Chooses snapshot row `index` and returns the new cursor in original
    /// input coordinates.
    ///
    /// A stale index raises `IndexError`, exactly like `Engine.select`.
    fn select(&self, py: Python<'_>, index: usize) -> PyResult<usize> {
        with_locked(py, &self.inner, move |facade| facade.choose(index))
    }

    /// Clears the constraint a prior selection pinned at `offset`
    /// (original coordinates). False for a free cell.
    fn clear_constraint(&self, py: Python<'_>, offset: usize) -> PyResult<bool> {
        with_locked(py, &self.inner, move |facade| {
            Ok(facade.clear_constraint(offset))
        })
    }

    /// Probes one chewing keystroke string: the parsed key, or ``None``
    /// when the live keyboard does not parse it.
    fn parse_one_chewing(&self, py: Python<'_>, text: &str) -> PyResult<Option<PyChewingKey>> {
        let owned = text.to_owned();
        with_locked(py, &self.inner, move |facade| {
            Ok(facade.parse_one_chewing(&owned).map(PyChewingKey::from))
        })
    }

    /// Probes one full-pinyin spelling: the parsed key, or ``None`` when it
    /// does not parse.
    fn parse_one_full_pinyin(&self, py: Python<'_>, text: &str) -> PyResult<Option<PyChewingKey>> {
        let owned = text.to_owned();
        with_locked(py, &self.inner, move |facade| {
            Ok(facade.parse_one_full_pinyin(&owned).map(PyChewingKey::from))
        })
    }

    /// The zhuyin symbol(s) one keystroke maps to on the live keyboard, or
    /// ``[]`` when the key is not on it.
    ///
    /// `key` must be a single character (the C ABI's plain `char`); anything
    /// else raises `ValueError`.
    fn in_keyboard(&self, py: Python<'_>, key: &str) -> PyResult<Vec<String>> {
        let bytes = key.as_bytes();
        let &[byte] = bytes else {
            return Err(PyValueError::new_err(in_keyboard_arity_message()));
        };
        with_locked(py, &self.inner, move |facade| Ok(facade.in_keyboard(byte)))
    }

    /// Renders the key's pinyin spelling under the live full-pinyin scheme,
    /// or ``None`` for the zero/invalid key.
    fn pinyin_string(&self, py: Python<'_>, key: &PyChewingKey) -> PyResult<Option<String>> {
        let key = key.inner;
        with_locked(py, &self.inner, move |facade| {
            Ok(facade.key_pinyin_string(key))
        })
    }

    /// Renders the key's zhuyin spelling, or ``None`` for the zero/invalid
    /// key.
    fn zhuyin_string(&self, py: Python<'_>, key: &PyChewingKey) -> PyResult<Option<String>> {
        let key = key.inner;
        with_locked(py, &self.inner, move |facade| {
            Ok(facade.key_zhuyin_string(key))
        })
    }

    /// Finishes the composition and returns its text.
    fn commit(&self, py: Python<'_>) -> PyResult<String> {
        with_locked(py, &self.inner, |facade| facade.commit())
    }

    /// Discards composition, selection, and parse state.
    fn reset(&self, py: Python<'_>) -> PyResult<()> {
        with_locked(py, &self.inner, |facade| {
            facade.reset();
            Ok(())
        })
    }

    /// Runs the n-best sentence decode for the current composition.
    ///
    /// Returns whether a lookup ran at all; rows land in :attr:`sentences`
    /// (and, collapsed onto the 1-best row, at the head of
    /// :attr:`candidates` after the next guess).
    fn guess_sentence(&self, py: Python<'_>) -> PyResult<bool> {
        with_locked(py, &self.inner, |facade| facade.guess_sentence())
    }

    /// Runs the sentence decode seeded with the phrases `prefix` names.
    fn guess_sentence_with_prefix(&self, py: Python<'_>, prefix: String) -> PyResult<bool> {
        with_locked(py, &self.inner, move |facade| {
            facade.guess_sentence_with_prefix(&prefix)
        })
    }

    /// The decoded text of n-best row `index`, or ``None``.
    fn sentence(&self, py: Python<'_>, index: usize) -> PyResult<Option<String>> {
        let index = u8::try_from(index)
            .map_err(|_| PyValueError::new_err("sentence row index exceeds 255"))?;
        with_locked(py, &self.inner, move |facade| {
            Ok(facade.sentence_text(index).map(str::to_owned))
        })
    }

    /// Trains the recorded history/sentence through the user store.
    ///
    /// Mirrors the native ``zhuyin_train``; refuses without a user dir.
    fn train(&self, py: Python<'_>) -> PyResult<()> {
        with_locked(py, &self.inner, |facade| {
            let Some(mut user) = facade.user() else {
                return Err(EngineError::UserModel(
                    "no user directory was configured for learning".to_owned(),
                ));
            };
            facade.train(&mut user)
        })
    }

    /// Persists user learning when anything changed; True when saved,
    /// False when there is no user store to save. A store-level failure
    /// propagates as :class:`OxpinyinError` rather than a silent False.
    fn save(&self, py: Python<'_>) -> PyResult<bool> {
        with_locked(py, &self.inner, |facade| {
            let Some(mut user) = facade.user() else {
                return Ok(false);
            };
            user.save()
                .map_err(|error| EngineError::UserModel(error.to_string()))
        })
    }

    /// Does nothing. Kept so that :pykeyword:`with` blocks and
    /// explicit-cleanup call styles both work on an engine.
    ///
    /// There is no early release to perform: the table handles are shared
    /// and reference-counted, and they drop when the last reference to them
    /// does. A "closed" engine is therefore still a working engine — calls
    /// after :meth:`close` behave exactly as calls before it.
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

    /// The live chewing keyboard as its C discriminant: 1 Standard, 2 Hsu,
    /// 3 Ibm, 4 Ginyieh, 5 Eten, 6 Eten26, 8 HsuDvorak, 9 DachenCp26.
    /// 7 (StandardDvorak) is unimplemented upstream-side and never reads
    /// back.
    #[getter]
    fn chewing_scheme(&self, py: Python<'_>) -> PyResult<u8> {
        with_locked(py, &self.inner, |facade| {
            Ok(chewing_scheme_value(facade.chewing_scheme()))
        })
    }

    /// Selects the chewing keyboard by C discriminant. Unknown values and
    /// the unimplemented StandardDvorak (7) raise `ValueError`.
    #[setter]
    fn set_chewing_scheme(&self, py: Python<'_>, scheme: u8) -> PyResult<()> {
        let scheme = chewing_scheme_from_value(scheme)
            .ok_or_else(|| PyValueError::new_err(unknown_chewing_scheme_message(scheme)))?;
        let accepted = with_locked(py, &self.inner, move |facade| {
            Ok(facade.set_chewing_scheme(scheme))
        })?;
        if !accepted {
            return Err(PyValueError::new_err(dvorak_scheme_message()));
        }
        Ok(())
    }

    /// The live full-pinyin scheme as its C discriminant: 1 Hanyu, 2 Luoma,
    /// 3 SecondaryZhuyin.
    #[getter]
    fn full_pinyin_scheme(&self, py: Python<'_>) -> PyResult<u8> {
        with_locked(py, &self.inner, |facade| {
            Ok(full_scheme_value(facade.full_scheme()))
        })
    }

    /// Selects the full-pinyin scheme by C discriminant. Anything outside
    /// 1..=3 raises `ValueError`.
    #[setter]
    fn set_full_pinyin_scheme(&self, py: Python<'_>, scheme: u8) -> PyResult<()> {
        let scheme = full_scheme_from_value(scheme)
            .ok_or_else(|| PyValueError::new_err(unknown_full_scheme_message(scheme)))?;
        with_locked(py, &self.inner, move |facade| {
            facade.set_full_scheme(scheme);
            Ok(())
        })
    }

    /// The original keystroke string of the active parse, else the
    /// session's raw buffer.
    #[getter]
    fn input(&self, py: Python<'_>) -> PyResult<String> {
        with_locked(py, &self.inner, |facade| Ok(facade.input().to_owned()))
    }

    /// Whether a composition is in progress.
    #[getter]
    fn composing(&self, py: Python<'_>) -> PyResult<bool> {
        with_locked(py, &self.inner, |facade| Ok(facade.is_composing()))
    }

    /// Bytes of session input already consumed by selections, in session
    /// (joined-pinyin) coordinates.
    #[getter]
    fn composition_offset(&self, py: Python<'_>) -> PyResult<usize> {
        with_locked(py, &self.inner, |facade| Ok(facade.composition_offset()))
    }

    /// Bytes of original input consumed by the most recent parse call.
    #[getter]
    fn parsed_len(&self, py: Python<'_>) -> PyResult<usize> {
        with_locked(py, &self.inner, |facade| Ok(facade.parsed_len()))
    }

    /// What a shell should display: selected text plus the raw remainder.
    #[getter]
    fn preedit(&self, py: Python<'_>) -> PyResult<String> {
        with_locked(py, &self.inner, |facade| Ok(facade.preedit()))
    }

    /// The last snapshot built by :meth:`guess_candidates` (or the last
    /// lookup), best first.
    #[getter]
    fn candidates(&self, py: Python<'_>) -> PyResult<Vec<ZhuyinCandidate>> {
        with_locked(py, &self.inner, |facade| Ok(snapshot(facade.candidates())))
    }

    /// Available sentence rows after :meth:`guess_sentence`, best first.
    #[getter]
    fn sentences(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        with_locked(py, &self.inner, |facade| {
            let mut rows = Vec::new();
            for index in 0..=u8::MAX {
                let Some(text) = facade.sentence_text(index) else {
                    break;
                };
                rows.push(text.to_owned());
            }
            Ok(rows)
        })
    }
}

/// A conversion offer from the candidate snapshot.
///
/// `candidate_type` is the zhuyin-local list tag — ``"best_match"`` for the
/// sentence row, ``"normal_after_cursor"`` / ``"normal_before_cursor"`` for
/// the guess direction's rows — never the pinyin eight-value tag its
/// discriminants collide with at 3 and 4.
#[pyclass(frozen, name = "Candidate")]
pub struct ZhuyinCandidate {
    /// The Chinese text this candidate would insert.
    #[pyo3(get)]
    pub text: String,
    /// One of ``"phrase"``, ``"addon"``, ``"sentence"``, ``"fallback"``.
    #[pyo3(get)]
    pub kind: &'static str,
    /// One of ``"best_match"``, ``"normal_after_cursor"``,
    /// ``"normal_before_cursor"``.
    #[pyo3(get)]
    pub candidate_type: &'static str,
    /// Original-input bytes absorbed by this candidate.
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
impl ZhuyinCandidate {
    fn __repr__(&self) -> String {
        format!(
            "Candidate(text={:?}, candidate_type={:?}, consumed_bytes={})",
            self.text, self.candidate_type, self.consumed_bytes
        )
    }

    fn __str__(&self) -> &str {
        &self.text
    }
}

/// Copies one snapshotted candidate into its Python representation.
fn convert(candidate: &FacadeCandidate) -> ZhuyinCandidate {
    ZhuyinCandidate {
        text: candidate.text().to_owned(),
        kind: kind_label(candidate.kind()),
        candidate_type: candidate.candidate_type().label(),
        consumed_bytes: candidate.consumed_bytes(),
        cost: candidate.cost(),
        nbest_index: candidate.nbest_index(),
    }
}

/// Copies an ordered snapshot window into Python representations.
fn snapshot(candidates: &[FacadeCandidate]) -> Vec<ZhuyinCandidate> {
    candidates.iter().map(convert).collect()
}

/// One parsed chewing key: the upstream `_ChewingKey` elements unpacked —
/// initial, middle, final, tone — with the display renderers from
/// `oxpinyin-chewing`.
#[pyclass(frozen, name = "ChewingKey")]
pub struct PyChewingKey {
    inner: ChewingKey,
}

impl From<ChewingKey> for PyChewingKey {
    fn from(inner: ChewingKey) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PyChewingKey {
    /// The key for a full-pinyin spelling at zero tone, or ``None`` when
    /// the spelling names no key.
    #[classmethod]
    fn from_pinyin(_cls: &Bound<'_, PyType>, text: &str) -> Option<Self> {
        ChewingKey::from_pinyin(text).map(Self::from)
    }

    /// Unpacks the two-byte ABI word (initial `0..5`, middle `5..7`, final
    /// `7..12`, tone `12..15`); the padding bit is dropped.
    #[classmethod]
    fn from_packed(_cls: &Bound<'_, PyType>, bits: u16) -> Self {
        Self::from(ChewingKey::from_packed(bits))
    }

    /// The `ChewingInitial` element value.
    #[getter]
    fn initial(&self) -> u8 {
        self.inner.initial
    }

    /// The `ChewingMiddle` element value.
    #[getter]
    fn middle(&self) -> u8 {
        self.inner.middle
    }

    /// The `ChewingFinal` element value.
    #[getter]
    fn r#final(&self) -> u8 {
        self.inner.final_
    }

    /// The `ChewingTone` element value.
    #[getter]
    fn tone(&self) -> u8 {
        self.inner.tone
    }

    /// The packed two-byte form the C ABI carries.
    #[getter]
    fn packed(&self) -> u16 {
        self.inner.to_packed()
    }

    /// The `content_table` row index, 0 for the zero and invalid keys.
    fn table_index(&self) -> usize {
        self.inner.table_index()
    }

    /// The canonical spelling with the tone digit for a non-zero tone.
    fn pinyin_string(&self) -> String {
        self.inner.pinyin_string()
    }

    /// The initial column; no tone.
    fn shengmu_string(&self) -> &'static str {
        self.inner.shengmu_string()
    }

    /// The middle+final column; no tone.
    fn yunmu_string(&self) -> &'static str {
        self.inner.yunmu_string()
    }

    /// The zhuyin spelling; zero and first tones bare, tones 2..5 with
    /// their tone mark.
    fn zhuyin_string(&self) -> String {
        self.inner.zhuyin_string()
    }

    /// The luoma spelling, tone digit appended for a non-zero tone —
    /// including the first tone, unlike zhuyin.
    fn luoma_pinyin_string(&self) -> String {
        self.inner.luoma_pinyin_string()
    }

    /// The secondary zhuyin spelling, tone digit appended for a non-zero
    /// tone — including the first tone.
    fn secondary_zhuyin_string(&self) -> String {
        self.inner.secondary_zhuyin_string()
    }

    fn __repr__(&self) -> String {
        format!(
            "ChewingKey(initial={}, middle={}, final={}, tone={})",
            self.inner.initial, self.inner.middle, self.inner.final_, self.inner.tone
        )
    }

    fn __str__(&self) -> String {
        self.inner.zhuyin_string()
    }
}
