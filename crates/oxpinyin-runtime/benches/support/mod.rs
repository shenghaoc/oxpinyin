//! Shared loaders for the window-scan performance benches.
#![allow(dead_code)]
//!
//! Benches refuse to start without the exported tables and the fetched
//! model20 cache: flat-100 fallback numbers would not describe the pinned
//! construction.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use oxpinyin_core::{Dictionary, LanguageModel, PhraseEntry, PhraseToken, SyllableKey};
use oxpinyin_data::lm::BigramTables;
use oxpinyin_data::{SystemDbm, SystemDictionary};
pub type ReadModel<'a> =
    oxpinyin_data::BigramLanguageModel<&'a oxpinyin_data::PhraseLibraries, &'a BigramTables>;
use oxpinyin_engine::{EmptyConfigSource, Session};

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

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `$PINYIN_EXPORT_DIR`, else `/tmp/oxpinyin-export`: a system data directory
/// for the compiled-in backend. Panics when it lacks one of the three DBMs.
pub fn export_dir() -> PathBuf {
    let dir = std::env::var_os("PINYIN_EXPORT_DIR")
        .map_or_else(|| PathBuf::from("/tmp/oxpinyin-export"), PathBuf::from);
    for dbm in [
        SystemDbm::PinyinIndex,
        SystemDbm::PhraseIndex,
        SystemDbm::Bigram,
    ] {
        let name = dbm.file_name();
        assert!(
            dir.join(&name).is_file(),
            "system data missing at {} ({name}); point PINYIN_EXPORT_DIR at a \
             libpinyin data dir of the same backend",
            dir.display()
        );
    }
    dir
}

/// Fetched model20 extraction: `$PINYIN_MODEL_DIR`, `$PINYIN_MODEL_CACHE/extracted`,
/// `$CARGO_TARGET_DIR/model20/extracted`, then `target/model20/extracted`.
/// Requires all eighteen model files. An invalid explicit directory is an error.
pub fn model_dir() -> PathBuf {
    let complete = |dir: &std::path::Path| {
        // Inventory of the pinned model20 archive; keep with tools/dev/model.py.
        [
            "art.table",
            "culture.table",
            "economy.table",
            "gb_char.table",
            "gbk_char.table",
            "geology.table",
            "history.table",
            "interpolation2.text",
            "life.table",
            "merged.table",
            "nature.table",
            "opengram.table",
            "people.table",
            "punct.table",
            "science.table",
            "society.table",
            "sport.table",
            "technology.table",
        ]
        .iter()
        .all(|name| dir.join(name).is_file())
    };
    let env = |name| std::env::var_os(name).map(PathBuf::from);
    if let Some(dir) = env("PINYIN_MODEL_DIR") {
        assert!(
            complete(&dir),
            "PINYIN_MODEL_DIR is not a complete model20 directory: {}; run just fetch-model",
            dir.display()
        );
        return dir;
    }
    let candidates = [
        env("PINYIN_MODEL_CACHE").map(|cache| cache.join("extracted")),
        env("CARGO_TARGET_DIR").map(|target| target.join("model20/extracted")),
        Some(repo_root().join("target/model20/extracted")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|dir| complete(dir))
        .expect("model cache missing; run just fetch-model")
}

/// Dictionary + real-frequency language model used by the pinned scan:
/// the system data directory's DBMs and chunk files, whose items carry
/// the real unigram counts.
pub fn load_real_tables() -> (SystemDictionary, BigramTables) {
    let export = export_dir();
    let dict = SystemDictionary::open(&export).expect("SystemDictionary opens");
    let mut lm = BigramTables::open(&export.join(SystemDbm::Bigram.file_name()))
        .expect("BigramLanguageModel opens");
    lm.set_lambda_from_table_conf(&export.join("table.conf"));
    assert!(
        lm.read_all(dict.libraries()).has_real_unigrams(),
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
    type PathMemo = ();

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
        self.stats.probe_ns.fetch_add(
            u64::try_from(started.elapsed().as_nanos()).expect("elapsed nanos fit u64"),
            Ordering::Relaxed,
        );
        self.stats.probes.fetch_add(1, Ordering::Relaxed);
        result
    }
}

/// Session over a counting dictionary and the real-frequency model.
pub fn counting_session<'a>(
    dict: CountingDict<'a>,
    lm: &'a BigramTables,
) -> Session<CountingDict<'a>, ReadModel<'a>> {
    let model = lm.read_all(dict.inner.libraries());
    Session::new(&EmptyConfigSource, dict, model).expect("Session::new")
}

/// Session over the real tables.
pub fn real_session<'a>(
    dict: &'a SystemDictionary,
    lm: &'a BigramTables,
) -> Session<&'a SystemDictionary, ReadModel<'a>> {
    Session::new(&EmptyConfigSource, dict, lm.read_all(dict.libraries())).expect("Session::new")
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

/// W2 parity inputs from the committed corpus, one per line; empty lines are
/// kept, because the empty input is a corpus member.
pub fn load_w2_inputs() -> Vec<String> {
    let corpus_dir = repo_root().join("tests/parity/corpus/inputs");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&corpus_dir)
        .expect("corpus directory")
        .map(|entry| entry.expect("corpus entry").path())
        .collect();
    files.sort();
    let mut inputs = Vec::new();
    for file in files {
        let bytes = std::fs::read(&file).expect("stratum");
        inputs.extend(bytes.split_inclusive(|byte| *byte == b'\n').map(|line| {
            String::from_utf8_lossy(line.strip_suffix(b"\n").unwrap_or(line)).into_owned()
        }));
    }
    assert!(!inputs.is_empty(), "W2 corpus is empty");
    inputs
}

/// Parse `interpolation2.text` from the fetched cache.
pub fn interpolation2_path() -> PathBuf {
    model_dir().join("interpolation2.text")
}
