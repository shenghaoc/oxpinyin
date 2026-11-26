//! `estimate_k_mixture_model` (libpinyin 074a2219
//! `utils/training/estimate_k_mixture_model.cpp`).
//!
//! `--bigram-file` (default `k_mixture_model_ngram.db`) and
//! `--deleted-bigram-file` (default `k_mixture_model_deleted_ngram.db`);
//! positional arguments are ignored (`:98-155`).

use std::ffi::OsString;

use oxpinyin_tool_support::input::{self as tool, OptionArg, OptionEntry};

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
    let parsed = tool::parse_options_or_exit(c"- estimate k mixture model", &entries, args);
    crate::k_mixture_model::estimate::run(parsed.filename(0), parsed.filename(1));
}
