//! Which data layout a directory holds, and which DBM library wrote it.
//!
//! `pinyin_init(systemdir, userdir)` is handed a directory and must decide
//! what is in it before opening anything. This module is that decision,
//! and only that decision: it inspects file presence and file headers, and
//! never parses a table. Deciding is portable and needs no Kyoto Cabinet,
//! so it compiles without the `kyotocabinet` feature; only *reading* a
//! compat directory needs the backend.
//!
//! # The filename is not the format
//!
//! This is the fact that shapes the whole module. libpinyin's data file
//! names are compile-time constants — `SYSTEM_BIGRAM "bigram.db"`,
//! `USER_BIGRAM "user_bigram.db"` (`src/pinyin_internal.h:56-58`) — and
//! they do **not** vary with the DBM backend it was configured with.
//! `--with-dbm=BerkeleyDB`, `--with-dbm=KyotoCabinet` and
//! `--with-dbm=Tkrzw` all produce a file called `bigram.db`, in three
//! mutually unreadable formats.
//!
//! So there is no `.kch` or `.kct` to look for: those are Kyoto Cabinet's
//! own naming convention, which libpinyin does not use. Detection is by
//! **magic**, and it has to distinguish the backends, because opening a
//! Kyoto Cabinet file with Berkeley DB (or the reverse) is a failure the
//! user would see as "your input method is broken".
//!
//! | Library | Bytes | Where |
//! |---|---|---|
//! | Berkeley DB Hash | `0x00061561` | offset 12 |
//! | Kyoto Cabinet | `KC\n\0` then `0x30` (Hash) or `0x31` (Tree) | offsets 0 and 8 |
//! | tkrzw | `TkrzwHDB\n` | offset 0 |
//!
//! Each measured on this machine: the installed `libpinyin-data` package's
//! 25.9 MB `bigram.db` carries the Berkeley DB magic, files created by
//! Kyoto Cabinet 1.2.80 carry `KC\n\0`, and files written by tkrzw 1.0.32
//! carry `TkrzwHDB\n`.
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

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The three files a libpinyin data directory must have for the compat
/// path, in the order the detector reports them.
const COMPAT_MARKERS: [&str; 3] = ["bigram.db", "phrase_index.bin", "pinyin_index.bin"];

/// The table stems an oxpinyin-native data directory must have; each is
/// probed with the compiled-in backend's extension
/// ([`crate::default_store_file`]) — "native" means this binary's own
/// backend wrote the directory.
const NATIVE_MARKERS: [&str; 2] = ["phrase_index", "pinyin_index"];

/// Berkeley DB's Hash magic (`DB_HASHMAGIC`), at offset 12 of the
/// metadata page, in the writing machine's byte order.
const DB_HASH_MAGIC: u32 = 0x0006_1561;

/// tkrzw's container magic (`tkrzw_dbm_hash.cc` `META_MAGIC_DATA`), at
/// offset 0 of both HashDBM and TreeDBM files.
const TKRZW_MAGIC: &[u8; 9] = b"TkrzwHDB\n";

/// Kyoto Cabinet's file magic, at offset 0.
const KC_MAGIC: [u8; 4] = *b"KC\n\0";

/// Kyoto Cabinet's database-type byte, at offset 8.
const KC_TYPE_AT: usize = 8;
/// `HashDB`.
const KC_TYPE_HASH: u8 = 0x30;
/// `TreeDB`.
const KC_TYPE_TREE: u8 = 0x31;

/// How many header bytes any of the checks needs.
const HEADER_BYTES: usize = 16;

/// Which DBM library wrote a libpinyin data file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dbm {
    /// Berkeley DB, `DB_HASH`.
    BerkeleyHash,
    /// Kyoto Cabinet, `HashDB`.
    KyotoHash,
    /// Kyoto Cabinet, `TreeDB`.
    KyotoTree,
    /// tkrzw (HashDBM and TreeDBM share one container magic; a TreeDBM
    /// file is a HashDBM container carrying tree pages).
    Tkrzw,
}

impl Dbm {
    /// A human name for a message.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::BerkeleyHash => "Berkeley DB (hash)",
            Self::KyotoHash => "Kyoto Cabinet (hash)",
            Self::KyotoTree => "Kyoto Cabinet (tree)",
            Self::Tkrzw => "tkrzw",
        }
    }
}

impl fmt::Display for Dbm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Which layout a directory holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataLayout {
    /// libpinyin's own files, and which library wrote the n-gram database.
    Compat(Dbm),
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
    /// A `bigram.db` is present but was written by no DBM this build
    /// recognises.
    UnknownDbm {
        /// The offending file.
        path: PathBuf,
        /// Its first header bytes, for the message.
        header: [u8; HEADER_BYTES],
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
                NATIVE_MARKERS.map(crate::default_store_file).join(", "),
            ),
            Self::UnknownDbm { path, header } => {
                write!(
                    f,
                    "{} was written by no DBM library this build recognises. Its first \
                     {HEADER_BYTES} bytes are ",
                    path.display(),
                )?;
                for byte in header {
                    write!(f, "{byte:02x}")?;
                }
                write!(
                    f,
                    "; expected Berkeley DB's {DB_HASH_MAGIC:#010x} at offset 12, or \
                     Kyoto Cabinet's \"KC\\n\\0\" or tkrzw's \"TkrzwHDB\\n\" at offset 0. \
                     libpinyin names this file bigram.db whichever DBM it was built \
                     against, so the name says nothing about the format"
                )
            }
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

/// The first [`HEADER_BYTES`] of `path`, zero-padded if it is shorter.
fn header_of(path: &Path) -> Result<[u8; HEADER_BYTES], LayoutError> {
    let mut header = [0_u8; HEADER_BYTES];
    let mut file = std::fs::File::open(path)?;
    let mut filled = 0;
    // A short file is not an error here — it simply matches no magic.
    while filled < HEADER_BYTES {
        match file.read(&mut header[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(header)
}

/// Which DBM wrote a file with this header, if any.
#[must_use]
pub fn dbm_of_header(header: &[u8; HEADER_BYTES]) -> Option<Dbm> {
    // Berkeley DB writes its magic in the creating machine's byte order
    // and reads either, so both count.
    let bdb = u32::from_ne_bytes([header[12], header[13], header[14], header[15]]);
    if bdb == DB_HASH_MAGIC || bdb.swap_bytes() == DB_HASH_MAGIC {
        return Some(Dbm::BerkeleyHash);
    }
    // tkrzw: HashDBM's meta magic, which a TreeDBM file also carries
    // (verified against tkrzw 1.0.32 `tkrzw_dbm_hash.cc` META_MAGIC_DATA
    // and a written TreeDBM file).
    if header.len() >= TKRZW_MAGIC.len() && header[..TKRZW_MAGIC.len()] == *TKRZW_MAGIC {
        return Some(Dbm::Tkrzw);
    }
    if header[..4] == KC_MAGIC {
        return match header[KC_TYPE_AT] {
            KC_TYPE_HASH => Some(Dbm::KyotoHash),
            KC_TYPE_TREE => Some(Dbm::KyotoTree),
            // A Kyoto Cabinet file of some other class — a directory
            // database, a plain-text database — is not one libpinyin
            // writes, so it is not recognised rather than guessed at.
            _ => None,
        };
    }
    None
}

/// Which DBM wrote `path`, if any.
///
/// # Errors
///
/// [`LayoutError::Io`] when the file cannot be read.
pub fn dbm_of(path: &Path) -> Result<Option<Dbm>, LayoutError> {
    Ok(dbm_of_header(&header_of(path)?))
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
/// message says where to look. [`LayoutError::UnknownDbm`] when a
/// `bigram.db` is present but was written by an unrecognised library.
pub fn detect(dir: &Path) -> Result<DataLayout, LayoutError> {
    let mut missing_compat: Vec<&'static str> = Vec::new();
    for name in COMPAT_MARKERS {
        match std::fs::metadata(dir.join(name)) {
            Ok(meta) if meta.is_file() => {}
            // Absent is the expected miss; a directory or other
            // non-file is as good as absent for that marker.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => missing_compat.push(name),
            Ok(_) => missing_compat.push(name),
            // Anything else (a permission denial, an EIO on the mount)
            // is a real I/O failure, not a missing directory: reporting
            // it as NoData would send the user hunting for files that
            // are right there.
            Err(e) => {
                return Err(LayoutError::Io(std::io::Error::new(
                    e.kind(),
                    format!("{}: {e}", dir.join(name).display()),
                )));
            }
        }
    }

    if missing_compat.is_empty() {
        // Present is not enough, and the name says nothing: which library
        // wrote the file decides which backend can read it.
        let bigram = dir.join("bigram.db");
        let header = header_of(&bigram)?;
        return match dbm_of_header(&header) {
            Some(dbm) => Ok(DataLayout::Compat(dbm)),
            None => Err(LayoutError::UnknownDbm {
                path: bigram,
                header,
            }),
        };
    }

    let mut missing_native: Vec<&'static str> = Vec::new();
    for stem in NATIVE_MARKERS {
        match std::fs::metadata(dir.join(crate::default_store_file(stem))) {
            Ok(meta) if meta.is_file() => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => missing_native.push(stem),
            Ok(_) => missing_native.push(stem),
            Err(e) => {
                return Err(LayoutError::Io(std::io::Error::new(
                    e.kind(),
                    format!(
                        "{}: {e}",
                        dir.join(crate::default_store_file(stem)).display()
                    ),
                )));
            }
        }
    }
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
/// `$(libdir)/libpinyin/data` under each plausible `libdir`, covering
/// Debian and Ubuntu's multiarch layout, Fedora and openSUSE's `lib64`, a
/// plain `lib`, and a `/usr/local` build. Nothing under `$datadir` is
/// searched, because libpinyin puts nothing there.
///
/// This is where to look, not what is there: hand a result to [`detect`].
#[must_use]
pub fn system_data_dirs() -> Vec<PathBuf> {
    // Debian/Ubuntu multiarch: `<prefix>/lib/<tuple>/libpinyin/data`.
    // `std::env::consts::ARCH` names the tuple for some targets but not all
    // (x86 → `i386`, arm → `arm-linux-gnueabihf`), so the constructed guess
    // below is followed by a scan of `<prefix>/lib` for any `*-linux-gnu*`
    // subdirectory actually present — which covers those and any future
    // tuple without hard-coding a list.
    let multiarch = format!("{}-linux-gnu", std::env::consts::ARCH);
    let mut dirs = Vec::new();
    for prefix in ["/usr", "/usr/local"] {
        let lib = Path::new(prefix).join("lib");
        dirs.push(lib.join(&multiarch).join("libpinyin/data"));
        if let Ok(entries) = std::fs::read_dir(&lib) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                if let Some(name) = name.to_str()
                    && name.contains("-linux-gnu")
                    && name != multiarch
                {
                    dirs.push(lib.join(name).join("libpinyin/data"));
                }
            }
        }
        for libdir in ["lib64", "lib"] {
            dirs.push(Path::new(prefix).join(libdir).join("libpinyin/data"));
        }
    }
    dirs
}

/// The first directory from [`system_data_dirs`] that holds libpinyin's
/// data, with the DBM that wrote it.
#[must_use]
pub fn find_system_data_dir() -> Option<(PathBuf, Dbm)> {
    system_data_dirs()
        .into_iter()
        .find_map(|dir| match detect(&dir) {
            Ok(DataLayout::Compat(dbm)) => Some((dir, dbm)),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::{DataLayout, Dbm, HEADER_BYTES, LayoutError, detect};

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

        /// A file carrying Berkeley DB's Hash magic at offset 12.
        fn write_berkeley(&self, name: &str) {
            let mut header = vec![0_u8; HEADER_BYTES];
            header[12..16].copy_from_slice(&super::DB_HASH_MAGIC.to_ne_bytes());
            self.write(name, &header);
        }

        /// A file carrying Kyoto Cabinet's magic and a type byte.
        fn write_kyoto(&self, name: &str, type_byte: u8) {
            let mut header = vec![0_u8; HEADER_BYTES];
            header[..4].copy_from_slice(&super::KC_MAGIC);
            header[super::KC_TYPE_AT] = type_byte;
            self.write(name, &header);
        }

        /// The two `.bin` markers, whose content the detector never reads.
        fn write_index_markers(&self) {
            self.write("phrase_index.bin", b"chunk");
            self.write("pinyin_index.bin", b"chunk");
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_kyoto_cabinet_bigram_is_recognised_under_libpinyins_own_filename() {
        // The point of the whole module: the file is called bigram.db, not
        // bigram.kch, because libpinyin's name is a compile-time constant
        // that does not vary with the DBM it was built against.
        let dir = TempDir::new("kc");
        dir.write_kyoto("bigram.db", super::KC_TYPE_HASH);
        dir.write_index_markers();
        assert_eq!(
            detect(&dir.0).expect("classify"),
            DataLayout::Compat(Dbm::KyotoHash),
        );
        assert!(
            !dir.0.join("bigram.kch").exists(),
            "there is no .kch file, and detection must not look for one"
        );
    }

    #[test]
    fn berkeley_and_kyoto_are_told_apart_under_the_same_filename() {
        // Opening one with the other's library is a failure the user sees
        // as a broken input method, so the detector has to separate them
        // from content alone.
        let bdb = TempDir::new("dbm-bdb");
        bdb.write_berkeley("bigram.db");
        bdb.write_index_markers();

        let kc = TempDir::new("dbm-kc");
        kc.write_kyoto("bigram.db", super::KC_TYPE_HASH);
        kc.write_index_markers();

        assert_eq!(
            detect(&bdb.0).expect("classify"),
            DataLayout::Compat(Dbm::BerkeleyHash),
        );
        assert_eq!(
            detect(&kc.0).expect("classify"),
            DataLayout::Compat(Dbm::KyotoHash),
        );
    }

    #[test]
    fn the_kyoto_type_byte_separates_hash_from_tree() {
        // Both share the KC\n\0 magic; offset 8 is what distinguishes
        // them, and libpinyin uses HashDB for the n-grams and TreeDB for
        // the phrase and chewing tables.
        let hash = TempDir::new("kc-hash");
        hash.write_kyoto("bigram.db", super::KC_TYPE_HASH);
        hash.write_index_markers();
        let tree = TempDir::new("kc-tree");
        tree.write_kyoto("bigram.db", super::KC_TYPE_TREE);
        tree.write_index_markers();
        assert_eq!(
            detect(&hash.0).expect("classify"),
            DataLayout::Compat(Dbm::KyotoHash),
        );
        assert_eq!(
            detect(&tree.0).expect("classify"),
            DataLayout::Compat(Dbm::KyotoTree),
        );
    }

    #[test]
    fn punct_bin_is_never_required() {
        // It is in the pin's data/Makefile.am but absent from the
        // installed 2.8.1 package. Requiring it would reject exactly the
        // installations the compat path exists for.
        let dir = TempDir::new("nopunct");
        dir.write_kyoto("bigram.db", super::KC_TYPE_HASH);
        dir.write_index_markers();
        assert!(!dir.0.join("punct.bin").exists(), "punct.bin is absent");
        assert!(matches!(
            detect(&dir.0).expect("classify without punct.bin"),
            DataLayout::Compat(_),
        ));
    }

    #[test]
    fn native_files_select_the_native_path() {
        let dir = TempDir::new("native");
        // The native probe looks for the compiled-in backend's filenames.
        dir.write(&crate::default_store_file("phrase_index"), b"data");
        dir.write(&crate::default_store_file("pinyin_index"), b"data");
        assert_eq!(detect(&dir.0).expect("classify"), DataLayout::Native);
    }

    #[test]
    fn both_layouts_present_takes_the_compat_path() {
        // The user installed oxpinyin beside a real libpinyin; reading
        // what they already have is the whole point of the drop-in.
        let dir = TempDir::new("both");
        dir.write_kyoto("bigram.db", super::KC_TYPE_HASH);
        dir.write_index_markers();
        dir.write("phrase_index", b"redb");
        dir.write("pinyin_index", b"redb");
        assert_eq!(
            detect(&dir.0).expect("classify"),
            DataLayout::Compat(Dbm::KyotoHash),
        );
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
        let dir = TempDir::new("partial");
        dir.write_kyoto("bigram.db", super::KC_TYPE_HASH);
        assert!(matches!(detect(&dir.0), Err(LayoutError::NoData { .. })));
    }

    #[test]
    fn an_unrecognised_bigram_is_refused_with_its_bytes() {
        // A tkrzw-built libpinyin puts a third format under this same
        // name. Refusing it by content beats mis-opening it.
        let dir = TempDir::new("unknown");
        dir.write("bigram.db", &[0x99_u8; 32]);
        dir.write_index_markers();
        let error = detect(&dir.0).expect_err("no magic matches");
        assert!(matches!(error, LayoutError::UnknownDbm { .. }));
        let message = error.to_string();
        assert!(
            message.contains("9999"),
            "the message must show the bytes it saw; got: {message}"
        );
        assert!(
            message.contains("tkrzw"),
            "and name the third backend that produces the same filename; got: {message}"
        );
    }

    #[test]
    fn the_installed_libpinyin_data_dir_classifies_with_its_real_dbm() {
        // The only case that matters: a directory the distro's own
        // libpinyin-data package created. Everything above builds its
        // fixture, so this is the one check that the marker set and the
        // magic offsets describe a real installation.
        let Some((dir, dbm)) = super::find_system_data_dir() else {
            let searched: Vec<String> = super::system_data_dirs()
                .iter()
                .map(|d| d.display().to_string())
                .collect();
            let message = format!(
                "SKIP: no installed libpinyin data directory among {}.",
                searched.join(", ")
            );
            assert!(
                std::env::var_os("OXPINYIN_KC_STRICT").is_none(),
                "{message} (OXPINYIN_KC_STRICT is set, so this skip is a failure)"
            );
            eprintln!("{message}");
            return;
        };
        assert_eq!(detect(&dir).expect("classify"), DataLayout::Compat(dbm));
        assert!(
            dir.to_string_lossy().contains("/lib"),
            "and it must have been found under $(libdir), not $datadir: {}",
            dir.display()
        );
        eprintln!("installed libpinyin data at {} is {dbm}", dir.display());
    }
}
