//! The token_reuse_addon_import family's case bodies: each answers the result document the old case returned.
//!
//! Token reuse and addon import (inventory row 45, #762, #913): the next token
//! an import takes after `mask_out` frees the top of a library's range, and the
//! rows a promoted addon phrase leaves in the next candidate list.

use crate::facade;
use crate::fixtures::{ReuseStep, token_reuse_sequence};
use crate::readers::{
    c, candidate_at, candidate_rows, candidate_text, candidate_type, n_candidate,
};
use crate::support::{J, Kit, P};

/// `USER_ALL`: every token of the user library.
const USER_ALL: (u32, u32) = (0x0f00_0000, 0x0700_0000);

/// The case body for `id`.
pub fn case(k: &mut Kit, id: &str) -> J {
    match id {
        "addon-promoted-listing-unsorted" => return addon_promoted_listing(k, 0),
        "addon-promoted-listing-by-frequency" => return addon_promoted_listing(k, 0x1c),
        _ => {}
    }
    let suffix = if facade::ZHUYIN { "-zhuyin" } else { "-pinyin" };
    let stem = id
        .strip_suffix(suffix)
        .unwrap_or_else(|| panic!("{id} is not a case of this facade"));
    if let Some(name) = stem.strip_prefix("issue762-token-reuse-") {
        return token_reuse_sequence(k, &token_reuse_steps(name), None, 7);
    }
    let name = stem
        .strip_prefix("issue762-addon-import-")
        .unwrap_or_else(|| panic!("no token_reuse_addon_import case {id}"));
    token_reuse_sequence(k, &addon_import_steps(name), None, 5)
}

/// `TOKEN_REUSE_SEQUENCES`' steps for `name`.
fn token_reuse_steps(name: &str) -> Vec<ReuseStep> {
    use ReuseStep::{Add, Mask, Reopen, Save};
    let all = Mask(USER_ALL.0, USER_ALL.1);
    let top = Mask(0xffff_ffff, 0x0700_0003);
    let middle = Mask(0xffff_ffff, 0x0700_0002);
    match name {
        "no-mask" => vec![Add(0), Add(1), Save, Add(2), Save],
        "mask-all" => vec![Add(0), Save, all, Save, Add(1), Save],
        "mask-highest" => vec![Add(0), Add(1), Add(2), Save, top, Save, Add(3), Save],
        "mask-middle" => vec![Add(0), Add(1), Add(2), Save, middle, Save, Add(3), Save],
        "mask-twice" => vec![
            Add(0),
            Add(1),
            Add(2),
            Save,
            top,
            middle,
            Add(3),
            Add(4),
            Save,
        ],
        "mask-import-mask" => vec![Add(0), Add(1), Save, middle, Add(2), middle, Add(3), Save],
        "mask-reopen" => vec![Add(0), Add(1), Save, middle, Save, Reopen, Add(2), Save],
        "mask-unsaved-reopen" => vec![Add(0), Add(1), Save, middle, Reopen, Add(2), Save],
        "mask-reopen-middle" => {
            vec![
                Add(0),
                Add(1),
                Add(2),
                Save,
                middle,
                Save,
                Reopen,
                Add(3),
                Save,
            ]
        }
        other => panic!("no token reuse sequence {other}"),
    }
}

/// `ADDON_IMPORT_SEQUENCES`' steps for `name`.
fn addon_import_steps(name: &str) -> Vec<ReuseStep> {
    use ReuseStep::{Add, Mask, Save};
    match name {
        "no-mask" => vec![Add(0), Add(1), Add(2), Save, Add(3), Save],
        "mask-highest" => vec![
            Add(0),
            Add(1),
            Add(2),
            Save,
            Mask(0xffff_ffff, 0x0500_0003),
            Add(3),
            Save,
        ],
        "mask-middle" => vec![
            Add(0),
            Add(1),
            Add(2),
            Save,
            Mask(0xffff_ffff, 0x0500_0002),
            Add(3),
            Save,
        ],
        other => panic!("no addon import sequence {other}"),
    }
}

/// `addon_promoted_listing`'s `listing`: a fresh instance's `shuxue` candidate
/// list under `sort_option`, its size and 书学 rows stored under `label`.
fn listing(ctx: P, sort_option: u32, out: &mut J, label: &str) -> (P, J) {
    let inst = facade::alloc_instance(ctx);
    facade::parse_more_full_pinyins(inst, c("shuxue").as_ptr());
    facade::guess_candidates(inst, 0, sort_option);
    let rows = candidate_rows(inst);
    let matching: Vec<(i32, String)> = rows
        .iter()
        .flatten()
        .filter(|(_, text)| text == "书学")
        .cloned()
        .collect();
    out.set(label, (rows.len(), matching));
    (inst, J::from(rows))
}

/// `addon_promoted_listing`: 书学's rows before and after promoting its addon entry.
fn addon_promoted_listing(k: &mut Kit, sort_option: u32) -> J {
    let ctx = k.ctx();
    assert!(facade::load_addon_phrase_library(ctx, 4));
    let mut out = J::map();
    let (inst, _) = listing(ctx, sort_option, &mut out, "before");
    let chosen = (0..n_candidate(inst))
        .filter_map(|index| candidate_at(inst, index))
        .find(|&cand| {
            candidate_type(inst, cand) == 6 && candidate_text(inst, cand).as_deref() == Some("书学")
        })
        .expect("no addon candidate for shuxue");
    out.set("choose", facade::choose_candidate(inst, 0, chosen));
    facade::free_instance(inst);
    let (inst, rows) = listing(ctx, sort_option, &mut out, "after");
    facade::free_instance(inst);
    out.set("rows after", rows);
    out
}
