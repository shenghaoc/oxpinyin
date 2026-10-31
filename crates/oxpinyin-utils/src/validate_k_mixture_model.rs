//! `validate_k_mixture_model` (libpinyin 074a2219
//! `utils/training/validate_k_mixture_model.cpp`).
//!
//! It has no `GOptionEntry`: the only option is GLib's automatic `--help`,
//! and the dead `print_help` text is not part of the parser. `argc` must be
//! exactly two, or `wrong arguments.` and `EINVAL` (`:146-155`).

use oxpinyin_input::tool;
use oxpinyin_kmm::Database;

/// `main` (`:141-173`).
pub fn main(args: impl IntoIterator<Item = std::ffi::OsString>) {
    let _ = tool::set_locale_from_environment();
    let parsed = match tool::parse_options(c"- validate k mixture model", &[], args) {
        Ok(parsed) => parsed,
        Err(tool::OptionsFailed) => tool::exit(tool::EINVAL),
    };
    if parsed.args.len() != 2 {
        tool::k_mixture_model::report_wrong_arguments();
        tool::exit(tool::EINVAL);
    }

    let filename = std::ffi::CString::new(parsed.args[1].clone()).unwrap_or_default();
    let bigram = Database::open_read_only(Some(&filename));
    oxpinyin_kmm::validate::run(&bigram);
}
