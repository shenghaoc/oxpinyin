//! The pin's process-locale side effect and table.conf λ reader.
//!
//! Compiled by both C facades from this one source (register row 39, #539).
//!
//! `SystemTableInfo2::load`, `UserTableInfo::load` and `UserTableInfo::save`
//! each open with `char * locale = setlocale(LC_NUMERIC, "C");` and close
//! with `setlocale(LC_NUMERIC, locale);` (`storage/table_info.cpp:197,291`,
//! `:328,372` and `:378,394` at `074a2219`). `setlocale` given a locale
//! name answers the name of the locale it has just installed, so `locale`
//! holds `"C"` and the closing call installs `"C"` a second time; every
//! early return (`:199-202`, `:207-224`, `:330-348`, `:380-383`) skips the
//! closing call with the same result. Each of the three methods therefore
//! leaves the process's `LC_NUMERIC` at `"C"`, on success and on failure
//! alike, and the entry points that reach them inherit the side effect:
//!
//! - `pinyin_init`: `load` on `table.conf` (`pinyin.cpp:337`) before any
//!   other step can fail, then `check_format`'s `load` and `save` of
//!   `user.conf` (`:178`, `:187`);
//! - `pinyin_save`: `mark_version`'s `save` (`:1143`), reached only past
//!   the two guards (`:1133-1137`: a user dir and `m_modified`);
//! - `pinyin_fini`: `mark_version`'s `save` (`:1200`), unconditionally.
//!
//! The zhuyin facade also loads `table.conf` first (`zhuyin.cpp:281`),
//! then only loads `user.conf` (`:132`); `zhuyin_save` writes it past the
//! user-dir and modified guards (`:548-552`, `:695`). `zhuyin_fini`
//! writes no marker (`:741-757`).
//!
//! A consumer that adopted a `zh_CN.UTF-8` environment with
//! `setlocale(LC_ALL, "")` finds `LC_NUMERIC` reset to `"C"` after any of
//! them. The ruling (`docs/findings/upstream-divergences.md` row 39) is to
//! reproduce it bug-for-bug, so each entry point above installs the same
//! locale at the same point. Only the C library's own `setlocale` can do
//! that, which is why this lives in the C-ABI crate and not in the
//! storage crates the pin's `table_info.cpp` corresponds to.

use std::os::raw::{c_char, c_int};

/// `LC_NUMERIC`'s value in `<locale.h>`: 1 under glibc and musl, 4 on the
/// BSD-derived libcs and MSVCRT.
#[cfg(any(target_os = "linux", target_os = "android"))]
const LC_NUMERIC: c_int = 1;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const LC_NUMERIC: c_int = 4;

unsafe extern "C" {
    fn sscanf(input: *const c_char, format: *const c_char, ...) -> c_int;
    fn setlocale(category: c_int, locale: *const c_char) -> *mut c_char;
}

/// Installs `"C"` as the process's `LC_NUMERIC`, as one call of the pin's
/// `SystemTableInfo2::load` / `UserTableInfo::load` / `UserTableInfo::save`
/// leaves it (`table_info.cpp:197`, `:328`, `:378` with their `:291`,
/// `:372`, `:394` re-installs of the same name).
pub(crate) fn pin_table_info_locale() {
    // SAFETY: `setlocale` is the C library's; `LC_NUMERIC` is a category
    // it defines and the locale name is a NUL-terminated string literal
    // that outlives the call. The returned pointer refers to the C
    // library's own storage and is not kept.
    unsafe {
        setlocale(LC_NUMERIC, c"C".as_ptr());
    }
}

/// Reads the pin's `%f\n` field using the same host libc, after the caller
/// installs `LC_NUMERIC="C"`. The count keeps the following header at the
/// position `fscanf` leaves it (`074a2219 src/storage/table_info.cpp:220`).
///
/// Both scanners use `__vfscanf_internal` (glibc 2.41,
/// `stdio-common/sscanf.c:29-42`, `isoc99_fscanf.c:30`). The final whitespace
/// directive consumes whitespace and pushes back the next byte
/// (`vfscanf-internal.c:3137-3144`; glibc 2.44:3214-3221). Preserve the
/// actual following bytes, rather than ending the string at the number.
pub(crate) fn read_lambda(input: &[u8]) -> Option<(f32, usize)> {
    // A FILE can contain NUL, but it cannot be part of a float or whitespace.
    // Leave that byte for the next header, just as fscanf's pushback does.
    let field = input.split(|byte| *byte == 0).next()?;
    let mut field = field.to_vec();
    field.push(0);
    let mut lambda = 0.0_f32;
    let mut consumed: isize = 0;
    // SAFETY: input and format are NUL-terminated and live through the call.
    // The variadic arguments match `%f` (a writable float/f32 pointer) and
    // `%tn` (a writable ptrdiff_t/isize pointer). A Rust slice length fits
    // isize, so the consumed count cannot overflow. The count directive
    // does not add an assignment to the return count; only `%f` does.
    let assigned = unsafe {
        sscanf(
            field.as_ptr().cast(),
            c"%f\n%tn".as_ptr(),
            &mut lambda as *mut f32,
            &mut consumed as *mut isize,
        )
    };
    if assigned != 1 {
        return None;
    }
    Some((lambda, usize::try_from(consumed).ok()?))
}

#[cfg(test)]
mod tests {
    use super::{pin_table_info_locale, read_lambda};
    use oxpinyin_data::table_info::{TableConf, TableConfError};

    /// Parses a `table.conf` whose lambda field is `text` and checks the
    /// outcome against what the host's own `read_lambda` result implies: no
    /// conversion is a header error; otherwise the header scan resumes at the
    /// consumed count, so the parse succeeds with the same `f32` bits only if
    /// the next line begins the source format, and fails with
    /// `UninitialisedSourceFormat` if the libc stopped inside the field.
    /// Returns whether the parse succeeded.
    fn check_against_host(text: &str) -> bool {
        let tail = format!("{text}\nsource table format:pinyin\ndatabase format:BerkeleyDB\n");
        let bytes =
            format!("binary format version:7\nmodel data version:14\nlambda parameter:{tail}");
        let actual = TableConf::parse(bytes.as_bytes(), read_lambda);
        let Some((value, consumed)) = read_lambda(tail.as_bytes()) else {
            assert_eq!(actual.unwrap_err(), TableConfError::Header, "{text}");
            return false;
        };
        if tail.as_bytes()[consumed..].starts_with(b"source table format:") {
            assert_eq!(
                actual.expect(text).lambda_f32().to_bits(),
                value.to_bits(),
                "{text}"
            );
            true
        } else {
            assert_eq!(
                actual.unwrap_err(),
                TableConfError::UninitialisedSourceFormat,
                "{text}"
            );
            false
        }
    }

    #[test]
    fn lambda_is_read_as_a_float_as_written() {
        pin_table_info_locale();
        // Forms every C library parses the same way.
        for text in [
            "0.5", "2", "-0.5", "5e-1", "0x1p-1", "0x.8", "0X1.P-1", "inf", "infinity", "nan",
        ] {
            assert!(check_against_host(text), "{text}");
        }
    }

    /// Malformed numbers (an exponent or hex prefix with no digits) and
    /// `nan(` forms are accepted, refused or cut short by each C library on
    /// its own terms, including how much input it consumes. This is a
    /// pass-through check: the field reaches the host `sscanf` intact and the
    /// consumed length is kept. It runs only where the host is glibc.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    #[test]
    fn malformed_lambda_follows_the_host_libc() {
        pin_table_info_locale();
        for text in [
            "1e", "1e+", "0x1p", "0x1p+", "0x1p-", "0x.", "nan(", "nan(1)",
        ] {
            check_against_host(text);
        }
    }

    #[test]
    fn lambda_reader_keeps_the_following_header_position() {
        pin_table_info_locale();
        for (bytes, consumed) in [
            (b" 0x1p-1\n \tsource".as_slice(), 10),
            (b"0.5source", 3),
            (b"0.5\0source", 3),
            (b"0.5", 3),
        ] {
            let (value, actual) = read_lambda(bytes).expect("conversion");
            assert_eq!(value, 0.5);
            assert_eq!(actual, consumed);
        }
        let prefix = "binary format version:7\nmodel data version:14\nlambda parameter:";
        let source = "source table format:pinyin\ndatabase format:BerkeleyDB\n";
        for field in ["0.5", "\n 0.5\n\t", "0x1p-1 "] {
            let conf = format!("{prefix}{field}{source}");
            assert_eq!(
                TableConf::parse(conf.as_bytes(), read_lambda)
                    .expect("header follows scanner")
                    .lambda_f32(),
                0.5
            );
        }
        let conf = format!("{prefix}0.5junk\n{source}");
        assert_eq!(
            TableConf::parse(conf.as_bytes(), read_lambda).unwrap_err(),
            TableConfError::UninitialisedSourceFormat
        );
    }
}
