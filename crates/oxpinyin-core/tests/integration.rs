//! oxpinyin-core's integration suites, built as one test binary.
//!
//! Each suite file under `tests/` is a module here, so its tests are named
//! `<file>::<test>` in the `integration` binary. One binary links the
//! dependency closure once instead of once per suite (#1005). `Cargo.toml`
//! sets `autotests = false`, so a new file under `tests/` builds only once
//! it is declared below, and a `tests/<dir>/main.rs` only once a `[[test]]`
//! in `Cargo.toml` names it; `every_suite_file_is_declared` fails until then.

mod fixture_provenance;
mod parser_acceptance;
mod scheme_parsers;
mod zhuyin_parse_parity;

/// With `autotests = false`, cargo builds only declared targets, so an
/// undeclared suite would silently never run. Every `*.rs` file directly
/// under `tests/` needs a `mod <stem>;` line above. Every directory there
/// that holds a `main.rs` (cargo's multi-file test layout) needs a
/// `[[test]]` in `Cargo.toml` with `path = "tests/<dir>/main.rs"`. Both are
/// read with `include_str!`, so they are the sources this binary was built
/// from. Directories without a `main.rs` (fixtures, data) are not suites,
/// and dot entries (editor and AppleDouble files) are skipped, as cargo's
/// own target discovery skips them.
#[test]
fn every_suite_file_is_declared() {
    use std::path::Path;

    let source = include_str!("integration.rs");
    let manifest = include_str!("../Cargo.toml");
    let declares = |text: &str, line: &str| text.lines().any(|l| l.trim() == line);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut checked = 0_usize;
    let mut undeclared = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("tests/ is readable") {
        let path = entry.expect("readable dir entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            if !path.join("main.rs").is_file() {
                continue;
            }
            let target = format!("tests/{name}/main.rs");
            if !declares(manifest, &format!("path = \"{target}\"")) {
                undeclared.push(target);
            }
        } else {
            let Some(stem) = name.strip_suffix(".rs") else {
                continue;
            };
            if !path.is_file() || name == "integration.rs" {
                continue;
            }
            if !declares(source, &format!("mod {stem};")) {
                undeclared.push(format!("tests/{name}"));
            }
        }
        checked += 1;
    }
    undeclared.sort();
    assert!(
        undeclared.is_empty(),
        "{undeclared:?} never build or run under autotests = false: declare a \
         tests/<stem>.rs file as `mod <stem>;` in tests/integration.rs, and a \
         tests/<dir>/main.rs as a [[test]] with that path in Cargo.toml"
    );
    assert!(
        checked > 0,
        "found no suite files under {} — the guard would be vacuous",
        dir.display()
    );
}
