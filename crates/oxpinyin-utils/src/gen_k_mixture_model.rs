//! `gen_k_mixture_model` (libpinyin 074a2219
//! `utils/training/gen_k_mixture_model.cpp`).
//!
//! `--skip-pi-gram-training` (`G_OPTION_FLAG_REVERSE` over a true default),
//! `--maximum-occurs-allowed` (20), `--maximum-increase-rates-allowed`
//! (3.0), `--k-mixture-model-file`; every remaining argument is one
//! document, in order (`:313-403`).

use std::ffi::{CString, OsString};

use oxpinyin_input::tool::{self, OptionArg, OptionEntry, OptionValue, OPTION_FLAG_REVERSE};

/// `main` (`:313-403`).
pub fn main(args: impl IntoIterator<Item = OsString>) {
    let _ = tool::set_locale_from_environment();
    let entries = [
        OptionEntry {
            long_name: c"skip-pi-gram-training",
            short_name: 0,
            flags: OPTION_FLAG_REVERSE,
            arg: OptionArg::Flag(true),
            description: Some(c"skip pi-gram training"),
            arg_description: None,
        },
        OptionEntry {
            long_name: c"maximum-occurs-allowed",
            short_name: 0,
            flags: 0,
            arg: OptionArg::Int(oxpinyin_kmm::generate::DEFAULT_MAX_OCCURS),
            description: Some(c"maximum occurs allowed"),
            arg_description: None,
        },
        OptionEntry {
            long_name: c"maximum-increase-rates-allowed",
            short_name: 0,
            flags: 0,
            arg: OptionArg::Double(oxpinyin_kmm::generate::DEFAULT_MAX_INCREASE_RATE),
            description: Some(c"maximum increase rates allowed"),
            arg_description: None,
        },
        OptionEntry {
            long_name: c"k-mixture-model-file",
            short_name: 0,
            flags: 0,
            arg: OptionArg::Filename(None),
            description: Some(c"k mixture model file"),
            arg_description: None,
        },
    ];
    let parsed = match tool::parse_options(c"- generate k mixture model", &entries, args) {
        Ok(parsed) => parsed,
        Err(tool::OptionsFailed) => tool::exit(tool::EINVAL),
    };
    let params = oxpinyin_kmm::generate::GenerateParams {
        max_occurs: match parsed.values[1] {
            OptionValue::Int(value) => value,
            _ => oxpinyin_kmm::generate::DEFAULT_MAX_OCCURS,
        },
        max_increase_rate: match parsed.values[2] {
            OptionValue::Double(value) => value,
            _ => oxpinyin_kmm::generate::DEFAULT_MAX_INCREASE_RATE,
        },
        train_pi_gram: match parsed.values[0] {
            OptionValue::Flag(value) => value,
            _ => true,
        },
    };
    let filename = match &parsed.values[3] {
        OptionValue::Filename(value) => value
            .clone()
            .and_then(|bytes| CString::new(bytes).ok()),
        _ => None,
    };
    let documents: Vec<CString> = parsed.args[1..]
        .iter()
        .filter_map(|arg| CString::new(arg.clone()).ok())
        .collect();
    let documents: Vec<&std::ffi::CStr> = documents.iter().map(CString::as_c_str).collect();

    let phrases = crate::phrase_index::load_system_phrases();
    oxpinyin_kmm::generate::run(filename.as_deref(), params, &documents, &phrases);
}
