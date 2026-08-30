//! Contract of `tools/model/fetch-model.sh` and the model-cache locator.
//!
//! These tests pin fetch infrastructure only. They do not open the language
//! model, the scorer, or a [`oxpinyin_engine::Session`].

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use pinyin_oracle::{
    EXPECTED_MODEL_FILES, ModelDirError, model_dir_is_complete, verified_model_dir,
};

static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-model-{}-{}-{}-{}",
            label,
            std::process::id(),
            nanos,
            SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn workspace_root() -> PathBuf {
    let start = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut dir = start.as_path();
    loop {
        if dir.join("crates/pinyin-oracle/Cargo.toml").is_file()
            && dir.join("tools/model/fetch-model.sh").is_file()
        {
            return dir.to_path_buf();
        }
        dir = dir
            .parent()
            .expect("workspace root contains tools/model/fetch-model.sh");
    }
}

fn write_complete_export(dir: &Path) {
    std::fs::create_dir_all(dir).expect("export dir");
    for name in EXPECTED_MODEL_FILES {
        std::fs::write(dir.join(name), b"placeholder").expect("export file");
    }
}

#[test]
fn verified_model_dir_accepts_the_eighteen_expected_files() {
    let scratch = Scratch::new("complete");
    write_complete_export(scratch.path());
    let path = verified_model_dir(scratch.path()).expect("complete export verifies");
    assert!(model_dir_is_complete(&path));
}

#[test]
fn verified_model_dir_rejects_a_partial_export() {
    let scratch = Scratch::new("partial");
    write_complete_export(scratch.path());
    std::fs::remove_file(scratch.path().join("interpolation2.text")).expect("remove model file");
    match verified_model_dir(scratch.path()) {
        Err(ModelDirError::Incomplete { missing, .. }) => {
            assert_eq!(missing, vec!["interpolation2.text"]);
        }
        other => panic!("expected Incomplete, got {other:?}"),
    }
}

#[cfg(unix)]
mod fetch_script {
    use super::{EXPECTED_MODEL_FILES, Scratch, workspace_root};
    use pinyin_oracle::MODEL20_SHA256;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};

    fn script() -> PathBuf {
        workspace_root().join("tools/model/fetch-model.sh")
    }

    fn run(args: &[&str]) -> Output {
        Command::new("bash")
            .arg(script())
            .args(args)
            .output()
            .expect("spawn fetch-model.sh")
    }

    fn stderr_text(output: &Output) -> String {
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    fn stdout_text(output: &Output) -> String {
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn assert_no_panic(output: &Output) {
        let combined = format!("{}{}", stdout_text(output), stderr_text(output));
        assert!(
            !combined.to_ascii_lowercase().contains("panic"),
            "fetch must fail cleanly, not panic:\n{combined}"
        );
        assert!(
            !combined.to_ascii_lowercase().contains("stack backtrace"),
            "fetch must fail cleanly, without a backtrace:\n{combined}"
        );
    }

    fn local_pinned_archive() -> Option<PathBuf> {
        if let Ok(raw) = std::env::var("PINYIN_MODEL_ARCHIVE") {
            let path = PathBuf::from(raw);
            if path.is_file() {
                return Some(path);
            }
        }
        let root = workspace_root();
        for rel in [
            "target/model20/downloads/model20.text.tar.gz",
            "target/oracle-pin-build/downloads/model20.text.tar.gz",
            "target/oracle-tkrzw-build/downloads/model20.text.tar.gz",
            "target/oracle/downloads/model20.text.tar.gz",
            "target/oracle-w2/downloads/model20.text.tar.gz",
        ] {
            let path = root.join(rel);
            if path.is_file() {
                return Some(path);
            }
        }
        None
    }

    #[test]
    fn checksum_mismatch_names_expected_and_actual() {
        let scratch = Scratch::new("bad-hash");
        let archive = scratch.path().join("corrupt.tar.gz");
        std::fs::write(&archive, b"not-the-pinned-archive").expect("corrupt archive");

        let cache = Scratch::new("bad-hash-cache");
        let output = run(&[
            "--cache-dir",
            cache.path().to_str().expect("utf-8 cache"),
            "--archive",
            archive.to_str().expect("utf-8 archive"),
        ]);

        assert!(!output.status.success(), "corrupt archive must be rejected");
        assert_no_panic(&output);
        let stderr = stderr_text(&output);
        assert!(
            stderr.contains("checksum mismatch"),
            "must name the failure class:\n{stderr}"
        );
        assert!(
            stderr.contains(MODEL20_SHA256),
            "must name the expected digest:\n{stderr}"
        );
        assert!(
            stderr.contains("actual:"),
            "must name the actual digest:\n{stderr}"
        );
        assert_ne!(
            stderr
                .lines()
                .find_map(|line| line.trim().strip_prefix("actual:"))
                .map_or("", str::trim),
            MODEL20_SHA256,
            "actual digest must differ from the pin:\n{stderr}"
        );
    }

    #[test]
    fn unreachable_url_fails_cleanly() {
        let cache = Scratch::new("bad-url");
        let url = "https://127.0.0.1:1/model20.text.tar.gz";
        let output = run(&[
            "--cache-dir",
            cache.path().to_str().expect("utf-8 cache"),
            "--url",
            url,
        ]);

        assert!(!output.status.success(), "unreachable URL must fail");
        assert_no_panic(&output);
        let stderr = stderr_text(&output);
        assert!(
            stderr.contains("failed to download") && stderr.contains(url),
            "must name the URL that could not be fetched:\n{stderr}"
        );
    }

    #[test]
    fn refuses_a_tracked_cache_path() {
        let root = workspace_root();
        let cache = root.join("crates/pinyin-oracle/__must_not_vendor_model20");
        let _ = std::fs::remove_dir_all(&cache);

        let output = run(&[
            "--cache-dir",
            cache.to_str().expect("utf-8 cache"),
            "--archive",
            "/dev/null",
        ]);

        let leftover = cache.exists();
        let _ = std::fs::remove_dir_all(&cache);
        assert!(
            !leftover,
            "a refused cache path must not be created inside the crate"
        );
        assert!(!output.status.success());
        assert_no_panic(&output);
        let stderr = stderr_text(&output);
        assert!(
            stderr.contains("git would track"),
            "must refuse a tracked path:\n{stderr}"
        );
    }

    fn assert_eighteen_files(extracted: &Path) {
        for name in EXPECTED_MODEL_FILES {
            assert!(
                extracted.join(name).is_file(),
                "missing extracted file {name}"
            );
        }
    }

    #[test]
    fn pinned_archive_extracts_and_second_run_skips() {
        let Some(archive) = local_pinned_archive() else {
            eprintln!(
                "pinned model archive not present locally; skipping extract/idempotency \
                 (run tools/model/fetch-model.sh)"
            );
            return;
        };

        let cache = Scratch::new("extract");
        let cache_str = cache.path().to_str().expect("utf-8 cache");
        let archive_str = archive.to_str().expect("utf-8 archive");

        let first = run(&["--cache-dir", cache_str, "--archive", archive_str]);
        assert!(
            first.status.success(),
            "pinned archive must extract:\n{}",
            stderr_text(&first)
        );
        assert_no_panic(&first);
        let extracted = PathBuf::from(stdout_text(&first).trim());
        assert!(extracted.starts_with(cache.path()));
        assert_eighteen_files(&extracted);

        let second = run(&["--cache-dir", cache_str, "--archive", archive_str]);
        assert!(
            second.status.success(),
            "second run must succeed:\n{}",
            stderr_text(&second)
        );
        assert_no_panic(&second);
        let stderr = stderr_text(&second);
        assert!(
            stderr.contains("skipping fetch"),
            "verified cache must skip re-download:\n{stderr}"
        );
        assert_eq!(stdout_text(&second).trim(), extracted.to_str().unwrap());
        assert_eighteen_files(&extracted);
    }
}
