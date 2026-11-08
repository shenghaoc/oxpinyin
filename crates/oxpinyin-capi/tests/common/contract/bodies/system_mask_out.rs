//! The system_mask_out family's case bodies: each answers the result document the old case returned.
//!
//! Inventory row 36 (#1175, #728, #545): `mask_out` over a system library item
//! imported into library 1, kept in the journal or saved (`phrase_index.cpp:855-856`,
//! `phrase_index_logger.h:245,267-268`), over a MODIFY record or a replaced
//! system chunk, and a refused remap that leaves the session intact (row 76).

use std::collections::BTreeMap;
use std::path::Path;

use crate::facade;
use crate::fixtures::{chunk, modify_header, private_system, write_log};
use crate::readers::{c, owned_out, seeded, tokens_of, unigram_of};
use crate::support::{J, Kit, StderrCapture, take_text};

/// 你 in library one.
const NI: u32 = 0x0100_1225;

/// Runs the case `id` and answers its result document.
pub fn case(k: &mut Kit, id: &str) -> J {
    let (name, _mode) = id.rsplit_once('-').expect("a mode suffix");
    let reload = |saved, mask, selected, headerless, dropped| Reload {
        saved,
        mask,
        selected,
        headerless,
        dropped,
    };
    let spec = match name {
        "mask-out-system-unsaved" => reload(false, 0xffff_ffff, false, false, false),
        "mask-out-system-saved" => reload(true, 0xffff_ffff, false, false, false),
        "abort-mask-out-surviving-add" => reload(true, 0x00ff_ffff, false, false, false),
        "mask-out-system-selected-add" => reload(true, 0x00ff_ffff, true, false, false),
        "mask-out-system-headerless-add" => reload(true, 0x00ff_ffff, false, true, false),
        "mask-out-dropped-add-full" => reload(true, 0xffff_ffff, false, false, true),
        "mask-out-dropped-add-headerless" => reload(true, 0x00ff_ffff, false, true, true),
        "mask-out-system-modify" => return system_modify(k, true, false),
        "mask-out-system-no-header" => return system_modify(k, false, false),
        "mask-out-system-disk-replacement" => return system_modify(k, true, true),
        "mask-out-unmappable-no-change" => return unmappable_keeps_session(k),
        other => panic!("no system_mask_out case {other}"),
    };
    system_reload(k, &spec)
}

/// `token_phrase_of`: the phrase getter's flag, length and text on the
/// cached instance, outputs seeded with zero and NULL.
fn token_phrase_of(k: &mut Kit, token: u32) -> J {
    let inst = k.inst();
    let (mut length, mut text) = (0_u32, std::ptr::null_mut());
    let ret = facade::token_get_phrase(inst, token, &raw mut length, &raw mut text);
    J::from((ret, length, take_text(text)))
}

/// `mask_out_system_reload`'s arguments.
struct Reload {
    saved: bool,
    mask: u32,
    selected: bool,
    headerless: bool,
    dropped: bool,
}

/// Imports `phrase` under `reading` (pinyin or zhuyin) into `library`,
/// answering the add.
fn import_one(k: &mut Kit, library: u8, phrase: &str, pinyin: &str, zhuyin: &str) -> bool {
    let it = facade::begin_add_phrases(k.ctx(), library);
    assert!(!it.is_null(), "the import iterator did not open");
    let reading = if facade::ZHUYIN { zhuyin } else { pinyin };
    let added = facade::iterator_add_phrase(it, c(phrase).as_ptr(), c(reading).as_ptr(), 1);
    facade::end_add_phrases(it);
    added
}

/// `mask_out_system_reload`: 龘龘 imported into library 1, optionally saved
/// (and its journal stripped of the leading MODIFY_HEADER), then masked out.
fn system_reload(k: &mut Kit, spec: &Reload) -> J {
    let phrase = "龘龘";
    assert!(
        tokens_of(k, phrase).is_empty(),
        "the setup phrase already exists"
    );
    let added = import_one(k, 1, phrase, "da2'da2", "ㄉㄚˊ ㄉㄚˊ");
    assert!(added, "the setup phrase did not add");
    let tokens = tokens_of(k, phrase);
    assert!(
        tokens.len() == 1 && tokens[0] >> 24 == 1,
        "the import used the wrong library"
    );
    let mut out = obj! { "added" => added, "before" => token_phrase_of(k, tokens[0]) };
    if spec.saved {
        let save = facade::save(k.ctx());
        out.set("save", save);
        assert!(save, "the setup save failed");
    }
    if spec.headerless {
        let data = std::fs::read(k.user.join("gb_char.dbin")).expect("the journal");
        let payload = &data[8..];
        assert_eq!(
            u32::from_le_bytes(payload[..4].try_into().expect("a word")),
            4
        );
        write_log(k, &payload[18..], "gb_char.dbin");
    }
    let value = if spec.selected {
        tokens[0] & 0x00ff_ffff
    } else {
        0
    };
    out.set("ret", facade::mask_out(k.ctx(), spec.mask, value));
    out.set("after", token_phrase_of(k, tokens[0]));
    out.set("phrase-table-before", tokens.clone());
    out.set("phrase-table-after", tokens_of(k, phrase));
    if spec.dropped {
        dropped_steps(k, &mut out, tokens[0], phrase);
    }
    out
}

/// The dropped-item observations: a refused unigram add and a sentence
/// lookup that skips the dangling table token.
fn dropped_steps(k: &mut Kit, out: &mut J, token: u32, phrase: &str) {
    let inst = k.inst();
    out.set(
        "add-unigram",
        facade::token_add_unigram_frequency(inst, token, 7),
    );
    out.set("unigram", unigram_of(k, token));
    out.set(
        "parsed",
        facade::parse_more_full_pinyins(inst, c("da2'da2").as_ptr()),
    );
    out.set("guessed", facade::guess_sentence(inst));
    let mut sentence = seeded();
    let ret = if facade::ZHUYIN {
        facade::get_sentence_only(inst, &raw mut sentence)
    } else {
        facade::get_sentence_at(inst, 0, &raw mut sentence)
    };
    let text = owned_out(sentence).unwrap_or_default();
    out.set("lookup", (ret, text.contains(phrase)));
}

/// `mask_out_system_modify`: 你's text replaced by 龘 through a MODIFY
/// record (with or without its header) or, `disk`, in a private system's
/// chunk after init, then a mask-out.
fn system_modify(k: &mut Kit, header: bool, disk: bool) -> J {
    let data = std::fs::read(k.data.join("gb_char.bin")).expect("gb_char.bin");
    let payload = &data[8..];
    let word = |at: usize| {
        usize::try_from(u32::from_le_bytes(
            payload[at..at + 4].try_into().expect("a word"),
        ))
        .expect("an offset")
    };
    let (total, index, content) = (word(0), word(4), word(8));
    let slot = usize::try_from(NI & 0x00ff_ffff).expect("a slot");
    let offset = content + word(index + slot * 4);
    let (length, prons) = (
        usize::from(payload[offset]),
        usize::from(payload[offset + 1]),
    );
    let size = 6 + length * 4 + prons * (length * 2 + 4);
    let old = &payload[offset..offset + size];
    assert!(length == 1 && old[6..10] == u32::from('你').to_le_bytes());
    let new = [&old[..6], &u32::from('龘').to_le_bytes(), &old[10..]].concat();
    let len16 = |bytes: &[u8]| u16::try_from(bytes.len()).expect("a record length");
    let record = [
        &3_u32.to_le_bytes()[..],
        &(NI & 0x00ff_ffff).to_le_bytes(),
        &len16(old).to_le_bytes(),
        &len16(&new).to_le_bytes(),
        old,
        &new,
    ]
    .concat();
    let system = disk.then(|| {
        let system = private_system(k);
        k.data = system.clone();
        k.ctx();
        system
    });
    let before = token_phrase_of(k, NI);
    let tokens = tokens_of(k, "你");
    if let Some(system) = &system {
        // Replace only a scratch link: the reference data is read-only.
        let path = system.join("gb_char.bin");
        std::fs::remove_file(&path).expect("unlink gb_char.bin");
        let replaced = [&payload[..offset], &new, &payload[offset + size..]].concat();
        std::fs::write(&path, chunk(&replaced)).expect("gb_char.bin");
    } else {
        let total = u32::try_from(total).expect("a total");
        let mut log = if header {
            modify_header(0, total, total)
        } else {
            Vec::new()
        };
        log.extend(record);
        write_log(k, &log, "gb_char.dbin");
    }
    let mask = if disk { 0xffff_ffff } else { 0x00ff_ffff };
    let ret = facade::mask_out(k.ctx(), mask, 0);
    obj! {
        "ret" => ret, "before" => before, "after" => token_phrase_of(k, NI),
        "phrase-table-before" => tokens, "phrase-table-after" => tokens_of(k, "你"),
    }
}

/// Every file under `root` by relative path, with its bytes.
fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("a user directory") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.is_file() {
                let name = path
                    .strip_prefix(root)
                    .expect("under the root")
                    .to_string_lossy()
                    .into_owned();
                found.insert(name, std::fs::read(&path).expect("a user file"));
            }
        }
    }
    found
}

/// The phrase observations of `tokens`.
fn phrases(k: &mut Kit, tokens: &[u32]) -> Vec<J> {
    tokens
        .iter()
        .map(|&token| token_phrase_of(k, token))
        .collect()
}

/// Row 76's live state: 龘龘 in library 1 and, after a save, 龘龘龘 in
/// library 7, and 你's unigram raised by 7. Answers the added tokens and the
/// trained unigram.
fn unmappable_setup(k: &mut Kit) -> (Vec<u32>, (bool, u32)) {
    let baseline = unigram_of(k, NI);
    assert!(baseline.0);
    let mut added = Vec::new();
    for (library, phrase, pinyin, zhuyin) in [
        (1, "龘龘", "da2'da2", "ㄉㄚˊ ㄉㄚˊ"),
        (7, "龘龘龘", "da2'da2'da2", "ㄉㄚˊ ㄉㄚˊ ㄉㄚˊ"),
    ] {
        if library == 7 {
            assert!(facade::save(k.ctx()));
        }
        assert!(tokens_of(k, phrase).is_empty());
        assert!(import_one(k, library, phrase, pinyin, zhuyin));
        let found = tokens_of(k, phrase);
        assert!(found.len() == 1 && found[0] >> 24 == u32::from(library));
        added.push(found[0]);
    }
    assert!(facade::token_add_unigram_frequency(k.inst(), NI, 7));
    let trained = unigram_of(k, NI);
    assert!(trained.0 && trained.1 == baseline.1 + 7);
    (added, trained)
}

/// `mask_out_unmappable_keeps_session`: with the private library 1 unlinked,
/// `mask_out` refuses the remap and leaves the files, the live counts and the
/// live items; the restored library saves and reopens with them.
fn unmappable_keeps_session(k: &mut Kit) -> J {
    let system = private_system(k);
    k.data = system.clone();
    k.ctx();
    let path = system.join("gb_char.bin");
    let original = std::fs::read(&path).expect("gb_char.bin");
    let (added, trained) = unmappable_setup(k);
    let before = phrases(k, &added);
    let saved_files = files(&k.user);
    std::fs::remove_file(&path).expect("unlink gb_char.bin");
    let capture = StderrCapture::start();
    let answer = facade::mask_out(k.ctx(), 0, 0);
    let stderr = capture.finish();
    let mut result = obj! {
        "mask" => answer,
        "line" => stderr == format!("mmap {} failed!\n", path.display()),
        "files_unchanged" => files(&k.user) == saved_files,
        "live_count_unchanged" => unigram_of(k, NI) == trained,
        "live_items_unchanged" => phrases(k, &added) == before,
    };
    std::fs::write(&path, original).expect("restore gb_char.bin");
    result.set("save", facade::save(k.ctx()));
    facade::free_instance(k.inst());
    facade::fini(k.ctx());
    k.forget();
    k.ctx();
    result.set("reopened_count_unchanged", unigram_of(k, NI) == trained);
    result.set("reopened_items_unchanged", phrases(k, &added) == before);
    result
}
