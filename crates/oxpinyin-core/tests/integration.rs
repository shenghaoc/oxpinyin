//! oxpinyin-core's integration suites, built as one test binary.
//!
//! Each suite file under `tests/` is a module here, so its tests are named
//! `<file>::<test>` in the `integration` binary. One binary links the
//! dependency closure once instead of once per suite (#1005). `Cargo.toml`
//! sets `autotests = false`, so a new file under `tests/` builds only once
//! it is declared below; `every_suite_file_is_declared` fails until it is.

mod fixture_provenance;
mod parser_acceptance;
mod scheme_parsers;
mod scoring;
mod scoring_fixture;
mod zhuyin_parse_parity;

/// Every `*.rs` file directly under `tests/` needs a `mod <stem>;` line
/// above. Without one it is not part of any target, so its tests would
/// silently never run. The lines are read from the source this binary was
/// built from. Directories (fixtures, data) are not suites, and dot files
/// (editor and AppleDouble files) are skipped, as cargo's own target
/// discovery skips them.
#[test]
fn every_suite_file_is_declared() {
    use std::path::Path;

    let source = include_str!("integration.rs");
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut checked = 0_usize;
    let mut undeclared = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("tests/ is readable") {
        let path = entry.expect("readable dir entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let Some(stem) = name.strip_suffix(".rs") else {
            continue;
        };
        if !path.is_file() || name.starts_with('.') || name == "integration.rs" {
            continue;
        }
        let declaration = format!("mod {stem};");
        if !source.lines().any(|line| line.trim() == declaration) {
            undeclared.push(format!("tests/{name}"));
        }
        checked += 1;
    }
    undeclared.sort();
    assert!(
        undeclared.is_empty(),
        "{undeclared:?} have no `mod <stem>;` line in tests/integration.rs, \
         so with autotests = false they never build or run"
    );
    assert!(
        checked > 0,
        "found no suite files under {} — the guard would be vacuous",
        dir.display()
    );
}
