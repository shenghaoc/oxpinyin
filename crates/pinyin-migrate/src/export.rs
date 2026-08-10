//! Public-ABI export: pinned oracle → portable redb tables.
//!
//! Implements the derivation frozen in `docs/findings/data-layer-export.md`:
//! the four default-loaded phrase libraries are enumerated through
//! `pinyin-oracle`'s export seam, tokens are resolved through the oracle's
//! own `pinyin_lookup_tokens`, and the results are written as
//! `pinyin_index.redb` (pinyin string → `{token, freq}` records) and
//! `phrase_index.redb` (token → text). The system bigram is copied
//! verbatim from the prefix's `bigram.db` via the Tkrzw bridge.

use std::collections::BTreeMap;
use std::path::Path;

use pinyin_oracle::{Oracle, OracleFlags, OraclePrefix};

use crate::write_redb;

/// The four default-loaded system phrase libraries, by token top byte.
const SYSTEM_LIBRARIES: [u8; 4] = [1, 2, 3, 4];

/// Names for the summary print, index-aligned with [`SYSTEM_LIBRARIES`].
const LIBRARY_NAMES: [&str; 4] = ["gb_char", "gbk_char", "opengram", "merged"];

/// Pinyin keys included in the `--mini` fixture subset.
///
/// Chosen to cover the dictionary unit tests: single syllables, two
/// multi-syllable phrases, and the `xian` / `xi'an` pair whose distinctness
/// is exactly what the apostrophe-separated key format exists to preserve.
const MINI_KEYS: [&str; 10] = [
    "a",
    "ni",
    "hao",
    "ni'hao",
    "ni'men",
    "zhong",
    "guo",
    "zhong'guo",
    "xian",
    "xi'an",
];

/// Runs the full export into `out_dir`.
///
/// With `mini`, only the [`MINI_KEYS`] subset of the index (plus the
/// tokens and bigram entries it references) is written — the reproducible
/// recipe for `fixtures/w3/`.
pub fn run(out_dir: &Path, mini: bool) -> Result<(), Box<dyn std::error::Error>> {
    let prefix = OraclePrefix::locate()?;
    eprintln!("exporting from pin {}", prefix.pin().pin_ref());
    let data_dir = prefix.data_dir().to_path_buf();
    let mut oracle = Oracle::open_with_temp_user_dir(prefix)?;

    // Phase 1: enumerate the four system libraries.
    let mut tuples = Vec::new();
    for (library, name) in SYSTEM_LIBRARIES.into_iter().zip(LIBRARY_NAMES) {
        let exported = oracle.export_phrases(library)?;
        eprintln!("  {name}: {} tuples", exported.len());
        tuples.extend(exported.into_iter().map(|tuple| (library, tuple)));
    }

    // Phase 2: resolve phrase text → token through the oracle itself.
    let mut session = oracle.session(OracleFlags::DEFAULT)?;
    let mut token_cache: BTreeMap<String, Vec<u32>> = BTreeMap::new();

    // pinyin string → (token → summed freq)
    let mut index: BTreeMap<String, BTreeMap<u32, u64>> = BTreeMap::new();
    // token → phrase text
    let mut phrases: BTreeMap<u32, String> = BTreeMap::new();

    let mut dropped_no_token = 0_u64;
    let mut negative_counts = 0_u64;
    let mut duplicate_pairs = 0_u64;
    let mut same_library_splits = 0_u64;

    for (library, tuple) in &tuples {
        let tokens = match token_cache.get(&tuple.phrase) {
            Some(tokens) => tokens.clone(),
            None => {
                let tokens = session.lookup_tokens(&tuple.phrase)?;
                token_cache.insert(tuple.phrase.clone(), tokens.clone());
                tokens
            }
        };
        let own: Vec<u32> = tokens
            .iter()
            .copied()
            .filter(|token| (token >> 24) as u8 == *library)
            .collect();
        if own.is_empty() {
            dropped_no_token += 1;
            continue;
        }
        if own.len() > 1 {
            same_library_splits += 1;
        }
        let freq = if tuple.count < 0 {
            negative_counts += 1;
            0
        } else {
            tuple.count as u64
        };
        for token in own {
            match phrases.get(&token) {
                None => {
                    phrases.insert(token, tuple.phrase.clone());
                }
                Some(existing) if existing == &tuple.phrase => {}
                Some(existing) => {
                    return Err(format!(
                        "token {token:#010x} resolves to both {existing:?} and {:?}",
                        tuple.phrase
                    )
                    .into());
                }
            }
            let per_token = index.entry(tuple.pinyin.clone()).or_default();
            if let Some(sum) = per_token.get_mut(&token) {
                duplicate_pairs += 1;
                *sum += freq;
            } else {
                per_token.insert(token, freq);
            }
        }
    }
    drop(session);

    eprintln!(
        "  index keys {} · phrases {} · dropped(no own-library token) {dropped_no_token} · \
         negative counts {negative_counts} · duplicate (pinyin, token) pairs {duplicate_pairs} · \
         same-library multi-token phrases {same_library_splits}",
        index.len(),
        phrases.len(),
    );

    // Optional mini filtering, after full collection so the subset is a
    // strict restriction of the real export.
    if mini {
        index.retain(|pinyin, _| MINI_KEYS.contains(&pinyin.as_str()));
        let kept: std::collections::BTreeSet<u32> = index
            .values()
            .flat_map(|records| records.keys().copied())
            .collect();
        phrases.retain(|token, _| kept.contains(token));
        eprintln!(
            "  mini subset: {} index keys, {} phrases",
            index.len(),
            phrases.len()
        );
    }

    // Serialise. Index records: {token u32 LE, freq u32 LE}, freq desc then
    // token asc; frequencies clamp to u32 after summing.
    let index_entries: Vec<(Vec<u8>, Vec<u8>)> = index
        .iter()
        .map(|(pinyin, records)| {
            let mut ordered: Vec<(u32, u64)> = records
                .iter()
                .map(|(token, freq)| (*token, *freq))
                .collect();
            ordered.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            let mut value = Vec::with_capacity(ordered.len() * 8);
            for (token, freq) in ordered {
                value.extend_from_slice(&token.to_le_bytes());
                value.extend_from_slice(&u32::try_from(freq).unwrap_or(u32::MAX).to_le_bytes());
            }
            (pinyin.as_bytes().to_vec(), value)
        })
        .collect();
    let phrase_entries: Vec<(Vec<u8>, Vec<u8>)> = phrases
        .iter()
        .map(|(token, text)| (token.to_le_bytes().to_vec(), text.as_bytes().to_vec()))
        .collect();

    std::fs::create_dir_all(out_dir)?;
    write_redb(&out_dir.join("pinyin_index.redb"), &index_entries)?;
    write_redb(&out_dir.join("phrase_index.redb"), &phrase_entries)?;

    // Bigram: verbatim Tkrzw copy, optionally restricted to the mini
    // phrase set's previous tokens (values always intact, so the
    // total-equals-sum invariant is preserved per entry).
    let bigram_src = data_dir.join("bigram.db");
    let path_c = std::ffi::CString::new(bigram_src.to_string_lossy().as_bytes())
        .map_err(|_| "bigram path contains a NUL byte")?;
    let reader = crate::tkrzw::TkrzwReader::open(&path_c)?;
    let mut bigram_entries = reader.entries()?;
    bigram_entries.sort_by(|a, b| a.0.cmp(&b.0));
    let full_bigram_len = bigram_entries.len();
    if mini {
        bigram_entries.retain(|(key, _)| {
            key.len() == 4
                && phrases.contains_key(&u32::from_le_bytes([key[0], key[1], key[2], key[3]]))
        });
    }
    eprintln!(
        "  bigram: {} entries ({} in source)",
        bigram_entries.len(),
        full_bigram_len
    );
    write_redb(&out_dir.join("bigram.redb"), &bigram_entries)?;

    eprintln!("export complete → {}", out_dir.display());
    Ok(())
}
