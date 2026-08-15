//! `pinyin-dictool` — standalone vocabulary conversion tool.
//!
//! Import mode turns the pinned user-vocabulary text format
//! (`docs/findings/dictool-format.md`) into the pinyin-rs user redb store
//! through the public C ABI import trio and `pinyin_save`.

#![warn(missing_docs)]

mod export;
mod format;
mod import;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

/// Process the command line and run one command.
pub fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match command(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("pinyin-dictool: {error}");
            ExitCode::FAILURE
        }
    }
}

fn command(args: &[OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let Some(first) = args.first().and_then(|arg| arg.to_str()) else {
        usage();
    };
    match first {
        "-h" | "--help" => {
            print_usage();
            Ok(())
        }
        "import" => {
            let mut user_dir: Option<PathBuf> = None;
            let mut file: Option<PathBuf> = None;
            let mut index = 1;
            while index < args.len() {
                let arg = args[index].to_string_lossy();
                match arg.as_ref() {
                    "--user-dir" => {
                        index += 1;
                        let Some(value) = args.get(index) else {
                            return Err("--user-dir requires a path".into());
                        };
                        user_dir = Some(PathBuf::from(value));
                    }
                    arg if !arg.starts_with('-') => {
                        if file.replace(PathBuf::from(args[index].clone())).is_some() {
                            return Err(format!("unexpected extra argument: {arg}").into());
                        }
                    }
                    _ => return Err(format!("unknown import option: {arg}").into()),
                }
                index += 1;
            }
            let Some(user_dir) = user_dir else {
                return Err("import requires --user-dir <path>".into());
            };
            let Some(file) = file else {
                return Err("import requires an input <file.txt>".into());
            };
            import::run(&user_dir, &file)?;
            Ok(())
        }
        "export" => {
            let mut user_dir: Option<PathBuf> = None;
            let mut file: Option<PathBuf> = None;
            let mut index = 1;
            while index < args.len() {
                let arg = args[index].to_string_lossy();
                match arg.as_ref() {
                    "--user-dir" => {
                        index += 1;
                        let Some(value) = args.get(index) else {
                            return Err("--user-dir requires a path".into());
                        };
                        user_dir = Some(PathBuf::from(value));
                    }
                    arg if !arg.starts_with('-') => {
                        if file.replace(PathBuf::from(args[index].clone())).is_some() {
                            return Err(format!("unexpected extra argument: {arg}").into());
                        }
                    }
                    _ => return Err(format!("unknown export option: {arg}").into()),
                }
                index += 1;
            }
            let Some(user_dir) = user_dir else {
                return Err("export requires --user-dir <path>".into());
            };
            export::run(&user_dir, file.as_deref())?;
            Ok(())
        }
        _ => usage(),
    }
}

fn usage() -> ! {
    eprintln!(
        "Usage:\n  pinyin-dictool import --user-dir <path> <file.txt>\n  \
         pinyin-dictool export --user-dir <path> [file.txt]\n  \
         pinyin-dictool --help"
    );
    std::process::exit(2);
}

fn print_usage() {
    println!(
        "Usage:\n  pinyin-dictool import --user-dir <path> <file.txt>\n  \
         pinyin-dictool export --user-dir <path> [file.txt]\n\n\
         Import/export speak the classic ibus-libpinyin interchange format:\n\
         phrase pinyin [count], space- or tab-separated, one record per line.\n\
         Dictool extensions: # comments and blank lines are skipped; malformed\n\
         lines are reported with line numbers.\n\n\
         Format: docs/findings/dictool-format.md"
    );
}
