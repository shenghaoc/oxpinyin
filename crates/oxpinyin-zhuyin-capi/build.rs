//! Link-time stamping of the zhuyin C ABI's identity, plus the baked
//! `libzhuyin.pc` content.
//!
//! # SONAME
//!
//! Upstream's `libzhuyin.so.15` SONAME comes from libtool `-version-info 15:0`
//! (the same `@LT_VERSION_INFO@` as libpinyin — `src/Makefile.am:118-121` at
//! the pin 0c5e80e1; `configure.ac`'s `libpinyin_abi_current=15`). Emitted as
//! a `cdylib` link arg.
//!
//! # Why there is no version script here
//!
//! A Rust `cdylib` cannot combine a named version script with rustc's own
//! anonymous script (GNU ld rejects the pair, rust-lld refuses to reassign
//! the symbols) — the same constraint recorded in `oxpinyin-capi`'s
//! `build.rs:26-41`. Symbol scope is therefore enforced in the SOURCE (only
//! the 52 `#[unsafe(no_mangle)]` `zhuyin_*` symbols), and the checked-in
//! `libzhuyin.ver` ships verbatim as the record plus for the packaging step.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

// Mirror `[package.metadata.capi.pkg_config].version` and the libtool
// ABI version in Cargo.toml. Keep in sync.
const PC_VERSION: &str = "2.11.91";
const PC_BINARY_VERSION: &str = "15.0";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=libzhuyin.pc.in");
    println!("cargo:rerun-if-env-changed=GLIB_LIBS");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libzhuyin.so.15");
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        emit_glib_link();
    }

    bake_pkg_config_template();
}

/// Bakes the build-time fields of `libzhuyin.pc.in` — `@VERSION@`,
/// `@LIBPINYIN_BINARY_VERSION@`, `@DATABASE_FORMAT@` — leaving the
/// install-time `@prefix@` / `@libdir@` for the wrapper.
fn bake_pkg_config_template() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    let template_path = Path::new(&manifest_dir).join("libzhuyin.pc.in");
    let template = fs::read_to_string(&template_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", template_path.display()));

    let substituted = template
        .replace("@VERSION@", PC_VERSION)
        .replace("@LIBPINYIN_BINARY_VERSION@", PC_BINARY_VERSION)
        .replace("@DATABASE_FORMAT@", &database_format());

    let body: String = substituted
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let baked = format!("{}\n", body.trim_start_matches('\n'));

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    let out_path = Path::new(&out_dir).join("libzhuyin.pc.in.baked");
    fs::write(&out_path, &baked).unwrap_or_else(|e| panic!("write {}: {e}", out_path.display()));

    if let Some(profile_dir) = target_profile_dir(&out_dir) {
        let _ = fs::write(profile_dir.join("libzhuyin.pc.in.baked"), &baked);
    }
}

fn database_format() -> String {
    if let Ok(explicit) = env::var("LIBPINYIN_DATABASE_FORMAT")
        && !explicit.trim().is_empty()
    {
        return explicit;
    }
    if env::var_os("CARGO_FEATURE_KYOTOCABINET").is_some() {
        "KyotoCabinet".to_owned()
    } else if env::var_os("CARGO_FEATURE_TKRZW").is_some() {
        "Tkrzw".to_owned()
    } else if env::var_os("CARGO_FEATURE_LMDB").is_some() {
        "LMDB".to_owned()
    } else {
        "redb".to_owned()
    }
}

fn target_profile_dir(out_dir: &str) -> Option<PathBuf> {
    Path::new(out_dir).ancestors().nth(3).map(Path::to_path_buf)
}

fn emit_glib_link() {
    if let Ok(explicit) = env::var("GLIB_LIBS")
        && !explicit.trim().is_empty()
    {
        for token in explicit.split_whitespace() {
            println!("cargo:rustc-link-arg={token}");
        }
        return;
    }
    let pkg_config = env::var("PKG_CONFIG").unwrap_or_else(|_| "pkg-config".to_owned());
    let probed = std::process::Command::new(&pkg_config)
        .args(["--libs", "glib-2.0"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok());
    match probed {
        Some(libs) => {
            for token in libs.split_whitespace() {
                if let Some(name) = token.strip_prefix("-l") {
                    println!("cargo:rustc-link-lib={name}");
                } else if let Some(path) = token.strip_prefix("-L") {
                    println!("cargo:rustc-link-search=native={path}");
                } else {
                    println!("cargo:rustc-link-arg={token}");
                }
            }
        }
        None => {
            println!("cargo:rustc-link-lib=glib-2.0");
        }
    }
}
