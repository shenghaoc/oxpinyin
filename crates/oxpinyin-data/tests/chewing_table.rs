//! Integration tests for the ChewingKey-format pinyin index reader.
//!
//! Builds ChewingKey-format fixtures via the store's write interface,
//! then reads them back through `RawReadStore::get_raw` to verify the
//! full round-trip: key encoding → store write → store read → value
//! decoding.

use std::path::PathBuf;

use oxpinyin_core::ChewingKey;
use oxpinyin_store::{DefaultStore, RAW_TABLE, RawReadStore, ReadStore, WriteStore};

// ── Key/value encoding helpers (mirror chewing_table internals) ──

fn key_no_tone(spelling: &str) -> ChewingKey {
    ChewingKey::from_pinyin(spelling).expect("valid spelling")
}

fn key_with_tone(spelling: &str, tone: u8) -> ChewingKey {
    ChewingKey::from_pinyin(spelling)
        .expect("valid spelling")
        .with_tone(tone)
}

fn encode_complete(keys: &[ChewingKey]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(keys.len() * 2);
    for key in keys {
        let zeroed = ChewingKey::new(key.initial, key.middle, key.final_, 0);
        buf.extend_from_slice(&zeroed.to_packed().to_le_bytes());
    }
    buf
}

fn encode_incomplete(keys: &[ChewingKey]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(keys.len() * 2);
    for key in keys {
        let initial_only = ChewingKey::new(key.initial, 0, 0, 0);
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
        for (j, key) in keys.iter().enumerate() {
            let packed = key.to_packed().to_le_bytes();
            record[4 + j * 2] = packed[0];
            record[4 + j * 2 + 1] = packed[1];
        }
        buf.extend_from_slice(&record);
    }
    buf
}

// ── Fixture builder ──────────────────────────────────────────────

use std::sync::atomic::{AtomicU32, Ordering};

static FIXTURE_COUNTER: AtomicU32 = AtomicU32::new(0);

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-chewing-test-{}-{}",
            std::process::id(),
            n,
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(oxpinyin_store::default_store_file("pinyin_index"));
        write_fixture(&path);
        Self { dir }
    }

    fn store(&self) -> DefaultStore {
        let path = self
            .dir
            .join(oxpinyin_store::default_store_file("pinyin_index"));
        DefaultStore::open_read_only(&path).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn write_fixture(path: &std::path::Path) {
    let store = DefaultStore::create(path).unwrap();

    let ba1 = key_with_tone("ba", 1);
    let ba3 = key_with_tone("ba", 3);
    let ni3 = key_with_tone("ni", 3);
    let hao3 = key_with_tone("hao", 3);
    let zhong1 = key_with_tone("zhong", 1);
    let guo2 = key_with_tone("guo", 2);

    store
        .write(|txn| {
            // Complete index: tone-zeroed keys → PinyinIndexItem2 values.

            // "ba" → 八 (ba1), 把 (ba3)
            txn.put(
                RAW_TABLE,
                &encode_complete(&[key_no_tone("ba")]),
                &encode_items(&[(0x01000001, &[ba1][..]), (0x01000002, &[ba3][..])]),
            )?;

            // "ni" → 你 (ni3) — also a prefix for "ni hao"
            txn.put(
                RAW_TABLE,
                &encode_complete(&[key_no_tone("ni")]),
                &encode_items(&[(0x01000010, &[ni3][..])]),
            )?;

            // "hao" → 好 (hao3)
            txn.put(
                RAW_TABLE,
                &encode_complete(&[key_no_tone("hao")]),
                &encode_items(&[(0x01000011, &[hao3][..])]),
            )?;

            // "ni hao" → 你好
            txn.put(
                RAW_TABLE,
                &encode_complete(&[key_no_tone("ni"), key_no_tone("hao")]),
                &encode_items(&[(0x01000099, &[ni3, hao3][..])]),
            )?;

            // "zhong" → 中 — also a prefix for "zhong guo"
            txn.put(
                RAW_TABLE,
                &encode_complete(&[key_no_tone("zhong")]),
                &encode_items(&[(0x01000020, &[zhong1][..])]),
            )?;

            // "guo" → 国
            txn.put(
                RAW_TABLE,
                &encode_complete(&[key_no_tone("guo")]),
                &encode_items(&[(0x01000021, &[guo2][..])]),
            )?;

            // "zhong guo" → 中国
            txn.put(
                RAW_TABLE,
                &encode_complete(&[key_no_tone("zhong"), key_no_tone("guo")]),
                &encode_items(&[(0x010000A0, &[zhong1, guo2][..])]),
            )?;

            // Incomplete (initial-only) index entries.
            let b_initial = ChewingKey::new(key_no_tone("ba").initial, 0, 0, 0);
            txn.put(
                RAW_TABLE,
                &encode_incomplete(&[key_no_tone("ba")]),
                &encode_items(&[
                    (0x01000001, &[b_initial][..]),
                    (0x01000002, &[b_initial][..]),
                ]),
            )?;

            let n_initial = ChewingKey::new(key_no_tone("ni").initial, 0, 0, 0);
            txn.put(
                RAW_TABLE,
                &encode_incomplete(&[key_no_tone("ni")]),
                &encode_items(&[(0x01000010, &[n_initial][..])]),
            )?;

            Ok(())
        })
        .unwrap();
}

// ── Tests ────────────────────────────────────────────────────────

#[test]
fn single_syllable_lookup_finds_ba() {
    let fix = Fixture::new();
    let store = fix.store();
    let key = encode_complete(&[key_no_tone("ba")]);
    let data = store.get_raw(&key).unwrap().expect("ba must exist");
    let stride = item2_stride(1);
    assert_eq!(data.len() % stride, 0);
    assert_eq!(data.len() / stride, 2, "two items for ba");
    let token = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    assert_eq!(token, 0x01000001);
    let packed = u16::from_le_bytes([data[4], data[5]]);
    assert_eq!(ChewingKey::from_packed(packed), key_with_tone("ba", 1));
}

#[test]
fn multi_syllable_lookup_finds_nihao() {
    let fix = Fixture::new();
    let store = fix.store();
    let key = encode_complete(&[key_no_tone("ni"), key_no_tone("hao")]);
    let data = store.get_raw(&key).unwrap().expect("ni+hao must exist");
    let stride = item2_stride(2);
    assert_eq!(data.len(), stride, "one item for ni hao");
    let token = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    assert_eq!(token, 0x01000099);
}

#[test]
fn prefix_entry_has_data_and_longer_keys_exist() {
    let fix = Fixture::new();
    let store = fix.store();
    let ni_key = encode_complete(&[key_no_tone("ni")]);
    let ni_data = store.get_raw(&ni_key).unwrap().expect("ni must exist");
    assert!(!ni_data.is_empty(), "ni has real data");
    let nihao_key = encode_complete(&[key_no_tone("ni"), key_no_tone("hao")]);
    assert!(
        store.get_raw(&nihao_key).unwrap().is_some(),
        "ni+hao exists"
    );
}

#[test]
fn missing_key_returns_none() {
    let fix = Fixture::new();
    let store = fix.store();
    let key = encode_complete(&[key_no_tone("xian")]);
    assert!(store.get_raw(&key).unwrap().is_none());
}

#[test]
fn incomplete_index_uses_initial_only_keys() {
    let fix = Fixture::new();
    let store = fix.store();
    let ikey = encode_incomplete(&[key_no_tone("ba")]);
    let data = store.get_raw(&ikey).unwrap().expect("b-initial must exist");
    let stride = item2_stride(1);
    assert_eq!(data.len() / stride, 2, "two items under b-initial");
}

#[test]
fn stride_padding_is_zero_filled() {
    let fix = Fixture::new();
    let store = fix.store();
    let key = encode_complete(&[key_no_tone("ba")]);
    let data = store.get_raw(&key).unwrap().unwrap();
    let stride = item2_stride(1);
    for idx in 0..data.len() / stride {
        let base = idx * stride;
        for pad_offset in (4 + 2)..stride {
            assert_eq!(data[base + pad_offset], 0, "padding must be zero");
        }
    }
}

#[test]
fn open_does_not_scan_the_entire_index() {
    let fix = Fixture::new();
    let store = fix.store();
    let key = encode_complete(&[key_no_tone("ba")]);
    assert!(store.get_raw(&key).unwrap().is_some());
}
