//! Native mini fixtures and malformed files shared by owner and C tests.

#![allow(dead_code)]

use std::path::PathBuf;

pub fn system_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(if env!("CARGO_PKG_NAME") == "oxpinyin" {
            "."
        } else {
            ".."
        })
        .join("fixtures")
        .join("w3")
        .join(oxpinyin::__internal::DEFAULT_STORE_EXT)
}

/// W3 under CJK directory and file names, with a fresh CJK user directory.
#[cfg(windows)]
pub fn cjk_system_and_user() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let system = root.path().join("系统");
    let user = root.path().join("用户");
    std::fs::create_dir(&system).unwrap();
    std::fs::create_dir(&user).unwrap();
    for entry in std::fs::read_dir(system_dir()).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), system.join(entry.file_name())).unwrap();
    }
    std::fs::rename(system.join("gb_char.bin"), system.join("汉字.bin")).unwrap();
    let conf = std::fs::read_to_string(system.join("table.conf")).unwrap();
    std::fs::write(
        system.join("table.conf"),
        conf.replace("gb_char.bin", "汉字.bin"),
    )
    .unwrap();
    (root, system, user)
}

/// A uniquely named scratch dir, removed when the owning guard drops.
fn scratch_dir(prefix: &str, what: &str) -> (PathBuf, tempfile::TempDir) {
    let dir = tempfile::Builder::new()
        .prefix(prefix)
        .suffix(".d")
        .tempdir()
        .expect(what);
    (dir.path().to_path_buf(), dir)
}

/// The name that starts a [`TempUserDir`]: the zhuyin C crate's own, and
/// the pinyin C crate's for every other includer.
fn user_dir_prefix() -> &'static str {
    if env!("CARGO_PKG_NAME") == "oxzhuyin-capi" {
        "oxzhuyin-capi"
    } else {
        "oxpinyin-capi"
    }
}

pub struct TempUserDir {
    pub(crate) path: PathBuf,
    _dir: tempfile::TempDir,
}

impl TempUserDir {
    pub(crate) fn new(tag: &str) -> Self {
        let (path, dir) = scratch_dir(&format!("{}-{tag}-", user_dir_prefix()), "temp user dir");
        Self { path, _dir: dir }
    }
}

pub struct TempSystemDir {
    pub(crate) path: PathBuf,
    _dir: tempfile::TempDir,
}

impl TempSystemDir {
    pub(crate) fn new(tag: &str) -> Self {
        let (path, dir) = scratch_dir(&format!("oxpinyin-capi-system-{tag}-"), "temp system dir");
        // The whole fixture directory of the compiled-in backend
        // (`fixtures/w3/<ext>`: the DBMs, the chunk files, table.conf),
        // so this helper works under every backend gate.
        for entry in std::fs::read_dir(system_dir()).expect("fixture dir") {
            let entry = entry.expect("fixture entry");
            if entry.file_type().expect("file type").is_file() {
                std::fs::copy(entry.path(), path.join(entry.file_name())).expect("copy fixture");
            }
        }
        Self { path, _dir: dir }
    }

    pub(crate) fn write(&self, name: &str, contents: &str) {
        std::fs::write(self.path.join(name), contents).expect("write temp system file");
    }
}

/// Hand-built `PhraseIndexLogger` records for the journal-replay tests
/// (`phrase_index_logger.h:158-215` layout, little-endian).
pub mod logs {
    fn record(kind: u32, token: u32, tail: &[u8]) -> Vec<u8> {
        [&kind.to_le_bytes()[..], &token.to_le_bytes(), tail].concat()
    }

    /// A `MODIFY_HEADER` carrying token `0x01000001`.
    #[must_use]
    pub fn header_token() -> Vec<u8> {
        record(4, 0x0100_0001, &[4, 0, 0, 0, 0, 0, 0, 0, 0, 0])
    }

    /// A null-token `MODIFY_HEADER`.
    #[must_use]
    pub fn header() -> Vec<u8> {
        record(4, 0, &[4, 0, 0, 0, 0, 0, 0, 0, 0, 0])
    }

    /// An ADD of a six-byte item at slot `0x99`.
    #[must_use]
    pub fn add() -> Vec<u8> {
        record(1, 0x0100_0099, &[6, 0, 0, 0, 1, 0, 0, 0])
    }

    /// A REMOVE with a six-byte old item at slot 0x99.
    #[must_use]
    pub fn remove() -> Vec<u8> {
        record(2, 0x0100_0099, &[6, 0, 0, 0, 1, 0, 0, 0])
    }

    /// A MODIFY of slot 1 whose old chunk no item matches: `merge` returns
    /// there (`phrase_index.cpp:483-484`).
    #[must_use]
    pub fn mismatched_modify() -> Vec<u8> {
        record(
            3,
            0x0100_0001,
            &[6, 0, 6, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
        )
    }
}

/// Writes a `user.conf` that conforms to the `table.conf` of `system`, with
/// open counter 0, so the next init keeps the profile under `user_dir`.
pub fn write_user_conf(user_dir: &std::path::Path, system: &std::path::Path) {
    let conf = oxpinyin::data::TableConf::load_from_dir(system).expect("system table.conf");
    let versions = oxpinyin::data::user_files::SystemVersions::from_conf(&conf);
    assert!(
        oxpinyin::data::user_files::UserTableInfo::conform_to(&versions)
            .save(user_dir.join("user.conf").as_path())
    );
}

/// A tree DBM of the running backend at `path` that holds exactly `rows`,
/// in place of any file there: a system table with records the library
/// never writes.
pub fn craft_tree(path: &std::path::Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    use oxpinyin::store::{WriteStore as _, WriteTxn as _};
    if path.exists() {
        std::fs::remove_file(path).expect("replace the DBM");
    }
    let store = oxpinyin::store::DefaultStore::create(path).expect("create the DBM");
    store
        .write(|txn| {
            for (key, value) in rows {
                txn.put_raw(key, value)?;
            }
            Ok(())
        })
        .expect("write the DBM rows");
}

/// A hash DBM of the running backend at `path` that holds exactly `rows`, in
/// place of any file there: a system bigram with grams the library never
/// writes.
pub fn craft_hash(path: &std::path::Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    use oxpinyin::store::{WriteStore as _, WriteTxn as _};
    if path.exists() {
        std::fs::remove_file(path).expect("replace the DBM");
    }
    let store = oxpinyin::store::DefaultStore::create_hash(path).expect("create the DBM");
    store
        .write(|txn| {
            for (key, value) in rows {
                txn.put_raw(key, value)?;
            }
            Ok(())
        })
        .expect("write the DBM rows");
}

/// Replaces the user index at `path` with one that holds exactly `rows`,
/// raw keys and values that the library may never write. It is written as
/// the library's own save writes a user index: a snapshot on Kyoto
/// Cabinet, a tree file on tkrzw and Berkeley DB.
pub fn craft_user_index(path: &std::path::Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    use oxpinyin::store::WriteStore as _;
    if path.exists() {
        std::fs::remove_file(path).expect("replace the user index");
    }
    oxpinyin::store::DefaultStore::write_user_index(path, |sink| {
        for (key, value) in rows {
            sink(key, value)?;
        }
        Ok(())
    })
    .expect("write the user index");
}

/// Replaces the user bigram at `path` with one that holds exactly `rows`,
/// grams that the library may never write. It is written as the library's
/// own save writes the user bigram: a snapshot on Kyoto Cabinet, a hash
/// file on tkrzw and Berkeley DB.
pub fn craft_user_bigram(path: &std::path::Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    use oxpinyin::store::{DefaultUserBigramDb, UserBigramDb as _};
    let db = DefaultUserBigramDb::empty().expect("empty user bigram");
    let mut rows = rows.to_vec();
    rows.sort();
    for (key, value) in rows {
        db.store(&key, &value).expect("store a fixture gram");
    }
    db.save_db(path).expect("save the user bigram");
}

/// Every record of the user container at `path`, sorted. `hash` reads the
/// user bigram; otherwise the file is a user index. Each is read as the
/// library reads it on the running backend.
pub fn store_rows(path: &std::path::Path, hash: bool) -> Vec<(Vec<u8>, Vec<u8>)> {
    use oxpinyin::store::RawReadStore as _;
    use std::ops::Bound;
    if hash {
        use oxpinyin::store::{DefaultUserBigramDb, UserBigramDb as _};
        let store = DefaultUserBigramDb::load_db(path).expect("load the user bigram");
        let mut rows = Vec::new();
        for key in store.keys().expect("predecessors") {
            let key = key.expect("a fixture predecessor is four bytes");
            let mut value = Vec::new();
            assert!(store.get(&key, &mut value).expect("read a gram"));
            rows.push((key.to_vec(), value));
        }
        rows.sort();
        return rows;
    }
    let store = oxpinyin::store::DefaultStore::open_user_index(path)
        .unwrap_or_else(|error| panic!("open {}: {error:?}", path.display()));
    let mut rows = Vec::new();
    store
        .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
            rows.push((key.to_vec(), value.to_vec()));
            Ok(())
        })
        .expect("read the container");
    rows.sort();
    rows
}

/// A stored `SingleGram`: the total, then `(token, count)` pairs
/// (`ngram.cpp:48-50`).
#[must_use]
pub fn gram_bytes(total: u32, items: &[(u32, u32)]) -> Vec<u8> {
    let mut bytes = total.to_le_bytes().to_vec();
    for (token, count) in items {
        bytes.extend(token.to_le_bytes());
        bytes.extend(count.to_le_bytes());
    }
    bytes
}

/// A `LOG_MODIFY_HEADER` record: type 4, the token, a `u16` length, then
/// the old and new totals (`phrase_index_logger.h:158-215`).
#[must_use]
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

pub fn plant_log(user_dir: &std::path::Path, file: &str, payload: &[u8]) {
    let conf = oxpinyin::data::TableConf::load_from_dir(&system_dir()).expect("fixture table.conf");
    let versions = oxpinyin::data::user_files::SystemVersions::from_conf(&conf);
    assert!(
        oxpinyin::data::user_files::UserTableInfo::conform_to(&versions)
            .save(user_dir.join("user.conf").as_path())
    );
    std::fs::write(
        user_dir.join(file),
        oxpinyin::data::chunk_format::build_memory_chunk(payload).expect("frame"),
    )
    .expect("write the log");
}

/// Writes a conforming profile for the fixture data set whose user bigram
/// holds `rows`, `(key, value)` verbatim — values the library never writes,
/// such as one too short for its `guint32` total.
pub fn plant_bigram(user_dir: &std::path::Path, rows: Vec<(Vec<u8>, Vec<u8>)>) {
    let conf = oxpinyin::data::TableConf::load_from_dir(&system_dir()).expect("fixture table.conf");
    let versions = oxpinyin::data::user_files::SystemVersions::from_conf(&conf);
    let mut marker = oxpinyin::data::user_files::UserTableInfo::conform_to(&versions);
    marker.open_counter = 1;
    assert!(marker.save(user_dir.join("user.conf").as_path()));
    craft_user_bigram(
        &user_dir.join(oxpinyin::data::user_files::UserDbm::Bigram.file_name()),
        &rows,
    );
}

/// Sets one row of the user phrase table (`user_phrase_index.bin`) to
/// `tokens`, keyed by the UCS-4 text of `phrase`, in the profile under
/// `user_dir`: the stored state is loaded when a profile exists (a fresh
/// directory starts empty), the row replaced, and the profile saved again
/// in the running backend's format. Its tokens were not written by the library.
pub fn set_phrase_table_row(user_dir: &std::path::Path, phrase: &str, tokens: &[u32]) {
    use oxpinyin::store::{RawReadStore as _, WriteStore as _};
    if !user_dir.join("user.conf").exists() {
        let conf =
            oxpinyin::data::TableConf::load_from_dir(&system_dir()).expect("fixture table.conf");
        let versions = oxpinyin::data::user_files::SystemVersions::from_conf(&conf);
        let mut marker = oxpinyin::data::user_files::UserTableInfo::conform_to(&versions);
        marker.open_counter = 1;
        assert!(marker.save(user_dir.join("user.conf").as_path()));
    }
    let path = user_dir.join(oxpinyin::data::user_files::UserDbm::PhraseIndex.file_name());
    let mut rows = std::collections::BTreeMap::new();
    if path.exists() {
        oxpinyin::store::DefaultStore::open_user_index(&path)
            .expect("open the phrase table")
            .copy_raw(|key, value| {
                rows.insert(key.to_vec(), value.to_vec());
            })
            .expect("read the phrase table");
    }
    let key = phrase
        .chars()
        .flat_map(|c| (c as u32).to_le_bytes())
        .collect();
    let value = tokens.iter().flat_map(|t| t.to_le_bytes()).collect();
    rows.insert(key, value);
    oxpinyin::store::DefaultStore::write_user_index(&path, |sink| {
        rows.iter().try_for_each(|(key, value)| sink(key, value))
    })
    .expect("write the phrase table");
}

/// A private system directory under `parent`: every fixture file linked in,
/// so a test can replace one library.
pub fn private_system(parent: &std::path::Path) -> PathBuf {
    let dir = parent.join("system");
    std::fs::create_dir_all(&dir).expect("private system dir");
    for entry in std::fs::read_dir(system_dir()).expect("fixture dir") {
        let entry = entry.expect("fixture entry");
        link_file(&entry.path(), &dir.join(entry.file_name()));
    }
    dir
}

/// A symbolic link at `link` to `target`, on every OS; on Windows the
/// process needs the symbolic-link privilege (Developer Mode or an elevated
/// process), and the fixture is never copied in its place.
pub fn link_file(target: &std::path::Path, link: &std::path::Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("link a fixture file");
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_file(target, link) {
        // ERROR_PRIVILEGE_NOT_HELD
        assert_ne!(
            error.raw_os_error(),
            Some(1314),
            "link a fixture file {}: this process lacks the symbolic-link privilege",
            link.display()
        );
        panic!("link a fixture file {}: {error}", link.display());
    }
}

/// Replaces the system library `file` with a non-empty directory whose entry
/// name is 25 bytes long. Where a directory reads as a chunk
/// ([`oxpinyin::data::phrase_library::directory_reads_as_a_chunk`]) the
/// pin's `MemoryChunk::mmap` asserts on its header read
/// (`memory_chunk.h:493`); elsewhere the library cannot be mapped. Answers which.
#[cfg(unix)]
pub fn directory_library(system: &std::path::Path, file: &str) -> bool {
    let path = system.join(file);
    std::fs::remove_file(&path).expect("unlink the library");
    std::fs::create_dir(&path).expect("directory library");
    std::fs::write(path.join("e".repeat(25)), b"x").expect("fill the directory");
    oxpinyin::data::phrase_library::directory_reads_as_a_chunk(&path)
}

/// Writes the files of the pin-written profile `case` from
/// `testdata/pin-profiles.txt` into `dir`, byte for byte.
/// The profiles hold tkrzw files, so only the tkrzw pinned-data tests read them.
#[cfg(feature = "tkrzw")]
pub fn write_pin_profile(case: &str, dir: &std::path::Path) {
    let text = include_str!("../../capi/testdata/pin-profiles.txt");
    write_profile(text, case, dir);
}

/// The zhuyin pin's saved profile, with the same record format.
#[cfg(feature = "tkrzw")]
pub fn write_zhuyin_pin_profile(case: &str, dir: &std::path::Path) {
    let text = include_str!("../../zhuyin-capi/testdata/pin-profiles.txt");
    write_profile(text, case, dir);
}

#[cfg(feature = "tkrzw")]
fn write_profile(text: &str, case: &str, dir: &std::path::Path) {
    let mut in_case = false;
    let mut file: Option<(String, Vec<u8>)> = None;
    let mut found = false;
    let flush = |file: &mut Option<(String, Vec<u8>)>| {
        if let Some((name, bytes)) = file.take() {
            std::fs::write(dir.join(name), bytes).expect("write a profile file");
        }
    };
    for line in text.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut words = line.split(' ');
        match words.next() {
            Some("profile") => {
                flush(&mut file);
                in_case = words.next() == Some(case);
                found |= in_case;
            }
            Some("file") if in_case => {
                flush(&mut file);
                let name = words.next().expect("a file name").to_owned();
                let size = words
                    .next()
                    .expect("a size")
                    .parse()
                    .expect("a decimal size");
                file = Some((name, vec![0; size]));
            }
            Some(offset) if in_case => {
                let (_, bytes) = file.as_mut().expect("a byte run follows a file line");
                let at: usize = offset.parse().expect("a decimal offset");
                let hex = words.next().expect("the run's bytes");
                for (index, pair) in hex.as_bytes().chunks(2).enumerate() {
                    let pair = std::str::from_utf8(pair).expect("ASCII hex");
                    bytes[at + index] = u8::from_str_radix(pair, 16).expect("hex byte");
                }
            }
            _ => {}
        }
    }
    flush(&mut file);
    assert!(found, "no profile {case} in pin-profiles.txt");
}

/// One library-1 item of [`items_system`]: its text as code points, its
/// keys and its unigram.
pub type SystemItem<'a> = (&'a [u32], &'a [u16], u32);

/// A private system under `parent`, linked from `source`, whose library 1
/// holds exactly `items` (tokens 0x01000001 on, in order) under `total`,
/// with matching pinyin and phrase indexes. Answers its path.
pub fn items_system(
    source: &std::path::Path,
    parent: &std::path::Path,
    items: &[SystemItem<'_>],
    total: u32,
) -> PathBuf {
    libraries_system(source, parent, &[(1, "gb_char.bin", items, total)])
}

/// One library of [`libraries_system`]: its index, its chunk file, its items
/// and its total.
pub type SystemLibrary<'a> = (u8, &'a str, &'a [SystemItem<'a>], u32);

/// A private system under `parent`, linked from `source`, in which each
/// library of `libraries` holds exactly its items (slots 1 on, in order)
/// under its total. The pinyin and phrase indexes hold the items of these
/// libraries and nothing else. Answers its path.
pub fn libraries_system(
    source: &std::path::Path,
    parent: &std::path::Path,
    libraries: &[SystemLibrary<'_>],
) -> PathBuf {
    use std::collections::BTreeMap;
    let system = parent.join("system");
    std::fs::create_dir_all(&system).expect("private system dir");
    for entry in std::fs::read_dir(source).expect("system dir") {
        let entry = entry.expect("system entry");
        link_file(&entry.path(), &system.join(entry.file_name()));
    }
    let mut phonetic: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    let mut phrases: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    for &(library, file, items, total) in libraries {
        let mut content = vec![0_u8; 8];
        let mut offsets = vec![0_u32];
        for (slot, (text, keys, unigram)) in items.iter().enumerate() {
            let token = u32::from(library) << 24 | u32::try_from(slot + 1).expect("slot");
            let raw_text: Vec<u8> = text.iter().flat_map(|word| word.to_le_bytes()).collect();
            let raw_keys: Vec<u8> = keys.iter().flat_map(|word| word.to_le_bytes()).collect();
            offsets.push(u32::try_from(content.len()).expect("offset"));
            content.extend([u8::try_from(text.len()).expect("length"), 1]);
            content.extend(unigram.to_le_bytes());
            content.extend(&raw_text);
            content.extend(&raw_keys);
            content.extend(10_u32.to_le_bytes());
            let index: Vec<u8> = keys
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
        let chunk = system.join(file);
        std::fs::remove_file(&chunk).expect("unlink the library");
        std::fs::write(
            &chunk,
            oxpinyin::data::chunk_format::build_memory_chunk(&payload).expect("frame"),
        )
        .expect("write the library");
    }
    for (name, rows) in [
        ("pinyin_index.bin", phonetic),
        ("phrase_index.bin", phrases),
    ] {
        let path = system.join(name);
        std::fs::remove_file(&path).expect("unlink an index");
        craft_tree(&path, &rows.into_iter().collect::<Vec<_>>());
    }
    system
}

/// One item of [`addon_system`]'s addon library 4.
pub struct AddonItem<'a> {
    /// The stored UCS-4 words.
    pub text: &'a [u32],
    /// The stored readings: packed keys, tones included, and a count.
    pub readings: &'a [(&'a [u16], u32)],
    /// The keys the addon pinyin index files the item under. An item with
    /// no reading is indexed all the same, as the pin's index and item
    /// bodies are separate files.
    pub indexed_as: &'a [u16],
    /// The stored unigram.
    pub unigram: u32,
}

/// A private system under `parent`, linked from `source`, whose addon
/// library 4 holds exactly `items` (tokens 0x04000001 on, in order), with
/// matching addon pinyin and phrase indexes. Answers its path.
pub fn addon_system(
    source: &std::path::Path,
    parent: &std::path::Path,
    items: &[AddonItem<'_>],
) -> PathBuf {
    use std::collections::BTreeMap;
    let system = parent.join("system");
    std::fs::create_dir_all(&system).expect("private system dir");
    for entry in std::fs::read_dir(source).expect("system dir") {
        let entry = entry.expect("system entry");
        link_file(&entry.path(), &system.join(entry.file_name()));
    }
    let words =
        |words: &[u16]| -> Vec<u8> { words.iter().flat_map(|word| word.to_le_bytes()).collect() };
    let mut content = vec![0_u8; 8];
    let mut offsets = vec![0_u32];
    let mut phonetic: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    let mut phrases: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    let mut total = 0_u32;
    for (slot, item) in items.iter().enumerate() {
        let token = 0x0400_0000 | u32::try_from(slot + 1).expect("slot");
        let raw_text: Vec<u8> = item
            .text
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        total = total.wrapping_add(item.unigram);
        offsets.push(u32::try_from(content.len()).expect("offset"));
        content.extend([
            u8::try_from(item.text.len()).expect("length"),
            u8::try_from(item.readings.len()).expect("readings"),
        ]);
        content.extend(item.unigram.to_le_bytes());
        content.extend(&raw_text);
        for (keys, count) in item.readings {
            content.extend(words(keys));
            content.extend(count.to_le_bytes());
        }
        let raw_keys = words(item.indexed_as);
        let index: Vec<u8> = item
            .indexed_as
            .iter()
            .flat_map(|word| (word & 4095).to_le_bytes())
            .collect();
        for end in (2..index.len()).step_by(2) {
            phonetic.entry(index[..end].to_vec()).or_default();
        }
        let mut record = token.to_le_bytes().to_vec();
        record.extend(&raw_keys);
        record.resize(record.len().div_ceil(4) * 4, 0);
        phonetic.entry(index).or_default().extend(record);
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
    let art = system.join("art.bin");
    std::fs::remove_file(&art).expect("unlink art.bin");
    std::fs::write(
        &art,
        oxpinyin::data::chunk_format::build_memory_chunk(&payload).expect("frame"),
    )
    .expect("art.bin");
    for (name, rows) in [
        ("addon_pinyin_index.bin", phonetic),
        ("addon_phrase_index.bin", phrases),
    ] {
        let path = system.join(name);
        std::fs::remove_file(&path).expect("unlink an addon index");
        craft_tree(&path, &rows.into_iter().collect::<Vec<_>>());
    }
    system
}

/// The row-130 library with an in-range but truncated second reading.
pub fn truncated_second_reading_system(parent: &std::path::Path) -> PathBuf {
    let system = items_system(&system_dir(), parent, &[(&[0x4e19], &[0x1081], 100)], 2000);
    let gb_char = system.join("gb_char.bin");
    let mut payload = std::fs::read(&gb_char).expect("gb_char.bin")[8..].to_vec();
    let content = u32::from_le_bytes(payload[8..12].try_into().expect("index two")) as usize;
    // The item at content offset 8 now declares two readings.
    payload[content + 8 + 1] = 2;
    std::fs::remove_file(&gb_char).expect("unlink gb_char.bin");
    std::fs::write(
        &gb_char,
        oxpinyin::data::chunk_format::build_memory_chunk(&payload).expect("frame"),
    )
    .expect("gb_char.bin");
    system
}

/// The two short rows of the table-conf-919-short-row-merged witness.
pub fn short_row_system(parent: &std::path::Path) -> PathBuf {
    let system = private_system(parent);
    let conf = system.join("table.conf");
    let stock = std::fs::read_to_string(&conf).unwrap();
    std::fs::remove_file(&conf).unwrap();
    std::fs::write(
        &conf,
        stock
            + "default GB_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin\n"
            + "SYSTEM_FILE default RESERVED NULL NULL NULL\n",
    )
    .unwrap();
    system
}

/// The checksum-valid chunk whose second separator fails the layout read.
pub fn replace_with_failed_layout(library: &std::path::Path, total: u32) {
    let mut payload = [0_u8; 32];
    for (at, value) in [(0, total), (4, 17), (8, 20), (12, 24)] {
        payload[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    payload[16] = b'#';
    payload[19] = b'!';
    let staged = library.with_extension("bin.new");
    std::fs::write(
        &staged,
        oxpinyin::data::chunk_format::build_memory_chunk(&payload).unwrap(),
    )
    .unwrap();
    std::fs::rename(staged, library).unwrap();
}
