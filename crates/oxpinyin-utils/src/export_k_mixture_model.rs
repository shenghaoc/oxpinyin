//! `export_k_mixture_model` (libpinyin 074a2219
//! `utils/training/export_k_mixture_model.cpp`).
//!
//! `--k-mixture-model-file` only; the text model goes to stdout. The
//! cwd-relative `table.conf` and its `SYSTEM_FILE` chunks are loaded first,
//! then the container is attached read-only; a refused attach is
//! `open %s failed.` and `ENOENT` (`:126-146`).

use std::ffi::{CString, OsString};

use oxpinyin_input::tool::{self, OptionArg, OptionEntry, OptionValue, OutputStream};

/// `main` (`:112-155`).
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
    let parsed = match tool::parse_options(c"- export k mixture model", &entries, args) {
        Ok(parsed) => parsed,
        Err(tool::OptionsFailed) => tool::exit(tool::EINVAL),
    };
    let filename = match &parsed.values[0] {
        OptionValue::Filename(value) => value
            .clone()
            .and_then(|bytes| CString::new(bytes).ok()),
        _ => None,
    };

    let phrases = crate::phrase_index::load_system_phrases();
    let Some(mut output) = OutputStream::stdout() else {
        tool::abort();
    };
    oxpinyin_kmm::export::run(filename.as_deref(), &phrases, &mut output);
}
