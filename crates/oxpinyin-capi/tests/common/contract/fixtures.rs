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

/// `_modify_header`: one `LOG_MODIFY_HEADER` record: type 4, the token, a `u16`
/// length, then the old and new totals.
pub fn modify_header(token: u32, old: u32, new: u32) -> Vec<u8> {
    [
        &4_u32.to_le_bytes()[..],
        &token.to_le_bytes(),
        &4_u16.to_le_bytes(),
        &old.to_le_bytes(),
        &new.to_le_bytes(),
    ]
    .concat()
}

/// `_write_log`: the user directory's journal `name` (`gb_char.dbin`, ...) as a
/// chunk holding `payload`.
pub fn write_log(k: &Kit, payload: &[u8], name: &str) {
    std::fs::write(k.user.join(name), chunk(payload)).expect("write the journal");
}

/// `log_walk`: the `.dbin` chunk's first MODIFY_HEADER totals and its record
/// counts (`PhraseIndexLogger::next_record`, `phrase_index_logger.h:158-211`).
/// The eight-byte framing header is skipped unchecked; the walk stops at an
/// unknown type, fewer than ten bytes or an incomplete header record.
pub fn log_walk(data: &[u8]) -> (crate::support::J, crate::support::J) {
    let payload = &data[8..];
    // `int.from_bytes` of a slice the payload's end cuts short.
    let read = |at: usize, width: usize| {
        let bytes = payload
            .get(at..payload.len().min(at + width))
            .unwrap_or(&[]);
        bytes
            .iter()
            .rev()
            .fold(0_u32, |value, byte| (value << 8) | u32::from(*byte))
    };
    let half = |at: usize| usize::try_from(read(at, 2)).expect("an offset");
    let mut pos = 0;
    let mut header = crate::support::J::Null;
    let mut counts: std::collections::BTreeMap<String, u32> = ["add", "remove", "modify", "header"]
        .into_iter()
        .map(|name| (name.to_owned(), 0))
        .collect();
    let mut count = |name: &str| *counts.get_mut(name).expect("a record kind") += 1;
    while pos + 10 <= payload.len() {
        match read(pos, 4) {
            4 => {
                let length = half(pos + 8);
                if pos + 10 + 2 * length > payload.len() {
                    break;
                }
                if header == crate::support::J::Null {
                    header = crate::support::J::from((read(pos + 10, 4), read(pos + 14, 4)));
                }
                count("header");
                pos += 10 + 2 * length;
            }
            kind @ (1 | 2) => {
                let length = half(pos + 8);
                count(if kind == 1 { "add" } else { "remove" });
                pos += 10 + length;
            }
            3 => {
                let (old, new) = (half(pos + 8), half(pos + 10));
                count("modify");
                pos += 12 + old + new;
            }
            _ => break,
        }
    }
    (header, crate::support::J::from(counts))
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

/// A symbolic link at `link` to `target` (the Python's `os.symlink`). On
/// Windows a symbolic link needs the symbolic-link privilege (Developer Mode or
/// an elevated process); without it the case fails here. It never copies: the
/// library would map the copy, and the case's later unlink or rename of it
/// would hit a sharing violation instead of the pin's observed result.
pub fn link_file(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("link a system file");
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_file(target, link) {
        // ERROR_PRIVILEGE_NOT_HELD.
        assert_ne!(
            error.raw_os_error(),
            Some(1314),
            "symbolic link {} -> {}: this process lacks the symbolic-link privilege \
             (enable Developer Mode or run elevated); the contract cases never copy the system data",
            link.display(),
            target.display()
        );
        panic!(
            "link a system file {} -> {}: {error}",
            link.display(),
            target.display()
        );
    }
}

/// `private_system`: a system directory under the scratch directory, every
/// file of the system data linked in ([`link_file`]).
pub fn private_system(k: &Kit) -> PathBuf {
    let system = k.scratch.join("sys-X");
    std::fs::create_dir(&system).expect("private system directory");
    for entry in std::fs::read_dir(&k.data).expect("system data") {
        let entry = entry.expect("system entry");
        link_file(&entry.path(), &system.join(entry.file_name()));
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

/// Copies the system file `from` to the case's own file `to` and gives the
/// copy the owner's write permission: the pinned data is read-only, and
/// `CopyFileEx` keeps the read-only attribute as `copy` keeps the mode.
pub fn writable_copy(from: &Path, to: &Path) {
    std::fs::copy(from, to).expect("copy a system file");
    let mut permissions = std::fs::metadata(to).expect("the copy").permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        permissions.set_mode(permissions.mode() | 0o200);
    }
    #[cfg(windows)]
    // The copy is the case's own scratch file, not a shared one.
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    std::fs::set_permissions(to, permissions).expect("make the copy writable");
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

/// `user_listing`: the files of `directory` (the user directory when `None`) by
/// name, each name's bytes read as Latin-1: `"dir"` for a directory, `user.conf`
/// by its text with `|` for each newline, a `.bin`/`.dbin` library chunk by its
/// size, one named `user_*` by `"chunk"` when its length word is its size less
/// the 8-byte header and `"dbm"` otherwise, any other file by `null`.
pub fn user_listing(k: &Kit, directory: Option<&Path>) -> crate::support::J {
    use crate::support::J;
    let directory = directory.unwrap_or(&k.user);
    let mut listing = J::map();
    for entry in std::fs::read_dir(directory).expect("the listed directory") {
        let entry = entry.expect("a listed entry");
        let name = entry.file_name();
        let bytes = name.as_encoded_bytes();
        let key: String = bytes.iter().map(|&byte| char::from(byte)).collect();
        let path = entry.path();
        let value = if path.is_dir() {
            J::from("dir")
        } else if bytes == b"user.conf" {
            J::from(
                std::fs::read_to_string(&path)
                    .expect("user.conf")
                    .replace('\n', "|"),
            )
        } else if bytes.ends_with(b".bin") || bytes.ends_with(b".dbin") {
            let data = std::fs::read(&path).expect("a library file");
            if bytes.starts_with(b"user_") {
                let framed = data.len() >= 8
                    && u64::from(u32::from_le_bytes(data[..4].try_into().expect("a word")))
                        == data.len() as u64 - 8;
                J::from(if framed { "chunk" } else { "dbm" })
            } else {
                J::from(data.len())
            }
        } else {
            J::Null
        };
        listing.set(key, value);
    }
    listing
}
