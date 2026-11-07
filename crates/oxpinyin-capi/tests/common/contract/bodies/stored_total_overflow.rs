//! A stored library total near the `guint32` wrap (#802) and chained MODIFY_HEADER logs.

use crate::facade;
use crate::fixtures::{modify_header, write_log, write_user_conf};
pub use crate::readers::FacadeOut;
use crate::readers::{facade_total_out, import_rows, tokens_of, unigram_of};
use crate::support::Kit;

/// The loaded libraries' totals in the pinned data, summed.
const FACADE_TOTAL: u32 = 51_051_831;
/// How far short of the wrap the planted header sets library 1's total.
const STORED_GAP: u32 = 5_000_000;

/// A system library chunk's stored total, after the `MemoryChunk` header.
fn stored_library_total(k: &Kit, name: &str) -> u32 {
    let bytes = std::fs::read(k.data.join(name)).expect("the system library");
    u32::from_le_bytes(bytes[8..12].try_into().expect("a total"))
}

/// Closes the cached context and plants `log` as gb_char.dbin under a conforming user.conf.
fn plant_log(k: &mut Kit, log: &[u8]) {
    facade::fini(k.ctx());
    k.forget();
    write_user_conf(k);
    write_log(k, log, "gb_char.dbin");
}

#[derive(Debug, PartialEq)]
pub struct Overflow {
    pub before: (bool, u32),
    pub add: bool,
    pub after: (bool, u32),
    pub facade: FacadeOut,
    pub import: Option<Vec<bool>>,
    pub saved: Option<bool>,
    pub add_saved: Option<bool>,
    pub after_saved: Option<(bool, u32)>,
    pub facade_saved: Option<FacadeOut>,
}

/// Plants library 1's total STORED_GAP short of the wrap, adds a delta that
/// fits (GAP - 1) or overshoots, and with `after_save` repeats the add after a
/// save compacts the totals (`phrase_index.cpp:170-174`, `pinyin.cpp:1139`).
pub fn stored_total_overflow(k: &mut Kit, refused: bool, after_save: bool) -> Overflow {
    let total = stored_library_total(k, "gb_char.bin");
    plant_log(k, &modify_header(0, total, STORED_GAP.wrapping_neg()));
    let delta = if refused {
        STORED_GAP
            .checked_add(
                4_000_000_000_u32
                    .wrapping_sub(FACADE_TOTAL)
                    .wrapping_add(total),
            )
            .expect("the overshooting delta fits a guint32")
    } else {
        STORED_GAP - 1
    };
    let token = tokens_of(k, "号")
        .into_iter()
        .find(|token| token >> 24 == 1)
        .expect("a library-1 token");
    let before = unigram_of(k, token);
    let add = facade::token_add_unigram_frequency(k.inst(), token, delta);
    let after = unigram_of(k, token);
    let facade_out = facade_total_out(k);
    let mut out = Overflow {
        before,
        add,
        after,
        facade: facade_out,
        import: None,
        saved: None,
        add_saved: None,
        after_saved: None,
        facade_saved: None,
    };
    if after_save {
        out.import = Some(import_rows(k, 7, &[("好你", "hao3'ni3", 2)]));
        out.saved = Some(facade::save(k.ctx()));
        out.add_saved = Some(facade::token_add_unigram_frequency(k.inst(), token, delta));
        out.after_saved = Some(unigram_of(k, token));
        out.facade_saved = Some(facade_total_out(k));
    }
    out
}

#[derive(Debug, PartialEq)]
pub struct Headers {
    pub total: u32,
    pub facade: FacadeOut,
}

/// Two MODIFY_HEADER records, the second chained to the first's new total or
/// to the original total (`phrase_index.cpp:503-512`).
pub fn log_headers(k: &mut Kit, chained: bool) -> Headers {
    let total = stored_library_total(k, "gb_char.bin");
    let shift = 2_000_000_u32.wrapping_sub(FACADE_TOTAL);
    let log = if chained {
        let first = total.wrapping_add(1_000_000_000);
        [
            modify_header(0, total, first),
            modify_header(0, first, total.wrapping_add(shift)),
        ]
        .concat()
    } else {
        [
            modify_header(0, total, total.wrapping_add(shift)),
            modify_header(0, total, total),
        ]
        .concat()
    };
    plant_log(k, &log);
    Headers {
        total,
        facade: facade_total_out(k),
    }
}
