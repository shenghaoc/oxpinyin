//! Table loading against the committed `fixtures/w3` binaries.
//!
//! Public-API integration tests moved out of the `src/` inline test mods;
//! each source file keeps the tests that pin private helpers
//! (`parse_header`/`record_size`, `decode_puncts`, `LeByteKey`).

use std::path::{Path, PathBuf};

use oxpinyin_data::{ContentTable, LookupTable, PunctTable};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures")
        .join("w3")
}

#[test]
fn load_all_fixtures() {
    let dir = fixtures_dir();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("bin") {
            continue;
        }
        let data = std::fs::read(&path).unwrap();
        let table = ContentTable::load(&data).unwrap_or_else(|e| {
            panic!("failed to load {}: {e}", path.display());
        });
        assert!(!table.is_empty(), "{} should have records", path.display());
        assert_eq!(
            table.version(),
            17,
            "{} should be version 17",
            path.display()
        );
    }
}

#[test]
fn culture_fixture_spot_check() {
    // culture.bin: 34 records, mostly fl=1 (compact format), one fl=2
    let dir = fixtures_dir();
    let data = std::fs::read(dir.join("culture.bin")).unwrap();
    let table = ContentTable::load(&data).unwrap();

    assert_eq!(table.len(), 34);
    assert_eq!(table.version(), 17);

    // First record: ng=2, fl=1, phrase_freq=1
    let r0 = table.get(0).unwrap();
    assert_eq!(r0.n_gram, 2);
    assert_eq!(r0.flags, 1);
    assert_eq!(r0.phrase_frequency, 1);
    assert_eq!(r0.tokens.len(), 2);
    // token[0] = 0x5B89, freq=0x517B=20859
    assert_eq!(r0.tokens[0].token, 0x5B89);
    assert_eq!(r0.tokens[0].frequency, 20859);
    // token[1] = 0x02350180, freq=100 (compact u16)
    assert_eq!(r0.tokens[1].token, 0x02350180);
    assert_eq!(r0.tokens[1].frequency, 100);

    // Record 2 (index 2): ng=2, fl=2 (3 token pairs: n_gram + 1 extra)
    let r2 = table.get(2).unwrap();
    assert_eq!(r2.n_gram, 2);
    assert_eq!(r2.flags, 2);
    assert_eq!(r2.tokens.len(), 3);
}

// ── PunctTable ──────────────────────────────────────────────────

#[test]
fn fixture_has_hao_and_de_in_table_order() {
    let table = PunctTable::open(&fixtures_dir().join("punct.redb")).unwrap();
    assert!(
        table.token_count() >= 3,
        "Option A export keeps the 好/中/国 tokens used by the punct differential"
    );
    assert_eq!(
        table.punctuations(16_779_429),
        ["，".to_owned(), "。".to_owned()]
    );
    assert_eq!(
        table.punctuations(16_778_715),
        [
            "，".to_owned(),
            "。".to_owned(),
            "“".to_owned(),
            "、".to_owned(),
            "；".to_owned()
        ]
    );
    assert!(table.punctuations(0).is_empty());
}

#[test]
fn missing_file_is_empty() {
    let table = PunctTable::open_optional(Path::new("/no/such/punct.redb"));
    assert!(table.is_empty());
}

// ── GenericLookupTable ──────────────────────────────────────────

#[test]
fn open_mini_index_fixture() {
    let table = LookupTable::open(&fixtures_dir().join("pinyin_index.redb")).unwrap();
    // The --mini export keeps the ten allowlisted pinyin keys.
    assert_eq!(table.len().unwrap(), 10);
    assert!(!table.is_empty().unwrap());
}

#[test]
fn keys_are_pinyin_strings() {
    let table = LookupTable::open(&fixtures_dir().join("pinyin_index.redb")).unwrap();
    let val = table.get(b"ni'hao").unwrap();
    assert!(val.is_some(), "ni'hao is in the mini allowlist");
    // Records are 8-byte {token, freq} pairs.
    assert_eq!(val.unwrap().len() % 8, 0);
}

#[test]
fn missing_key_returns_none() {
    let table = LookupTable::open(&fixtures_dir().join("pinyin_index.redb")).unwrap();
    let val = table.get(b"nonexistent").unwrap();
    assert!(val.is_none());
}

#[test]
fn iter_all_fixture_files() {
    let dir = fixtures_dir();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("redb") {
            continue;
        }
        let table = LookupTable::open(&path).unwrap_or_else(|e| {
            panic!("failed to open {}: {e}", path.display());
        });
        let count = table.len().unwrap();
        assert!(count > 0, "{} should have records", path.display());

        // Verify iteration matches count.
        let entries = table.iter().count();
        assert_eq!(entries as u64, count);
    }
}
