//! `prune_k_mixture_model` (libpinyin 074a2219
//! `utils/training/prune_k_mixture_model.cpp`).
//!
//! `-k/--pruneK` (default 3) and `--CDF` (default 0.99), then exactly one
//! positional file; `argc != 2` is `wrong arguments.` and `EINVAL`
//! (`:120-123`).

use std::ffi::{CString, OsString};

use oxpinyin_input::tool::{self, OptionArg, OptionEntry, OptionValue};

/// `main` (`:107-191`).
pub fn main(args: impl IntoIterator<Item = OsString>) {
    let _ = tool::set_locale_from_environment();
    let entries = [
        OptionEntry {
            long_name: c"pruneK",
            short_name: b'k',
            flags: 0,
            arg: OptionArg::Int(oxpinyin_kmm::prune::DEFAULT_PRUNE_K),
            description: Some(c"k parameter"),
            arg_description: None,
        },
        OptionEntry {
            long_name: c"CDF",
            short_name: 0,
            flags: 0,
            arg: OptionArg::Double(oxpinyin_kmm::prune::DEFAULT_CDF),
            description: Some(c"CDF parameter"),
            arg_description: None,
        },
    ];
    let parsed = match tool::parse_options(c"- prune k mixture model", &entries, args) {
        Ok(parsed) => parsed,
        Err(tool::OptionsFailed) => tool::exit(tool::EINVAL),
    };
    if parsed.args.len() != 2 {
        tool::k_mixture_model::report_wrong_arguments();
        tool::exit(tool::EINVAL);
    }
    let prune_k = match parsed.values[0] {
        OptionValue::Int(value) => value,
        _ => 0,
    };
    let cdf = match parsed.values[1] {
        OptionValue::Double(value) => value,
        _ => 0.0,
    };
    let filename = CString::new(parsed.args[1].clone()).unwrap_or_default();
    oxpinyin_kmm::prune::run(Some(&filename), prune_k, cdf);
}
