//! `ngseg`, `spseg` and `mergeseq` (`utils/segment/*.cpp` at libpinyin
//! 074a2219): the pin's names, options, messages, inputs and outputs.
//!
//! The three utilities read `table.conf`, `phrase_index.bin` and the
//! `SYSTEM_FILE` chunk files from the current working directory, relative
//! names included (`src/include/pinyin_internal.h:55-66`); `ngseg` also
//! attaches `bigram.db`. Their edge calls — the GLib option parser,
//! `std::getline`'s contract, `fprintf`, `perror`, `exit` and `abort` —
//! live in `oxpinyin_tool_support::input`; this module owns what each line means.
//!
//! The phrase table is the pin's DBM (`FacadePhraseTable3`), not an
//! inverted index over the chunk files: `phrase::PhraseSearch` filters
//! its stored tokens by the loaded-library set exactly as
//! `PhraseTableEntry::search` does, and answers `SEARCH_CONTINUED` for
//! every present key.

#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]

#[forbid(unsafe_code)]
mod data;
#[forbid(unsafe_code)]
mod input;
pub mod mergeseq;
pub mod ngseg;
#[forbid(unsafe_code)]
mod phrase;
pub mod spseg;
#[forbid(unsafe_code)]
mod trellis;

mod helpers;
pub(crate) use helpers::{
    abort_on_pin_assert, c_string, extra_enter_entry, open_output, outputfile_entry, parse_options,
    refuse_extra_arguments, segment_lines, text_of,
};

#[cfg(test)]
#[forbid(unsafe_code)]
mod tests;
