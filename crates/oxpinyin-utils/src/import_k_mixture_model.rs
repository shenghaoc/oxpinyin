//! `import_k_mixture_model` (libpinyin 074a2219
//! `utils/training/import_k_mixture_model.cpp`).
//!
//! `--k-mixture-model-file` only; the text model is stdin (`:258-317`). The
//! cwd-relative `table.conf` and its `SYSTEM_FILE` chunks are loaded before
//! the container is attached.

use std::ffi::{CString, OsString};

use oxpinyin_input::tool::{self, LineReader, OptionArg, OptionEntry, OptionValue};

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
    let parsed = match tool::parse_options(c"- import k mixture model", &entries, args) {
        Ok(parsed) => parsed,
        Err(tool::OptionsFailed) => tool::exit(tool::EINVAL),
    };
    let filename = match &parsed.values[0] {
        OptionValue::Filename(value) => value.clone().and_then(|bytes| CString::new(bytes).ok()),
        _ => None,
    };

    let phrases = crate::phrase_index::load_system_phrases();
    let mut input = LineReader::stdin();
    oxpinyin_kmm::import::run(filename.as_deref(), &phrases, &mut input);
}
