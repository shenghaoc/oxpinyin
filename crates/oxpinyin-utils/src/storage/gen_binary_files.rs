//! `gen_binary_files` (`utils/storage/gen_binary_files.cpp` at libpinyin
//! 074a2219).
//!
//! Reads `table.conf` from the current directory and, for the default and
//! addon table sets, loads each `SYSTEM_FILE`/`DICTIONARY` `.table` file
//! three times from one rewind: into the pinyin index (`ChewingLargeTable2`),
//! the phrase index (`PhraseLargeTable3`) and the phrase-library chunk
//! (`FacadePhraseIndex`). It compacts the chunks, saves the system chunks
//! (`save_phrase_index`) and the dictionary chunks (`save_dictionary`), and
//! optionally loads `punct.table` into `punct.bin`.

use std::collections::BTreeMap;
use std::ffi::{CStr, OsString};
use std::path::PathBuf;

use oxpinyin_data::chunk_write::{ChunkItem, PHRASE_MASK};
use oxpinyin_data::row_format::phrase_index::encode_tokens;
use oxpinyin_data::row_format::pinyin_index::{
    PinyinIndexItem, decode_items, encode_complete_key, encode_incomplete_key, encode_item,
};
use oxpinyin_data::table_entries::exact_compare2;
use oxpinyin_data::table_info::{FileType, PhoneticType, TableConf, TableRow};
use oxpinyin_input::tool;
use oxpinyin_input::tool::OptionValue;
use oxpinyin_input::tool::storage::TableFile;

use crate::storage::dbm::Tree;
use crate::storage::libraries::save_library;
use crate::storage::pinyin;
use crate::storage::table_text::{read_pinyin_record, read_punct_record};
use crate::storage::{
    gen_punct_entry, load_table_conf, name_c_string, parse_options, table_dir, table_dir_entry,
};

/// `main` (`gen_binary_files.cpp:111-152`).
pub fn main(args: impl IntoIterator<Item = OsString>) {
    let _ = tool::set_locale_from_environment();
    let entries = [table_dir_entry(), gen_punct_entry()];
    let parsed = parse_options(c"- generate binary files", &entries, args);
    let directory = table_dir(&parsed, 0);
    let gen_punct = matches!(parsed.values[1], OptionValue::Flag(true));

    let conf = load_table_conf();
    let phonetic = conf.phonetic_type();

    generate_binary_files(
        c"pinyin_index.bin",
        c"phrase_index.bin",
        &conf,
        false,
        phonetic,
        directory.as_deref(),
    );
    generate_binary_files(
        c"addon_pinyin_index.bin",
        c"addon_phrase_index.bin",
        &conf,
        true,
        phonetic,
        directory.as_deref(),
    );

    if gen_punct {
        generate_punct_table(c"punct.bin", directory.as_deref());
    }
}

/// One index file's path as the store opens it.
fn path_of(name: &CStr) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        PathBuf::from(std::ffi::OsStr::from_bytes(name.to_bytes()))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(name.to_string_lossy().into_owned())
    }
}

/// The rows one `generate_binary_files` call walks (default or addon).
fn rows(conf: &TableConf, addon: bool) -> Vec<&TableRow> {
    (0..16_u8)
        .filter_map(|index| {
            if addon {
                conf.addon_row(index)
            } else {
                conf.default_row(index)
            }
        })
        .collect()
}

/// The per-library chunk items while the table file is read.
type ChunkMap = BTreeMap<u32, ChunkItem>;

/// `generate_binary_files` (`:36-86`).
fn generate_binary_files(
    pinyin_name: &CStr,
    phrase_name: &CStr,
    conf: &TableConf,
    addon: bool,
    phonetic: PhoneticType,
    directory: Option<&CStr>,
) {
    // `attach` ignores its answer; a failed open leaves `m_db` NULL, which
    // the first `add_index` asserts against. Modelled as `None`.
    let pinyin_table = Tree::create(&path_of(pinyin_name)).ok();
    let phrase_table = Tree::create(&path_of(phrase_name)).ok();

    let mut libraries: [Option<ChunkMap>; 16] = Default::default();
    for (index, row) in rows(conf, addon).into_iter().enumerate() {
        if !matches!(row.file_type, FileType::SystemFile | FileType::Dictionary) {
            continue;
        }
        let Some(tablename) = row.table.as_ref() else {
            // `g_build_filename(table_dir, NULL, NULL)` and `fopen` on the
            // directory; no C caller passes a NULL name upstream.
            tool::abort();
        };
        let name = name_c_string(tablename.as_ref()).unwrap_or_else(|| tool::abort());
        let joined =
            tool::storage::build_filename(directory, &name).unwrap_or_else(|| tool::abort());
        let Some(mut file) = TableFile::open(&joined) else {
            tool::storage::report_open_failed(&name);
            tool::exit(tool::ENOENT);
        };
        load_chewing(pinyin_table.as_ref(), &mut file, phonetic);
        file.rewind();
        load_phrase_table(phrase_table.as_ref(), &mut file);
        file.rewind();
        load_phrase_index(index as u8, &mut file, &mut libraries);
    }

    compact(&mut libraries);
    save_chunks(conf, addon, false, &libraries);
    save_chunks(conf, addon, true, &libraries);
}

/// `ChewingLargeTable2::load_text` (`src/storage/chewing_large_table2.cpp:112-158`).
fn load_chewing(table: Option<&Tree>, file: &mut TableFile, phonetic: PhoneticType) {
    while let Some(record) = read_pinyin_record(file) {
        let len = tool::utf8_strlen(&record.phrase);
        let keys = match phonetic {
            PhoneticType::Pinyin => pinyin::parse(&record.pinyin),
            PhoneticType::Zhuyin => pinyin::parse_zhuyin(&record.pinyin),
        };
        if len != i64::try_from(keys.len()).unwrap_or(i64::MAX) {
            tool::storage::report_chewing_skip(
                &record.pinyin,
                &record.phrase,
                record.token,
                record.freq,
            );
            continue;
        }
        add_index(table, &keys, record.token);
    }
}

/// `ChewingLargeTable2::add_index` (`:179-198`): both key spaces, then
/// `add_index_internal`'s prefix fill.
fn add_index(table: Option<&Tree>, keys: &[oxpinyin_core::ChewingKey], token: u32) {
    let incomplete = encode_incomplete_key(keys);
    add_index_space(table, &incomplete, keys, token);
    let complete = encode_complete_key(keys);
    add_index_space(table, &complete, keys, token);
}

/// `ChewingLargeTable2::add_index_internal` (`chewing_large_table2_bdb.cpp:290-358`,
/// `chewing_large_table2_tkrzwdb.cpp:260-313`).
fn add_index_space(
    table: Option<&Tree>,
    key: &[u8],
    keys: &[oxpinyin_core::ChewingKey],
    token: u32,
) {
    let Some(table) = table else {
        // `assert(NULL != m_db)`.
        tool::abort();
    };
    // `add_index_internal(phrase_length, ...)`'s switch has cases 1..=16
    // and `abort()` elsewhere.
    if keys.is_empty() || keys.len() > crate::storage::MAX_PHRASE_LENGTH {
        tool::abort();
    }
    let phrase_length = key.len() / 2;
    match table.get(key) {
        None => {
            let insert = encode_item(token, keys);
            let _ = table.put(key, &insert);
            // recursively add keys for continued information.
            for length in (1..phrase_length).rev() {
                let prefix = &key[..2 * length];
                if table.get(prefix).is_some() {
                    return;
                }
                let _ = table.put(prefix, &[]);
            }
        }
        Some(value) => {
            let mut records = decode_items(&value, phrase_length).unwrap_or_else(|_| tool::abort());
            let mut insert_at = records.len();
            for (index, record) in records.iter().enumerate() {
                match exact_compare2(&record.keys, keys) {
                    std::cmp::Ordering::Less => continue,
                    std::cmp::Ordering::Equal => {
                        if record.token == token {
                            return;
                        }
                        if record.token > token {
                            insert_at = index;
                            break;
                        }
                    }
                    std::cmp::Ordering::Greater => {
                        insert_at = index;
                        break;
                    }
                }
            }
            records.insert(
                insert_at,
                PinyinIndexItem {
                    token,
                    keys: keys.to_vec(),
                },
            );
            let mut encoded = Vec::with_capacity(value.len() + 4 + 2 * keys.len());
            for record in &records {
                encoded.extend_from_slice(&encode_item(record.token, &record.keys));
            }
            let _ = table.put(key, &encoded);
        }
    }
}

/// `PhraseLargeTable3::load_text` (`src/storage/phrase_large_table3.cpp:125-140`).
fn load_phrase_table(table: Option<&Tree>, file: &mut TableFile) {
    while let Some(record) = read_pinyin_record(file) {
        let (phrase, _written) = tool::utf8_to_ucs4(&record.phrase);
        let phrase = phrase.unwrap_or_default();
        add_phrase_index(table, &phrase, record.token);
    }
}

/// `PhraseLargeTable3::add_index` (`phrase_large_table3_bdb.cpp:268-334`).
fn add_phrase_index(table: Option<&Tree>, phrase: &[u32], token: u32) {
    let Some(table) = table else {
        // `assert(NULL != m_db)`.
        tool::abort();
    };
    let key = oxpinyin_data::row_format::phrase_index::encode_ucs4_key_scalars(phrase);
    match table.get(&key) {
        None => {
            let _ = table.put(&key, &token.to_le_bytes());
            for length in (1..phrase.len()).rev() {
                let prefix = &key[..4 * length];
                if table.get(prefix).is_some() {
                    return;
                }
                let _ = table.put(prefix, &[]);
            }
        }
        Some(value) => {
            let mut tokens = oxpinyin_data::row_format::phrase_index::decode_tokens(&value)
                .unwrap_or_else(|_| tool::abort());
            if let Err(position) = tokens.binary_search(&token) {
                tokens.insert(position, token);
            }
            let _ = table.put(&key, &encode_tokens(&tokens));
        }
    }
}

/// `FacadePhraseIndex::load_text` (`src/storage/phrase_index.cpp:522-596`).
fn load_phrase_index(index: u8, file: &mut TableFile, libraries: &mut [Option<ChunkMap>; 16]) {
    let mut items: ChunkMap = BTreeMap::new();
    let mut cur_token = 0_u32;
    let mut item = fresh_item();
    while let Some(record) = read_pinyin_record(file) {
        // `assert(PHRASE_INDEX_LIBRARY_INDEX(token) == phrase_index)`.
        if crate::storage::library_of(record.token) != index {
            tool::abort();
        }
        let (phrase, written) = tool::utf8_to_ucs4(&record.phrase);
        let phrase = phrase.unwrap_or_default();
        // `set_phrase_string(written, phrase)`: the guint8 `written` field
        // and that many words.
        let length = usize::try_from(written).unwrap_or(0) % 256;
        let phrase: Vec<u32> = phrase.into_iter().take(length).collect();
        if cur_token == 0 {
            cur_token = record.token;
            item.phrase = phrase.clone();
        }
        if cur_token != record.token {
            items.insert(cur_token & PHRASE_MASK, item);
            item = fresh_item();
            cur_token = record.token;
            item.phrase = phrase;
        }
        let keys = pinyin::parse(&record.pinyin);
        if item.phrase.len() == keys.len() {
            add_pronunciation(&mut item, &keys, record.token, record.freq);
        } else {
            tool::storage::report_phrase_index_skip(&record.pinyin, &record.phrase);
        }
    }
    items.insert(cur_token & PHRASE_MASK, item);
    libraries[usize::from(index)] = Some(items);
}

/// `FacadePhraseIndex::add_phrase_item`'s `PhraseItem`.
fn fresh_item() -> ChunkItem {
    ChunkItem {
        phrase: Vec::new(),
        unigram: 0,
        prons: Vec::new(),
    }
}

/// `PhraseItem::add_pronunciation` (`src/storage/phrase_index.cpp:56-93`);
/// `freq` is `input.freq` narrowed to the `guint32` delta.
fn add_pronunciation(
    item: &mut ChunkItem,
    keys: &[oxpinyin_core::ChewingKey],
    token: u32,
    freq: usize,
) {
    let _ = token;
    let packed = pinyin::packed(keys);
    let delta = u32::try_from(freq).unwrap_or(u32::MAX);
    let mut total_freq = 0_u32;
    for (stored, stored_freq) in &mut item.prons {
        total_freq = total_freq.wrapping_add(*stored_freq);
        if *stored == packed {
            // protect against total_freq overflow.
            if delta > 0 && total_freq > total_freq.wrapping_add(delta) {
                return;
            }
            *stored_freq = stored_freq.wrapping_add(delta);
            return;
        }
    }
    item.prons.push((packed, delta));
}

/// `FacadePhraseIndex::compact` (`src/storage/phrase_index.cpp:648-675`):
/// the token walk starts at 1, so slot zero is dropped.
fn compact(libraries: &mut [Option<ChunkMap>; 16]) {
    for library in libraries.iter_mut().flatten() {
        library.retain(|slot, _| *slot >= 1);
    }
}

/// `save_phrase_index` (`utils/utils_helper.h:97-120`) for `DICTIONARY`
/// rows, or `save_dictionary` (`:122-145`) for `SYSTEM_FILE` rows.
fn save_chunks(
    conf: &TableConf,
    addon: bool,
    dictionary: bool,
    libraries: &[Option<ChunkMap>; 16],
) {
    let wanted = if dictionary {
        FileType::Dictionary
    } else {
        FileType::SystemFile
    };
    for (index, row) in rows(conf, addon).into_iter().enumerate() {
        if row.file_type != wanted {
            continue;
        }
        let Some(name) = row.system.as_ref() else {
            // `MemoryChunk::save(NULL)`.
            tool::abort();
        };
        let name = name_c_string(name.as_ref()).unwrap_or_else(|| tool::abort());
        let file = match &libraries[index] {
            Some(items) => {
                let items: Vec<(u32, ChunkItem)> = items
                    .iter()
                    .map(|(slot, item)| (*slot, item.clone()))
                    .collect();
                oxpinyin_data::chunk_write::build_chunk(&items).unwrap_or_else(|_| tool::abort())
            }
            // The pin stores an empty `MemoryChunk` when the library never
            // loaded: the eight-byte zero header.
            None => vec![0; 8],
        };
        save_library(&name, &file);
    }
}

/// `generate_punct_table` (`gen_binary_files.cpp:88-109`).
fn generate_punct_table(table_name: &CStr, directory: Option<&CStr>) {
    let Ok(table) = Tree::create(&path_of(table_name)) else {
        tool::storage::report_open_failed(table_name);
        tool::exit(tool::ENOENT);
    };
    let joined =
        tool::storage::build_filename(directory, c"punct.table").unwrap_or_else(|| tool::abort());
    let Some(mut file) = TableFile::open(&joined) else {
        tool::storage::report_open_failed(&joined);
        tool::exit(tool::ENOENT);
    };
    while let Some(record) = read_punct_record(&mut file) {
        append_punctuation(&table, record.token, &record.punctuation);
    }
}

/// `PunctTable::append_punctuation` / `PunctTableEntry::append_punctuation`
/// (`src/storage/punct_table.cpp:97-115`, `:158-167`).
fn append_punctuation(table: &Tree, token: u32, punctuation: &CStr) {
    let key = token.to_le_bytes();
    let (ucs4, _written) = tool::utf8_to_ucs4(punctuation);
    let ucs4 = ucs4.unwrap_or_default();
    let existing = table.get(&key).unwrap_or_default();
    // `get_all_punctuations`: zero-terminated UCS-4 runs.
    let mut runs: Vec<Vec<u32>> = Vec::new();
    let mut current = Vec::new();
    for word in existing
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
    {
        if word == 0 {
            runs.push(std::mem::take(&mut current));
        } else {
            current.push(word);
        }
    }
    if runs.contains(&ucs4) {
        tool::storage::report_duplicated_punctuation(punctuation);
        return;
    }
    let mut value = existing;
    for word in &ucs4 {
        value.extend_from_slice(&word.to_le_bytes());
    }
    value.extend_from_slice(&0_u32.to_le_bytes());
    let _ = table.put(&key, &value);
}
