//! The concrete engine assembly shared by every oxpinyin consumer.
//!
//! Wiring only: this crate loads the converted system tables, installs the
//! unigram model and λ, opens the optional user store, and hands out
//! [`Session`]s over the merged backends. The algorithms stay where they
//! belong — decoding/composition in `oxpinyin-engine`, tables and model math
//! in `oxpinyin-data`, user state in `oxpinyin-user` — and the user-count
//! overlay arithmetic lives in `oxpinyin-data`'s `*_with_user_delta` methods,
//! which [`RuntimeLm`] merely feeds. Centralizing the assembly here is what
//! keeps the C ABI (`oxpinyin-capi`) and the Python binding
//! (`oxpinyin-python`) from silently diverging: one construction, one set of
//! parity-tested semantics.
//!
//! Pure Rust; no FFI of any kind, `unsafe_code` forbidden.
#![warn(missing_docs)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use oxpinyin_core::{
    Cost, Dictionary, LanguageModel, NbestStepCosts, PhraseEntry, PhraseToken, SyllableKey,
    UserCountDelta,
};
use oxpinyin_data::{
    BigramLanguageModel, DictError, InterpolationError, LmError, PunctTable, SystemDictionary,
};
use oxpinyin_engine::{ConfigSource, EngineError, Session, StoragePaths};
use oxpinyin_user::{PinyinKey, UserLookup, UserStore};

/// File name of the redb user store under the user data directory.
pub const USER_STORE_FILE: &str = "user_store.redb";

// ── Open errors ─────────────────────────────────────────────────────────

/// Why a runtime could not be opened.
///
/// Deliberately distinguishes missing files ([`OpenError::Missing`]) from
/// present-but-unreadable/unparsable ones, so adapters can raise
/// `FileNotFoundError`, `OSError` and friends without string matching.
#[derive(Debug)]
#[non_exhaustive]
pub enum OpenError {
    /// A required path does not exist (or is not a plain file).
    Missing(PathBuf),
    /// A required file exists but could not be read.
    Io(PathBuf, std::io::Error),
    /// The production constructor ran where no `interpolation2.text`
    /// unigram model sits next to the redb tables. The pinned three-key
    /// candidate ranking needs the real frequencies.
    ModelMissing(PathBuf),
    /// The dictionary tables failed to open or parse.
    Dict(DictError),
    /// The language model failed to open or parse.
    Lm(LmError),
    /// `interpolation2.text` exists but is unreadable or unparsable.
    Interpolation(InterpolationError),
}

impl core::fmt::Display for OpenError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Missing(path) => write!(f, "missing file: {}", path.display()),
            Self::Io(path, error) => write!(f, "cannot read {}: {error}", path.display()),
            Self::ModelMissing(path) => write!(
                f,
                "no real-unigram model at {} — pass it via the converted \
                 system dir, or use the fixture-mode constructor",
                path.display()
            ),
            Self::Dict(error) => write!(f, "dictionary error: {error}"),
            Self::Lm(error) => write!(f, "language model error: {error}"),
            Self::Interpolation(error) => write!(f, "unigram model error: {error}"),
        }
    }
}

impl std::error::Error for OpenError {}

/// Where the language model takes its unigram counts from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UnigramSource {
    /// Require a parsable `interpolation2.text`; absence is an open failure.
    Real,
    /// Derive flat unigram counts from the phrase index when
    /// `interpolation2.text` is absent. Fixture-only semantics: ranking
    /// degrades off the pinned three-key order.
    FlatExportForFixtures,
}

// ── Addon libraries ─────────────────────────────────────────────────────

/// Loaded addon libraries, shared by every session derived from one
/// runtime.
struct AddonSet {
    loaded: BTreeMap<u8, SystemDictionary>,
}

impl AddonSet {
    fn new() -> Self {
        Self {
            loaded: BTreeMap::new(),
        }
    }

    fn load(&mut self, index: u8, system_dir: &Path) -> bool {
        if self.loaded.contains_key(&index) {
            return false;
        }
        let pinyin = system_dir.join(format!("addon_{index}_pinyin_index.redb"));
        let phrase = system_dir.join(format!("addon_{index}_phrase_index.redb"));
        let Ok(dict) = SystemDictionary::open(&pinyin, &phrase) else {
            return false;
        };
        self.loaded.insert(index, dict);
        true
    }

    fn lookup_into(
        &self,
        syllables: &[SyllableKey],
        out: &mut Vec<PhraseEntry>,
    ) -> Result<(), DictError> {
        out.clear();
        if self.loaded.is_empty() {
            return Ok(());
        }
        // `SystemDictionary::lookup_into` replaces its output vec. One
        // scratch is filled, drained into `out`, and reused so the addon
        // path never hits the allocating `Dictionary::lookup` default.
        let mut scratch = Vec::new();
        for dict in self.loaded.values() {
            dict.lookup_into(syllables, &mut scratch)?;
            out.append(&mut scratch);
        }
        Ok(())
    }

    fn prefix_exists(&self, syllables: &[SyllableKey]) -> Result<bool, DictError> {
        for dict in self.loaded.values() {
            if dict.phrase_prefix_exists(syllables)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn unigram_freq(&self, token: u32) -> Option<u64> {
        self.loaded
            .values()
            .find_map(|dict| dict.unigram_count(token))
    }

    fn unigram_total(&self) -> Option<u64> {
        if self.loaded.is_empty() {
            return None;
        }
        // Saturating, like every other total in this crate: `Iterator::sum`
        // would wrap on release and panic on debug — a profile-dependent
        // divergence determinism cannot afford.
        Some(
            self.loaded
                .values()
                .map(SystemDictionary::unigram_total)
                .fold(0_u64, u64::saturating_add),
        )
    }

    fn is_empty(&self) -> bool {
        self.loaded.is_empty()
    }

    /// The addon phrase item behind `token`: its text, its pronunciations as
    /// `(key sequence, count)` pairs, and its copied unigram frequency — the
    /// `get_phrase_item` half of the promotion (`pinyin.cpp:2534-2549`).
    ///
    /// `None` when no loaded addon dictionary owns `token`. A pronunciation
    /// whose spelling does not map back to syllable keys is dropped, the same
    /// rule the reverse rendering applies.
    fn phrase_item(&self, token: u32) -> Option<AddonPhraseItem> {
        for dict in self.loaded.values() {
            let Ok(Some(text)) = dict.phrase_text(token) else {
                continue;
            };
            let Ok(prons) = dict.pronunciations(token) else {
                continue;
            };
            let readings = prons
                .into_iter()
                .filter_map(|(pinyin, freq)| Some((pinyin_to_keys(&pinyin)?, freq)))
                .collect();
            let unigram = dict.unigram_count(token).unwrap_or(0);
            return Some(AddonPhraseItem {
                text,
                readings,
                unigram,
            });
        }
        None
    }
}

/// A chosen addon phrase item, ready to promote into default nibble 5.
pub struct AddonPhraseItem {
    /// Phrase text.
    pub text: String,
    /// Pronunciations as `(key sequence, count)` pairs.
    pub readings: Vec<(Vec<PinyinKey>, u64)>,
    /// The item's copied unigram frequency (`add_phrase_item`).
    pub unigram: u64,
}

/// Splits a `'`-joined pinyin spelling into [`SyllableKey`] ids, or `None`
/// when any syllable is not a frozen key.
fn pinyin_to_keys(pinyin: &str) -> Option<Vec<PinyinKey>> {
    pinyin
        .split('\'')
        .map(|syllable| SyllableKey::from_text(syllable).map(|key| key.index() as PinyinKey))
        .collect()
}

// ── Merged backends ─────────────────────────────────────────────────────

/// The generation-stamped user-phrase lookup cache shared by clones.
type LookupCache = Arc<Mutex<Option<(u64, Arc<UserLookup>)>>>;

/// The system dictionary with the user-phrase lookup merged in.
///
/// `SystemDictionary` is not `Clone`, so it rides an `Arc`.
#[derive(Clone)]
pub struct RuntimeDict {
    system: Arc<SystemDictionary>,
    user: Option<UserStore>,
    user_lookup_cache: LookupCache,
    addons: Arc<RwLock<AddonSet>>,
    punct: Arc<PunctTable>,
}

impl RuntimeDict {
    fn user_lookup(&self) -> Result<Arc<UserLookup>, DictError> {
        let Some(store) = self.user.as_ref() else {
            return Ok(Arc::new(UserLookup::empty()));
        };
        let mut cache = self
            .user_lookup_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        UserLookup::refresh_in(&mut cache, store)
            .map_err(|error| DictError::Parse(error.to_string()))?;
        Ok(cache
            .as_ref()
            .map(|(_, lookup)| Arc::clone(lookup))
            .unwrap_or_else(|| Arc::new(UserLookup::empty())))
    }

    /// The underlying system table set, without the user overlay.
    #[must_use]
    pub fn system(&self) -> &SystemDictionary {
        &self.system
    }

    /// Punctuation strings registered for `token`, if the punct table
    /// shipped with the system dir.
    #[must_use]
    pub fn punctuations(&self, token: u32) -> &[String] {
        self.punct.punctuations(token)
    }

    /// Loads addon library `index` from `system_dir`; `false` when already
    /// loaded or the tables are missing/unopenable.
    pub fn load_addon(&self, index: u8, system_dir: &Path) -> bool {
        let mut addons = self.addons.write().unwrap_or_else(|p| p.into_inner());
        addons.load(index, system_dir)
    }

    /// The addon phrase item behind `token`, for the choose-promotion path.
    pub fn addon_phrase_item(&self, token: u32) -> Option<AddonPhraseItem> {
        let addons = self.addons.read().unwrap_or_else(|p| p.into_inner());
        addons.phrase_item(token)
    }
}

impl Dictionary for RuntimeDict {
    type Syllable = SyllableKey;
    type Entry = PhraseEntry;
    type Error = DictError;

    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        let mut entries = Vec::new();
        self.lookup_into(syllables, &mut entries)?;
        Ok(entries)
    }

    fn lookup_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<(), Self::Error> {
        self.system.lookup_into(syllables, out)?;
        out.extend(self.user_lookup()?.lookup(syllables));
        Ok(())
    }

    fn phrase_prefix_exists(&self, syllables: &[Self::Syllable]) -> Result<bool, Self::Error> {
        if self.system.phrase_prefix_exists(syllables)? {
            return Ok(true);
        }
        Ok(self.user_lookup()?.phrase_prefix_exists(syllables))
    }

    fn lookup_addon(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        let mut entries = Vec::new();
        self.lookup_addon_into(syllables, &mut entries)?;
        Ok(entries)
    }

    fn lookup_addon_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<(), Self::Error> {
        let addons = self.addons.read().unwrap_or_else(|p| p.into_inner());
        addons.lookup_into(syllables, out)
    }

    fn phrase_prefix_exists_addon(
        &self,
        syllables: &[Self::Syllable],
    ) -> Result<bool, Self::Error> {
        let addons = self.addons.read().unwrap_or_else(|p| p.into_inner());
        addons.prefix_exists(syllables)
    }

    fn phrase_index_item_count(&self) -> Result<u64, Self::Error> {
        // System items only: the parity surface the ranking denominator
        // reproduces runs an empty user store, where this is the whole
        // facade. A trained store's user items are not folded in.
        self.system.phrase_index_item_count()
    }
}

/// The bigram language model with the user-count overlay.
#[derive(Clone)]
pub struct RuntimeLm {
    inner: Arc<BigramLanguageModel>,
    user: Option<UserStore>,
    addons: Arc<RwLock<AddonSet>>,
}

impl RuntimeLm {
    fn delta(&self, prev: Option<u32>, token: u32) -> Result<UserCountDelta, LmError> {
        let Some(store) = self.user.as_ref() else {
            return Ok(UserCountDelta::ZERO);
        };
        store
            .count_delta(prev, token)
            .map_err(|error| LmError::User(error.to_string()))
    }

    /// The phrase-index total the pin's amplified law divides by, as the
    /// pinned predicted-candidate path constructs it (`pinyin.cpp:1813-1814`,
    /// live per call): the LM total with user extra (`None` without real
    /// unigrams → 0) plus the caller's phrase-index item count. Must not be
    /// snapshotted — training changes it.
    #[must_use]
    pub fn amplified_total(&self, item_count: u64) -> u64 {
        <Self as LanguageModel>::unigram_total(self)
            .ok()
            .flatten()
            .unwrap_or(0)
            .saturating_add(item_count)
    }
}

impl LanguageModel for RuntimeLm {
    type Token = PhraseToken;
    type Error = LmError;

    fn score(
        &self,
        history: &[Self::Token],
        token: &Self::Token,
        edge_cost: Cost,
    ) -> Result<Cost, Self::Error> {
        let extra = self.delta(history.last().map(|token| token.value()), token.value())?;
        self.inner
            .score_with_user_delta(history, token, edge_cost, extra)
    }

    fn unigram_freq(&self, token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        let extra = match self.user.as_ref() {
            None => 0,
            Some(store) => store
                .unigram_delta(token.value())
                .map_err(|error| LmError::User(error.to_string()))?,
        };
        Ok(self
            .inner
            .unigram_freq_with_user_delta(token.value(), extra))
    }

    fn has_real_unigrams(&self) -> bool {
        self.inner.has_real_unigrams()
    }

    fn unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        if !self.inner.has_real_unigrams() {
            return Ok(None);
        }
        let extra = match self.user.as_ref() {
            None => 0,
            Some(store) => store
                .unigram_total()
                .map_err(|error| LmError::User(error.to_string()))?,
        };
        Ok(Some(self.inner.unigram_total().saturating_add(extra)))
    }

    fn addon_unigram_freq(&self, token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        let addons = self.addons.read().unwrap_or_else(|p| p.into_inner());
        if addons.is_empty() {
            return Ok(None);
        }
        Ok(Some(addons.unigram_freq(token.value()).unwrap_or(0)))
    }

    fn addon_unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        let addons = self.addons.read().unwrap_or_else(|p| p.into_inner());
        Ok(addons.unigram_total())
    }

    /// The §5 overlay `score` takes, forwarded into the n-best step costs.
    fn nbest_step_costs(
        &self,
        prev: &Self::Token,
        token: &Self::Token,
    ) -> Result<NbestStepCosts, Self::Error> {
        let extra = self.delta(Some(prev.value()), token.value())?;
        self.inner
            .nbest_step_costs_with_user_delta(prev, token, extra)
    }
}

// ── Runtime ─────────────────────────────────────────────────────────────

/// A [`Session`] over this crate's concrete backends.
pub type RuntimeSession = Session<RuntimeDict, RuntimeLm>;

// Compile-time proof that adapters may share a `Runtime` across threads
// behind their own synchronization.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RuntimeDict>();
    assert_send_sync::<RuntimeLm>();
    assert_send_sync::<UserStore>();
};

/// One opened engine: configuration-independent wiring for the converted
/// system tables, the unigram model, λ, the optional learning store, and the
/// merged backends sessions decode over.
///
/// Clones are cheap (table handles are shared); sessions built through
/// [`Runtime::new_session`] share those handles too.
pub struct Runtime {
    paths: StoragePaths,
    dict: RuntimeDict,
    lm: RuntimeLm,
    user: Option<UserStore>,
}

impl Runtime {
    /// Opens the production configuration: redb tables under `system_dir`,
    /// real unigrams from `interpolation2.text` next to them, λ from
    /// `table.conf` when present, and — when `user_dir` is given — the
    /// learning store (its creation or read failure degrades to "no user
    /// state", matching the C ABI so a bad user dir cannot fail init).
    ///
    /// # Errors
    ///
    /// Returns [`OpenError`] when the system data cannot be opened or the
    /// unigram model is missing/unparsable.
    pub fn open(system_dir: &Path, user_dir: Option<&Path>) -> Result<Self, OpenError> {
        Self::open_with_unigrams(system_dir, user_dir, UnigramSource::Real)
    }

    /// Opens fixture semantics like the committed `fixtures/w3` mini tables:
    /// flat unigram counts derive from the phrase index when
    /// `interpolation2.text` is absent. Test/dev surface only — parity with
    /// the pin is defined for the real-model configuration.
    ///
    /// # Errors
    ///
    /// Same as [`Runtime::open`].
    pub fn open_fixtures(system_dir: &Path, user_dir: Option<&Path>) -> Result<Self, OpenError> {
        Self::open_with_unigrams(system_dir, user_dir, UnigramSource::FlatExportForFixtures)
    }

    fn open_with_unigrams(
        system_dir: &Path,
        user_dir: Option<&Path>,
        source: UnigramSource,
    ) -> Result<Self, OpenError> {
        let pinyin_index = system_dir.join("pinyin_index.redb");
        let phrase_index = system_dir.join("phrase_index.redb");
        let bigram = system_dir.join("bigram.redb");
        require_file(&pinyin_index)?;
        require_file(&phrase_index)?;
        require_file(&bigram)?;

        let dict = SystemDictionary::open(&pinyin_index, &phrase_index).map_err(OpenError::Dict)?;
        let mut lm = BigramLanguageModel::open(&bigram).map_err(OpenError::Lm)?;
        // λ rides the install's table.conf when one ships; absent, the
        // pinned default stands.
        lm.set_lambda_from_table_conf(&system_dir.join("table.conf"));

        let interpolation2 = system_dir.join("interpolation2.text");
        if interpolation2.is_file() {
            lm.set_unigrams_from_interpolation2(&interpolation2)
                .map_err(OpenError::Interpolation)?;
        } else {
            match source {
                UnigramSource::Real => return Err(OpenError::ModelMissing(interpolation2)),
                UnigramSource::FlatExportForFixtures => lm.set_unigrams_from_dict(&dict),
            }
        }

        // An empty path means no user directory; otherwise an unusable
        // directory must not fail init either — training then refuses,
        // upstream-style.
        let user = user_dir
            .filter(|dir| !dir.as_os_str().is_empty())
            .and_then(|dir| UserStore::open(&dir.join(USER_STORE_FILE)).ok());

        let addons = Arc::new(RwLock::new(AddonSet::new()));
        let punct = PunctTable::open_optional(&system_dir.join("punct.redb"));

        Ok(Self {
            paths: StoragePaths::new(user_dir.unwrap_or(Path::new("")))
                .with_system_dirs([system_dir]),
            dict: RuntimeDict {
                system: Arc::new(dict),
                user: user.clone(),
                user_lookup_cache: LookupCache::default(),
                addons: Arc::clone(&addons),
                punct: Arc::new(punct),
            },
            lm: RuntimeLm {
                inner: Arc::new(lm),
                user: user.clone(),
                addons,
            },
            user,
        })
    }

    /// Builds a fresh session over this backend set. Sessions are cheap:
    /// they share the table handles.
    ///
    /// Configuration arrives per call (`oxpinyin-capi` passes its layered
    /// `Config`; simple embedders pass [`EmptyConfigSource`]), because the
    /// session reads it once at construction.
    ///
    /// # Errors
    ///
    /// Forwards [`EngineError`] from session construction (currently
    /// infallible over valid backends).
    pub fn new_session(&self, config: &dyn ConfigSource) -> Result<RuntimeSession, EngineError> {
        Session::new(
            config,
            self.paths.clone(),
            self.dict.clone(),
            self.lm.clone(),
        )
    }

    /// Storage locations handed to each session.
    #[must_use]
    pub fn paths(&self) -> StoragePaths {
        self.paths.clone()
    }

    /// A handle clone of the merged dictionary backend.
    #[must_use]
    pub fn dict(&self) -> RuntimeDict {
        self.dict.clone()
    }

    /// A handle clone of the overlaid language model backend.
    #[must_use]
    pub fn lm(&self) -> RuntimeLm {
        self.lm.clone()
    }

    /// Clone of the user-learning handle, when the runtime was opened with
    /// a usable user directory.
    #[must_use]
    pub fn user_store(&self) -> Option<UserStore> {
        self.user.clone()
    }

    /// Loads addon library `index` from `system_dir`; `false` when already
    /// loaded or the library tables do not open.
    pub fn load_addon(&self, index: u8, system_dir: &Path) -> bool {
        self.dict.load_addon(index, system_dir)
    }
}

fn require_file(path: &Path) -> Result<(), OpenError> {
    let meta = std::fs::metadata(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => OpenError::Missing(path.to_path_buf()),
        _ => OpenError::Io(path.to_path_buf(), error),
    })?;
    if meta.is_file() {
        Ok(())
    } else {
        Err(OpenError::Io(
            path.to_path_buf(),
            std::io::Error::other("not a regular file"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxpinyin_engine::{EmptyConfigSource, KeyOutcome, Selection};

    fn w3_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fixtures")
            .join("w3")
    }

    #[test]
    fn sends_and_syncs() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RuntimeSession>();
        assert_send_sync::<Runtime>();
    }

    #[test]
    fn opens_the_w3_fixture_and_decodes_nihao() {
        let runtime =
            Runtime::open_fixtures(&w3_dir(), None).expect("fixture dir opens in fixture mode");
        let mut session = runtime.new_session(&EmptyConfigSource).expect("session");
        let outcome = session.type_pinyin("nihao").expect("batch typing");
        assert_eq!(outcome, KeyOutcome::Consumed);
        let first = session
            .candidates()
            .get(0)
            .expect("nihao has candidates in the fixture")
            .text()
            .to_owned();
        assert_eq!(first, "你好");

        session.guess_sentence().expect("sentence guess");
        let best = session.sentence_text(0).expect("n-best row 0");
        assert_eq!(best, "你好");
    }

    #[test]
    fn production_open_requires_the_interpolation_model() {
        match Runtime::open(&w3_dir(), None) {
            Err(OpenError::ModelMissing(path)) => {
                assert!(path.ends_with("interpolation2.text"), "{path:?}")
            }
            Err(other) => panic!("expected ModelMissing, got {other}"),
            Ok(_) => panic!("fixture dir must not open in production mode"),
        }
    }

    #[test]
    fn selection_advances_and_commit_returns_the_chosen_text() {
        let runtime = Runtime::open_fixtures(&w3_dir(), None).expect("open");
        let mut session = runtime.new_session(&EmptyConfigSource).expect("session");
        session.type_pinyin("nihao").expect("typing");
        let advanced = session.select(0).expect("first candidate selects");
        assert_eq!(advanced, Selection::Completed);
        assert_eq!(session.commit().expect("commit"), "你好");
        assert!(!session.is_composing());
    }

    #[test]
    fn regular_file_as_user_dir_degrades_to_no_user_state() {
        let marker = std::env::temp_dir().join(format!(
            "oxpinyin-runtime-file-user-{}.tmp",
            std::process::id()
        ));
        std::fs::write(&marker, b"not a directory").expect("temp file");
        let runtime = Runtime::open_fixtures(&w3_dir(), Some(&marker));
        // A non-directory user path must not fail init (capi contract); it
        // simply leaves the engine without learning state.
        if let Err(error) = &runtime {
            panic!("expected open to succeed over a file user dir: {error}");
        }
        assert!(runtime.expect("open").user_store().is_none());
        std::fs::remove_file(&marker).expect("cleanup temp file");
    }

    #[test]
    fn empty_user_path_means_no_user_state_not_a_cwd_file() {
        // capi contract: an empty user dir string disables learning; the
        // merged runtime must not fall back to creating `user_store.redb`
        // in the process's working directory.
        let runtime = Runtime::open_fixtures(&w3_dir(), Some(Path::new(""))).expect("open");
        assert!(runtime.user_store().is_none());
    }

    #[test]
    fn addon_unigram_totals_stay_none_until_a_library_loads() {
        let runtime = Runtime::open_fixtures(&w3_dir(), None).expect("open");
        let lm = runtime.lm();
        assert_eq!(
            LanguageModel::addon_unigram_total(&lm).expect("infallible query"),
            None,
            "no addon library is loaded"
        );
        // The committed w3 carries the addon_4 pair; loading is idempotent.
        let index = 4_u8;
        assert!(runtime.load_addon(index, &w3_dir()));
        assert!(!runtime.load_addon(index, &w3_dir()));
        assert!(
            LanguageModel::addon_unigram_total(&lm)
                .expect("infallible query")
                .is_some(),
            "the loaded addon library owns items"
        );
    }
}
