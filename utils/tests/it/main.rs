//! The verification tests of oxpinyin-utils: each module runs the built
//! utilities and checks their arguments, standard streams, exit status and
//! files. `support` holds the helpers more than one module uses.

#[path = "support/fixtures.rs"]
mod fixtures;
mod gen_pinyin_table;
mod gen_zhuyin_table;
mod k_mixture_model_to_interpolation;
mod kmm_tools;
mod segmentation;
mod storage_tools;
mod support;
// The REPL's tests include support.rs and support/fixtures.rs each by
// #[path], so the fixture is declared here, not in support.rs, and the
// harness's own checks below likewise.
#[path = "support/tests.rs"]
mod support_tests;
mod training;
