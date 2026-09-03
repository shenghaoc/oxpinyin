//! Shared loaders for the Stage-2 C-ABI benches.
//!
//! Benches refuse to start without the exported tables and the fetched
//! model20 cache: fixture-scale numbers would not describe the pinned
//! construction. They do not assert the 10190-class candidate pins.

#![allow(dead_code)]

use std::ffi::CString;
use std::os::raw::{c_char, c_uint, c_void};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use pinyin_capi::{pinyin_fini, pinyin_init};

/// Opaque `pinyin_context_t *`. C ABI symbols are `#[no_mangle]` on the
/// rlib; declaring them here avoids expanding the crate's Rust public
/// surface (which would trip `clippy::not_unsafe_ptr_arg_deref`).
pub type Context = *mut c_void;
/// Opaque `pinyin_instance_t *`.
pub type Instance = *mut c_void;

unsafe extern "C" {
    pub fn pinyin_alloc_instance(context: Context) -> Instance;
    pub fn pinyin_free_instance(instance: Instance);
    pub fn pinyin_reset(instance: Instance) -> bool;
    pub fn pinyin_set_options(context: Context, options: u32) -> bool;
    pub fn pinyin_parse_more_full_pinyins(instance: Instance, pinyins: *const c_char) -> usize;
    pub fn pinyin_guess_candidates(instance: Instance, offset: usize, sort_option: c_uint) -> bool;
    pub fn pinyin_get_n_candidate(instance: Instance, num: *mut c_uint) -> bool;
    pub fn pinyin_guess_sentence(instance: Instance) -> bool;
    pub fn pinyin_get_sentence(instance: Instance, index: u8, sentence: *mut *mut c_char) -> bool;
}

/// Frozen pin reference from `docs/findings/oracle-environment.md`.
pub const PIN_REF: &str = concat!(
    "libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c",
    "+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155",
    "+dbm-tkrzw"
);

/// Pinned SHA-256 of `model20.text.tar.gz`, re-exported from the canonical
/// model20-cache locator (`oxpinyin_testsupport::model_cache`).
pub use oxpinyin_testsupport::model_cache::{MODEL20_SHA256, export_dir, missing_model_files};

/// W8 parity-profile option word: `IS_PINYIN | PINYIN_INCOMPLETE |
/// USE_DIVIDED_TABLE | USE_RESPLIT_TABLE` (`0x18a`).
pub const W8_OPTIONS: u32 = 0x18a;

/// W8 guess sort: `SORT_BY_PHRASE_LENGTH | SORT_BY_PINYIN_LENGTH |
/// SORT_BY_FREQUENCY` (`0x1e`).
pub const W8_SORT: u32 = 0x1e;

/// Short full-pinyin input.
pub const PARSE_SHORT: &str = "ni";

/// Medium full-pinyin input (two words, twelve letters).
pub const PARSE_MEDIUM: &str = "nihaozhongguo";

/// Junk-leading input: a non-`a-z`/`'` prefix then a real syllable.
pub const PARSE_JUNK_LEADING: &str = "1nihao";

/// Phrase used for offset-0 / mid-phrase candidate guess.
pub const GUESS_PHRASE: &str = "nihaozhongguo";

/// Byte offset after `nihao` in [`GUESS_PHRASE`].
pub const GUESS_MID_OFFSET: usize = 5;

/// System phrase token used by the scan-perf populated-store arm.
pub const HOT_TOKEN: u32 = 0x0100_1225;

fn cache_root_of(extracted: &Path) -> PathBuf {
    let canonical = extracted
        .canonicalize()
        .unwrap_or_else(|_| extracted.to_path_buf());
    canonical
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or(canonical)
}

fn marker_sha256(cache_root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(cache_root.join("verified")).ok()?;
    text.lines().find_map(|line| {
        line.strip_prefix("sha256=")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn file_sha256(path: &Path) -> Option<String> {
    let sha256sum = Command::new("sha256sum").arg(path).output();
    let output = match sha256sum {
        Ok(out) if out.status.success() => out,
        _ => Command::new("shasum")
            .args(["-a", "256"])
            .arg(path)
            .output()
            .ok()
            .filter(|out| out.status.success())?,
    };
    let text = String::from_utf8(output.stdout).ok()?;
    text.split_whitespace().next().map(str::to_owned)
}

/// The extracted tree is the pinned model20 only when the fetch-script
/// `verified` marker (or the sibling archive digest) matches [`MODEL20_SHA256`].
fn assert_pinned_model(extracted: &Path) -> PathBuf {
    let missing = missing_model_files(extracted);
    assert!(
        missing.is_empty(),
        "model dir {} is incomplete (missing {missing:?}); run tools/model/fetch-model.sh",
        extracted.display()
    );
    let cache_root = cache_root_of(extracted);
    if marker_sha256(&cache_root).as_deref() == Some(MODEL20_SHA256) {
        return extracted
            .canonicalize()
            .unwrap_or_else(|_| extracted.to_path_buf());
    }
    let archive = cache_root.join("downloads").join("model20.text.tar.gz");
    if archive.is_file() {
        let digest = file_sha256(&archive).unwrap_or_default();
        assert_eq!(
            digest,
            MODEL20_SHA256,
            "archive {} is not the pinned model20 (got {digest}); run tools/model/fetch-model.sh",
            archive.display()
        );
        return extracted
            .canonicalize()
            .unwrap_or_else(|_| extracted.to_path_buf());
    }
    panic!(
        "model dir {} is not a verified model20 cache (no sha256={} marker); run tools/model/fetch-model.sh",
        extracted.display(),
        MODEL20_SHA256
    );
}

/// Fetched model20 extraction; panics if the cache is absent or off-pin.
pub fn model_dir() -> PathBuf {
    if let Some(raw) = std::env::var_os("PINYIN_MODEL_DIR") {
        return assert_pinned_model(&PathBuf::from(raw));
    }
    if let Some(raw) = std::env::var_os("PINYIN_MODEL_CACHE") {
        let dir = PathBuf::from(raw).join("extracted");
        if dir.is_dir() {
            return assert_pinned_model(&dir);
        }
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let default = manifest
        .join("..")
        .join("..")
        .join("target")
        .join("model20")
        .join("extracted");
    assert_pinned_model(&default)
}

fn link_or_copy(src: &Path, dst: &Path) {
    if dst.exists() {
        return;
    }
    #[cfg(unix)]
    {
        if std::os::unix::fs::symlink(src, dst).is_ok() {
            return;
        }
    }
    std::fs::copy(src, dst).unwrap_or_else(|error| {
        panic!("link/copy {} -> {}: {error}", src.display(), dst.display())
    });
}

/// One directory that `pinyin_init` can open: the exported tables (named
/// by the compiled-in backend, like [`export_dir`] asserts them) plus the
/// pinned `interpolation2.text`.
pub fn staged_system_dir() -> &'static Path {
    static STAGED: OnceLock<PathBuf> = OnceLock::new();
    STAGED
        .get_or_init(|| {
            let export = export_dir();
            let model = model_dir();
            let staged =
                std::env::temp_dir().join(format!("oxpinyin-stage2-system-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&staged);
            std::fs::create_dir_all(&staged).expect("stage system dir");
            for stem in ["pinyin_index", "phrase_index", "bigram"] {
                let name = oxpinyin_store::default_store_file(stem);
                link_or_copy(&export.join(&name), &staged.join(&name));
            }
            link_or_copy(
                &model.join("interpolation2.text"),
                &staged.join("interpolation2.text"),
            );
            staged
        })
        .as_path()
}

fn cstring(path: &Path) -> CString {
    CString::new(path.to_str().expect("UTF-8 path")).expect("no interior NUL")
}

/// Live C-ABI context + instance over the pinned tables.
pub struct CapiBench {
    /// `pinyin_context_t *`.
    pub context: Context,
    /// `pinyin_instance_t *`.
    pub instance: Instance,
    user_dir: PathBuf,
}

impl CapiBench {
    /// `pinyin_init` + `pinyin_alloc_instance` + the W8 option word.
    pub fn open() -> Self {
        let system = cstring(staged_system_dir());
        let user_dir = std::env::temp_dir().join(format!(
            "oxpinyin-stage2-user-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&user_dir).expect("user dir");
        let user = cstring(&user_dir);
        let context: Context = pinyin_init(system.as_ptr(), user.as_ptr()).cast();
        assert!(
            !context.is_null(),
            "pinyin_init failed on pinned tables at {}",
            staged_system_dir().display()
        );
        // SAFETY: `context` is a live handle from `pinyin_init`; the
        // matching `pinyin_fini` runs in `Drop`.
        assert!(unsafe { pinyin_set_options(context, W8_OPTIONS) });
        // SAFETY: `context` is still live.
        let instance = unsafe { pinyin_alloc_instance(context) };
        assert!(!instance.is_null(), "pinyin_alloc_instance failed");
        Self {
            context,
            instance,
            user_dir,
        }
    }
}

impl Drop for CapiBench {
    fn drop(&mut self) {
        if !self.instance.is_null() {
            // SAFETY: `instance` came from `pinyin_alloc_instance` and has
            // not been freed yet.
            unsafe {
                pinyin_free_instance(self.instance);
            }
            self.instance = std::ptr::null_mut();
        }
        if !self.context.is_null() {
            pinyin_fini(self.context.cast());
            self.context = std::ptr::null_mut();
        }
        let _ = std::fs::remove_dir_all(&self.user_dir);
    }
}

/// Print the pin identity once, at the top of a Criterion run.
///
/// The SHA is printed only after [`model_dir`] has accepted the extracted
/// tree as the pinned model20.
pub fn print_pin_header() {
    let dir = model_dir();
    eprintln!("oxpinyin Stage-2 benches");
    eprintln!("  pin: {PIN_REF}");
    eprintln!("  model20 SHA-256: {MODEL20_SHA256}");
    eprintln!("  model dir: {}", dir.display());
}
