//! Frozen product outputs and persistent files for the REPL modes at pin 074a2219.

#[cfg(unix)]
mod chewing;
#[cfg(unix)]
mod chewing_table;
#[path = "../../../oxpinyin-utils/tests/it/support.rs"]
mod common;
#[cfg(unix)]
#[path = "../../../oxpinyin-utils/tests/it/support/fixtures.rs"]
mod fixtures;
#[cfg(unix)]
mod matrix;
#[cfg(unix)]
mod parser2;
#[cfg(unix)]
mod phrase;
#[cfg(unix)]
mod phrase_lookup;
#[cfg(unix)]
mod phrase_table;
#[cfg(unix)]
mod pinyin;
#[cfg(unix)]
mod pinyin_lookup;
#[cfg(unix)]
mod support;
mod table_guards;
#[cfg(unix)]
mod zhuyin;
