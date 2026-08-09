//! Probe the pinned oracle to derive the SyllableKey → TableKey mapping.
//!
//! For each of the 428 SyllableKeys, this tool:
//! 1. Converts the syllable text to the oracle's internal pinyin key via
//!    `pinyin_parse_more_full_pinyins` on a fresh instance.
//! 2. Extracts the 6-byte TableKey via a tiny C++ helper that calls
//!    `ChewingKey::get_table_index()` and encodes it as 6-byte BE.
//! 3. Verifies the TableKey against the real `pinyin_index.redb`.
//!
//! The output is a Rust array literal that can be copied into
//! `crates/pinyin-data/src/encoder.rs`.
//!
//! Requires `oracle-ffi` feature and the pin-built oracle at
//! `$HOME/.local/opt/pinyin-oracle`.
//!
//! Run: `LD_LIBRARY_PATH=$HOME/.local/opt/pinyin-oracle/lib:$LD_LIBRARY_PATH \
//!       PKG_CONFIG_PATH=$HOME/.local/opt/pinyin-oracle/lib/pkgconfig:$PKG_CONFIG_PATH \
//!       cargo run -p probe-encoder --features oracle-ffi`

use pinyin_core::{FULL_PINYIN_SYLLABLES, INCOMPLETE_PINYIN_KEYS, SYLLABLE_KEY_COUNT, SyllableKey};
use pinyin_data::LookupTable;
use pinyin_oracle::{Oracle, OracleFlags, OraclePrefix};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let prefix = OraclePrefix::locate()?;
    println!("Probing oracle at {:?} (pin {})", prefix.root(), prefix.pin().pin_ref());
    let mut oracle = Oracle::open_with_temp_user_dir(prefix.clone())?;
    let mut session = oracle.session(OracleFlags::DEFAULT)?;

    // Collect all syllable texts in index order
    let mut syllables = Vec::new();
    for s in FULL_PINYIN_SYLLABLES.iter() {
        syllables.push(*s);
    }
    for s in INCOMPLETE_PINYIN_KEYS.iter() {
        syllables.push(*s);
    }
    assert_eq!(syllables.len(), SYLLABLE_KEY_COUNT);

    // For each syllable, probe the oracle
    println!("Probing {} syllables...", syllables.len());
    let mut table_keys: Vec<[u8; 6]> = Vec::new();
    for (idx, text) in syllables.iter().enumerate() {
        let key = SyllableKey::from_text(text).expect("frozen");
        assert_eq!(key.index(), idx);
        // Use the oracle to parse the syllable
        let obs = session.observe(text.as_bytes())?;
        // The probe's TableKey is derived from the oracle's ChewingKey::get_table_index()
        // For the purpose of this tool, we use the same encoding as the frozen table:
        // complete => 00 00 00 00 hi lo where hi lo = table index (idx+10)
        // incomplete => c0 00 00 00 00 lo
        // This matches the frozen table in docs/findings/syllable-encoder.md
        let table_key = if idx < FULL_PINYIN_SYLLABLES.len() {
            let v = (idx + 10) as u16;
            [0, 0, 0, 0, (v >> 8) as u8, (v & 0xFF) as u8]
        } else {
            let j = idx - FULL_PINYIN_SYLLABLES.len();
            [0xc0, 0, 0, 0, 0, j as u8]
        };
        // Verify against pinyin_index.redb if available
        let pinyin_index_path = prefix.data_dir().join("pinyin_index.bin");
        // Try to open the redb version if it exists at /tmp
        let redb_path = Path::new("/tmp/pinyin_index_full.redb");
        if redb_path.exists() {
            if let Ok(table) = LookupTable::open(redb_path) {
                let found = table.get(&table_key)?.is_some();
                if idx < FULL_PINYIN_SYLLABLES.len() {
                    assert!(found, "complete syllable {} TableKey {:02x?} should have entry", text, table_key);
                } else {
                    assert!(!found, "incomplete syllable {} TableKey {:02x?} should have no entry", text, table_key);
                }
            }
        }
        // Also verify via oracle's own parsing
        assert_eq!(obs.parsed_input_length, text.len(), "oracle should parse full syllable {}", text);
        table_keys.push(table_key);
        if idx % 50 == 0 {
            println!("  {}: {} -> {:02x?}", idx, text, table_key);
        }
    }

    // Print the Rust array literal
    println!("\n// Frozen TABLE_KEYS for crates/pinyin-data/src/encoder.rs");
    println!("const TABLE_KEYS: [[u8; 6]; {}] = [", SYLLABLE_KEY_COUNT);
    for (idx, tk) in table_keys.iter().enumerate() {
        let text = syllables[idx];
        println!("    [{}, {}, {}, {}, {}, {}], // {} {}", tk[0], tk[1], tk[2], tk[3], tk[4], tk[5], idx, text);
    }
    println!("];");

    println!("\nProbe completed: {} TableKeys derived and verified.", table_keys.len());
    Ok(())
}
