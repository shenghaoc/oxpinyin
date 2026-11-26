//! `merge_k_mixture_model` (libpinyin 074a2219
//! `utils/training/merge_k_mixture_model.cpp`).
//!
//! `--result-file` is the only entry. The result container is attached
//! before any source is opened (`:225-226`), so an absent result name is the
//! pin's NULL attach and the first source's magic-header `check_result`
//! aborts; with no sources the PIN exits 0 (`:228-235`).

use std::ffi::{CString, OsString};

use oxpinyin_tool_support::input::{self as tool, OptionArg, OptionEntry};

/// `main` (`:210-237`).
pub fn main(args: impl IntoIterator<Item = OsString>) {
    let _ = tool::set_locale_from_environment();
    let entries = [OptionEntry {
        long_name: c"result-file",
        short_name: 0,
        flags: 0,
        arg: OptionArg::Filename(None),
        description: Some(c"merged result file"),
        arg_description: None,
    }];
    let parsed = tool::parse_options_or_exit(c"- merge k mixture model", &entries, args);

    let sources: Vec<&std::ffi::CStr> = parsed.operands().iter().map(CString::as_c_str).collect();
    crate::k_mixture_model::merge::run(parsed.filename(0), &sources);
}
