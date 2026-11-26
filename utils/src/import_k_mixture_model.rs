//! `import_k_mixture_model` (libpinyin 074a2219
//! `utils/training/import_k_mixture_model.cpp`).
//!
//! `--k-mixture-model-file` only; the text model is stdin (`:258-317`). The
//! cwd-relative `table.conf` and its `SYSTEM_FILE` chunks are loaded before
//! the container is attached.

use std::ffi::OsString;

use oxpinyin_tool_support::input::{self as tool, LineReader, OptionArg, OptionEntry};

/// `main` (`:258-317`).
pub fn main(args: impl IntoIterator<Item = OsString>) {
    let _ = tool::set_locale_from_environment();
    let entries = [OptionEntry {
        long_name: c"k-mixture-model-file",
        short_name: 0,
        flags: 0,
        arg: OptionArg::Filename(None),
        description: Some(c"k mixture model file"),
        arg_description: None,
    }];
    let parsed = tool::parse_options_or_exit(c"- import k mixture model", &entries, args);

    let phrases = crate::phrase_index::load_system_phrases();
    let mut input = LineReader::stdin();
    crate::k_mixture_model::import::run(parsed.filename(0), &phrases, &mut input);
}
