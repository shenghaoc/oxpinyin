//! The library_loading family's case bodies (#1175):
//! system libraries broken, replaced, unloaded and reloaded around init, load,
//! save and mask-out. Each answers the result document the old case returned.

mod breaks;
mod chunks;
mod items;
mod saves;
mod unloaded;

use std::ffi::c_char;
use std::path::Path;

use crate::facade;
use crate::fixtures::{chunk, log_walk, modify_header, write_log};
use crate::readers::c;
use crate::support::{J, Kit, P, UNTOUCHED, take_text};

/// Runs the case `id` and answers its result document.
pub fn case(k: &mut Kit, id: &str) -> J {
    let base = id
        .strip_suffix("-zhuyin")
        .or_else(|| id.strip_suffix("-pinyin"))
        .unwrap_or(id);
    match base {
        "stderr-library-init-missing" => breaks::library_probe(k, "init", "missing"),
        "stderr-library-init-empty" => breaks::library_probe(k, "init", "empty"),
        "stderr-library-load-empty" => breaks::library_probe(k, "load", "empty"),
        "stderr-library-load-checksum" => breaks::library_probe(k, "load", "checksum"),
        "stderr-library-save-short" => breaks::library_probe(k, "save", "short"),
        "stderr-library-save-missing" => breaks::library_probe(k, "save", "missing"),
        "stderr-library-mask-out-bad-length" => breaks::library_probe(k, "mask-out", "bad-length"),
        "stderr-library-mask-out-short" => breaks::library_probe(k, "mask-out", "short"),
        "stderr-library-addon-checksum" => breaks::library_probe(k, "addon", "checksum"),
        "library-directory-guard-reserved-save" => breaks::directory_guard(k, "save", "reserved"),
        "library-directory-guard-reserved-mask-out" => {
            breaks::directory_guard(k, "mask-out", "reserved")
        }
        "library-directory-guard-unloaded-save" => breaks::directory_guard(k, "save", "unloaded"),
        "library-directory-guard-unloaded-mask-out" => {
            breaks::directory_guard(k, "mask-out", "unloaded")
        }
        "crash-oob-layout" => breaks::crash_layout_init(k, "oob-layout"),
        "short-chunk-layout" => breaks::crash_layout_init(k, "short-chunk"),
        _ => match base.strip_prefix("stderr-library-control-") {
            Some(site) => breaks::library_control(k, site),
            None => chunk_or_later(k, base),
        },
    }
}

/// The chunk-fixture cases, then the rest.
fn chunk_or_later(k: &mut Kit, base: &str) -> J {
    match base {
        "sparse-library-init" => chunks::init_probe(k, "sparse"),
        "header-only-library-init" => chunks::init_probe(k, "header-only"),
        "second-separator-library-init" => chunks::init_probe(k, "second-separator"),
        "third-separator-library-init" => chunks::init_probe(k, "third-separator"),
        "sparse-library-load" => chunks::load_probe(k, "sparse"),
        "header-only-library-load" => chunks::load_probe(k, "header-only"),
        "sparse-library-save" => chunks::save_probe(k, "sparse"),
        "header-only-library-save" => chunks::save_probe(k, "header-only"),
        "sparse-library-mask-out" => chunks::mask_probe(k, "sparse"),
        "sparse-library-candidates" => chunks::candidates_probe(k),
        "sparse-library-addon" => chunks::addon_probe(k, "sparse", false),
        "sparse-library-addon-reload" => chunks::addon_probe(k, "sparse", true),
        "sparse-library-reinit" => chunks::reinit_probe(k),
        "sparse-library-tokens" => chunks::token_probe(k),
        "nonzero-total-candidates" => chunks::nonzero_total_candidates(k),
        "nonzero-total-save" => chunks::nonzero_total_save(k),
        "chunk-frame-system-exact" => chunks::frame_system(k, false),
        "chunk-frame-system-trailing" => chunks::frame_system(k, true),
        "save-replacement-valid-to-invalid" | "save-fatal-valid-to-invalid" => {
            saves::replacement_probe(k, "valid", "invalid")
        }
        "save-replacement-invalid-to-valid" => saves::replacement_probe(k, "invalid", "valid"),
        "save-replacement-valid-to-different-valid" => {
            saves::replacement_probe(k, "valid", "different")
        }
        "save-unchanged-valid" => saves::replacement_probe(k, "valid", "same"),
        "save-unchanged-layout-failed" => saves::replacement_probe(k, "invalid", "same"),
        "save-replacement-valid-to-different-valid-with-user-override" => {
            saves::changed_with_override(k)
        }
        "crash-save-missing-system" => saves::missing_system(k),
        "crash-save-remap-failed-retry" => saves::remap_retry(k),
        "save-unloaded-replacement" => saves::unloaded_replacement(k),
        "aliased-offset-unigram" => items::aliased_offset_unigram(k),
        "internally-overlapping-modify" => items::internally_overlapping_modify(k),
        "overlapping-modify-before-add" => items::overlapping_modify_before_add(k),
        "transient-reload-replacement" => items::transient_reload_replacement(k),
        "high-system-library" => items::high_system_library(k),
        "unloaded-cached-pronunciation" => unloaded::cached_pronunciation(k),
        other => match other.strip_prefix("unloaded-library-") {
            Some(variant) => unloaded::operation(k, variant),
            None => panic!("no library_loading case {other}"),
        },
    }
}

/// `LIBRARY_FACADES`' library file: library 4 on both facades.
const LIBRARY: &str = "merged.bin";

/// `SPARSE_CHUNK_SIZE`: 2^32 + 8.
const SPARSE_CHUNK_SIZE: u64 = (1 << 32) + 8;

/// A word as little-endian bytes.
fn le(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}

/// A half-word as little-endian bytes.
fn le16(value: usize) -> [u8; 2] {
    u16::try_from(value).expect("a u16").to_le_bytes()
}

/// The little-endian word at `at`.
fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("a word"))
}

/// A word as a byte offset.
fn at_offset(value: u32) -> usize {
    usize::try_from(value).expect("an offset")
}

/// Bytes as lowercase hex (Python's `bytes.hex`).
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Bytes' SHA-256, as lowercase hex.
fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    hex(&sha2::Sha256::digest(bytes))
}

/// The facade's `init` on `system` and the kit's user directory (`k.init(system=...)`).
fn init(k: &Kit, system: &Path) -> P {
    k.init(Some(system), Some(&k.user))
}

/// The facade's parse call of `LIBRARY_FACADES` on `text`.
fn parse(inst: P, text: &[u8]) -> usize {
    if facade::ZHUYIN {
        facade::parse_more_chewings(inst, c(text).as_ptr())
    } else {
        facade::parse_more_full_pinyins(inst, c(text).as_ptr())
    }
}

/// `train_once`: a fresh instance of `ctx` parses `LIBRARY_FACADES`' text,
/// guesses and trains (rank 0 on pinyin), answering the training's flag.
fn train_once(ctx: P) -> bool {
    let inst = facade::alloc_instance(ctx);
    parse(inst, if facade::ZHUYIN { b"su3cl3" } else { b"nihao" });
    facade::guess_sentence(inst);
    facade::train_top(inst)
}

/// The cases' `mask_out(ctx, 0xff000000, 0x04000000)`: library 4's tokens.
fn mask_four(ctx: P) -> bool {
    facade::mask_out(ctx, 0xff00_0000, 0x0400_0000)
}

/// A user file's bytes as hex, `None` when absent (the cases' `journals`).
fn journal(k: &Kit, name: &str) -> Option<String> {
    let path = k.user.join(name);
    path.exists()
        .then(|| hex(&std::fs::read(&path).expect("the user log")))
}

/// Replaces `path` by a `.new` sibling and a rename (`os.replace`).
fn replace_file(path: &Path, data: &[u8]) {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".new");
    std::fs::write(&tmp, data).expect("write the replacement");
    std::fs::rename(&tmp, path).expect("rename the replacement");
}

/// A symbolic link at `link` to `target` ([`crate::fixtures::link_file`]).
fn link(target: &Path, link: &Path) {
    crate::fixtures::link_file(target, link);
}

/// `token_phrase_of`: `[ret, length, text]` of `token_get_phrase`.
fn token_phrase(inst: P, token: u32) -> J {
    let mut length = 0_u32;
    let mut text: *mut c_char = std::ptr::null_mut();
    let ret = facade::token_get_phrase(inst, token, &raw mut length, &raw mut text);
    J::from((ret, length, take_text(text)))
}

/// `unigram_of` on `inst`: `[ret, freq]`, the frequency seeded UNTOUCHED.
fn unigram(inst: P, token: u32) -> (bool, u32) {
    let mut freq = UNTOUCHED;
    let ret = facade::token_get_unigram_frequency(inst, token, &raw mut freq);
    (ret, freq)
}

/// `chunk_limits`: 12 GiB of address space, 60 CPU seconds and no core dump for
/// this case's child. The pin's init maps 2^32+8 and its save, mask and reinit
/// map it again while the earlier mapping is held, so 12 GiB admits the pin's
/// init, one re-map and the retained mapping; the CPU bound and dumpable=0 keep
/// a surprise inside the child.
fn chunk_limits() {
    // macOS does not enforce RLIMIT_AS, and Windows has no rlimit.
    #[cfg(target_os = "linux")]
    {
        let cpu = libc::rlimit {
            rlim_cur: 60,
            rlim_max: 60,
        };
        let space = libc::rlimit {
            rlim_cur: 12 << 30,
            rlim_max: 12 << 30,
        };
        // SAFETY: plain system calls on this process with valid limit structs.
        unsafe {
            assert_eq!(libc::setrlimit(libc::RLIMIT_CPU, &raw const cpu), 0);
            assert_eq!(libc::setrlimit(libc::RLIMIT_AS, &raw const space), 0);
            assert_eq!(
                libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0),
                0,
                "PR_SET_DUMPABLE failed"
            );
        }
    }
}

/// `chunk_stats`: the fixture's logical and allocated sizes, in bytes.
fn chunk_stats(out: &mut J, path: &Path) {
    let meta = std::fs::metadata(path).expect("the fixture");
    out.set("fixture_size", meta.len());
    out.set("fixture_allocated", allocated(path, &meta));
}

/// The bytes a file holds on disk: `st_blocks` in 512-byte units on Unix; on
/// Windows the size `GetCompressedFileSizeW` reports, the allocated ranges of a
/// sparse file (the actual size of a file that is neither sparse nor compressed).
fn allocated(path: &Path, meta: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        let _ = path;
        std::os::unix::fs::MetadataExt::blocks(meta) * 512
    }
    #[cfg(windows)]
    {
        let _ = meta;
        win32::compressed_size(path)
    }
}

/// `sparse_chunk`: the 2^32+8 chunk `MemoryChunk::mmap` truncates to a zero
/// payload. `guint32 data_len = file_size - header` (`memory_chunk.h:499`) turns
/// 2^32 into 0, so the length word and the checksum word both read 0 and the pin
/// accepts the all-zero payload. Only the 8-byte header is written: the rest is
/// a hole (on NTFS the file is marked sparse first, `FSCTL_SET_SPARSE`), so the
/// fixture is 4 GiB logically and one block physically.
fn sparse_chunk(path: &Path) {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("create the sparse chunk");
    #[cfg(windows)]
    win32::set_sparse(&file);
    // Length word 0, checksum 0.
    file.write_all(&[0; 8]).expect("the chunk header");
    file.set_len(SPARSE_CHUNK_SIZE)
        .expect("extend the sparse chunk");
    drop(file);
    let meta = std::fs::metadata(path).expect("the sparse chunk");
    assert_eq!(meta.len(), SPARSE_CHUNK_SIZE);
    assert!(
        allocated(path, &meta) <= 1 << 20,
        "the fixture materialised"
    );
}

/// The two Win32 calls the sparse fixture needs, declared here: the test
/// targets take no Windows binding crate.
#[cfg(windows)]
mod win32 {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt as _;
    use std::os::windows::io::AsRawHandle as _;
    use std::path::Path;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn DeviceIoControl(
            device: *mut c_void,
            control_code: u32,
            in_buffer: *const c_void,
            in_size: u32,
            out_buffer: *mut c_void,
            out_size: u32,
            returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn GetCompressedFileSizeW(file_name: *const u16, size_high: *mut u32) -> u32;
    }

    /// `FSCTL_SET_SPARSE` (winioctl.h): `CTL_CODE(FILE_DEVICE_FILE_SYSTEM, 49,
    /// METHOD_BUFFERED, FILE_SPECIAL_ACCESS)`.
    const FSCTL_SET_SPARSE: u32 = 0x0009_00C4;
    /// `GetCompressedFileSizeW`'s failure value for the low word.
    const INVALID_FILE_SIZE: u32 = 0xFFFF_FFFF;

    /// Marks `file` sparse, so that extending it past the written bytes leaves a
    /// hole rather than allocating them.
    pub fn set_sparse(file: &std::fs::File) {
        let mut returned = 0_u32;
        // SAFETY: an open file handle, no buffers, and a live out-parameter.
        let ok = unsafe {
            DeviceIoControl(
                file.as_raw_handle(),
                FSCTL_SET_SPARSE,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                &raw mut returned,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(
            ok,
            0,
            "FSCTL_SET_SPARSE failed: {}",
            std::io::Error::last_os_error()
        );
    }

    /// The bytes `path` holds on disk: the allocated ranges of a sparse file, or
    /// the actual size of a file that is neither sparse nor compressed.
    pub fn compressed_size(path: &Path) -> u64 {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut high = 0_u32;
        // SAFETY: a NUL-terminated wide path and a live out-parameter.
        let low = unsafe { GetCompressedFileSizeW(wide.as_ptr(), &raw mut high) };
        if low == INVALID_FILE_SIZE {
            let error = std::io::Error::last_os_error();
            assert_eq!(
                error.raw_os_error(),
                Some(0),
                "GetCompressedFileSizeW failed: {error}"
            );
        }
        (u64::from(high) << 32) | u64::from(low)
    }
}

/// `nonzero_layout_chunk`: a chunk with total 123 and an invalid second
/// sub-index separator, in a valid length/checksum header. The pin reads 123
/// into the sub-index's local total and does not add it to the facade
/// aggregate (`phrase_index.cpp:237-248`); the offsets follow
/// `SubPhraseIndex::load` (`phrase_index.cpp`), whose total is read before the
/// separator check.
fn nonzero_layout_chunk() -> Vec<u8> {
    let mut payload = [0_u8; 32];
    payload[0..4].copy_from_slice(&le(123));
    payload[4..8].copy_from_slice(&le(17));
    payload[8..12].copy_from_slice(&le(20));
    payload[12..16].copy_from_slice(&le(24));
    payload[16] = b'#';
    payload[19] = b'!';
    chunk(&payload)
}
