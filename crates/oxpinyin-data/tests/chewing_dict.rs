//! Integration tests for `ChewingDictionary` — the lazy P2 dictionary.
//!
//! Creates a pair of ChewingKey-format pinyin-index and phrase-index
//! fixtures, then exercises the `Dictionary` trait through
//! `ChewingDictionary`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use oxpinyin_core::{ChewingKey, Completeness, Dictionary, SyllableKey};
use oxpinyin_data::ChewingDictionary;
use oxpinyin_store::{DefaultStore, WriteStore};

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
        write_phrase_index(&dir.join(oxpinyin_store::default_store_file("phrase_index")));
        Self { dir }
    }

    fn dict(&self) -> ChewingDictionary {
        ChewingDictionary::open(
            &self
                .dir
                .join(oxpinyin_store::default_store_file("pinyin_index")),
            &self
                .dir
                .join(oxpinyin_store::default_store_file("phrase_index")),
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
    let store = DefaultStore::create(path).unwrap();
    let ni3 = key("ni").with_tone(3);
    let hao3 = key("hao").with_tone(3);
    let zhong1 = key("zhong").with_tone(1);
    let guo2 = key("guo").with_tone(2);

    store
        .write(|txn| {
            // Complete index entries.
            txn.put_raw(
                &encode_complete(&[key("ni")]),
                &encode_items(&[(0x01000010, &[ni3][..])]),
            )?;
            txn.put_raw(
                &encode_complete(&[key("hao")]),
                &encode_items(&[(0x01000011, &[hao3][..])]),
            )?;
            txn.put_raw(
                &encode_complete(&[key("ni"), key("hao")]),
                &encode_items(&[(0x01000099, &[ni3, hao3][..])]),
            )?;
            txn.put_raw(
                &encode_complete(&[key("zhong")]),
                &encode_items(&[(0x01000020, &[zhong1][..])]),
            )?;
            txn.put_raw(
                &encode_complete(&[key("guo")]),
                &encode_items(&[(0x01000021, &[guo2][..])]),
            )?;
            txn.put_raw(
                &encode_complete(&[key("zhong"), key("guo")]),
                &encode_items(&[(0x010000A0, &[zhong1, guo2][..])]),
            )?;

            // Incomplete index entries.
            let n_init = ChewingKey::new(key("ni").initial, 0, 0, 0);
            txn.put_raw(
                &encode_incomplete(&[key("ni")]),
                &encode_items(&[(0x01000010, &[n_init][..])]),
            )?;

            Ok(())
        })
        .unwrap();
}

fn write_phrase_index(path: &std::path::Path) {
    let store = DefaultStore::create(path).unwrap();
    store
        .write(|txn| {
            txn.put("data", &0x01000010_u32.to_le_bytes(), "你".as_bytes())?;
            txn.put("data", &0x01000011_u32.to_le_bytes(), "好".as_bytes())?;
            txn.put("data", &0x01000020_u32.to_le_bytes(), "中".as_bytes())?;
            txn.put("data", &0x01000021_u32.to_le_bytes(), "国".as_bytes())?;
            txn.put("data", &0x01000099_u32.to_le_bytes(), "你好".as_bytes())?;
            txn.put("data", &0x010000A0_u32.to_le_bytes(), "中国".as_bytes())?;
            Ok(())
        })
        .unwrap();
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
    assert_eq!(dict.phrase_text(0x01000099), Some("你好"));
    assert_eq!(dict.phrase_text(0xFFFFFFFF), None);
}
