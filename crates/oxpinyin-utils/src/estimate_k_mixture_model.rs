//! `estimate_k_mixture_model` (libpinyin 074a2219
//! `utils/training/estimate_k_mixture_model.cpp`).
//!
//! `--bigram-file` (default `k_mixture_model_ngram.db`) and
//! `--deleted-bigram-file` (default `k_mixture_model_deleted_ngram.db`);
//! positional arguments are ignored (`:98-155`).

use std::ffi::{CString, OsString};

use oxpinyin_input::tool::{self, OptionArg, OptionEntry, OptionValue};

/// `main` (`:98-155`).
pub fn main(args: impl IntoIterator<Item = OsString>) {
    let _ = tool::set_locale_from_environment();
    let entries = [
        OptionEntry {
            long_name: c"bigram-file",
            short_name: 0,
            flags: 0,
            arg: OptionArg::Filename(Some(c"k_mixture_model_ngram.db")),
            description: Some(c"the bigram file"),
            arg_description: None,
        },
        OptionEntry {
            long_name: c"deleted-bigram-file",
            short_name: 0,
            flags: 0,
            arg: OptionArg::Filename(Some(c"k_mixture_model_deleted_ngram.db")),
            description: Some(c"the deleted bigram file"),
            arg_description: None,
        },
    ];
    let parsed = match tool::parse_options(c"- estimate k mixture model", &entries, args) {
        Ok(parsed) => parsed,
        Err(tool::OptionsFailed) => tool::exit(tool::EINVAL),
    };
    let filename = |value: &OptionValue| match value {
        OptionValue::Filename(value) => value.clone().and_then(|bytes| CString::new(bytes).ok()),
        _ => None,
    };
    let bigram_filename = filename(&parsed.values[0]);
    let deleted_filename = filename(&parsed.values[1]);
    oxpinyin_kmm::estimate::run(bigram_filename.as_deref(), deleted_filename.as_deref());
}
