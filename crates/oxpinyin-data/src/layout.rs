//! Which data layout a directory holds — libpinyin's or oxpinyin's.
//!
//! `pinyin_init(systemdir, userdir)` is handed a directory and must decide
//! what is in it before opening anything. This module is that decision,
//! and only that decision: it inspects file presence and file headers, and
//! never parses a table. Deciding is portable and needs no Berkeley DB, so
//! it compiles without the `bdb` feature; only *reading* a compat
//! directory needs the backend.
//!
//! # Where libpinyin's data actually is
//!
//! `$(libdir)/libpinyin/data`, from `data/Makefile.am`'s
//! `libpinyin_dbdir` — on Debian and Ubuntu
//! `/usr/lib/x86_64-linux-gnu/libpinyin/data`. A *library* path, not
//! `$datadir`: anything that guesses `share` finds nothing.
//!
//! # What counts as present
//!
//! The compat marker set is `bigram.db` + `phrase_index.bin` +
//! `pinyin_index.bin`. Deliberately **not** `punct.bin`: it is in the
//! pin's `data/Makefile.am` but absent from the installed 2.8.1 package,
//! arriving in a later release, so requiring it would reject exactly the
//! installations this path exists to serve.
//!
//! `bigram.db` is checked by its header rather than its name, because the
//! whole compat path turns on it being a Berkeley DB Hash file and a
//! wrongly-named file should fail the detector, not the open.

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The three files a libpinyin data directory must have for the compat
/// path, in the order the detector reports them.
const COMPAT_MARKERS: [&str; 3] = ["bigram.db", "phrase_index.bin", "pinyin_index.bin"];

/// The files an oxpinyin-native data directory must have.
const NATIVE_MARKERS: [&str; 2] = ["phrase_index", "pinyin_index"];

/// Berkeley DB's Hash magic, little-endian in the file's own byte order.
///
/// `DB_HASHMAGIC` is `0x00061561`; the generated bindings agree, and the
/// installed 25.9 MB `bigram.db` carries it. Berkeley DB writes the
/// header in native order and tolerates the swapped form on read, so both
/// are accepted here.
const DB_HASH_MAGIC: u32 = 0x0006_1561;

/// Which layout a directory holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataLayout {
    /// libpinyin's own files: read through the compatibility backend.
    Compat,
    /// oxpinyin's store-backed tables.
    Native,
}

/// Why a directory could not be classified.
#[derive(Debug)]
#[non_exhaustive]
pub enum LayoutError {
    /// Neither layout's markers were found.
    ///
    /// Carries what was searched so the message can name it: failing
    /// closed is only useful if the user can see where to look.
    NoData {
        /// The directory that was inspected.
        dir: PathBuf,
        /// libpinyin markers that were missing.
        missing_compat: Vec<&'static str>,
        /// oxpinyin markers that were missing.
        missing_native: Vec<&'static str>,
    },
    /// A `bigram.db` is present but is not a Berkeley DB Hash file.
    NotABerkeleyHash {
        /// The offending file.
        path: PathBuf,
        /// Its first four bytes, as read.
        magic: u32,
    },
    /// The directory could not be read.
    Io(std::io::Error),
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoData {
                dir,
                missing_compat,
                missing_native,
            } => write!(
                f,
                "no usable data in {}: for the libpinyin compatibility layout it lacks \
                 {} (searched for {}), and for the native layout it lacks {} (searched \
                 for {}). libpinyin installs its data under $(libdir)/libpinyin/data — \
                 on Debian and Ubuntu /usr/lib/<arch>/libpinyin/data — not under \
                 /usr/share",
                dir.display(),
                missing_compat.join(", "),
                COMPAT_MARKERS.join(", "),
                missing_native.join(", "),
                NATIVE_MARKERS.join(", "),
            ),
            Self::NotABerkeleyHash { path, magic } => write!(
                f,
                "{} is not a Berkeley DB Hash file: its magic is {magic:#010x}, not \
                 {DB_HASH_MAGIC:#010x}. libpinyin opens bigram.db as DB_HASH, so a file \
                 of another type here is not the one this path can read",
                path.display(),
            ),
            Self::Io(error) => write!(f, "reading the data directory: {error}"),
        }
    }
}

impl std::error::Error for LayoutError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for LayoutError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Whether `path` starts with Berkeley DB's Hash magic.
///
/// Berkeley DB puts the magic at offset 12 of the metadata page, not at
/// offset 0, and writes it in the machine's own byte order; a file written
/// on the other endianness is still readable, so both orders count.
fn is_berkeley_hash(path: &Path) -> Result<bool, LayoutError> {
    let mut header = [0_u8; 16];
    let mut file = std::fs::File::open(path)?;
    if let Err(error) = file.read_exact(&mut header) {
        // A file too short to hold a metadata page is not one.
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            return Ok(false);
        }
        return Err(error.into());
    }
    let magic = u32::from_ne_bytes([header[12], header[13], header[14], header[15]]);
    Ok(magic == DB_HASH_MAGIC || magic.swap_bytes() == DB_HASH_MAGIC)
}

/// The magic `path` carries, for reporting a mismatch.
fn header_magic(path: &Path) -> u32 {
    let mut header = [0_u8; 16];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_or(0, |()| {
            u32::from_ne_bytes([header[12], header[13], header[14], header[15]])
        })
}

/// Classifies `dir`.
///
/// Both layouts present resolves to [`DataLayout::Compat`]: a directory
/// that holds libpinyin's files as well as oxpinyin's is one where the
/// user installed oxpinyin alongside a real libpinyin, and the point of
/// the drop-in is to read what they already have.
///
/// # Errors
///
/// [`LayoutError::NoData`] when neither marker set is complete — naming
/// what was searched, because failing closed is only useful if the
/// message says where to look. [`LayoutError::NotABerkeleyHash`] when a
/// `bigram.db` is present but is a file of another type.
pub fn detect(dir: &Path) -> Result<DataLayout, LayoutError> {
    let missing_compat: Vec<&'static str> = COMPAT_MARKERS
        .into_iter()
        .filter(|name| !dir.join(name).is_file())
        .collect();

    if missing_compat.is_empty() {
        // Present is not enough: the compat path can only read a DB_HASH
        // file, so a bigram.db of another type fails here rather than at
        // the open, where the message would be libdb's rather than ours.
        let bigram = dir.join("bigram.db");
        if !is_berkeley_hash(&bigram)? {
            return Err(LayoutError::NotABerkeleyHash {
                magic: header_magic(&bigram),
                path: bigram,
            });
        }
        return Ok(DataLayout::Compat);
    }

    let missing_native: Vec<&'static str> = NATIVE_MARKERS
        .into_iter()
        .filter(|name| !dir.join(name).is_file())
        .collect();
    if missing_native.is_empty() {
        return Ok(DataLayout::Native);
    }

    Err(LayoutError::NoData {
        dir: dir.to_path_buf(),
        missing_compat,
        missing_native,
    })
}

/// The directories libpinyin's data is installed into, most specific
/// first.
///
/// `$(libdir)/libpinyin/data` under each plausible `libdir`, which covers
/// Debian and Ubuntu's multiarch layout, Fedora and openSUSE's `lib64`,
/// a plain `lib`, and a `/usr/local` build. Nothing under `$datadir` is
/// searched, because libpinyin puts nothing there.
///
/// This is where to look, not what is there: hand a result to [`detect`].
#[must_use]
pub fn system_data_dirs() -> Vec<PathBuf> {
    // Debian's multiarch tuple. `std::env::consts::ARCH` gives the Rust
    // arch name, which matches Debian's for the targets that matter here
    // (`x86_64`, `aarch64`, `riscv64`, `powerpc64`); a miss simply means
    // this candidate does not exist and the next one is tried.
    let multiarch = format!("lib/{}-linux-gnu", std::env::consts::ARCH);
    let mut dirs = Vec::new();
    for prefix in ["/usr", "/usr/local"] {
        for libdir in [multiarch.as_str(), "lib64", "lib"] {
            dirs.push(Path::new(prefix).join(libdir).join("libpinyin/data"));
        }
    }
    dirs
}

/// The first directory from [`system_data_dirs`] that holds libpinyin's
/// data, or `None`.
#[must_use]
pub fn find_system_data_dir() -> Option<PathBuf> {
    system_data_dirs()
        .into_iter()
        .find(|dir| detect(dir).is_ok_and(|layout| layout == DataLayout::Compat))
}

#[cfg(test)]
mod tests {
    use super::{DataLayout, LayoutError, detect};

    /// A temp directory removed on drop.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("oxpinyin-layout-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create temp dir");
            Self(dir)
        }

        fn write(&self, name: &str, bytes: &[u8]) {
            std::fs::write(self.0.join(name), bytes).expect("write marker");
        }

        /// A file whose metadata page carries Berkeley DB's Hash magic.
        fn write_hash_db(&self, name: &str) {
            let mut header = vec![0_u8; 16];
            header[12..16].copy_from_slice(&super::DB_HASH_MAGIC.to_ne_bytes());
            self.write(name, &header);
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn libpinyin_files_select_the_compat_path() {
        let dir = TempDir::new("compat");
        dir.write_hash_db("bigram.db");
        dir.write("phrase_index.bin", b"chunk");
        dir.write("pinyin_index.bin", b"chunk");
        assert_eq!(detect(&dir.0).expect("classify"), DataLayout::Compat);
    }

    #[test]
    fn punct_bin_is_never_required() {
        // It is in the pin's data/Makefile.am but absent from the
        // installed 2.8.1 package. Requiring it would reject exactly the
        // installations the compat path exists for.
        let dir = TempDir::new("nopunct");
        dir.write_hash_db("bigram.db");
        dir.write("phrase_index.bin", b"chunk");
        dir.write("pinyin_index.bin", b"chunk");
        assert!(!dir.0.join("punct.bin").exists(), "punct.bin is absent");
        assert_eq!(
            detect(&dir.0).expect("classify without punct.bin"),
            DataLayout::Compat,
        );
    }

    #[test]
    fn native_files_select_the_native_path() {
        let dir = TempDir::new("native");
        dir.write("phrase_index", b"redb");
        dir.write("pinyin_index", b"redb");
        assert_eq!(detect(&dir.0).expect("classify"), DataLayout::Native);
    }

    #[test]
    fn both_layouts_present_takes_the_compat_path() {
        // The user installed oxpinyin beside a real libpinyin; reading
        // what they already have is the whole point of the drop-in.
        let dir = TempDir::new("both");
        dir.write_hash_db("bigram.db");
        dir.write("phrase_index.bin", b"chunk");
        dir.write("pinyin_index.bin", b"chunk");
        dir.write("phrase_index", b"redb");
        dir.write("pinyin_index", b"redb");
        assert_eq!(detect(&dir.0).expect("classify"), DataLayout::Compat);
    }

    #[test]
    fn neither_layout_fails_closed_naming_what_was_searched() {
        let dir = TempDir::new("empty");
        let error = detect(&dir.0).expect_err("an empty directory has no data");
        let message = error.to_string();
        assert!(matches!(error, LayoutError::NoData { .. }));
        for name in ["bigram.db", "phrase_index.bin", "pinyin_index.bin"] {
            assert!(
                message.contains(name),
                "the failure must name {name}, which it searched for; got: {message}"
            );
        }
        assert!(
            message.contains("libpinyin/data"),
            "and must say where libpinyin actually installs its data, since \
             $(libdir) rather than $datadir is the thing people get wrong; got: {message}"
        );
    }

    #[test]
    fn an_incomplete_compat_set_does_not_select_compat() {
        // A stray bigram.db beside nothing else is not a libpinyin data
        // directory, and must not be treated as one.
        let dir = TempDir::new("partial");
        dir.write_hash_db("bigram.db");
        assert!(matches!(detect(&dir.0), Err(LayoutError::NoData { .. })));
    }

    #[test]
    fn the_installed_libpinyin_data_dir_classifies_as_compat() {
        // The only case that matters: a directory the distro's own
        // libpinyin-data package created. Everything above builds its
        // fixture, so this is the one check that the marker set and the
        // magic offset describe a real installation rather than this
        // module's idea of one.
        let Some(dir) = super::find_system_data_dir() else {
            let searched: Vec<String> = super::system_data_dirs()
                .iter()
                .map(|d| d.display().to_string())
                .collect();
            let message = format!(
                "SKIP: no installed libpinyin data directory among {}. This is the only \
                 test that classifies a directory the distro created rather than one \
                 this module built.",
                searched.join(", ")
            );
            assert!(
                std::env::var_os("OXPINYIN_BDB_STRICT").is_none(),
                "{message} (OXPINYIN_BDB_STRICT is set, so this skip is a failure)"
            );
            eprintln!("{message}");
            return;
        };
        assert_eq!(super::detect(&dir).expect("classify"), DataLayout::Compat);
        assert!(
            dir.to_string_lossy().contains("/lib"),
            "and it must have been found under $(libdir), not $datadir: {}",
            dir.display()
        );
    }

    #[test]
    fn a_bigram_db_that_is_not_a_berkeley_hash_is_refused() {
        // Present is not enough. A file of another type must fail the
        // detector with our message, not libdb's at open time.
        let dir = TempDir::new("wrongmagic");
        dir.write("bigram.db", &[0_u8; 32]);
        dir.write("phrase_index.bin", b"chunk");
        dir.write("pinyin_index.bin", b"chunk");
        let error = detect(&dir.0).expect_err("the magic does not match");
        assert!(matches!(error, LayoutError::NotABerkeleyHash { .. }));
        assert!(
            error.to_string().contains("DB_HASH"),
            "the message must say what libpinyin opens it as; got: {error}"
        );
    }
}
