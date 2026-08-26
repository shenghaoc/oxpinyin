//! Concrete engine assembly: system tables + bigram model + optional user
//! store, wired onto the generic [`oxpinyin_engine::Session`].
//!
//! This is the Rust half of the Python binding's attach point, kept free of
//! any Python types so the `native-dump` binary exercises exactly these
//! constructions through the ordinary public API.
//!
//! The shape mirrors `oxpinyin-capi`'s context wiring (`CapiContext::
//! new_with_unigrams` and its `SharedDict`/`SharedLm` backends), minus what
//! the C ABI needs beyond decoding (addon phrase libraries, the punctuation
//! table, live option words). The user-count overlay arithmetic itself lives
//! in [`oxpinyin_data::BigramLanguageModel`]'s `*_with_user_delta` methods
//! and is *not* duplicated here; this module only forwards deltas from the
//! user store, as the capi wrappers do.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use oxpinyin_core::{
    Cost, Dictionary, LanguageModel, NbestStepCosts, PhraseEntry, PhraseToken, SyllableKey,
    UserCountDelta,
};
use oxpinyin_data::{
    BigramLanguageModel, DictError, InterpolationError, LmError, SystemDictionary,
};
use oxpinyin_engine::{
    Config, ConfigSource, EmptyConfigSource, EngineError, Session, StoragePaths,
};
use oxpinyin_user::{UserLookup, UserStore};

/// The generation-stamped user-phrase lookup cache shared by clones.
type LookupCache = Arc<std::sync::Mutex<Option<(u64, Arc<UserLookup>)>>>;

/// Why an engine could not be opened.
///
/// Deliberately distinguishes missing files ([`OpenError::Missing`]) from
/// present-but-unreadable/unparsable ones, so the Python boundary can raise
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
            Self::Io(path, error) => {
                write!(f, "cannot read {}: {error}", path.display())
            }
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
///
/// Mirrors capi's constructor split between production
/// (`interpolation2.text` required) and the committed mini fixtures (flat
/// counts exported from the phrase index).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnigramSource {
    /// Require a parsable `interpolation2.text`; absence is an open failure.
    Real,
    /// Derive flat unigram counts from the phrase index when
    /// `interpolation2.text` is absent. Fixture-only semantics: ranking
    /// degrades off the pinned three-key order.
    FlatExportForFixtures,
}

/// The system dictionary with the user-phrase lookup merged in.
///
/// `SystemDictionary` is not `Clone`, so it rides an `Arc`; capi's
/// `SharedDict` wraps for exactly the same reason.
#[derive(Clone)]
pub struct RuntimeDict {
    system: Arc<SystemDictionary>,
    user: Option<UserStore>,
    /// Refreshed on demand after training invalidates the cached view;
    /// shared so every clone sees the same cache generation.
    lookup_cache: LookupCache,
}

impl RuntimeDict {
    fn user_lookup(&self) -> Result<Arc<UserLookup>, DictError> {
        let Some(store) = self.user.as_ref() else {
            return Ok(Arc::new(UserLookup::empty()));
        };
        let mut cache = self
            .lookup_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        UserLookup::refresh_in(&mut cache, store)
            .map_err(|error| DictError::Parse(error.to_string()))?;
        Ok(cache
            .as_ref()
            .map(|(_, lookup)| Arc::clone(lookup))
            .unwrap_or_else(|| Arc::new(UserLookup::empty())))
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

    fn phrase_index_item_count(&self) -> Result<u64, Self::Error> {
        // System items only, as in capi: the pinned ranking denominator runs
        // the empty-user-store surface; trained items are not folded in.
        self.system.phrase_index_item_count()
    }
}

/// The bigram language model with the user-count overlay.
#[derive(Clone)]
pub struct RuntimeLm {
    inner: Arc<BigramLanguageModel>,
    user: Option<UserStore>,
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

/// A [`Session`] over this crate's concrete backends.
pub type RuntimeSession = Session<RuntimeDict, RuntimeLm>;

/// One loaded engine backend set: configuration, storage locations, and the
/// two decoded-over backends plus the user-learning handle.
pub struct Runtime {
    config: Config,
    paths: StoragePaths,
    dict: RuntimeDict,
    lm: RuntimeLm,
    user: Option<UserStore>,
}

// Compile-time proof the binding may park a `Runtime` behind a mutex and
// touch it from any thread that holds the GIL released.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RuntimeDict>();
    assert_send_sync::<RuntimeLm>();
    assert_send_sync::<UserStore>();
};

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
    /// flat unigram counts derived from the phrase index when
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
        // pinned default stands (same rule as capi).
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

        // capi contract: an unusable user directory must not fail init;
        // training then refuses (`train()` reports it), upstream-style.
        let user = user_dir.and_then(|dir| UserStore::open(&dir.join("user_store.redb")).ok());

        Ok(Self {
            config: Config::default(),
            paths: StoragePaths::new(user_dir.unwrap_or(Path::new("")))
                .with_system_dirs([system_dir]),
            dict: RuntimeDict {
                system: Arc::new(dict),
                user: user.clone(),
                lookup_cache: Arc::default(),
            },
            lm: RuntimeLm {
                inner: Arc::new(lm),
                user: user.clone(),
            },
            user,
        })
    }

    /// Configuration the session reads once at construction.
    pub fn config(&self) -> &dyn ConfigSource {
        &self.config
    }

    /// Storage locations handed to each session.
    pub fn paths(&self) -> StoragePaths {
        self.paths.clone()
    }

    /// Builds a fresh session over this backend set. Sessions are cheap:
    /// they share the table handles.
    ///
    /// # Errors
    ///
    /// Forwards [`EngineError`] from session construction (currently
    /// infallible over valid backends).
    pub fn new_session(&self) -> Result<RuntimeSession, EngineError> {
        Session::new(
            &EmptyConfigSource,
            self.paths.clone(),
            self.dict.clone(),
            self.lm.clone(),
        )
    }

    /// Clones of the user-learning handle, when the engine was opened with
    /// a usable user directory.
    pub fn user_store(&self) -> Option<UserStore> {
        self.user.clone()
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
        assert_send_sync::<Session<RuntimeDict, RuntimeLm>>();
        assert_send_sync::<Runtime>();
    }

    #[test]
    fn opens_the_w3_fixture_and_decodes_nihao() {
        let runtime =
            Runtime::open_fixtures(&w3_dir(), None).expect("fixture dir opens in fixture mode");
        let mut session = runtime.new_session().expect("session");
        let outcome = session.type_pinyin("nihao").expect("batch typing");
        assert_eq!(outcome, oxpinyin_engine::KeyOutcome::Consumed);
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
        let mut session = runtime.new_session().expect("session");
        session.type_pinyin("nihao").expect("typing");
        let advanced = session.select(0).expect("first candidate selects");
        assert_eq!(advanced, oxpinyin_engine::Selection::Completed);
        assert_eq!(session.commit().expect("commit"), "你好");
        assert!(!session.is_composing());
    }
}
