//! The verification tests of oxpinyin-capi: each module calls the exported C
//! functions on the committed fixtures and checks their answers,
//! out-parameters, aborts, standard error and the files they write.
//! `support` holds the helpers more than one module uses.

#![expect(unsafe_code, reason = "calls the C ABI and reclaims what it returns")]

mod choose;
mod contract;
mod foreign_candidates;
mod guess_offset;
mod guess_predict;
mod import_export;
mod init_and_save;
mod keys;
mod library_loading;
mod library_tokens;
mod locale;
mod matrix_bounds;
mod null_arguments;
mod parse_keys;
mod phrase;
#[cfg(feature = "tkrzw")]
mod pinned_data;
mod pipeline;
mod sentence;
mod support;
// The harness's own checks sit beside it.
#[path = "support/tests.rs"]
mod support_tests;
#[path = "../../../oxpinyin/src/user/test_child.rs"]
mod test_child;
mod token_getters;
mod training;
mod unreadable_candidates;
mod user_store;
mod userdir_arg;
