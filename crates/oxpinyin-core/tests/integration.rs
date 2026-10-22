//! oxpinyin-core's integration suites, built as one test binary.
//!
//! Each file under `tests/` is a module here, so its tests are named
//! `<file>::<test>` in the `integration` binary. One binary links the
//! dependency closure once instead of once per suite (#1005). `Cargo.toml`
//! sets `autotests = false`: a new file under `tests/` runs only once it is
//! declared below.

mod fixture_provenance;
mod parser_acceptance;
mod scheme_parsers;
mod scoring;
mod scoring_fixture;
mod zhuyin_parse_parity;
