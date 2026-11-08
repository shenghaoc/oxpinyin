//! The bigram_removal family's case bodies: each answers the result document the old case returned.
//!
//! Inventory row 31 (#1175, #740): removal subtracts from the stored `guint32`
//! total, never from a sum of survivors (`ngram.cpp:76-102`,
//! `ngram_bdb.cpp:215-254`; `pinyin.cpp:1230,3766`; `zhuyin.cpp:765`). The
//! persisted rows are read after the save and again after a reopen.

use std::path::Path;

use oxpinyin_store::{DefaultStore, RawReadStore as _};

use crate::facade;
use crate::fixtures::{craft_hash, write_user_conf};
use crate::readers::{add_user_phrase, c, candidate_at, candidate_type, n_candidate, tokens_of};
use crate::support::{J, Kit, P};

/// One removal case's parameters (`removal_bigram`).
struct Removal {
    total: u32,
    survivors: [u32; 2],
    removed: u32,
    candidate: bool,
    multiple: bool,
    unchanged_total: u32,
}

/// `d18_bigram_db(k, path, read_keys=keys)`: the stored values of `keys`.
fn stored(path: &Path, keys: &[Vec<u8>]) -> Vec<String> {
    let store = DefaultStore::open_user_bigram(path).expect("the saved bigram opens");
    keys.iter()
        .map(|key| {
            let mut value = Vec::new();
            assert!(store.get_raw(key, &mut value).expect("read the bigram"));
            value.iter().map(|byte| format!("{byte:02x}")).collect()
        })
        .collect()
}

/// `_user_candidate_rows`: the NORMAL user candidates of the current list.
fn user_candidate_rows(inst: P) -> Vec<P> {
    (0..n_candidate(inst))
        .map(|index| candidate_at(inst, index).unwrap_or(std::ptr::null_mut()))
        .filter(|&cand| candidate_type(inst, cand) == 2 && facade::is_user_candidate(inst, cand))
        .collect()
}

/// Frees the cached instance and context and forgets them.
fn close(k: &mut Kit) {
    facade::free_instance(k.inst());
    facade::fini(k.ctx());
    k.forget();
}

/// Imports 龘龘 and plants the user grams; answers its token, the keys and the path.
fn plant(k: &mut Kit, case: &Removal) -> (u32, Vec<Vec<u8>>, std::path::PathBuf) {
    let reading: &[u8] = if facade::ZHUYIN {
        "ㄉㄚˊ ㄉㄚˊ".as_bytes()
    } else {
        b"da2'da2"
    };
    add_user_phrase(k, "龘龘", &[reading], 5);
    let token = tokens_of(k, "龘龘")
        .into_iter()
        .find(|token| token >> 24 == 7)
        .expect("a user token");
    let predecessors: Vec<u32> = ["你", "我"]
        .iter()
        .map(|text| tokens_of(k, text)[0])
        .collect();
    let successors: Vec<u32> = ["好", "号"]
        .iter()
        .map(|text| tokens_of(k, text)[0])
        .collect();
    assert!(facade::save(k.ctx()));
    close(k);
    let mut records: Vec<(u32, u32)> = successors
        .iter()
        .copied()
        .zip(case.survivors)
        .chain([(token, case.removed)])
        .collect();
    if case.multiple {
        records.push((token + 1, 1));
    }
    records.sort_unstable();
    let mut gram = case.total.to_le_bytes().to_vec();
    for (successor, count) in records {
        gram.extend(successor.to_le_bytes());
        gram.extend(count.to_le_bytes());
    }
    let unchanged: Vec<u8> = [case.unchanged_total, successors[0], 2]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    let keys: Vec<Vec<u8>> = predecessors
        .iter()
        .map(|token| token.to_le_bytes().to_vec())
        .collect();
    let path = k.user.join("user_bigram.db");
    craft_hash(
        &path,
        &[(keys[0].clone(), gram), (keys[1].clone(), unchanged)],
    );
    write_user_conf(k);
    (token, keys, path)
}

/// The removal: a user candidate's removal, or a `mask_out`.
fn remove(k: &mut Kit, case: &Removal, token: u32) -> bool {
    if case.candidate {
        let inst = k.inst();
        assert_eq!(facade::parse_more_full_pinyins(inst, c("dada").as_ptr()), 4);
        assert!(facade::guess_candidates(inst, 0, 0));
        let candidates = user_candidate_rows(inst);
        assert!(!candidates.is_empty(), "missing removable user candidate");
        return facade::remove_user_candidate(inst, candidates[0]);
    }
    let (mask, value) = if case.multiple {
        (0xff00_0000, 0x0700_0000)
    } else {
        (0xffff_ffff, token)
    };
    facade::mask_out(k.ctx(), mask, value)
}

/// `removal_bigram`'s probe.
fn probe(k: &mut Kit, case: &Removal) -> J {
    let (token, keys, path) = plant(k, case);
    let removed = remove(k, case, token);
    assert!(facade::set_options(k.ctx(), 1 << 5));
    let inst = k.inst();
    assert_eq!(facade::parse_more_full_pinyins(inst, c("wo").as_ptr()), 2);
    assert!(facade::guess_sentence(inst));
    let trained = facade::train_top(inst);
    let saved = facade::save(k.ctx());
    close(k);
    let stored_rows = stored(&path, &keys);
    facade::fini(k.ctx());
    k.forget();
    let reopened = stored(&path, &keys);
    obj! {
        "removed" => removed,
        "trained" => trained,
        "saved" => saved,
        "stored" => stored_rows,
        "reopened" => reopened,
    }
}

/// The registered case `id`'s parameters.
fn removal(id: &str) -> Removal {
    let rest = id
        .strip_prefix("removal-bigram-")
        .and_then(|rest| rest.strip_suffix(if facade::ZHUYIN { "-zhuyin" } else { "-pinyin" }))
        .unwrap_or_else(|| panic!("no case {id}"));
    if rest == "multiple" {
        return Removal {
            total: 1,
            survivors: [0xffff_ffff, 0],
            removed: 1,
            candidate: false,
            multiple: true,
            unchanged_total: 17,
        };
    }
    let (label, candidate) = if let Some(label) = rest.strip_suffix("-mask-out") {
        (label, false)
    } else {
        let label = rest
            .strip_suffix("-remove-candidate")
            .unwrap_or_else(|| panic!("no case {id}"));
        (label, true)
    };
    let (total, survivors, removed) = match label {
        "reported" => (1, [0xffff_ffff, 1], 1),
        "underflow" => (0, [2, 3], 1),
        "independent-total" => (17, [2, 3], 4),
        "remove-zero" => (17, [2, 3], 0),
        "zero-survivors" => (1, [0, 0], 1),
        "wrap-after" => (1, [2, 3], 0xffff_ffff),
        "ordinary" => (9, [2, 3], 4),
        other => panic!("no label {other}"),
    };
    Removal {
        total,
        survivors,
        removed,
        candidate,
        multiple: false,
        unchanged_total: if label == "ordinary" { 2 } else { 17 },
    }
}

/// Runs the case `id`.
pub fn case(k: &mut Kit, id: &str) -> J {
    probe(k, &removal(id))
}
