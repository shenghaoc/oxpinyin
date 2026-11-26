//! The verification tests of oxzhuyin-capi: each module calls the exported C
//! functions on the committed fixtures and checks their answers,
//! out-parameters, aborts, standard error and the files they write.
//! `support` holds the helpers more than one module uses.

#![expect(unsafe_code, reason = "calls the C ABI and reclaims what it returns")]

mod companion_headers;
mod foreign_candidates;
mod import_and_user_store;
mod init_and_save;
mod matrix_bounds;
mod null_arguments;
mod parse_keys;
mod parse_matrix;
#[cfg(feature = "tkrzw")]
mod pinned_data;
mod sentence_and_candidates;
mod support;
mod token_getters;
mod unreadable_candidates;
mod userdir_arg;
