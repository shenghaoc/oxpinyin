//! The journal_replay family's case bodies: journal
//! replay of system items (#850, #851, #870, #871), the unigram lifecycle (#853),
//! removed items (#1231) and chunk frames (#1576). Each body answers the result
//! document the old case returned.

mod chunk_frame;
mod lifecycle;
mod pron;
mod removed;
mod replay;

use std::collections::BTreeMap;
use std::path::Path;

use crate::facade;
use crate::fixtures::log_walk;
use crate::readers::{FacadeOut, GuessRows, c, unigram_of};
use crate::support::{J, Kit, P, take_text};

/// Runs the case `id` and answers its result document.
pub fn case(k: &mut Kit, id: &str) -> J {
    let (name, _mode) = id.rsplit_once('-').expect("a mode suffix");
    if let Some(name) = name.strip_prefix("replay-pron-") {
        return pron::case(k, name);
    }
    if let Some(name) = name.strip_prefix("replay-") {
        return replay::case(k, name);
    }
    match name {
        "unigram-refused-reload" => lifecycle::refused_add_reload(k),
        "removed-candidate" => removed::removed_candidate(k),
        "chunk-frame-journal-exact" => chunk_frame::journal(k),
        "chunk-frame-user-trailing" => chunk_frame::user(k),
        "unigram-lifecycle-overflow" => lifecycle::unigram_lifecycle(k),
        other => panic!("no journal_replay case {other}"),
    }
}

/// The journal record kinds (`phrase_index_logger.h`).
const LOG_ADD: u32 = 1;
const LOG_REMOVE: u32 = 2;
const LOG_MODIFY: u32 = 3;

/// One record as `PhraseIndexLogger::next_record` reads it
/// (`phrase_index_logger.h:160-209`): ADD and REMOVE carry one length and
/// one chunk, MODIFY both lengths and both chunks.
fn log_record(kind: u32, token: u32, old: &[u8], new: &[u8]) -> Vec<u8> {
    let len = |chunk: &[u8]| u16::try_from(chunk.len()).expect("a record chunk length");
    let mut record = [kind.to_le_bytes(), token.to_le_bytes()].concat();
    match kind {
        LOG_ADD => {
            record.extend(len(new).to_le_bytes());
            record.extend(new);
        }
        LOG_REMOVE => {
            record.extend(len(old).to_le_bytes());
            record.extend(old);
        }
        _ => {
            record.extend(len(old).to_le_bytes());
            record.extend(len(new).to_le_bytes());
            record.extend(old);
            record.extend(new);
        }
    }
    record
}

fn add_record(token: u32, new: &[u8]) -> Vec<u8> {
    log_record(LOG_ADD, token, &[], new)
}

fn remove_record(token: u32, old: &[u8]) -> Vec<u8> {
    log_record(LOG_REMOVE, token, old, &[])
}

fn modify_record(token: u32, old: &[u8], new: &[u8]) -> Vec<u8> {
    log_record(LOG_MODIFY, token, old, new)
}

/// `get_phrase_item`'s length from the header bytes alone.
fn item_size(item: &[u8]) -> usize {
    let (length, prons) = (usize::from(item[0]), usize::from(item[1]));
    6 + 4 * length + prons * (2 * length + 4)
}

/// A system chunk's total, its items `{slot: bytes}` at their header length,
/// and the item-area offsets `{slot: offset}` (`_system_items`).
struct Library {
    total: u32,
    items: BTreeMap<u32, Vec<u8>>,
    offsets: BTreeMap<u32, u32>,
}

impl Library {
    /// The highest slot that holds an item.
    fn max_slot(&self) -> u32 {
        *self.items.keys().next_back().expect("an item")
    }

    /// The first slot whose item passes `pick`, in slot order.
    fn first(&self, pick: impl Fn(&[u8]) -> bool) -> u32 {
        *self
            .items
            .iter()
            .find(|(_, item)| pick(item))
            .expect("a matching item")
            .0
    }

    /// The slots in ascending item-area offset.
    fn by_offset(&self) -> Vec<(u32, u32)> {
        let mut ordered: Vec<(u32, u32)> = self.offsets.iter().map(|(s, o)| (*s, *o)).collect();
        ordered.sort_by_key(|&(_, offset)| offset);
        ordered
    }
}

fn system_items(data: &Path, name: &str) -> Library {
    let bytes = std::fs::read(data.join(name)).expect("a system library");
    let payload = &bytes[8..];
    let word =
        |offset: usize| u32::from_le_bytes(payload[offset..offset + 4].try_into().expect("a word"));
    let (total, index, content) = (word(0), word(4) as usize, word(8) as usize);
    let mut items = BTreeMap::new();
    let mut offsets = BTreeMap::new();
    for slot in 0..(content - index - 1) / 4 {
        let offset = word(index + slot * 4);
        if offset != 0 {
            let start = content + offset as usize;
            let end = (start + item_size(&payload[start..start + 2])).min(payload.len());
            let slot = u32::try_from(slot).expect("a slot");
            items.insert(slot, payload[start..end].to_vec());
            offsets.insert(slot, offset);
        }
    }
    Library {
        total,
        items,
        offsets,
    }
}

/// `item` with its unigram and/or its last pronunciation's frequency
/// replaced; the size is unchanged.
fn item_with(item: &[u8], unigram: Option<u32>, freq: Option<u32>) -> Vec<u8> {
    let mut item = item.to_vec();
    if let Some(unigram) = unigram {
        item[2..6].copy_from_slice(&unigram.to_le_bytes());
    }
    if let Some(freq) = freq {
        let at = item.len() - 4;
        item[at..].copy_from_slice(&freq.to_le_bytes());
    }
    item
}

/// `item` with its last byte changed: the same size, so only the `memcmp`
/// can refuse it.
fn flip(item: &[u8]) -> Vec<u8> {
    let mut item = item.to_vec();
    *item.last_mut().expect("a byte") ^= 0xFF;
    item
}

/// The item's counted four-byte little-endian character fields, the layout
/// of `PhraseItem::get_phrase_string` (`phrase_index.cpp:103-105`). `None`
/// for a field that is not a Unicode scalar.
fn item_text(item: &[u8]) -> Option<String> {
    (0..usize::from(item[0]))
        .map(|i| {
            let at = 6 + 4 * i;
            char::from_u32(u32::from_le_bytes(item[at..at + 4].try_into().ok()?))
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    hex(&sha2::Sha256::digest(bytes))
}

/// Inserts every key of the object `other` into the object `out`.
fn merge(out: &mut J, other: J) {
    let J::M(map) = other else {
        panic!("merge a non-object")
    };
    for (key, value) in map {
        out.set(key, value);
    }
}

/// `token_phrase_of`: the phrase getter's flag, length and text, outputs
/// seeded with zero and NULL.
fn token_phrase_of(k: &mut Kit, token: u32) -> J {
    phrase_j(k.inst(), token)
}

fn phrase_j(inst: P, token: u32) -> J {
    let (mut length, mut text) = (0_u32, std::ptr::null_mut());
    let ret = facade::token_get_phrase(inst, token, &raw mut length, &raw mut text);
    J::from((ret, length, take_text(text)))
}

/// The phrase text a [`token_phrase_of`] answer holds.
fn phrase_text(phrase: &J) -> Option<String> {
    match &phrase.items()[2] {
        J::S(text) => Some(text.clone()),
        _ => None,
    }
}

fn unigram_j(k: &mut Kit, token: u32) -> J {
    J::from(unigram_of(k, token))
}

/// `remember_export`: the export rows of `phrase` in `library`. `adjacent`
/// stops at the first row of another phrase after the phrase's own. The
/// zhuyin facade has no export API.
fn export(k: &mut Kit, library: u32, phrase: Option<&str>, adjacent: bool) -> J {
    if facade::ZHUYIN {
        return J::from("NO EXPORT API");
    }
    let it = facade::begin_get_phrases(k.ctx(), library);
    let mut rows = Vec::new();
    while facade::iterator_has_next_phrase(it) {
        let (mut text, mut reading, mut count) =
            (std::ptr::null_mut(), std::ptr::null_mut(), 0_i32);
        let ret =
            facade::iterator_get_next_phrase(it, &raw mut text, &raw mut reading, &raw mut count);
        let (text, reading) = (take_text(text), take_text(reading));
        if text.as_deref() == phrase {
            rows.push(J::from((ret, text, reading, count)));
        } else if adjacent && !rows.is_empty() {
            break;
        }
    }
    facade::end_get_phrases(it);
    J::from(rows)
}

fn guess_j(rows: GuessRows) -> J {
    obj! { "ret" => rows.ret, "rows" => rows.rows }
}

fn facade_j(out: FacadeOut) -> J {
    obj! {
        "parsed" => out.parsed,
        "guessed" => out.guessed,
        "sentences" => out.sentences,
        "candidates" => out.candidates,
        "segment" => out.segment,
    }
}

/// `_facade_total_out` on the cached instance.
fn facade_total(k: &mut Kit) -> J {
    facade_j(crate::readers::facade_total_out(k))
}

fn training_j(training: super::case_helpers::Training) -> J {
    obj! {
        "chosen" => training.chosen,
        "kind" => training.kind,
        "cursor" => training.cursor,
        "after-choose" => training.after_choose,
        "guess" => training.guess,
        "train" => training.train,
    }
}

/// `train_selected_top` as its result document.
fn train_selected_top(k: &mut Kit, wanted: &str, token: u32) -> J {
    training_j(super::case_helpers::train_selected_top(k, wanted, token))
}

/// `import_rows` as its result document.
fn import_rows(k: &mut Kit, library: u8, rows: &[(&str, &str, i32)]) -> J {
    J::from(crate::readers::import_rows(k, library, rows))
}

/// The import that dirties a save: 你好 into library 7 with count one.
fn import_nihao(k: &mut Kit) -> bool {
    let reading = if facade::ZHUYIN {
        "ㄋㄧˇ ㄏㄠˇ"
    } else {
        "ni3'hao3"
    };
    let it = facade::begin_add_phrases(k.ctx(), 7);
    let added = facade::iterator_add_phrase(it, c("你好").as_ptr(), c(reading).as_ptr(), 1);
    facade::end_add_phrases(it);
    added
}

/// The cached instance's parse of the facade's training input for 癫痫.
fn parse_training_input(k: &mut Kit) -> usize {
    let inst = k.inst();
    if facade::ZHUYIN {
        facade::parse_more_chewings(inst, c("2u04vu04").as_ptr())
    } else {
        facade::parse_more_full_pinyins(inst, c("dian4xian4").as_ptr())
    }
}

/// The user directory's `name`.
fn user_file(k: &Kit, name: &str) -> Vec<u8> {
    std::fs::read(k.user.join(name)).expect("a user file")
}

/// Frees the cached instance, finalises the cached context and forgets both,
/// opening them first where the case has not (`k.fn('free_instance')(k.inst)`
/// then `k.fn('fini')(k.ctx)`).
fn close(k: &mut Kit) {
    k.inst();
    k.close();
}
