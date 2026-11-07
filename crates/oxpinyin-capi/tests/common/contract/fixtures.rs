//! The crafted files the contract case bodies plant (`contract_fixtures.py`,
//! `contract_cell_fixtures.py`, the crafting half of `contract_case_helpers.py`).
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use oxpinyin_store::{DefaultStore, WriteStore as _, WriteTxn as _};

use crate::support::Kit;

/// `_ucs4_key`: each character as a little-endian four-byte phrase-table key unit.
pub fn ucs4_key(text: &str) -> Vec<u8> {
    text.chars()
        .flat_map(|ch| u32::from(ch).to_le_bytes())
        .collect()
}

/// `_chunk`: a `MemoryChunk` image, the length and checksum header words
/// (`memory_chunk.h:543-547`) and `payload`.
pub fn chunk(payload: &[u8]) -> Vec<u8> {
    let aligned = payload.len() & !0x3;
    let (words, _) = payload[..aligned].as_chunks::<4>();
    let mut checksum = words
        .iter()
        .fold(0_u32, |sum, word| sum ^ u32::from_le_bytes(*word));
    for (shift, byte) in payload[aligned..].iter().enumerate() {
        checksum ^= u32::from(*byte) << (8 * shift);
    }
    let length = u32::try_from(payload.len()).expect("a chunk length");
    [&length.to_le_bytes()[..], &checksum.to_le_bytes(), payload].concat()
}

/// `_write_user_conf`: a `user.conf` conforming to the system data's table.conf,
/// with open counter 0.
pub fn write_user_conf(k: &Kit) {
    let conf = std::fs::read_to_string(k.data.join("table.conf")).expect("table.conf");
    let mut text = String::new();
    for key in [
        "binary format version:",
        "model data version:",
        "database format:",
    ] {
        let value = conf
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .unwrap_or_else(|| panic!("table.conf has no {key}"));
        text.push_str(&format!("{key}{}\n", value.trim()));
    }
    text.push_str("open counter:0\n");
    std::fs::write(k.user.join("user.conf"), text).expect("write user.conf");
}

/// `private_system`: a system directory under the scratch directory, every
/// file of the system data linked in (copied where links need privileges).
pub fn private_system(k: &Kit) -> PathBuf {
    let system = k.scratch.join("sys-X");
    std::fs::create_dir(&system).expect("private system directory");
    for entry in std::fs::read_dir(&k.data).expect("system data") {
        let entry = entry.expect("system entry");
        let target = system.join(entry.file_name());
        #[cfg(unix)]
        std::os::unix::fs::symlink(entry.path(), &target).expect("link a system file");
        #[cfg(not(unix))]
        std::fs::copy(entry.path(), &target).expect("copy a system file");
    }
    system
}

/// One edit of [`conf_system`].
pub enum ConfEdit<'a> {
    /// Replace every occurrence of the first text with the second; the first must occur.
    Replace(&'a str, &'a str),
    /// Append a line.
    Append(&'a str),
    /// Drop every line containing the text.
    Drop(&'a str),
}

/// `conf_system`: a private system directory whose table.conf is the system
/// data's with `edits` applied.
pub fn conf_system(k: &Kit, edits: &[ConfEdit<'_>]) -> PathBuf {
    let system = private_system(k);
    let path = system.join("table.conf");
    let mut text = std::fs::read_to_string(k.data.join("table.conf")).expect("table.conf");
    for edit in edits {
        match edit {
            ConfEdit::Replace(from, to) => {
                assert!(text.contains(from), "table.conf has no {from}");
                text = text.replace(from, to);
            }
            ConfEdit::Append(line) => {
                text.push_str(line);
                text.push('\n');
            }
            ConfEdit::Drop(needle) => {
                text = text
                    .split_inclusive('\n')
                    .filter(|line| !line.contains(needle))
                    .collect();
            }
        }
    }
    std::fs::remove_file(&path).expect("unlink the linked table.conf");
    std::fs::write(&path, text).expect("write table.conf");
    system
}

/// `_craft_db(path, rows, 1)`: a tree DBM in this cell's own format holding
/// exactly `rows`, replacing any file at `path`.
pub fn craft_tree(path: &Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    if path.exists() {
        std::fs::remove_file(path).expect("replace the DBM");
    }
    let store = DefaultStore::create(path).expect("create the DBM");
    store
        .write(|txn| {
            for (key, value) in rows {
                txn.put_raw(key, value)?;
            }
            Ok(())
        })
        .expect("write the DBM rows");
}

/// `_craft_db(path, rows, 2)` / `d18_bigram_db`: a hash DBM in this cell's own
/// format holding exactly `rows`, replacing any file at `path`.
pub fn craft_hash(path: &Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    if path.exists() {
        std::fs::remove_file(path).expect("replace the DBM");
    }
    let store = DefaultStore::create_hash(path).expect("create the DBM");
    store
        .write(|txn| {
            for (key, value) in rows {
                txn.put_raw(key, value)?;
            }
            Ok(())
        })
        .expect("write the DBM rows");
}

/// `_gram_bytes`: a stored SingleGram, its total then `(token, count)` pairs
/// (`ngram.cpp:48-50`, `:133-151`).
pub fn gram_bytes(total: u32, items: &[(u32, u32)]) -> Vec<u8> {
    let mut bytes = total.to_le_bytes().to_vec();
    for (token, count) in items {
        bytes.extend(token.to_le_bytes());
        bytes.extend(count.to_le_bytes());
    }
    bytes
}

/// `_replace_user_bigram`: closes the context and replaces user_bigram.db with
/// `rows` (predecessor token, raw value) under a conforming user.conf.
pub fn replace_user_bigram(k: &mut Kit, rows: &[(u32, Vec<u8>)]) {
    let inst = k.inst_if_any();
    if !inst.is_null() {
        crate::facade::free_instance(inst);
    }
    crate::facade::fini(k.ctx());
    k.forget();
    let rows: Vec<(Vec<u8>, Vec<u8>)> = rows
        .iter()
        .map(|(token, value)| (token.to_le_bytes().to_vec(), value.clone()))
        .collect();
    craft_hash(&k.user.join("user_bigram.db"), &rows);
    write_user_conf(k);
}

/// `items_system`: a private system whose library 1 holds exactly `items`
/// (text code points, key words, unigram), with matching pinyin and phrase
/// indexes; `isolated` also drops the other libraries, the addon indexes' rows
/// and the system bigram. The kit then opens on it.
pub fn items_system(k: &mut Kit, items: &[(&[u32], &[u16], u32)], total: u32, isolated: bool) {
    use std::collections::BTreeMap;
    let system = private_system(k);
    let mut content = vec![0_u8; 8];
    let mut offsets = vec![0_u32];
    let mut phonetic: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    let mut phrases: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    for (slot, (text, reading, unigram)) in items.iter().enumerate() {
        let token = 0x0100_0000 | u32::try_from(slot + 1).expect("slot");
        let raw_text: Vec<u8> = text.iter().flat_map(|word| word.to_le_bytes()).collect();
        let raw_keys: Vec<u8> = reading.iter().flat_map(|word| word.to_le_bytes()).collect();
        offsets.push(u32::try_from(content.len()).expect("offset"));
        content.extend([u8::try_from(text.len()).expect("length"), 1]);
        content.extend(unigram.to_le_bytes());
        content.extend(&raw_text);
        content.extend(&raw_keys);
        content.extend(10_u32.to_le_bytes());
        let index: Vec<u8> = reading
            .iter()
            .flat_map(|word| (word & 4095).to_le_bytes())
            .collect();
        for end in (2..index.len()).step_by(2) {
            phonetic.entry(index[..end].to_vec()).or_default();
        }
        let mut item = token.to_le_bytes().to_vec();
        item.extend(&raw_keys);
        item.resize(item.len().div_ceil(4) * 4, 0);
        phonetic.entry(index).or_default().extend(item);
        for end in (4..raw_text.len()).step_by(4) {
            phrases.entry(raw_text[..end].to_vec()).or_default();
        }
        phrases
            .entry(raw_text)
            .or_default()
            .extend(token.to_le_bytes());
    }
    let count = u32::try_from(offsets.len()).expect("offsets");
    let index_two = 17 + count * 4 + 1;
    let content_len = u32::try_from(content.len()).expect("content");
    let mut payload: Vec<u8> = [total, 17, index_two, index_two + content_len + 1]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    payload.push(b'#');
    payload.extend(offsets.iter().flat_map(|value| value.to_le_bytes()));
    payload.push(b'#');
    payload.extend(&content);
    payload.push(b'#');
    let gb_char = system.join("gb_char.bin");
    std::fs::remove_file(&gb_char).expect("unlink gb_char.bin");
    std::fs::write(&gb_char, chunk(&payload)).expect("gb_char.bin");
    for (name, rows) in [
        ("pinyin_index.bin", phonetic),
        ("phrase_index.bin", phrases),
    ] {
        let path = system.join(name);
        std::fs::remove_file(&path).expect("unlink an index");
        craft_tree(&path, &rows.into_iter().collect::<Vec<_>>());
    }
    if isolated {
        let path = system.join("table.conf");
        let text = std::fs::read_to_string(&path).expect("table.conf");
        std::fs::remove_file(&path).expect("unlink table.conf");
        let kept: String = text
            .split_inclusive('\n')
            .filter(|line| {
                !(line.starts_with("default ") || line.starts_with("addon "))
                    || line.starts_with("default GB_DICTIONARY ")
            })
            .collect();
        std::fs::write(&path, kept).expect("table.conf");
        for name in ["addon_pinyin_index.bin", "addon_phrase_index.bin"] {
            let path = system.join(name);
            std::fs::remove_file(&path).expect("unlink an addon index");
            craft_tree(&path, &[]);
        }
        let path = system.join("bigram.db");
        std::fs::remove_file(&path).expect("unlink bigram.db");
        craft_hash(&path, &[]);
    }
    write_user_conf(k);
    k.data = system;
}
