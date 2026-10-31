//! `merge_k_mixture_model` (libpinyin 074a2219
//! `utils/training/merge_k_mixture_model.cpp`).
//!
//! `--result-file` is the only entry. The result container is attached
//! before any source is opened (`:225-226`), so an absent result name is the
//! pin's NULL attach and the first source's magic-header `check_result`
//! aborts; with no sources the PIN exits 0 (`:228-235`).

use std::ffi::{CString, OsString};

use oxpinyin_input::tool::{self, OptionArg, OptionEntry, OptionValue};

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
    let parsed = match tool::parse_options(c"- merge k mixture model", &entries, args) {
        Ok(parsed) => parsed,
        Err(tool::OptionsFailed) => tool::exit(tool::EINVAL),
    };

    let result_filename = match &parsed.values[0] {
        OptionValue::Filename(value) => value.clone().and_then(|bytes| CString::new(bytes).ok()),
        _ => None,
    };

    let sources: Vec<CString> = parsed.args[1..]
        .iter()
        .filter_map(|arg| CString::new(arg.clone()).ok())
        .collect();
    let sources: Vec<&std::ffi::CStr> = sources.iter().map(CString::as_c_str).collect();

    oxpinyin_kmm::merge::run(result_filename.as_deref(), &sources);
}
