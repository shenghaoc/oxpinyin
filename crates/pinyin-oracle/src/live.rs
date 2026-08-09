//! Safe wrapper driving the live pin-built libpinyin.
//!
//! Compiled only with the `oracle-ffi` feature. Every raw pointer obtained from
//! libpinyin is consumed inside the method that obtained it: borrowed strings
//! are copied into owned `String`s before returning, and no handle escapes.
//! An instance cannot outlive its context because [`Session`] borrows the
//! [`Oracle`], so the ordering is a compile-time property rather than a review
//! rule.
//!
//! The observation protocol mirrors `tools/capture/capture.c`, which produced
//! the frozen fixtures: fresh user directory, learning untouched,
//! `DYNAMIC_ADJUST` rejected, candidates capped at 10 with the uncapped total
//! retained.

use core::ffi::{CStr, c_uint};
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::flags::SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY;
use crate::observation::{
    MAX_CAPTURED_CANDIDATES, OracleCompleteness, OracleObservation, OracleSegment,
};
use crate::{OracleError, OracleFlags, OraclePrefix, ffi};

/// A fresh, empty user directory removed when dropped.
///
/// The parity protocol requires the oracle to start with no user state. Reusing
/// a directory across runs would let an earlier run's state leak into a later
/// one and silently break determinism.
#[derive(Debug)]
struct FreshUserDir {
    path: PathBuf,
}

impl FreshUserDir {
    fn create(parent: &Path) -> Result<Self, OracleError> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let unique = format!(
            "pinyin-oracle-user-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = parent.join(unique);

        // Remove any stale directory of the same name before creating, so a
        // crashed previous run cannot contribute state.
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).map_err(|source| OracleError::ManifestUnreadable {
            path: path.clone(),
            source,
        })?;

        Ok(Self { path })
    }
}

impl Drop for FreshUserDir {
    fn drop(&mut self) {
        // Best effort: a leftover temp directory is untidy, not incorrect, and
        // Drop must not panic.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A live oracle context over a pin-verified prefix.
///
/// Holding this value implies the prefix matched the frozen pin: [`Oracle::open`]
/// takes an [`OraclePrefix`], which can only be built by a successful
/// verification.
#[derive(Debug)]
pub struct Oracle {
    prefix: OraclePrefix,
    context: *mut ffi::PinyinContext,
    user_dir: FreshUserDir,
    flags: Option<OracleFlags>,
}

impl Oracle {
    /// Opens a context over `prefix`, with a fresh user directory under
    /// `user_dir_parent`.
    ///
    /// # Errors
    ///
    /// Returns an error if the user directory cannot be created, if either path
    /// cannot cross the C boundary, or if `pinyin_init` returns NULL.
    pub fn open(prefix: OraclePrefix, user_dir_parent: &Path) -> Result<Self, OracleError> {
        let user_dir = FreshUserDir::create(user_dir_parent)?;
        let system_c = path_to_cstring(prefix.data_dir())?;
        let user_c = path_to_cstring(&user_dir.path)?;

        // SAFETY: both pointers come from live `CString`s that outlive this
        // call, and are valid NUL-terminated C strings. `pinyin_init` either
        // returns an owned context or NULL; we check for NULL below and take
        // ownership, releasing it in `Drop` via `pinyin_fini` exactly once.
        let context = unsafe { ffi::pinyin_init(system_c.as_ptr(), user_c.as_ptr()) };

        if context.is_null() {
            return Err(OracleError::ContextInitFailed {
                system_dir: prefix.data_dir().to_path_buf(),
                user_dir: user_dir.path.clone(),
            });
        }

        Ok(Self {
            prefix,
            context,
            user_dir,
            flags: None,
        })
    }

    /// Opens a context using the system temporary directory for user state.
    ///
    /// # Errors
    ///
    /// As [`Oracle::open`].
    pub fn open_with_temp_user_dir(prefix: OraclePrefix) -> Result<Self, OracleError> {
        let parent = std::env::temp_dir();
        Self::open(prefix, &parent)
    }

    /// The verified prefix backing this context.
    #[must_use]
    pub fn prefix(&self) -> &OraclePrefix {
        &self.prefix
    }

    /// The pin reference stamped into observations from this context.
    #[must_use]
    pub fn pin_ref(&self) -> &str {
        self.prefix.pin().pin_ref()
    }

    /// The fresh user-state directory this context was opened with.
    ///
    /// Exposed so a run can record, or assert the emptiness of, the user state
    /// the parity protocol requires. The directory is removed when this
    /// [`Oracle`] is dropped.
    #[must_use]
    pub fn user_dir(&self) -> &Path {
        &self.user_dir.path
    }

    /// Applies `flags` to the context if they are not already in force.
    fn apply_flags(&mut self, flags: OracleFlags) -> Result<(), OracleError> {
        if self.flags == Some(flags) {
            return Ok(());
        }

        // SAFETY: `self.context` is non-null and owned by `self` for the whole
        // borrow. `flags.bits()` is a plain `u32`; `OracleFlags` already
        // guarantees the DYNAMIC_ADJUST bit is clear, so the parity protocol
        // cannot be violated through this path.
        let ok = unsafe { ffi::pinyin_set_options(self.context, flags.bits()) };

        if !ok {
            return Err(OracleError::Call {
                function: "pinyin_set_options",
            });
        }
        self.flags = Some(flags);
        Ok(())
    }

    /// Observes one input with a freshly allocated instance.
    ///
    /// This is the frozen protocol: a new instance per observation, exactly as
    /// `tools/capture/capture.c` does for each fixture case.
    ///
    /// # Errors
    ///
    /// Returns an error if the input carries an interior NUL, if instance
    /// allocation fails, if a libpinyin call reports failure, or if the oracle
    /// reports a parsed length beyond the input.
    pub fn observe(
        &mut self,
        input: &[u8],
        flags: OracleFlags,
    ) -> Result<OracleObservation, OracleError> {
        let mut session = self.session(flags)?;
        session.observe_without_reset(input)
    }

    /// Opens a reusable session that clears state with `pinyin_reset`.
    ///
    /// Cheaper than a fresh instance per input for corpus-scale runs. The
    /// determinism test asserts it agrees with [`Oracle::observe`] exactly.
    ///
    /// # Errors
    ///
    /// Returns an error if the flags cannot be applied or the instance cannot be
    /// allocated.
    pub fn session(&mut self, flags: OracleFlags) -> Result<Session<'_>, OracleError> {
        self.apply_flags(flags)?;

        // SAFETY: `self.context` is non-null and stays owned by `self` for the
        // lifetime of the returned `Session`, which borrows `self` mutably.
        let instance = unsafe { ffi::pinyin_alloc_instance(self.context) };

        if instance.is_null() {
            return Err(OracleError::InstanceAllocFailed);
        }

        Ok(Session {
            instance,
            flags,
            pin_ref: self.prefix.pin().pin_ref().to_owned(),
            _oracle: core::marker::PhantomData,
        })
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        // SAFETY: `self.context` was returned non-null by `pinyin_init`, has not
        // been freed (nothing else calls `pinyin_fini`), and every `Session`
        // borrowed `self` mutably, so all instances are already freed. Runs at
        // most once because `drop` is called once for a non-`Copy` value.
        unsafe { ffi::pinyin_fini(self.context) };
        // `user_dir` is dropped after this, removing user state the oracle may
        // have written during `pinyin_fini`.
    }
}

/// One instance over an [`Oracle`], reset between inputs.
///
/// The lifetime ties the instance to its context: the borrow checker rejects
/// dropping the `Oracle` while a `Session` is alive, so `pinyin_free_instance`
/// always precedes `pinyin_fini`.
#[derive(Debug)]
pub struct Session<'oracle> {
    instance: *mut ffi::PinyinInstance,
    flags: OracleFlags,
    pin_ref: String,
    _oracle: core::marker::PhantomData<&'oracle mut ()>,
}

impl Session<'_> {
    /// Clears parse and candidate state, keeping the instance allocated.
    ///
    /// # Errors
    ///
    /// Returns an error if `pinyin_reset` reports failure.
    pub fn reset(&mut self) -> Result<(), OracleError> {
        // SAFETY: `self.instance` was returned non-null by
        // `pinyin_alloc_instance` and is owned by `self` until `Drop`. Reset
        // invalidates pointers previously borrowed from this instance; none are
        // held across this call because every borrow is confined to the method
        // body that obtained it.
        let ok = unsafe { ffi::pinyin_reset(self.instance) };

        if ok {
            Ok(())
        } else {
            Err(OracleError::Call {
                function: "pinyin_reset",
            })
        }
    }

    /// Resets, then observes `input`.
    ///
    /// # Errors
    ///
    /// Returns an error if reset fails, if `input` carries an interior NUL, if a
    /// libpinyin call reports failure, or if the oracle's reported parsed length
    /// exceeds the input length.
    pub fn observe(&mut self, input: &[u8]) -> Result<OracleObservation, OracleError> {
        self.reset()?;
        self.observe_without_reset(input)
    }

    fn observe_without_reset(&mut self, input: &[u8]) -> Result<OracleObservation, OracleError> {
        let input_c = CString::new(input).map_err(|_| OracleError::InteriorNul {
            what: "oracle input",
        })?;

        // SAFETY: `input_c` is a valid NUL-terminated C string that outlives the
        // call. The instance is non-null and owned by `self`. The return value
        // is a plain `size_t` count, not a pointer.
        let parse_return =
            unsafe { ffi::pinyin_parse_more_full_pinyins(self.instance, input_c.as_ptr()) };

        // SAFETY: same instance validity as above; returns a plain `size_t`.
        let parsed_input_length = unsafe { ffi::pinyin_get_parsed_input_length(self.instance) };

        if parsed_input_length > input.len() {
            return Err(OracleError::ParsedLengthOutOfRange {
                parsed: parsed_input_length,
                input_len: input.len(),
            });
        }

        let (candidate_total, candidates) = if parsed_input_length > 0 {
            self.collect_candidates()?
        } else {
            (0, Vec::new())
        };

        let segments = self.collect_segments(parsed_input_length)?;

        Ok(OracleObservation {
            pin_ref: self.pin_ref.clone(),
            family: None,
            case: None,
            input: input.to_vec(),
            flags: self.flags,
            parse_return,
            parsed_input_length,
            segments,
            candidate_total,
            candidates,
            remainder: input[parsed_input_length..].to_vec(),
        })
    }

    /// Builds the candidate list and copies the first
    /// [`MAX_CAPTURED_CANDIDATES`] strings.
    fn collect_candidates(&mut self) -> Result<(u32, Vec<String>), OracleError> {
        // SAFETY: instance is non-null and owned by `self`. A false return means
        // no candidate list was built, which is a normal outcome rather than an
        // error, so we report zero candidates instead of failing.
        let built = unsafe {
            ffi::pinyin_guess_candidates(
                self.instance,
                0,
                SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY as c_uint,
            )
        };

        if !built {
            return Ok((0, Vec::new()));
        }

        let mut total: c_uint = 0;
        // SAFETY: the out-pointer is a valid, aligned, writable pointer to an
        // initialised `c_uint` that outlives the call.
        let ok = unsafe { ffi::pinyin_get_n_candidate(self.instance, &raw mut total) };

        if !ok {
            return Err(OracleError::Call {
                function: "pinyin_get_n_candidate",
            });
        }

        let capped = usize::try_from(total)
            .unwrap_or(usize::MAX)
            .min(MAX_CAPTURED_CANDIDATES);
        let mut candidates = Vec::with_capacity(capped);

        for index in 0..capped {
            let index_c = c_uint::try_from(index).map_err(|_| OracleError::Call {
                function: "pinyin_get_candidate",
            })?;
            let mut candidate: *mut ffi::LookupCandidate = core::ptr::null_mut();

            // SAFETY: the out-pointer is valid and writable, and `index_c` is
            // below the count the oracle just reported. On success libpinyin
            // writes a pointer borrowed from the instance, which we never free.
            let ok =
                unsafe { ffi::pinyin_get_candidate(self.instance, index_c, &raw mut candidate) };

            if !ok || candidate.is_null() {
                return Err(OracleError::Call {
                    function: "pinyin_get_candidate",
                });
            }

            let mut text: *const core::ffi::c_char = core::ptr::null();
            // SAFETY: `candidate` is non-null and was just borrowed from this
            // instance, which has not been mutated since. The out-pointer is
            // valid and writable.
            let ok = unsafe {
                ffi::pinyin_get_candidate_string(self.instance, candidate, &raw mut text)
            };

            if !ok || text.is_null() {
                return Err(OracleError::Call {
                    function: "pinyin_get_candidate_string",
                });
            }

            // SAFETY: `text` is a non-null NUL-terminated string owned by the
            // instance and valid until the next mutating call on it. We copy it
            // immediately and never free it, matching the ownership table in
            // docs/findings/oracle-ffi-seam.md.
            let borrowed = unsafe { CStr::from_ptr(text) };

            let owned = borrowed
                .to_str()
                .map_err(|_| OracleError::NonUtf8 {
                    function: "pinyin_get_candidate_string",
                })?
                .to_owned();
            candidates.push(owned);
        }

        Ok((total, candidates))
    }

    /// Walks the parsed prefix, mirroring the traversal in
    /// `tools/capture/capture.c`.
    ///
    /// Offsets advance to each segment's end, or by one byte when the oracle
    /// yields nothing usable, which is what makes the walk terminate on the
    /// F-E-01 shape instead of spinning.
    fn collect_segments(&mut self, parsed_len: usize) -> Result<Vec<OracleSegment>, OracleError> {
        let mut segments = Vec::new();
        let mut offset = 0_usize;

        while offset < parsed_len {
            let mut key: *mut ffi::ChewingKey = core::ptr::null_mut();
            // SAFETY: the out-pointer is valid and writable, and `offset` is
            // below the oracle's own reported parsed length. Any returned
            // pointer is borrowed from the instance and never freed here.
            let has_key =
                unsafe { ffi::pinyin_get_pinyin_key(self.instance, offset, &raw mut key) };

            if !has_key || key.is_null() {
                offset += 1;
                continue;
            }

            let mut rest: *mut ffi::ChewingKeyRest = core::ptr::null_mut();
            // SAFETY: as above; `rest` is borrowed from the instance.
            let has_rest =
                unsafe { ffi::pinyin_get_pinyin_key_rest(self.instance, offset, &raw mut rest) };

            if !has_rest || rest.is_null() {
                // Catalogue row F-E-01: a key with no key-rest.
                segments.push(OracleSegment::MissingKeyRest { offset });
                offset += 1;
                continue;
            }

            let mut begin: u16 = 0;
            let mut end: u16 = 0;
            // SAFETY: `rest` is non-null and borrowed from this unmutated
            // instance; both out-pointers are valid, aligned and writable.
            let has_positions = unsafe {
                ffi::pinyin_get_pinyin_key_rest_positions(
                    self.instance,
                    rest,
                    &raw mut begin,
                    &raw mut end,
                )
            };

            if !has_positions {
                segments.push(OracleSegment::MissingPosition { offset });
                offset += 1;
                continue;
            }

            let mut text: *mut core::ffi::c_char = core::ptr::null_mut();
            // SAFETY: `key` is non-null and borrowed from this unmutated
            // instance; the out-pointer is valid and writable. On success
            // libpinyin transfers ownership of a GLib allocation, freed below.
            let has_text =
                unsafe { ffi::pinyin_get_pinyin_string(self.instance, key, &raw mut text) };

            if !has_text || text.is_null() {
                segments.push(OracleSegment::MissingPinyinString { offset });
                offset += 1;
                continue;
            }

            let syllable = {
                // SAFETY: `text` is a non-null NUL-terminated GLib string we now
                // own. Copied before freeing; the pointer is not used after.
                let borrowed = unsafe { CStr::from_ptr(text) };
                let copied = borrowed.to_str().map(str::to_owned);
                // SAFETY: `text` came from `pinyin_get_pinyin_string`, which
                // transfers ownership of a GLib allocation. Freed exactly once
                // here, and never touched afterwards.
                unsafe { ffi::g_free(text.cast()) };
                copied.map_err(|_| OracleError::NonUtf8 {
                    function: "pinyin_get_pinyin_string",
                })?
            };

            // SAFETY: `key` is still the pointer borrowed above from this
            // unmutated instance. Returns C `bool`.
            let incomplete = unsafe { ffi::pinyin_get_pinyin_is_incomplete(self.instance, key) };

            segments.push(OracleSegment::Syllable {
                syllable,
                begin,
                end,
                completeness: if incomplete {
                    OracleCompleteness::Partial
                } else {
                    OracleCompleteness::Complete
                },
            });

            offset = if usize::from(end) > offset {
                usize::from(end)
            } else {
                offset + 1
            };
        }

        Ok(segments)
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        // SAFETY: `self.instance` was returned non-null by
        // `pinyin_alloc_instance`, is owned solely by `self`, and is freed
        // exactly once here. The borrow on the `Oracle` guarantees the context
        // outlives this call.
        unsafe { ffi::pinyin_free_instance(self.instance) };
    }
}

fn path_to_cstring(path: &Path) -> Result<CString, OracleError> {
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    };
    #[cfg(not(unix))]
    let bytes = path
        .to_str()
        .ok_or_else(|| OracleError::PathNotRepresentable {
            path: path.to_path_buf(),
        })?
        .as_bytes()
        .to_vec();

    CString::new(bytes).map_err(|_| OracleError::PathNotRepresentable {
        path: path.to_path_buf(),
    })
}
