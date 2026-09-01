//! Integration tests for `ChewingDictionary` — the lazy P2/P3
//! dictionary.
//!
//! Creates a ChewingKey-format pinyin-index DBM (bare keyspace rows)
//! and a gb_char phrase-library chunk file, then exercises the
//! `Dictionary` trait through `ChewingDictionary`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use oxpinyin_core::{ChewingKey, Completeness, Dictionary, SyllableKey};
use oxpinyin_data::ChewingDictionary;
use oxpinyin_store::DefaultStore;

mod support;
use support::{ChunkBuilder, write_raw_rows};

static FIXTURE_COUNTER: AtomicU32 = AtomicU32::new(0);

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-chewing-dict-test-{}-{}",
            std::process::id(),
            n,
        ));
        std::fs::create_dir_all(&dir).unwrap();
        write_pinyin_index(&dir.join(oxpinyin_store::default_store_file("pinyin_index")));
        write_gb_char_chunk(&dir.join("gb_char.bin"));
        Self { dir }
    }

    fn dict(&self) -> ChewingDictionary {
        ChewingDictionary::open(
            &self
                .dir
                .join(oxpinyin_store::default_store_file("pinyin_index")),
            &self.dir,
        )
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// ── Encoding helpers ─────────────────────────────────────────────

fn key(spelling: &str) -> ChewingKey {
    ChewingKey::from_pinyin(spelling).unwrap()
}

fn encode_complete(keys: &[ChewingKey]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(keys.len() * 2);
    for k in keys {
        let zeroed = ChewingKey::new(k.initial, k.middle, k.final_, 0);
        buf.extend_from_slice(&zeroed.to_packed().to_le_bytes());
    }
    buf
}

fn encode_incomplete(keys: &[ChewingKey]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(keys.len() * 2);
    for k in keys {
        let initial_only = ChewingKey::new(k.initial, 0, 0, 0);
        buf.extend_from_slice(&initial_only.to_packed().to_le_bytes());
    }
    buf
}

fn item2_stride(phrase_length: usize) -> usize {
    let raw = 4 + 2 * phrase_length;
    (raw + 3) & !3
}

fn encode_items(entries: &[(u32, &[ChewingKey])]) -> Vec<u8> {
    let mut buf = Vec::new();
    for (token, keys) in entries {
        let stride = item2_stride(keys.len());
        let mut record = vec![0u8; stride];
        record[..4].copy_from_slice(&token.to_le_bytes());
        for (j, k) in keys.iter().enumerate() {
            let packed = k.to_packed().to_le_bytes();
            record[4 + j * 2] = packed[0];
            record[4 + j * 2 + 1] = packed[1];
        }
        buf.extend_from_slice(&record);
    }
    buf
}

// ── Fixture writers ──────────────────────────────────────────────

fn write_pinyin_index(path: &std::path::Path) {
    let ni3 = key("ni").with_tone(3);
    let hao3 = key("hao").with_tone(3);
    let zhong1 = key("zhong").with_tone(1);
    let guo2 = key("guo").with_tone(2);

    // Bare keyspace rows, the libpinyin DBM layout (no table framing).
    let rows: Vec<(Vec<u8>, Vec<u8>)> = vec![
        // Complete index entries.
        (
            encode_complete(&[key("ni")]),
            encode_items(&[(0x01000010, &[ni3][..])]),
        ),
        (
            encode_complete(&[key("hao")]),
            encode_items(&[(0x01000011, &[hao3][..])]),
        ),
        (
            encode_complete(&[key("ni"), key("hao")]),
            encode_items(&[(0x01000099, &[ni3, hao3][..])]),
        ),
        (
            encode_complete(&[key("zhong")]),
            encode_items(&[(0x01000020, &[zhong1][..])]),
        ),
        (
            encode_complete(&[key("guo")]),
            encode_items(&[(0x01000021, &[guo2][..])]),
        ),
        (
            encode_complete(&[key("zhong"), key("guo")]),
            encode_items(&[(0x010000A0, &[zhong1, guo2][..])]),
        ),
        // Incomplete index entry.
        (
            encode_incomplete(&[key("ni")]),
            encode_items(&[(
                0x01000010,
                &[ChewingKey::new(key("ni").initial, 0, 0, 0)][..],
            )]),
        ),
    ];
    write_raw_rows::<DefaultStore>(path, &rows);
}

/// The gb_char chunk file: the six nibble-1 tokens with their text and
/// one pronunciation each. `total_freq` is the sum of the stored
/// unigrams, exactly what `SubPhraseIndex::store` records.
fn write_gb_char_chunk(path: &std::path::Path) {
    let ni3 = key("ni").with_tone(3);
    let hao3 = key("hao").with_tone(3);
    let zhong1 = key("zhong").with_tone(1);
    let guo2 = key("guo").with_tone(2);

    let pron1 = |key: ChewingKey| vec![(key.to_packed().to_le_bytes().to_vec(), 5_u32)];
    let pron2 = |a: ChewingKey, b: ChewingKey| {
        let mut keys = a.to_packed().to_le_bytes().to_vec();
        keys.extend_from_slice(&b.to_packed().to_le_bytes());
        vec![(keys, 5_u32)]
    };

    let mut builder = ChunkBuilder::new(30);
    builder.add(0x10, 5, "你", pron1(ni3));
    builder.add(0x11, 5, "好", pron1(hao3));
    builder.add(0x20, 5, "中", pron1(zhong1));
    builder.add(0x21, 5, "国", pron1(guo2));
    builder.add(0x99, 5, "你好", pron2(ni3, hao3));
    builder.add(0xA0, 5, "中国", pron2(zhong1, guo2));
    std::fs::write(path, builder.build()).unwrap();
}

// ── Helpers ──────────────────────────────────────────────────────

fn syl(text: &str) -> SyllableKey {
    SyllableKey::from_text(text).expect("frozen syllable")
}

fn syls(text: &str) -> Vec<SyllableKey> {
    text.split(',').map(syl).collect()
}

// ── Tests ────────────────────────────────────────────────────────

#[test]
fn open_does_not_scan_pinyin_index() {
    let fix = Fixture::new();
    let _dict = fix.dict();
}

#[test]
fn single_syllable_lookup() {
    let fix = Fixture::new();
    let dict = fix.dict();
    let entries = dict.lookup(&syls("ni")).unwrap();
    assert!(!entries.is_empty(), "ni must find 你");
    assert_eq!(entries[0].text(), "你");
}

#[test]
fn multi_syllable_lookup() {
    let fix = Fixture::new();
    let dict = fix.dict();
    let entries = dict.lookup(&syls("ni,hao")).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].text(), "你好");
}

#[test]
fn empty_lookup_returns_empty() {
    let fix = Fixture::new();
    let dict = fix.dict();
    let entries = dict.lookup(&[]).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn unknown_syllable_returns_empty() {
    let fix = Fixture::new();
    let dict = fix.dict();
    let entries = dict.lookup(&syls("xian")).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn phrase_prefix_exists_for_prefix() {
    let fix = Fixture::new();
    let dict = fix.dict();
    // "ni" is a prefix of "ni hao" — should report continuation.
    assert!(dict.phrase_prefix_exists(&syls("ni")).unwrap());
}

#[test]
fn phrase_prefix_exists_for_exact_match() {
    let fix = Fixture::new();
    let dict = fix.dict();
    assert!(dict.phrase_prefix_exists(&syls("ni,hao")).unwrap());
}

#[test]
fn phrase_prefix_does_not_exist_for_dead_end() {
    let fix = Fixture::new();
    let dict = fix.dict();
    assert!(!dict.phrase_prefix_exists(&syls("guo,guo")).unwrap());
}

#[test]
fn phrase_prefix_exists_for_empty_sequence() {
    let fix = Fixture::new();
    let dict = fix.dict();
    assert!(dict.phrase_prefix_exists(&[]).unwrap());
}

#[test]
fn incomplete_key_prefix_exists() {
    let fix = Fixture::new();
    let dict = fix.dict();
    let n = SyllableKey::from_text("n").expect("initial-only key");
    assert_eq!(n.completeness(), Completeness::Partial);
    assert!(dict.phrase_prefix_exists(&[n]).unwrap());
}

#[test]
fn lookup_into_matches_lookup() {
    let fix = Fixture::new();
    let dict = fix.dict();
    let ni = syls("ni");
    let lookup_result = dict.lookup(&ni).unwrap();
    let mut into_result = Vec::new();
    dict.lookup_into(&ni, &mut into_result).unwrap();
    assert_eq!(lookup_result.len(), into_result.len());
    for (a, b) in lookup_result.iter().zip(into_result.iter()) {
        assert_eq!(a.text(), b.text());
        assert_eq!(a.token(), b.token());
    }
}

#[test]
fn phrase_text_resolves_token() {
    let fix = Fixture::new();
    let dict = fix.dict();
    assert_eq!(dict.phrase_text(0x01000099).as_deref(), Some("你好"));
    assert_eq!(dict.phrase_text(0xFFFFFFFF), None);
}
