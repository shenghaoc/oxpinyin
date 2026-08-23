//! Shared loaders for the window-scan performance benches.
#![allow(dead_code)]
//!
//! Benches refuse to start without the exported tables and the fetched
//! model20 cache: flat-100 fallback numbers would not describe the pinned
//! construction.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use oxpinyin_core::{Dictionary, LanguageModel, PhraseEntry, PhraseToken, SyllableKey};
use oxpinyin_data::{BigramLanguageModel, LookupTable, SystemDictionary};
use oxpinyin_engine::{EmptyConfigSource, Session, StoragePaths};
use pinyin_oracle::model_cache;

/// Twenty committed-corpus lines spanning short, long, ambiguous,
/// apostrophe, incomplete, and junk-adjacent inputs.
pub const CYCLE_INPUTS: &[&str] = &[
    "ni",
    "wo",
    "de",
    "nihao",
    "zhongguo",
    "xian",
    "fangan",
    "xi'an",
    "bu'tian",
    "fan'gan",
    "n",
    "zh",
    "chongke",
    "caisho",
    "paolen",
    "waimenggu",
    "lenglan",
    "naoxion",
    "liangniejue",
    "chuaipengdengzaimiu",
];

/// `/tmp/oxpinyin-export` or `$PINYIN_EXPORT_DIR`.
pub fn export_dir() -> PathBuf {
    let dir = std::env::var_os("PINYIN_EXPORT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/oxpinyin-export"));
    for name in ["pinyin_index.redb", "phrase_index.redb", "bigram.redb"] {
        assert!(
            dir.join(name).is_file(),
            "exported tables missing at {} ({name}); tables are committed under fixtures/w3/",
            dir.display()
        );
    }
    dir
}

/// Fetched model20 extraction; panics if the cache is absent.
pub fn model_dir() -> PathBuf {
    model_cache::locate_model_dir()
        .expect("PINYIN_MODEL_DIR / cache lookup")
        .expect("model cache missing; run tools/model/fetch-model.sh")
}

/// Dictionary + real-frequency language model used by the pinned scan.
pub fn load_real_tables() -> (SystemDictionary, BigramLanguageModel) {
    let export = export_dir();
    let model = model_dir();
    let dict = SystemDictionary::open(
        &export.join("pinyin_index.redb"),
        &export.join("phrase_index.redb"),
    )
    .expect("SystemDictionary opens");
    let mut lm =
        BigramLanguageModel::open(&export.join("bigram.redb")).expect("BigramLanguageModel opens");
    lm.set_unigrams_from_interpolation2(&model.join("interpolation2.text"))
        .expect("interpolation2.text parses");
    assert!(
        lm.has_real_unigrams(),
        "loaded model must expose real unigrams"
    );
    (dict, lm)
}

/// A [`SystemDictionary`] that counts prefix probes and lookups.
pub struct CountingDict<'a> {
    /// Inner dictionary.
    pub inner: &'a SystemDictionary,
    /// Counters.
    pub stats: &'a ProbeStats,
}

/// Atomic counters for [`CountingDict`].
#[derive(Debug, Default)]
pub struct ProbeStats {
    /// `phrase_prefix_exists` calls.
    pub probes: AtomicU64,
    /// Nanoseconds spent inside `phrase_prefix_exists`.
    pub probe_ns: AtomicU64,
    /// `lookup` calls.
    pub lookups: AtomicU64,
    /// Sum of entries returned by `lookup`.
    pub lookup_entries: AtomicU64,
}

impl ProbeStats {
    /// Snapshot as ordinary integers.
    pub fn snapshot(&self) -> (u64, u64, u64, u64) {
        (
            self.probes.load(Ordering::Relaxed),
            self.probe_ns.load(Ordering::Relaxed),
            self.lookups.load(Ordering::Relaxed),
            self.lookup_entries.load(Ordering::Relaxed),
        )
    }
}

impl Dictionary for CountingDict<'_> {
    type Syllable = SyllableKey;
    type Entry = PhraseEntry;
    type Error = oxpinyin_data::DictError;

    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        self.stats.lookups.fetch_add(1, Ordering::Relaxed);
        let entries = self.inner.lookup(syllables)?;
        self.stats
            .lookup_entries
            .fetch_add(entries.len() as u64, Ordering::Relaxed);
        Ok(entries)
    }

    fn phrase_prefix_exists(&self, syllables: &[Self::Syllable]) -> Result<bool, Self::Error> {
        let started = std::time::Instant::now();
        let result = self.inner.phrase_prefix_exists(syllables);
        self.stats
            .probe_ns
            .fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        self.stats.probes.fetch_add(1, Ordering::Relaxed);
        result
    }
}

/// Session over a counting dictionary and the real-frequency model.
pub fn counting_session<'a>(
    dict: CountingDict<'a>,
    lm: &'a BigramLanguageModel,
) -> Session<CountingDict<'a>, &'a BigramLanguageModel> {
    Session::new(&EmptyConfigSource, StoragePaths::new("user"), dict, lm).expect("Session::new")
}

/// Session over the real tables.
pub fn real_session<'a>(
    dict: &'a SystemDictionary,
    lm: &'a BigramLanguageModel,
) -> Session<&'a SystemDictionary, &'a BigramLanguageModel> {
    Session::new(&EmptyConfigSource, StoragePaths::new("user"), dict, lm).expect("Session::new")
}

/// Types `input` one character at a time, refreshing after each.
pub fn type_keystrokes<D, L>(session: &mut Session<D, L>, input: &str)
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: std::fmt::Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: std::fmt::Display,
{
    session.reset();
    for character in input.chars() {
        let mut buf = [0_u8; 4];
        let _ = session.type_pinyin(character.encode_utf8(&mut buf));
    }
}

/// Types the whole input in one refresh, matching the parity harness.
pub fn type_batch<D, L>(session: &mut Session<D, L>, input: &str)
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: std::fmt::Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: std::fmt::Display,
{
    session.reset();
    let _ = session.type_pinyin(input);
}

/// Rebuilds the two prefix-probe tables the same way `SystemDictionary` does.
pub fn load_prefix_tables() -> (Box<[String]>, Box<[String]>) {
    let export = export_dir();
    let index = LookupTable::open(&export.join("pinyin_index.redb")).expect("pinyin_index");
    let mut pinyin_keys = Vec::new();
    let mut initial_keys = Vec::new();
    for (key, _value) in index.iter() {
        let pinyin = std::str::from_utf8(key).expect("utf-8 key").to_owned();
        let mut initial = String::new();
        for (position, syllable) in pinyin.split('\'').enumerate() {
            if position > 0 {
                initial.push('\'');
            }
            match syllable_initial(syllable) {
                Some(prefix) => initial.push_str(prefix),
                None => initial.push('0'),
            }
        }
        pinyin_keys.push(pinyin);
        initial_keys.push(initial);
    }
    pinyin_keys.sort_unstable();
    pinyin_keys.dedup();
    initial_keys.sort_unstable();
    initial_keys.dedup();
    (
        pinyin_keys.into_boxed_slice(),
        initial_keys.into_boxed_slice(),
    )
}

fn syllable_initial(text: &str) -> Option<&'static str> {
    oxpinyin_core::syllable_initial(text)
}

/// Heap bytes of a `Box<[String]>`: slice header + each `String`'s buffer.
pub fn string_table_bytes(keys: &[String]) -> usize {
    let headers = std::mem::size_of_val(keys);
    let buffers: usize = keys.iter().map(|key| key.capacity()).sum();
    headers + buffers
}

/// The `SEARCH_CONTINUED` probe, copied so benches can time it in isolation.
pub fn prefix_probe(sorted: &[String], joined: &str) -> bool {
    match sorted.binary_search_by(|candidate| candidate.as_str().cmp(joined)) {
        Ok(_) => true,
        Err(index) => {
            sorted
                .get(index)
                .is_some_and(|candidate| candidate.starts_with(joined))
                && sorted[index].as_bytes().get(joined.len()) == Some(&b'\'')
        }
    }
}

/// W2 parity inputs from the committed corpus.
///
/// Reads the committed stratum files directly instead of running
/// `corpus::generate()`: the generator re-parses the syllable inventory to
/// discover file names, and those allocations must not land in a profile
/// that wraps the decode loop.
pub fn load_w2_inputs() -> Vec<String> {
    use pinyin_oracle::corpus;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let corpus_dir = root.join(corpus::CORPUS_DIR);
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&corpus_dir)
        .expect("corpus directory")
        .map(|entry| entry.expect("corpus entry").path())
        .collect();
    files.sort();
    let mut inputs = Vec::new();
    for file in files {
        let bytes = std::fs::read(&file).expect("stratum");
        inputs.extend(corpus::Stratum::parse_file_bytes(&bytes));
    }
    assert!(!inputs.is_empty(), "W2 corpus is empty");
    inputs
}

/// Parse `interpolation2.text` from the fetched cache.
pub fn interpolation2_path() -> PathBuf {
    model_dir().join("interpolation2.text")
}
