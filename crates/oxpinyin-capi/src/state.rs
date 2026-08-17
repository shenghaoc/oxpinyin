//! Real backing state behind the opaque C handles.
//!
//! `CapiContext` lives behind `pinyin_context_t *` and `CapiInstance`
//! behind `pinyin_instance_t *`. The opaque `#[repr(C)]` types in
//! [`crate::types`] exist only for the generated C header.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::ffi::CString;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use oxpinyin_core::{
    Cost, Dictionary, DoublePinyinParse, DoublePinyinScheme, LanguageModel, OptionBits,
    PINYIN_INCOMPLETE, PhraseEntry, PhraseToken, SyllableKey, UserCountDelta, ZhuyinParse,
    ZhuyinScheme,
};
use oxpinyin_data::{BigramLanguageModel, DictError, LmError, PunctTable, SystemDictionary};
use oxpinyin_engine::{CandidateKind, Config, Session, StoragePaths};
use oxpinyin_user::{
    ExportedPhrase, NETWORK_DICTIONARY, SENTENCE_START, USER_DICTIONARY, UserLookup, UserStore,
    is_user_file_token,
};

use crate::types::{LookupCandidate, PinyinContext, PinyinInstance};

/// File name of the redb user store under the user data directory.
///
/// T3 opens it (the training entry points need somewhere to write); T5 owns
/// the save cycle behind it: `pinyin_save` gates on the §4 `m_modified`
/// flag and compacts, while durability is redb's per-commit guarantee —
/// there is no `.tmp`/rename dance, so this single file is the whole
/// on-disk contract.
pub(crate) const USER_STORE_FILE: &str = "user_store.redb";

// ── Shared backends ─────────────────────────────────────────────────────
//
// Context and instance both hold `Arc` clones. Instances must not borrow
// the context as `'static`: `pinyin_fini` drops the context Box while
// instances may still be alive, and a `'static` reference would then be a
// use-after-free.

/// Loaded addon libraries, shared by every instance of one context.
pub(crate) struct AddonSet {
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

    fn lookup(&self, syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, DictError> {
        let mut out = Vec::new();
        for dict in self.loaded.values() {
            out.extend(dict.lookup(syllables)?);
        }
        Ok(out)
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
            None
        } else {
            Some(
                self.loaded
                    .values()
                    .map(SystemDictionary::unigram_total)
                    .sum(),
            )
        }
    }

    fn is_empty(&self) -> bool {
        self.loaded.is_empty()
    }
}

struct SharedDictInner {
    system: SystemDictionary,
    user: Option<UserStore>,
    user_lookup: Mutex<Option<(u64, Arc<UserLookup>)>>,
    addons: Arc<RwLock<AddonSet>>,
    punct: PunctTable,
}

/// `Arc` wrapper so instances share the context's dictionary without a
/// `'static` borrow.
#[derive(Clone)]
pub(crate) struct SharedDict(Arc<SharedDictInner>);

impl SharedDict {
    fn new(
        system: SystemDictionary,
        user: Option<UserStore>,
        addons: Arc<RwLock<AddonSet>>,
        punct: PunctTable,
    ) -> Self {
        Self(Arc::new(SharedDictInner {
            system,
            user,
            user_lookup: Mutex::new(None),
            addons,
            punct,
        }))
    }

    pub(crate) fn system(&self) -> &SystemDictionary {
        &self.0.system
    }

    pub(crate) fn punctuations(&self, token: u32) -> &[String] {
        self.0.punct.punctuations(token)
    }

    pub(crate) fn load_addon(&self, index: u8, system_dir: &Path) -> bool {
        let mut addons = self
            .0
            .addons
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        addons.load(index, system_dir)
    }

    fn user_lookup(&self) -> Result<Arc<UserLookup>, DictError> {
        let Some(store) = self.0.user.as_ref() else {
            return Ok(Arc::new(UserLookup::empty()));
        };
        let mut cache = self
            .0
            .user_lookup
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

impl Dictionary for SharedDict {
    type Syllable = SyllableKey;
    type Entry = PhraseEntry;
    type Error = DictError;

    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        let mut entries = self.0.system.lookup(syllables)?;
        entries.extend(self.user_lookup()?.lookup(syllables));
        Ok(entries)
    }

    fn phrase_prefix_exists(&self, syllables: &[Self::Syllable]) -> Result<bool, Self::Error> {
        if self.0.system.phrase_prefix_exists(syllables)? {
            return Ok(true);
        }
        Ok(self.user_lookup()?.phrase_prefix_exists(syllables))
    }

    fn lookup_addon(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        let addons = self
            .0
            .addons
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        addons.lookup(syllables)
    }

    fn phrase_prefix_exists_addon(
        &self,
        syllables: &[Self::Syllable],
    ) -> Result<bool, Self::Error> {
        let addons = self
            .0
            .addons
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        addons.prefix_exists(syllables)
    }
}

/// `Arc` wrapper so instances share the context's language model without
/// a `'static` borrow.
///
/// The optional [`UserStore`] is the §5 overlay: `score` and
/// `unigram_freq` saturating-add its counts onto the system model before
/// the frozen λ blend. `None` (no user dir) and an empty store are both
/// identity.
#[derive(Clone)]
pub(crate) struct SharedLm {
    inner: Arc<BigramLanguageModel>,
    user: Option<UserStore>,
    addons: Arc<RwLock<AddonSet>>,
}

impl SharedLm {
    fn user_delta(
        &self,
        history: &[PhraseToken],
        token: &PhraseToken,
    ) -> Result<UserCountDelta, LmError> {
        let Some(store) = self.user.as_ref() else {
            return Ok(UserCountDelta::ZERO);
        };
        store
            .count_delta(history.last().map(|t| t.value()), token.value())
            .map_err(|error| LmError::User(error.to_string()))
    }
}

impl LanguageModel for SharedLm {
    type Token = PhraseToken;
    type Error = LmError;

    fn score(
        &self,
        history: &[Self::Token],
        token: &Self::Token,
        edge_cost: Cost,
    ) -> Result<Cost, Self::Error> {
        let delta = self.user_delta(history, token)?;
        self.inner
            .score_with_user_delta(history, token, edge_cost, delta)
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
        let addons = self
            .addons
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if addons.is_empty() {
            return Ok(None);
        }
        Ok(Some(addons.unigram_freq(token.value()).unwrap_or(0)))
    }

    fn addon_unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        let addons = self
            .addons
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(addons.unigram_total())
    }
}

// ── Context ─────────────────────────────────────────────────────────────

pub(crate) type CapiSession = Session<SharedDict, SharedLm>;

/// State behind `pinyin_context_t *`.
///
/// Owns the dictionary and language model. Instances receive `Arc` clones
/// so they do not borrow the context.
pub(crate) struct CapiContext {
    pub(crate) paths: StoragePaths,
    pub(crate) config: Config,
    dict: Option<SharedDict>,
    lm: Option<SharedLm>,
    /// The user-learning store, shared by value-clone with every instance.
    ///
    /// `None` when the caller passed an empty user directory — the
    /// libpinyin situation where `pinyin_train` refuses — or when the store
    /// file cannot be opened (a missing/inaccessible user dir must not make
    /// `pinyin_init` fail; training then degrades to `false`, upstream-style).
    user: Option<UserStore>,
    /// Live `PINYIN_INCOMPLETE` bit. Shared with every instance so
    /// `pinyin_set_options` remasks already-allocated sessions.
    pub(crate) incomplete: Arc<AtomicBool>,
    /// Live double-pinyin scheme. Shared with every instance so
    /// `pinyin_set_double_pinyin_scheme` remasks already-allocated sessions.
    pub(crate) double_scheme: Arc<AtomicI32>,
    /// Live Zhuyin scheme. Shared with every instance so
    /// `pinyin_set_zhuyin_scheme` remasks already-allocated sessions.
    pub(crate) zhuyin_scheme: Arc<AtomicI32>,
    /// Live `USE_TONE` bit for the bopomofo context.
    pub(crate) use_tone: Arc<AtomicBool>,
    /// Live option word. Shared with every instance so `pinyin_set_options`
    /// remasks already-allocated sessions.
    pub(crate) options: Arc<AtomicU32>,
}

/// Where [`CapiContext::new`] takes unigram counts from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UnigramSource {
    /// Require a parsable `interpolation2.text` next to the redb tables.
    RealOnly,
    /// Test fixtures and the W3 mini tables use export-ABI flat counts.
    FlatExportForFixtures,
}

impl CapiContext {
    pub(crate) fn new(system_dir: &str, user_dir: &str) -> Option<Self> {
        Self::new_with_unigrams(system_dir, user_dir, UnigramSource::RealOnly)
    }

    /// Fixture/test constructor: the W3 mini system dir deliberately has no
    /// model file, so it opts into the old flat-export behaviour explicitly.
    pub(crate) fn new_for_fixtures(system_dir: &str, user_dir: &str) -> Option<Self> {
        Self::new_with_unigrams(system_dir, user_dir, UnigramSource::FlatExportForFixtures)
    }

    fn new_with_unigrams(
        system_dir: &str,
        user_dir: &str,
        unigram_source: UnigramSource,
    ) -> Option<Self> {
        if system_dir.is_empty() {
            return None;
        }
        let paths = StoragePaths::new(user_dir).with_system_dirs([system_dir]);

        let sys = Path::new(system_dir);
        let dict = SystemDictionary::open(
            &sys.join("pinyin_index.redb"),
            &sys.join("phrase_index.redb"),
        )
        .ok()?;
        let mut lm = BigramLanguageModel::open(&sys.join("bigram.redb")).ok()?;
        // Read λ from the install's table.conf when present (data-formats.md
        // §3); a real install ships one. Absent (no table.conf in the dir),
        // the pinned 0.312699 default stands.
        lm.set_lambda_from_table_conf(&sys.join("table.conf"));

        // W8 fork-bootstrap wiring: the fork's runtime system dir carries
        // the fetched pinned model as `interpolation2.text`; install its
        // real phrase-index counts so the candidate construction runs the
        // pinned three-key order.  A present-but-unparsable file is an init
        // failure, matching upstream's NULL on load failure.
        let interpolation2 = sys.join("interpolation2.text");
        if interpolation2.is_file() {
            lm.set_unigrams_from_interpolation2(&interpolation2).ok()?;
        } else {
            match unigram_source {
                UnigramSource::RealOnly => return None,
                UnigramSource::FlatExportForFixtures => lm.set_unigrams_from_dict(&dict),
            }
        }

        let user = if user_dir.is_empty() {
            None
        } else {
            UserStore::open(&Path::new(user_dir).join(USER_STORE_FILE)).ok()
        };

        let addons = Arc::new(RwLock::new(AddonSet::new()));
        let punct = PunctTable::open_optional(&sys.join("punct.redb"));
        Some(Self {
            paths,
            config: Config::default(),
            dict: Some(SharedDict::new(
                dict,
                user.clone(),
                Arc::clone(&addons),
                punct,
            )),
            lm: Some(SharedLm {
                inner: Arc::new(lm),
                user: user.clone(),
                addons,
            }),
            user,
            incomplete: Arc::new(AtomicBool::new(true)),
            double_scheme: Arc::new(AtomicI32::new(DoublePinyinScheme::Ms as i32)),
            zhuyin_scheme: Arc::new(AtomicI32::new(ZhuyinScheme::Standard as i32)),
            use_tone: Arc::new(AtomicBool::new(false)),
            options: Arc::new(AtomicU32::new(PINYIN_INCOMPLETE)),
        })
    }

    /// User-store-only context for standalone migration tools
    /// (`oxpinyin-dictool import`). The C ABI `pinyin_init` still requires
    /// system tables — its contract is a decoder context — while this
    /// Rust-only constructor lets a tool drive the import/export/save trio
    /// without carrying a system dictionary. `pinyin_alloc_instance` reports
    /// `None` for such a context because there is nothing to decode with.
    pub(crate) fn new_user_only(user_dir: &str) -> Option<Self> {
        if user_dir.is_empty() {
            return None;
        }
        let user = UserStore::open(&Path::new(user_dir).join(USER_STORE_FILE)).ok()?;
        Some(Self {
            paths: StoragePaths::new(user_dir),
            config: Config::default(),
            dict: None,
            lm: None,
            user: Some(user),
            incomplete: Arc::new(AtomicBool::new(true)),
            double_scheme: Arc::new(AtomicI32::new(DoublePinyinScheme::Ms as i32)),
            zhuyin_scheme: Arc::new(AtomicI32::new(ZhuyinScheme::Standard as i32)),
            use_tone: Arc::new(AtomicBool::new(false)),
            options: Arc::new(AtomicU32::new(PINYIN_INCOMPLETE)),
        })
    }

    pub(crate) fn alloc_instance(&self) -> Option<CapiInstance> {
        let session = Session::new(
            &self.config,
            self.paths.clone(),
            self.dict.clone()?,
            self.lm.clone()?,
        )
        .ok()?;
        Some(CapiInstance {
            session,
            candidates: Vec::new(),
            parsed_len: 0,
            user: self.user.clone(),
            dict: self.dict.clone()?,
            incomplete: Arc::clone(&self.incomplete),
            double_scheme: Arc::clone(&self.double_scheme),
            zhuyin_scheme: Arc::clone(&self.zhuyin_scheme),
            use_tone: Arc::clone(&self.use_tone),
            options: Arc::clone(&self.options),
            double_parse: None,
            double_input: String::new(),
            zhuyin_parse: None,
            zhuyin_input: String::new(),
        })
    }

    /// Clone of the context's user store, if this context has one.
    ///
    /// The import iterator owns this clone; because the store's §4 dirty flag
    /// is shared by every clone, `pinyin_end_add_phrases` can arm
    /// `m_modified` through it without retaining a context pointer.
    pub(crate) fn user_store(&self) -> Option<UserStore> {
        self.user.clone()
    }

    /// `pinyin_save`'s body (§4): `false` without a user dir (upstream
    /// `pinyin.cpp:1133`), otherwise the store's gated save — `false` when
    /// unmodified (`:1136`), `true` after a dirty save.
    pub(crate) fn save_user(&mut self) -> bool {
        match self.user.as_mut() {
            None => false,
            Some(store) => store.save().unwrap_or(false),
        }
    }

    /// `pinyin_mask_out`'s body: the store-level deletion, or `false`
    /// without a user store.
    pub(crate) fn mask_out(&mut self, mask: u32, value: u32) -> bool {
        match self.user.as_mut() {
            None => false,
            Some(store) => store.mask_out(mask, value).is_ok(),
        }
    }

    /// Load addon library `index` from the first system data dir.
    pub(crate) fn load_addon(&self, index: u8) -> bool {
        let Some(dict) = self.dict.as_ref() else {
            return false;
        };
        let Some(system_dir) = self.paths.system_data_dirs().first() else {
            return false;
        };
        dict.load_addon(index, system_dir)
    }

    /// §9 phrase-export materialization. [`USER_DICTIONARY`] and
    /// [`NETWORK_DICTIONARY`] export their stored rows; any other index
    /// exports an empty list.
    pub(crate) fn export_phrases(&self, index: u32) -> Option<Vec<ExportedPhrase>> {
        let index = u8::try_from(index).ok()?;
        if index != USER_DICTIONARY && index != NETWORK_DICTIONARY {
            return Some(Vec::new());
        }
        self.user.as_ref()?.export_phrases_in(index).ok()
    }

    /// §9 bigram-export materialization with upstream's filters and
    /// rendering (`pinyin_begin_get_bigram_phrases` in `pinyin.cpp`):
    /// skip `sentence_start` predecessors and counts at or below the
    /// first-seed threshold (`initial_seed − 1` = 68); phrase = prev text +
    /// next text; pinyin = prev pinyin + `'` + next pinyin (one row per
    /// pronunciation combination); count = stored × 2 (upstream's local
    /// `unigram_factor`).
    /// False when this context cannot render every exportable bigram row
    /// (user-store-only, and at least one stored pair needs the system
    /// phrase index). Callers must fail the snapshot rather than skip those
    /// rows into an incomplete file.
    pub(crate) fn can_render_export_bigrams(&self) -> bool {
        const INITIAL_SEED: u64 = 23 * 3;
        if self.dict.is_some() {
            return true;
        }
        let Some(store) = self.user.as_ref() else {
            return true;
        };
        let Ok(raw) = store.export_bigrams() else {
            return false;
        };
        !raw.iter().any(|(prev, cur, count)| {
            *prev != SENTENCE_START
                && *count >= INITIAL_SEED
                && (!is_user_file_token(*prev) || !is_user_file_token(*cur))
        })
    }

    pub(crate) fn export_bigram_rows(&self) -> Option<Vec<ExportedBigramRow>> {
        const INITIAL_SEED: u64 = 23 * 3;
        let store = self.user.as_ref()?;
        let raw = store.export_bigrams().ok()?;
        let mut rows = Vec::new();
        // Memoize the (text, pinyins) rendering: a system token recurs across
        // many bigram rows and `render_token` is an O(pinyin-index) scan, so
        // resolving it once per distinct token keeps the export off the
        // rows×index quadratic.
        let mut rendered: std::collections::HashMap<u32, Option<(String, Vec<String>)>> =
            std::collections::HashMap::new();
        for (prev, cur, count) in raw {
            if prev == SENTENCE_START {
                continue;
            }
            // Upstream's threshold is `initial_seed - 1` = 68.
            if count < INITIAL_SEED {
                continue;
            }
            let Some((prev_text, prev_pinyins)) = rendered
                .entry(prev)
                .or_insert_with(|| self.render_token(prev))
                .clone()
            else {
                continue;
            };
            let Some((cur_text, cur_pinyins)) = rendered
                .entry(cur)
                .or_insert_with(|| self.render_token(cur))
                .clone()
            else {
                continue;
            };
            let phrase = format!("{prev_text}{cur_text}");
            for first in &prev_pinyins {
                for second in &cur_pinyins {
                    rows.push(ExportedBigramRow {
                        phrase: phrase.clone(),
                        pinyin: format!("{first}'{second}"),
                        count: i64::try_from(count.saturating_mul(2)).unwrap_or(i64::MAX),
                    });
                }
            }
        }
        Some(rows)
    }

    /// `(text, pinyin spellings)` for a token: user tokens render from the
    /// user store's phrase/pronunciation tables, system tokens from the
    /// system phrase index and the pinyin index (reverse-scanned).
    fn render_token(&self, token: u32) -> Option<(String, Vec<String>)> {
        if is_user_file_token(token) {
            let store = self.user.as_ref()?;
            let phrase = store.phrase(token).ok().flatten()?;
            // Render each reading through the shared `render_pinyin` helper,
            // skipping any unrenderable one — the same rule `export_phrases`
            // applies, so the phrase and bigram exports stay consistent.
            let pinyins: Vec<String> = phrase
                .pronunciations()
                .iter()
                .filter_map(|pronunciation| pronunciation.render_pinyin())
                .collect();
            if pinyins.is_empty() {
                return None;
            }
            Some((phrase.text().to_owned(), pinyins))
        } else {
            let dict = self.dict.as_ref()?;
            let text = dict.system().phrase_text(token).ok().flatten()?;
            let pinyins: Vec<String> = dict
                .system()
                .pronunciations(token)
                .ok()?
                .into_iter()
                .map(|(pinyin, _freq)| pinyin)
                .collect();
            if pinyins.is_empty() {
                return None;
            }
            Some((text, pinyins))
        }
    }
}

/// One rendered §9 bigram-export row: concatenated phrase text, the
/// `'`-joined pronunciation of the pair, and the scaled count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportedBigramRow {
    /// Concatenated predecessor + successor phrase text.
    pub phrase: String,
    /// The `'`-joined pronunciation of the pair.
    pub pinyin: String,
    /// The rendered bigram count (`stored × 2`).
    pub count: i64,
}

// ── Instance ────────────────────────────────────────────────────────────

/// One snapshotted candidate, stored inside `CapiInstance` so that
/// `lookup_candidate_t *` can borrow into it across C calls.
pub(crate) struct CapiCandidate {
    pub(crate) text: CString,
    pub(crate) kind: CandidateKind,
    pub(crate) candidate_type: crate::types::lookup_candidate_type_t,
    pub(crate) nbest_index: u8,
    /// Bytes of raw input this candidate consumed, snapshotted at guess time
    /// so `pinyin_choose_candidate` can report the new cursor position.
    pub(crate) consumed_bytes: usize,
    /// The candidate's scoring token, snapshotted so the training entry
    /// points (`pinyin_train`'s observation, predicted-candidate training,
    /// `pinyin_is_user_candidate`) can resolve it without re-decoding.
    /// `None` for sentence-level and fallback candidates, which carry no
    /// token and are not trained (§2: only pinned phrases train).
    pub(crate) token: Option<PhraseToken>,
}

/// State behind `pinyin_instance_t *`.
pub(crate) struct CapiInstance {
    pub(crate) session: CapiSession,
    /// Snapshotted candidates, rebuilt by `pinyin_guess_candidates`.
    /// `lookup_candidate_t *` pointers borrow into this vec.
    pub(crate) candidates: Vec<CapiCandidate>,
    /// Bytes of raw input consumed by the most recent parse call — upstream
    /// `m_parsed_len` (`pinyin.cpp:84`), returned by
    /// `pinyin_get_parsed_input_length` (`pinyin.cpp:1611-1613`).
    /// Allocation and reset both store 0 (`pinyin.cpp:1318,2692`).
    pub(crate) parsed_len: usize,
    /// Clone of the context's user store. `None` under an empty user dir.
    pub(crate) user: Option<UserStore>,
    /// Shared dictionary (system + user-file + addon set) for prediction.
    pub(crate) dict: SharedDict,
    /// Shared live `PINYIN_INCOMPLETE` flag from the owning context.
    pub(crate) incomplete: Arc<AtomicBool>,
    /// Shared live double-pinyin scheme from the owning context.
    pub(crate) double_scheme: Arc<AtomicI32>,
    /// Most recent double-pinyin parse, when the last parse call was the
    /// double-pinyin entry point. Used for aux text and candidate-offset
    /// mapping back to the original double-pinyin input bytes.
    pub(crate) double_parse: Option<DoublePinyinParse>,
    /// Original double-pinyin input for sentence/preedit fallback display.
    pub(crate) double_input: String,
    /// Shared live Zhuyin scheme from the owning context.
    pub(crate) zhuyin_scheme: Arc<AtomicI32>,
    /// Most recent Zhuyin parse, when the last parse call was the chewing
    /// entry point.
    pub(crate) zhuyin_parse: Option<ZhuyinParse>,
    /// Original Zhuyin input for sentence/preedit fallback display.
    pub(crate) zhuyin_input: String,
    /// Shared live `USE_TONE` flag from the owning context.
    pub(crate) use_tone: Arc<AtomicBool>,
    /// Shared live option word from the owning context.
    pub(crate) options: Arc<AtomicU32>,
}

impl CapiInstance {
    /// Drop composition state the way `pinyin_reset` does: session, candidate
    /// snapshot, and stored parse length all return to empty/zero.
    pub(crate) fn reset_parse_state(&mut self) {
        self.session.reset();
        self.candidates.clear();
        self.parsed_len = 0;
        self.double_parse = None;
        self.double_input.clear();
        self.zhuyin_parse = None;
        self.zhuyin_input.clear();
    }

    /// The current live option word.
    pub(crate) fn options(&self) -> OptionBits {
        OptionBits::from_bits(self.options.load(Ordering::Relaxed))
    }
}

// ── Pointer casts ───────────────────────────────────────────────────────
//
// The opaque `PinyinContext` / `PinyinInstance` types in the C header are
// zero-sized sentinels. What the pointer actually addresses is a heap-
// allocated `CapiContext` / `CapiInstance`. These helpers centralise the
// cast so each call site stays readable.

/// Casts a `*mut PinyinContext` to `&CapiContext`.
///
/// # Safety
///
/// `ptr` must be non-null and produced by `Box::into_raw(Box::new(CapiContext { .. }))`.
/// The returned reference must not outlive the `Box` (i.e. must not be used
/// after `pinyin_fini` reconstructs and drops it), and must not be stored in
/// a `CapiInstance` or any other longer-lived location.
pub(crate) unsafe fn context_ref<'a>(ptr: *mut PinyinContext) -> &'a CapiContext {
    // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
    unsafe { &*(ptr.cast::<CapiContext>()) }
}

/// Casts a `*mut PinyinContext` to `&mut CapiContext`.
///
/// # Safety
///
/// `ptr` must be non-null and produced by `Box::into_raw(Box::new(CapiContext { .. }))`.
/// No other reference to the same context may exist, and the returned
/// reference must not outlive the `Box` (i.e. must not be used after
/// `pinyin_fini` reconstructs and drops it) or be stored in a `CapiInstance`.
pub(crate) unsafe fn context_mut<'a>(ptr: *mut PinyinContext) -> &'a mut CapiContext {
    // SAFETY: Caller guarantees the pointer is valid and unique for the chosen lifetime.
    unsafe { &mut *(ptr.cast::<CapiContext>()) }
}

/// Casts a `*mut PinyinInstance` to `&CapiInstance`.
///
/// # Safety
///
/// `ptr` must be non-null and produced by `Box::into_raw(Box::new(CapiInstance { .. }))`.
/// The returned reference must not outlive the `Box` (i.e. must not be used
/// after `pinyin_free_instance` reconstructs and drops it), and must not be
/// stored in any longer-lived location.
pub(crate) unsafe fn instance_ref<'a>(ptr: *mut PinyinInstance) -> &'a CapiInstance {
    // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
    unsafe { &*(ptr.cast::<CapiInstance>()) }
}

/// Casts a `*mut PinyinInstance` to `&mut CapiInstance`.
///
/// # Safety
///
/// `ptr` must be non-null and produced by `Box::into_raw(Box::new(CapiInstance { .. }))`.
/// No other reference to the same instance may exist, and the returned
/// reference must not outlive the `Box` (i.e. must not be used after
/// `pinyin_free_instance` reconstructs and drops it) or be stored elsewhere.
pub(crate) unsafe fn instance_mut<'a>(ptr: *mut PinyinInstance) -> &'a mut CapiInstance {
    // SAFETY: Caller guarantees the pointer is valid and unique for the chosen lifetime.
    unsafe { &mut *(ptr.cast::<CapiInstance>()) }
}

/// Converts a `CapiContext` into a `*mut PinyinContext` for return to C.
pub(crate) fn box_context(ctx: CapiContext) -> *mut PinyinContext {
    Box::into_raw(Box::new(ctx)).cast()
}

/// Converts a `CapiInstance` into a `*mut PinyinInstance` for return to C.
pub(crate) fn box_instance(inst: CapiInstance) -> *mut PinyinInstance {
    Box::into_raw(Box::new(inst)).cast()
}

/// Casts a `*mut LookupCandidate` back to `&CapiCandidate`.
///
/// # Safety
///
/// `ptr` must be non-null and point into an active `CapiInstance::candidates`
/// vec (produced by [`candidate_ptr`]).
pub(crate) unsafe fn candidate_ref<'a>(ptr: *mut LookupCandidate) -> &'a CapiCandidate {
    // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
    unsafe { &*(ptr.cast::<CapiCandidate>()) }
}

/// Returns a `*mut LookupCandidate` pointing to a `CapiCandidate`.
pub(crate) fn candidate_ptr(cand: &CapiCandidate) -> *mut LookupCandidate {
    (cand as *const CapiCandidate as *mut CapiCandidate).cast()
}
