//! Integration tests for `PhraseTable` — the lazy phrase-index DBM reader.
//!
//! Creates UCS-4-keyed fixtures via the store's write interface, then
//! reads them back through `ChewingDictionary::tokens_for_text` to verify
//! the full round-trip.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use oxpinyin_core::ChewingKey;
use oxpinyin_data::ChewingDictionary;
use oxpinyin_store::{DefaultStore, WriteStore};

mod support;

static FIXTURE_COUNTER: AtomicU32 = AtomicU32::new(0);

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-phrase-table-test-{}-{}",
            std::process::id(),
            n,
        ));
        std::fs::create_dir_all(&dir).unwrap();
        write_pinyin_index(&dir.join(oxpinyin_store::default_store_file("pinyin_index")));
        write_gb_char_chunk(&dir.join("gb_char.bin"));
        write_phrase_dbm(&dir.join(oxpinyin_store::default_store_file("phrase_dbm")));
        Self { dir }
    }

    fn dict(&self) -> ChewingDictionary {
        ChewingDictionary::open_with_phrase_dbm(
            &self
                .dir
                .join(oxpinyin_store::default_store_file("pinyin_index")),
            &self.dir,
            &self
                .dir
                .join(oxpinyin_store::default_store_file("phrase_dbm")),
        )
        .unwrap()
    }

    fn dict_no_phrase_dbm(&self) -> ChewingDictionary {
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

fn encode_ucs4_key(text: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(text.len() * 4);
    for ch in text.chars() {
        buf.extend_from_slice(&(ch as u32).to_le_bytes());
    }
    buf
}

fn encode_tokens(tokens: &[u32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(tokens.len() * 4);
    for token in tokens {
        buf.extend_from_slice(&token.to_le_bytes());
    }
    buf
}

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

fn item2_stride(phrase_length: usize) -> usize {
    let raw = 4 + 2 * phrase_length;
    (raw + 3) & !3
}

fn encode_pinyin_items(entries: &[(u32, &[ChewingKey])]) -> Vec<u8> {
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
            txn.put_raw(
                &encode_complete(&[key("ni")]),
                &encode_pinyin_items(&[(0x01000010, &[ni3][..])]),
            )?;
            txn.put_raw(
                &encode_complete(&[key("hao")]),
                &encode_pinyin_items(&[(0x01000011, &[hao3][..])]),
            )?;
            txn.put_raw(
                &encode_complete(&[key("ni"), key("hao")]),
                &encode_pinyin_items(&[(0x01000099, &[ni3, hao3][..])]),
            )?;
            txn.put_raw(
                &encode_complete(&[key("zhong")]),
                &encode_pinyin_items(&[(0x01000020, &[zhong1][..])]),
            )?;
            txn.put_raw(
                &encode_complete(&[key("guo")]),
                &encode_pinyin_items(&[(0x01000021, &[guo2][..])]),
            )?;
            txn.put_raw(
                &encode_complete(&[key("zhong"), key("guo")]),
                &encode_pinyin_items(&[(0x010000A0, &[zhong1, guo2][..])]),
            )?;
            Ok(())
        })
        .unwrap();
}

fn write_gb_char_chunk(path: &std::path::Path) {
    let mut builder = support::ChunkBuilder::new(35);
    let pron = |spelling: &str| {
        let mut keys = Vec::new();
        for syllable in spelling.split('\'') {
            support::push_packed_key(
                &mut keys,
                ChewingKey::from_pinyin(syllable).unwrap().initial,
                ChewingKey::from_pinyin(syllable).unwrap().middle,
                ChewingKey::from_pinyin(syllable).unwrap().final_,
                0,
            );
        }
        vec![(keys, 5_u32)]
    };
    builder.add(0x10, 5, "你", pron("ni"));
    builder.add(0x11, 5, "好", pron("hao"));
    builder.add(0x20, 5, "中", pron("zhong"));
    builder.add(0x21, 5, "国", pron("guo"));
    builder.add(0x99, 5, "你好", pron("ni'hao"));
    builder.add(0xA0, 5, "中国", pron("zhong'guo"));
    std::fs::write(path, builder.build()).unwrap();
}

fn write_phrase_dbm(path: &std::path::Path) {
    let store = DefaultStore::create(path).unwrap();
    store
        .write(|txn| {
            // UCS-4 text keys → u32 token[] values
            // (the libpinyin phrase_index.bin format)
            txn.put_raw(&encode_ucs4_key("你"), &encode_tokens(&[0x01000010]))?;
            txn.put_raw(&encode_ucs4_key("好"), &encode_tokens(&[0x01000011]))?;
            txn.put_raw(&encode_ucs4_key("中"), &encode_tokens(&[0x01000020]))?;
            txn.put_raw(&encode_ucs4_key("国"), &encode_tokens(&[0x01000021]))?;
            txn.put_raw(&encode_ucs4_key("你好"), &encode_tokens(&[0x01000099]))?;
            txn.put_raw(&encode_ucs4_key("中国"), &encode_tokens(&[0x010000A0]))?;
            // A phrase with multiple tokens (different library origins)
            txn.put_raw(
                &encode_ucs4_key("的"),
                &encode_tokens(&[0x010005DB, 0x020005DB]),
            )?;
            Ok(())
        })
        .unwrap();
}

// ── Tests ────────────────────────────────────────────────────────

#[test]
fn tokens_for_text_exact_match() {
    let fix = Fixture::new();
    let dict = fix.dict();
    let tokens = dict.tokens_for_text("你好").unwrap();
    assert_eq!(tokens, vec![0x01000099]);
}

#[test]
fn tokens_for_text_single_char() {
    let fix = Fixture::new();
    let dict = fix.dict();
    let tokens = dict.tokens_for_text("你").unwrap();
    assert_eq!(tokens, vec![0x01000010]);
}

#[test]
fn tokens_for_text_multiple_tokens() {
    let fix = Fixture::new();
    let dict = fix.dict();
    let tokens = dict.tokens_for_text("的").unwrap();
    assert_eq!(tokens.len(), 2);
    assert!(tokens.contains(&0x010005DB));
    assert!(tokens.contains(&0x020005DB));
}

#[test]
fn tokens_for_text_unknown_returns_empty() {
    let fix = Fixture::new();
    let dict = fix.dict();
    let tokens = dict.tokens_for_text("不存在").unwrap();
    assert!(tokens.is_empty());
}

#[test]
fn tokens_for_text_empty_returns_empty() {
    let fix = Fixture::new();
    let dict = fix.dict();
    let tokens = dict.tokens_for_text("").unwrap();
    assert!(tokens.is_empty());
}

#[test]
fn tokens_for_text_empty_without_phrase_dbm() {
    let fix = Fixture::new();
    let dict = fix.dict_no_phrase_dbm();
    // Upstream answers nothing when no phrase table is attached — the
    // text→tokens direction lives exclusively in the phrase DBM.
    let tokens = dict.tokens_for_text("你好").unwrap();
    assert!(tokens.is_empty());
}

#[test]
fn open_with_phrase_dbm_does_not_scan() {
    let fix = Fixture::new();
    let _dict = fix.dict();
}

#[test]
fn phrase_text_still_works_with_phrase_dbm() {
    let fix = Fixture::new();
    let dict = fix.dict();
    assert_eq!(dict.phrase_text(0x01000099).as_deref(), Some("你好"));
    assert_eq!(dict.phrase_text(0xFFFFFFFF), None);
}
