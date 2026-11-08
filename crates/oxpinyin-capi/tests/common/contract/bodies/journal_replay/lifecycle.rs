//! #853 lifecycle: the rules are read from phrase_index.cpp:150-178,
//! :237-269, :392-436, :648-675; pinyin.cpp:956, :1132-1145,
//! :1253-1285, and phonetic_lookup.h:925-929 at 074a2219.

use super::{facade_total, import_nihao, log_walk, sha256_hex, system_items, unigram_j, user_file};
use crate::facade;
use crate::fixtures::{modify_header, private_system, write_log, write_user_conf};
use crate::readers::{c, reopen};
use crate::support::{J, Kit};

/// The loaded libraries' totals in the pinned data, summed.
const FACADE_TOTAL: u32 = 51_051_831;
/// Observes a system unigram through the selected mutation, dirty save and
/// reopen sequence, on private system links and a single-character item (你
/// for `import-system`). Variants prepare removed, wrapping, overflowing or
/// missing items, repeat adds, reload/mask a library, replace its disk bytes,
/// import a system reading or train. `timing` additionally performs 1000
/// increments and records diagnostic nanoseconds; `repeat` performs a second
/// dirty save and reopen. The final context stays live.
pub(super) fn unigram_lifecycle(k: &mut Kit) -> J {
    let system = private_system(k);
    k.data = system.clone();
    let name = "gb_char.bin";
    let library = 1;
    let lib = system_items(&system, name);
    let slot = lib.first(|item| item[0] == 1 && item[1] == 1);
    let token = (library << 24) | slot;
    write_user_conf(k);
    write_log(k, &modify_header(0, lib.total, 0xffff_fffc), "gb_char.dbin");
    assert!(!k.ctx().is_null());
    let add = |k: &mut Kit, token: u32, delta: u32| {
        facade::token_add_unigram_frequency(k.inst(), token, delta)
    };
    let mut out = obj! { "before" => unigram_j(k, token) };
    out.set("add", add(k, token, 7));
    out.set("after", unigram_j(k, token));
    if !facade::ZHUYIN {
        out.set("facade", facade_total(k));
    }
    out.set("clean-save", facade::save(k.ctx()));
    out.set("import", import_nihao(k));
    out.set("save", facade::save(k.ctx()));
    let log = user_file(k, &name.replace(".bin", ".dbin"));
    out.set(
        "journal",
        (log.len(), sha256_hex(&log), J::from(log_walk(&log))),
    );
    out.set("after-save", unigram_j(k, token));
    reopen(k);
    out.set("reopen", unigram_j(k, token));
    out.set("add-reopened", add(k, token, 5));
    out.set("after-reopened", unigram_j(k, token));
    out
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
