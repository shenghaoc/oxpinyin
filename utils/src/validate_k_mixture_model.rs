//! `validate_k_mixture_model` (libpinyin 074a2219
//! `utils/training/validate_k_mixture_model.cpp`).
//!
//! It has no `GOptionEntry`: the only option is GLib's automatic `--help`,
//! and the dead `print_help` text is not part of the parser. `argc` must be
//! exactly two, or `wrong arguments.` and `EINVAL` (`:146-155`).

use crate::k_mixture_model::database::Database;
use oxpinyin_tool_support::input as tool;

/// `main` (`:141-173`).
pub fn main(args: impl IntoIterator<Item = std::ffi::OsString>) {
    let _ = tool::set_locale_from_environment();
    let parsed = tool::parse_options_or_exit(c"- validate k mixture model", &[], args);
    if parsed.args.len() != 2 {
        tool::k_mixture_model::report_wrong_arguments();
        tool::exit(tool::EINVAL);
    }

    let bigram = Database::open_read_only(Some(&parsed.args[1]));
    crate::k_mixture_model::validate::run(&bigram);
}
