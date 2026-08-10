//! Verification of the exported tables against the live oracle.
//!
//! `docs/findings/data-layer-export.md` §Verification:
//!
//! 1. The dictionary tables round-trip: an independent re-derivation from
//!    the live oracle's public ABI must equal `pinyin_index.redb` and
//!    `phrase_index.redb` record for record.
//! 2. Every `bigram.redb` entry parses under the frozen schema with
//!    `total == Σ count`, every successor belongs to a system library,
//!    and resolved spot checks hold for three common previous tokens.
//!
//! The tests skip with a notice when `/tmp/pinyin-rs-export` is absent;
//! generate it with
//! `cargo run -p pinyin-migrate --features oracle-ffi -- export --out-dir /tmp/pinyin-rs-export`.

#![cfg(feature = "oracle-ffi")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pinyin_data::LookupTable;
use pinyin_oracle::{Oracle, OracleFlags, OraclePrefix};

fn export_dir() -> Option<PathBuf> {
    let dir = Path::new("/tmp/pinyin-rs-export").to_path_buf();
    if dir.join("pinyin_index.redb").exists()
        && dir.join("phrase_index.redb").exists()
        && dir.join("bigram.redb").exists()
    {
        Some(dir)
    } else {
        eprintln!(
            "exported tables not found at /tmp/pinyin-rs-export; skipping \
             (run pinyin-migrate export first)"
        );
        None
    }
}

#[test]
fn dictionary_tables_round_trip_against_the_oracle() {
    let Some(dir) = export_dir() else { return };

    // Independent re-derivation, straight from the seam. Deliberately not a
    // call into the exporter: two implementations of the frozen recipe have
    // to agree byte for byte.
    let prefix = OraclePrefix::locate().expect("oracle prefix");
    let mut oracle = Oracle::open_with_temp_user_dir(prefix).expect("context opens");

    let mut tuples = Vec::new();
    for library in [1_u8, 2, 3, 4] {
        for tuple in oracle.export_phrases(library).expect("library exports") {
            tuples.push((library, tuple));
        }
    }

    let mut session = oracle.session(OracleFlags::DEFAULT).expect("session");
    let mut token_cache: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    let mut index: BTreeMap<String, BTreeMap<u32, u64>> = BTreeMap::new();
    let mut phrases: BTreeMap<u32, String> = BTreeMap::new();

    for (library, tuple) in &tuples {
        let tokens = token_cache
            .entry(tuple.phrase.clone())
            .or_insert_with(|| session.lookup_tokens(&tuple.phrase).expect("lookup_tokens"))
            .clone();
        let freq = u64::try_from(tuple.count.max(0)).expect("non-negative");
        for token in tokens {
            if (token >> 24) as u8 != *library {
                continue;
            }
            phrases.insert(token, tuple.phrase.clone());
            *index
                .entry(tuple.pinyin.clone())
                .or_default()
                .entry(token)
                .or_insert(0) += freq;
        }
    }

    // pinyin_index.redb: same keys, same {token, freq} records in the
    // frozen order (freq desc, token asc).
    let table = LookupTable::open(&dir.join("pinyin_index.redb")).expect("index opens");
    assert_eq!(
        table.len().expect("len"),
        index.len() as u64,
        "index key count differs from re-derivation"
    );
    for (pinyin, records) in &index {
        let stored = table
            .get(pinyin.as_bytes())
            .expect("get")
            .unwrap_or_else(|| panic!("{pinyin:?} missing from pinyin_index.redb"));
        let mut ordered: Vec<(u32, u64)> = records.iter().map(|(t, f)| (*t, *f)).collect();
        ordered.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut expected = Vec::with_capacity(ordered.len() * 8);
        for (token, freq) in ordered {
            expected.extend_from_slice(&token.to_le_bytes());
            expected.extend_from_slice(&u32::try_from(freq).unwrap_or(u32::MAX).to_le_bytes());
        }
        assert_eq!(stored, expected, "records differ for {pinyin:?}");
    }

    // phrase_index.redb: same token → text mapping.
    let table = LookupTable::open(&dir.join("phrase_index.redb")).expect("phrase opens");
    assert_eq!(
        table.len().expect("len"),
        phrases.len() as u64,
        "phrase count differs from re-derivation"
    );
    for (token, text) in &phrases {
        let stored = table
            .get(&token.to_le_bytes())
            .expect("get")
            .unwrap_or_else(|| panic!("token {token:#010x} missing from phrase_index.redb"));
        assert_eq!(
            stored,
            text.as_bytes(),
            "text differs for token {token:#010x}"
        );
    }
}

#[test]
fn bigram_invariant_and_spot_checks() {
    let Some(dir) = export_dir() else { return };

    let table = LookupTable::open(&dir.join("bigram.redb")).expect("bigram opens");
    assert_eq!(
        table.len().expect("len"),
        56_359,
        "the pinned bigram has 56,359 previous-token entries"
    );

    // Total-equals-sum over every entry, successors in system libraries.
    let mut successors_of: BTreeMap<u32, Vec<(u32, u32)>> = BTreeMap::new();
    for (key, value) in table.iter().expect("iter") {
        assert_eq!(key.len(), 4, "bigram key {key:02x?} is not a u32 token");
        let prev = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        assert!(
            value.len() >= 4 && (value.len() - 4).is_multiple_of(8),
            "bigram value for {prev:#010x} is not 4 + 8n bytes"
        );
        let total = u64::from(u32::from_le_bytes([value[0], value[1], value[2], value[3]]));
        let mut sum = 0_u64;
        let mut records = Vec::new();
        for chunk in value[4..].chunks_exact(8) {
            let next = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let count = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
            let library = next >> 24;
            assert!(
                (1..=4).contains(&library),
                "successor {next:#010x} of {prev:#010x} is outside the system libraries"
            );
            sum += u64::from(count);
            records.push((next, count));
        }
        assert_eq!(total, sum, "total != Σ count for {prev:#010x}");
        successors_of.insert(prev, records);
    }

    // Resolved spot checks: for three common previous tokens the top
    // successor by count is 的, the pin's dominant continuation.
    let prefix = OraclePrefix::locate().expect("oracle prefix");
    let mut oracle = Oracle::open_with_temp_user_dir(prefix).expect("context opens");
    let mut session = oracle.session(OracleFlags::DEFAULT).expect("session");

    for (name, prev) in [
        ("你", 0x0100_1225_u32),
        ("我", 0x0100_1dd7),
        ("中", 0x0100_2585),
    ] {
        let resolved = session
            .token_text(prev)
            .expect("token_text")
            .expect("spot-check token resolves");
        assert_eq!(resolved, name, "token {prev:#010x} is {name}");

        let records = successors_of
            .get(&prev)
            .unwrap_or_else(|| panic!("{name} has no bigram entry"));
        let &(top_token, top_count) = records
            .iter()
            .max_by_key(|(_, count)| *count)
            .expect("non-empty successor list");
        let top_text = session
            .token_text(top_token)
            .expect("token_text")
            .expect("top successor resolves");
        assert_eq!(
            top_text, "的",
            "{name}'s top successor ({top_count}) should be 的"
        );
    }
}
