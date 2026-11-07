//! #853 lifecycle: the rules are read from phrase_index.cpp:150-178,
//! :237-269, :392-436, :648-675; pinyin.cpp:956, :1132-1145,
//! :1253-1285, and phonetic_lookup.h:925-929 at 074a2219.

use super::{
    facade_total, guess_j, import_nihao, import_rows, log_walk, remove_record, replace_library,
    sha256_hex, system_items, unigram_j, user_file,
};
use crate::facade;
use crate::fixtures::{chunk, modify_header, private_system, write_log, write_user_conf};
use crate::readers::{GuessRows, c, guess_rows, reopen, zhuyin_guess_rows};
use crate::support::{J, Kit, P, UNTOUCHED};

/// The loaded libraries' totals in the pinned data, summed.
const FACADE_TOTAL: u32 = 51_051_831;

/// 癫痫's token.
const TRAINED_TOKEN: u32 = 0x0100_3e57;

/// Observes a system unigram through the selected mutation, dirty save and
/// reopen sequence, on private system links and a single-character item (你
/// for `import-system`). Variants prepare removed, wrapping, overflowing or
/// missing items, repeat adds, reload/mask a library, replace its disk bytes,
/// import a system reading or train. `timing` additionally performs 1000
/// increments and records diagnostic nanoseconds; `repeat` performs a second
/// dirty save and reopen. The final context stays live.
pub(super) fn unigram_lifecycle(k: &mut Kit, variant: &str) -> J {
    let system = private_system(k);
    k.data = system.clone();
    let reload = variant == "reload";
    let name = if reload {
        "gbk_char.bin"
    } else {
        "gb_char.bin"
    };
    let library: u32 = if reload { 2 } else { 1 };
    let lib = system_items(&system, name);
    let mut slot = lib.first(|item| item[0] == 1 && item[1] == 1);
    if variant == "import-system" {
        slot = 0x1225; // 你
    }
    let (token, item) = ((library << 24) | slot, lib.items[&slot].clone());
    write_user_conf(k);
    match variant {
        "removed" => {
            let log = [
                modify_header(0, lib.total, lib.total),
                remove_record(token, &item),
            ]
            .concat();
            write_log(k, &log, "gb_char.dbin");
        }
        "wrap" => {
            // A valid layout with an intentionally inconsistent stored total:
            // item arithmetic wraps independently of the guarded library total.
            let mut payload = std::fs::read(system.join(name)).expect("library")[8..].to_vec();
            let start = unigram_at(&payload, lib.offsets[&slot]);
            payload[start..start + 4].copy_from_slice(&0xffff_fffc_u32.to_le_bytes());
            // A private symlink: never write the source data.
            replace_library(&system, name, &payload);
        }
        "overflow" => write_log(k, &modify_header(0, lib.total, 0xffff_fffc), "gb_char.dbin"),
        _ => {}
    }
    assert!(!k.ctx().is_null());
    let reading = if facade::ZHUYIN { "ㄋㄧˇ" } else { "ni3" };
    if variant == "import-system" {
        assert_eq!(
            import_rows(k, 1, &[("你", reading, 9)]),
            J::from(vec![true])
        );
    }
    let add = |k: &mut Kit, token: u32, delta: u32| {
        facade::token_add_unigram_frequency(k.inst(), token, delta)
    };
    let watched = if variant == "missing" {
        (library << 24) | (lib.max_slot() + 20)
    } else {
        token
    };
    let mut out = obj! { "before" => unigram_j(k, watched) };
    out.set("add", add(k, watched, 7));
    out.set("after", unigram_j(k, watched));
    if variant == "timing" {
        let started = std::time::Instant::now();
        for _ in 0..1000 {
            assert!(add(k, token, 1));
        }
        out.set("~increment-ns", nanos(started));
    }
    if variant == "repeat" {
        out.set("again", add(k, watched, 11));
        out.set("after-again", unigram_j(k, watched));
        let other = k.alloc();
        let mut frequency = 0_u32;
        let ret = facade::token_get_unigram_frequency(other, watched, &raw mut frequency);
        out.set("other-instance", (ret, frequency));
        facade::free_instance(other);
    }
    if matches!(variant, "overflow" | "missing" | "removed") && !facade::ZHUYIN {
        out.set("facade", facade_total(k));
    }
    out.set("clean-save", facade::save(k.ctx()));
    match variant {
        "reload" => {
            out.set("unload", facade::unload_phrase_library(k.ctx(), 2));
            out.set("unloaded-add", add(k, token, 13));
            out.set("load", facade::load_phrase_library(k.ctx(), 2));
            out.set("after-load", unigram_j(k, token));
        }
        "mask" => {
            out.set("mask", facade::mask_out(k.ctx(), 0xffff_ffff, 0));
            out.set("after-mask", unigram_j(k, token));
        }
        "replacement" => {
            let path = system.join(name);
            let mut payload = std::fs::read(&path).expect("library")[8..].to_vec();
            let start = unigram_at(&payload, lib.offsets[&slot]);
            payload[start..start + 4].copy_from_slice(&37_u32.to_le_bytes());
            let old = u32::from_le_bytes(item[2..6].try_into().expect("a unigram"));
            let total = lib.total.wrapping_sub(old).wrapping_add(37);
            payload[..4].copy_from_slice(&total.to_le_bytes());
            let temporary = system.join(format!("{name}.new"));
            std::fs::write(&temporary, chunk(&payload)).expect("the replacement");
            std::fs::rename(&temporary, &path).expect("replace the library");
        }
        "import-system" => {
            out.set("system-import", import_rows(k, 1, &[("你", reading, 17)]));
        }
        "training" => {
            out.set("training-add", add(k, TRAINED_TOKEN, 7));
            out.set("training", token_unigram_trained(k));
        }
        _ => {}
    }
    // An import makes save dirty without replacing the watched system item.
    out.set("import", import_nihao(k));
    let started = std::time::Instant::now();
    out.set("save", facade::save(k.ctx()));
    if variant == "timing" {
        out.set("~save-ns", nanos(started));
    }
    let log = user_file(k, &name.replace(".bin", ".dbin"));
    out.set(
        "journal",
        (log.len(), sha256_hex(&log), J::from(log_walk(&log))),
    );
    out.set("after-save", unigram_j(k, watched));
    reopen(k);
    out.set("reopen", unigram_j(k, watched));
    out.set("add-reopened", add(k, watched, 5));
    out.set("after-reopened", unigram_j(k, watched));
    if variant == "repeat" {
        out.set("second-import", import_nihao(k));
        out.set("second-save", facade::save(k.ctx()));
        reopen(k);
        out.set("second-reopen", unigram_j(k, watched));
    }
    out
}

/// The payload offset of the unigram of the item at item-area `offset`.
fn unigram_at(payload: &[u8], offset: u32) -> usize {
    let content = u32::from_le_bytes(payload[8..12].try_into().expect("a word")) as usize;
    content + offset as usize + 2
}

/// `token_unigram_trained` as its result document.
fn token_unigram_trained(k: &mut Kit) -> J {
    let trained = super::super::case_helpers::token_unigram_trained(k);
    obj! {
        "token" => trained.token,
        "parsed" => trained.parsed,
        "before" => trained.before,
        "training" => super::training_j(trained.training),
        "after" => trained.after,
    }
}

/// A refused add stays in the facade total across a reload of the library. The facade
/// total takes the delta before the sub-index answers (`phrase_index.h:628-635`), a
/// sub-index whose `guint32` total cannot take it refuses (`phrase_index.cpp:170-175`),
/// and `load` swaps the old sub-index's total out of the facade and the new one's in
/// (`phrase_index.cpp:237-248`): the refused delta is in neither. The log header makes
/// library 2's total 0xfffffffc, so the delta (the one that wraps the facade total to
/// zero) is refused, and the library is unloaded and loaded again; the pin's zero total
/// then asserts at the first ranked row (`pinyin.cpp:1859`; `zhuyin.cpp:1261`): row 58's
/// registered zero-total path, with its policy.
pub(super) fn refused_add_reload(k: &mut Kit) -> J {
    let system = private_system(k);
    k.data = system.clone();
    let lib = system_items(&system, "gbk_char.bin");
    let slot = lib.first(|item| item[0] == 1 && item[1] == 1);
    write_user_conf(k);
    write_log(
        k,
        &modify_header(0, lib.total, 0xffff_fffc),
        "gbk_char.dbin",
    );
    assert!(!k.ctx().is_null());
    let facade_total = FACADE_TOTAL
        .wrapping_add(0xffff_fffc)
        .wrapping_sub(lib.total);
    let mut out = obj! {
        "add" => facade::token_add_unigram_frequency(
            k.inst(),
            (2 << 24) | slot,
            facade_total.wrapping_neg(),
        ),
    };
    out.set("unload", facade::unload_phrase_library(k.ctx(), 2));
    out.set("load", facade::load_phrase_library(k.ctx(), 2));
    let inst = k.alloc();
    if facade::ZHUYIN {
        facade::parse_more_chewings(inst, c("su3cl3").as_ptr());
        out.set("ret", facade::guess_candidates_after_cursor(inst, 0));
    } else {
        facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
        out.set("ret", facade::guess_candidates(inst, 0, 0x1e));
    }
    out
}

fn unigram_at_inst(inst: P, token: u32) -> J {
    let mut freq = UNTOUCHED;
    let ret = facade::token_get_unigram_frequency(inst, token, &raw mut freq);
    J::from((ret, freq))
}

/// Unigram updates around a save with a literal NULL user directory: a
/// private library's stored total replaced with 0xfffffffb, increments of two
/// and four around the save, and the dynamic pricing after them.
///
/// The pin's save guards precede compact (pinyin.cpp:1132-1145,
/// zhuyin.cpp:547-554). Both C facades already enforce those guards;
/// direct reporting-helper coverage is a user-store unit test.
pub(super) fn unigram_null_user_save(k: &mut Kit) -> J {
    let system = private_system(k);
    k.data = system.clone();
    let mut payload = std::fs::read(system.join("gb_char.bin")).expect("library")[8..].to_vec();
    payload[..4].copy_from_slice(&0xffff_fffb_u32.to_le_bytes());
    // Replace our private symlink, never the oracle data.
    replace_library(&system, "gb_char.bin", &payload);
    // The old kit's `k._ctx = k.init(system, None, literal=True)`: the cached
    // context and instance are this NULL-user context's, held here.
    let ctx = k.init(Some(&system), None);
    assert!(!ctx.is_null());
    let inst = facade::alloc_instance(ctx);
    assert!(!inst.is_null());
    let token = 0x0100_0001;
    let mut out = obj! { "before" => unigram_at_inst(inst, token) };
    out.set(
        "near-bound",
        facade::token_add_unigram_frequency(inst, token, 2),
    );
    out.set("before-save", unigram_at_inst(inst, token));
    out.set("save", facade::save(ctx));
    out.set(
        "across-bound",
        facade::token_add_unigram_frequency(inst, token, 4),
    );
    out.set("after-refusal", unigram_at_inst(inst, token));
    // `_dynamic_adjust_out(k, b'ni3')`.
    assert!(facade::set_options(ctx, 0x18a | (1 << 9)));
    let parsed = facade::parse_more_full_pinyins(inst, c("ni3").as_ptr());
    let rows: GuessRows = if facade::ZHUYIN {
        zhuyin_guess_rows(inst, 0)
    } else {
        guess_rows(inst, 0, 0x1e)
    };
    let mut pricing = obj! { "parsed" => parsed };
    super::merge(&mut pricing, guess_j(rows));
    out.set("pricing", pricing);
    out
}

fn nanos(started: std::time::Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}
