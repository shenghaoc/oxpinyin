//! Round-trip test: datagen → compat conversion → P2/P3 readers.
//!
//! Verifies that the compatibility conversion produces data readable
//! by the P2 ChewingTable and P3 PhraseTable readers.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use oxpinyin_core::ChewingKey;
use oxpinyin_datagen::compat::{convert_phrase_index, convert_pinyin_index};
use oxpinyin_store::{DefaultStore, RAW_TABLE, RawReadStore, ReadStore, WriteStore};

static FIXTURE_COUNTER: AtomicU32 = AtomicU32::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-compat-{}-{}-{}",
            prefix,
            std::process::id(),
            n,
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn make_pinyin_entry(pinyin: &str, records: &[(u32, u32)]) -> (Vec<u8>, Vec<u8>) {
    let key = pinyin.as_bytes().to_vec();
    let mut value = Vec::new();
    for &(token, freq) in records {
        value.extend_from_slice(&token.to_le_bytes());
        value.extend_from_slice(&freq.to_le_bytes());
    }
    (key, value)
}

fn make_phrase_entry(token: u32, text: &str) -> (Vec<u8>, Vec<u8>) {
    (token.to_le_bytes().to_vec(), text.as_bytes().to_vec())
}

fn encode_complete_key(keys: &[ChewingKey]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(keys.len() * 2);
    for key in keys {
        let zeroed = ChewingKey::new(key.initial, key.middle, key.final_, 0);
        buf.extend_from_slice(&zeroed.to_packed().to_le_bytes());
    }
    buf
}

fn encode_ucs4_key(text: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(text.len() * 4);
    for ch in text.chars() {
        buf.extend_from_slice(&(ch as u32).to_le_bytes());
    }
    buf
}

fn item2_stride(phrase_length: usize) -> usize {
    let raw = 4 + 2 * phrase_length;
    (raw + 3) & !3
}

#[test]
fn pinyin_index_round_trip_through_store() {
    let entries = vec![
        make_pinyin_entry("ni", &[(0x01000010, 100)]),
        make_pinyin_entry("hao", &[(0x01000011, 80)]),
        make_pinyin_entry("ni'hao", &[(0x01000099, 50)]),
    ];

    let converted = convert_pinyin_index(&entries).unwrap();
    assert!(!converted.is_empty());

    let dir = TempDir::new("pinyin");
    let path = dir
        .path
        .join(oxpinyin_store::default_store_file("pinyin_compat"));
    let store = DefaultStore::create(&path).unwrap();
    store
        .write(|txn| {
            for (key, value) in &converted {
                txn.put(RAW_TABLE, key, value)?;
            }
            Ok(())
        })
        .unwrap();
    drop(store);

    let store = DefaultStore::open_read_only(&path).unwrap();

    let ni = ChewingKey::from_pinyin("ni").unwrap();
    let hao = ChewingKey::from_pinyin("hao").unwrap();

    // Single syllable lookup
    let ni_key = encode_complete_key(&[ni]);
    let ni_value = store.get_raw(&ni_key).unwrap();
    assert!(ni_value.is_some(), "ni key must exist");
    let ni_data = ni_value.unwrap();
    let stride = item2_stride(1);
    assert!(ni_data.len().is_multiple_of(stride));
    let token = u32::from_le_bytes([ni_data[0], ni_data[1], ni_data[2], ni_data[3]]);
    assert_eq!(token, 0x01000010);

    // Multi-syllable lookup
    let nihao_key = encode_complete_key(&[ni, hao]);
    let nihao_value = store.get_raw(&nihao_key).unwrap();
    assert!(nihao_value.is_some(), "ni'hao key must exist");
    let nihao_data = nihao_value.unwrap();
    let stride2 = item2_stride(2);
    assert_eq!(nihao_data.len(), stride2);
    let nihao_token =
        u32::from_le_bytes([nihao_data[0], nihao_data[1], nihao_data[2], nihao_data[3]]);
    assert_eq!(nihao_token, 0x01000099);

    // Prefix marker for "ni" (from ni'hao) — ni already has real data,
    // so the marker overlaps with the real entry.
    let ni_exists = store.get_raw(&ni_key).unwrap().is_some();
    assert!(ni_exists, "ni must exist (real entry or marker)");
}

#[test]
fn phrase_index_round_trip_through_store() {
    let entries = vec![
        make_phrase_entry(0x01000010, "你"),
        make_phrase_entry(0x01000011, "好"),
        make_phrase_entry(0x01000099, "你好"),
    ];

    let converted = convert_phrase_index(&entries).unwrap();
    assert!(!converted.is_empty());

    let dir = TempDir::new("phrase");
    let path = dir
        .path
        .join(oxpinyin_store::default_store_file("phrase_compat"));
    let store = DefaultStore::create(&path).unwrap();
    store
        .write(|txn| {
            for (key, value) in &converted {
                txn.put(RAW_TABLE, key, value)?;
            }
            Ok(())
        })
        .unwrap();
    drop(store);

    let store = DefaultStore::open_read_only(&path).unwrap();

    // UCS-4 text key → token lookup
    let ni_key = encode_ucs4_key("你");
    let ni_value = store.get_raw(&ni_key).unwrap();
    assert!(ni_value.is_some(), "你 key must exist");
    let ni_tokens = ni_value.unwrap();
    assert_eq!(ni_tokens.len(), 4, "one token");
    let token = u32::from_le_bytes([ni_tokens[0], ni_tokens[1], ni_tokens[2], ni_tokens[3]]);
    assert_eq!(token, 0x01000010);

    // Multi-char phrase
    let nihao_key = encode_ucs4_key("你好");
    let nihao_value = store.get_raw(&nihao_key).unwrap();
    assert!(nihao_value.is_some());
    let nihao_tokens = nihao_value.unwrap();
    assert_eq!(nihao_tokens.len(), 4);
    let nihao_token = u32::from_le_bytes([
        nihao_tokens[0],
        nihao_tokens[1],
        nihao_tokens[2],
        nihao_tokens[3],
    ]);
    assert_eq!(nihao_token, 0x01000099);

    // Missing phrase
    let missing = store.get_raw(&encode_ucs4_key("不存在")).unwrap();
    assert!(missing.is_none());
}

#[test]
fn bigram_format_is_already_compatible() {
    let prev_token: u32 = 0x01000010;
    let total: u32 = 100;
    let next: u32 = 0x01000099;
    let count: u32 = 100;

    let mut value = Vec::new();
    value.extend_from_slice(&total.to_le_bytes());
    value.extend_from_slice(&next.to_le_bytes());
    value.extend_from_slice(&count.to_le_bytes());

    let dir = TempDir::new("bigram");
    let path = dir
        .path
        .join(oxpinyin_store::default_store_file("bigram_compat"));
    let store = DefaultStore::create(&path).unwrap();
    store
        .write(|txn| {
            txn.put(RAW_TABLE, &prev_token.to_le_bytes(), &value)?;
            Ok(())
        })
        .unwrap();
    drop(store);

    let store = DefaultStore::open_read_only(&path).unwrap();
    let result = store.get_raw(&prev_token.to_le_bytes()).unwrap();
    assert!(result.is_some());
    let data = result.unwrap();
    assert_eq!(data.len(), 12);
    let read_total = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    assert_eq!(read_total, 100);
}

#[test]
fn punct_format_is_already_compatible() {
    let token: u32 = 0x01000295;
    let value = b"\xef\xbc\x8c\x00\xe3\x80\x82\x00"; // ，\0。\0

    let dir = TempDir::new("punct");
    let path = dir
        .path
        .join(oxpinyin_store::default_store_file("punct_compat"));
    let store = DefaultStore::create(&path).unwrap();
    store
        .write(|txn| {
            txn.put(RAW_TABLE, &token.to_le_bytes(), value)?;
            Ok(())
        })
        .unwrap();
    drop(store);

    let store = DefaultStore::open_read_only(&path).unwrap();
    let result = store.get_raw(&token.to_le_bytes()).unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap(), value.to_vec());
}
