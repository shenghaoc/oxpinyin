//! `prune_k_mixture_model` (libpinyin 074a2219
//! `utils/training/prune_k_mixture_model.cpp`).
//!
//! `-k/--pruneK` (default 3) and `--CDF` (default 0.99), then exactly one
//! positional file; `argc != 2` is `wrong arguments.` and `EINVAL`
//! (`:120-123`).

use std::ffi::OsString;

use oxpinyin_tool_support::input::{self as tool, OptionArg, OptionEntry};

/// `main` (`:107-191`).
pub fn main(args: impl IntoIterator<Item = OsString>) {
    let _ = tool::set_locale_from_environment();
    let entries = [
        OptionEntry {
            long_name: c"pruneK",
            short_name: b'k',
            flags: 0,
            arg: OptionArg::Int(crate::k_mixture_model::prune::DEFAULT_PRUNE_K),
            description: Some(c"k parameter"),
            arg_description: None,
        },
        OptionEntry {
            long_name: c"CDF",
            short_name: 0,
            flags: 0,
            arg: OptionArg::Double(crate::k_mixture_model::prune::DEFAULT_CDF),
            description: Some(c"CDF parameter"),
            arg_description: None,
        },
    ];
    let parsed = tool::parse_options_or_exit(c"- prune k mixture model", &entries, args);
    if parsed.args.len() != 2 {
        tool::k_mixture_model::report_wrong_arguments();
        tool::exit(tool::EINVAL);
    }
    crate::k_mixture_model::prune::run(Some(&parsed.args[1]), parsed.int(0), parsed.double(1));
}
