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
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::differential::{ObservationSource, Source as DiffSource};
use crate::flags::SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY;
use crate::observation::{
    CandidateInfo, MAX_CAPTURED_CANDIDATES, OracleCandidateType, OracleCompleteness,
    OracleObservation, OracleSegment,
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

/// Serialises oracle contexts across the whole process.
///
/// The pinned libpinyin does not tolerate two live contexts being used
/// concurrently in one process: doing so trips
/// `pinyin.cpp:_check_offset: Assertion 'zero_key != key' failed`, which is an
/// `abort()` and therefore unrecoverable. Measured directly — the full corpus
/// run is stable single-threaded and aborts when several test threads each open
/// a context.
///
/// Holding this lock for an [`Oracle`]'s whole lifetime makes concurrent use
/// impossible by construction, rather than a rule callers have to remember or a
/// `--test-threads=1` incantation that a plain `cargo test` would skip.
static ORACLE_LOCK: Mutex<()> = Mutex::new(());

/// A live oracle context over a pin-verified prefix.
///
/// Holding this value implies the prefix matched the frozen pin: [`Oracle::open`]
/// takes an [`OraclePrefix`], which can only be built by a successful
/// verification.
///
/// At most one `Oracle` exists per process at a time; see [`ORACLE_LOCK`].
/// Constructing a second one blocks until the first is dropped.
#[derive(Debug)]
pub struct Oracle {
    prefix: OraclePrefix,
    context: *mut ffi::PinyinContext,
    user_dir: FreshUserDir,
    flags: Option<OracleFlags>,
    /// Released on drop, after `pinyin_fini`.
    _lock: MutexGuard<'static, ()>,
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
        // Poisoning only means an earlier holder panicked. The C library's state
        // is per-context and this context has not been created yet, so taking
        // the lock anyway is correct and avoids one panic disabling the harness.
        let lock = ORACLE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);

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
            _lock: lock,
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

/// The W14 sentence-surface capture for one input.
///
/// `sentences[i]` is `pinyin_get_sentence(i)`; `candidates` carries the
/// per-candidate `(text, type, nbest)` the guess produced, in list order
/// with the n-best rows prepended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SentenceSurface {
    /// Whether `pinyin_guess_sentence` reported success.
    pub guessed: bool,
    /// Decoded sentences for indices `0..=max_index`, `None` past the end.
    pub sentences: Vec<Option<String>>,
    /// Uncapped candidate total, as [`crate::observation::OracleObservation`]
    /// reports it.
    pub candidate_total: u32,
    /// The first [`crate::observation::MAX_CAPTURED_CANDIDATES`] candidates.
    pub candidates: Vec<CandidateInfo>,
}

/// One `(phrase, pinyin, count)` tuple from a phrase-library export.
///
/// `pinyin` is the oracle's own spelling: syllables joined by `'`
/// (e.g. `ni'hao`), the exact form `docs/findings/data-layer-export.md`
/// freezes as the index key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedPhrase {
    /// Phrase text, UTF-8.
    pub phrase: String,
    /// Apostrophe-separated pinyin spelling.
    pub pinyin: String,
    /// Count as reported by the export iterator; `-1` means default.
    pub count: i32,
}

impl Oracle {
    /// Exports every `(phrase, pinyin, count)` tuple of phrase library
    /// `library` via the public export iterator.
    ///
    /// # Errors
    ///
    /// Returns an error if the iterator cannot be created, a step reports
    /// failure, or a string is not UTF-8.
    pub fn export_phrases(&mut self, library: u8) -> Result<Vec<ExportedPhrase>, OracleError> {
        // SAFETY: `self.context` is non-null and owned by `self` for the whole
        // call. A NULL iterator is checked below.
        let iter = unsafe { ffi::pinyin_begin_get_phrases(self.context, c_uint::from(library)) };

        if iter.is_null() {
            return Err(OracleError::Call {
                function: "pinyin_begin_get_phrases",
            });
        }

        let mut tuples = Vec::new();
        let result = loop {
            // SAFETY: `iter` is non-null (checked above) and stays owned by
            // this call until `pinyin_end_get_phrases` below.
            let has_next = unsafe { ffi::pinyin_iterator_has_next_phrase(iter) };
            if !has_next {
                break Ok(());
            }

            let mut phrase: *mut core::ffi::c_char = core::ptr::null_mut();
            let mut pinyin: *mut core::ffi::c_char = core::ptr::null_mut();
            let mut count: core::ffi::c_int = -1;
            // SAFETY: `iter` is non-null; the out-pointers are valid, aligned
            // and writable. On success libpinyin transfers ownership of both
            // strings, released with `g_free` below exactly once each.
            let ok = unsafe {
                ffi::pinyin_iterator_get_next_phrase(
                    iter,
                    &raw mut phrase,
                    &raw mut pinyin,
                    &raw mut count,
                )
            };

            if !ok {
                break Err(OracleError::Call {
                    function: "pinyin_iterator_get_next_phrase",
                });
            }

            let copy = |ptr: *mut core::ffi::c_char| -> Result<String, OracleError> {
                if ptr.is_null() {
                    return Err(OracleError::Call {
                        function: "pinyin_iterator_get_next_phrase",
                    });
                }
                // SAFETY: `ptr` is a non-null NUL-terminated GLib string we
                // now own; copied before the `g_free` below.
                let borrowed = unsafe { CStr::from_ptr(ptr) };
                borrowed
                    .to_str()
                    .map(str::to_owned)
                    .map_err(|_| OracleError::NonUtf8 {
                        function: "pinyin_iterator_get_next_phrase",
                    })
            };
            let phrase_copy = copy(phrase);
            let pinyin_copy = copy(pinyin);
            // SAFETY: both pointers came from an ownership-transferring out
            // param of the call above; freed exactly once, never used after.
            // NULL is tolerated by g_free.
            unsafe {
                ffi::g_free(phrase.cast());
                ffi::g_free(pinyin.cast());
            }

            match (phrase_copy, pinyin_copy) {
                (Ok(phrase), Ok(pinyin)) => tuples.push(ExportedPhrase {
                    phrase,
                    pinyin,
                    count,
                }),
                (Err(error), _) | (_, Err(error)) => break Err(error),
            }
        };

        // SAFETY: `iter` is non-null and owned by this call; ended exactly
        // once on every path, and never used after.
        unsafe { ffi::pinyin_end_get_phrases(iter) };

        result.map(|()| tuples)
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
        let (parse_return, parsed_input_length) = self.parse_input(input)?;

        let (candidate_total, infos) = if parsed_input_length > 0 {
            self.collect_candidates()?
        } else {
            (0, Vec::new())
        };
        // Per-candidate type and n-best index live on the W2-CAND path (`observe_candidate_infos`).
        let candidates = infos.into_iter().map(|info| info.text).collect();

        let segments = self.collect_segments(parsed_input_length, input)?;

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

    /// Parses `input` into the instance and returns
    /// `(parse_return, parsed_input_length)`, erroring if the oracle reports a
    /// parsed prefix longer than the input it was given.
    ///
    /// Shared by [`Self::observe_without_reset`] and
    /// [`Self::observe_candidate_infos`] so both enumerate candidates over the
    /// identical parse state.
    fn parse_input(&mut self, input: &[u8]) -> Result<(usize, usize), OracleError> {
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

        Ok((parse_return, parsed_input_length))
    }

    /// Resets, parses `input`, and returns each candidate's string, public
    /// type, and n-best index — the W2-CAND capture surface.
    ///
    /// The candidate stage is exactly [`Self::observe`]'s: the same
    /// `pinyin_guess_candidates` sort word and the same depth cap via
    /// [`Self::collect_candidates`], so the returned `text`s are identical, and
    /// in the same order, to `observe(input)?.candidates`. Only the two extra
    /// public accessors (`pinyin_get_candidate_type`,
    /// `pinyin_get_candidate_nbest_index`) are read; nothing else about a
    /// candidate is reachable from the pinned public header
    /// (`docs/findings/candidate-construction.md` §1.6).
    ///
    /// # Errors
    ///
    /// As [`Self::observe`], plus [`OracleError::UnknownCandidateType`] if the
    /// oracle reports a `lookup_candidate_type_t` outside the eight the pinned
    /// header declares.
    pub fn observe_candidate_infos(
        &mut self,
        input: &[u8],
    ) -> Result<Vec<CandidateInfo>, OracleError> {
        self.reset()?;
        let (_parse_return, parsed_input_length) = self.parse_input(input)?;
        if parsed_input_length == 0 {
            return Ok(Vec::new());
        }
        let (_total, infos) = self.collect_candidates()?;
        Ok(infos)
    }

    /// Resets, parses `input`, guesses the sentence, and returns the decoded
    /// n-best sentences plus the candidate list the guess produced — the W14
    /// sentence-surface capture.
    ///
    /// The protocol is the ibus order: `pinyin_guess_sentence` before
    /// `pinyin_guess_candidates`, so the candidate list carries the prepended
    /// `NBEST_MATCH_CANDIDATE` rows (`pinyin.cpp:2290-2298`).
    ///
    /// `pinyin_get_sentence` **asserts** (aborts) for `index` at or past
    /// `m_nbest_results.size()`, so the read depth is derived from what the
    /// candidate list proves: index 0 is always safe (upstream returns false
    /// for an empty result set), and an index above 0 is read only when the
    /// list carries an `NBEST_MATCH` row with that index — a surviving row
    /// proves the result set is larger. Deduplicated rows are not decoded,
    /// matching what the public ABI exposes at all.
    ///
    /// # Errors
    ///
    /// As [`Self::observe_candidate_infos`].
    pub fn observe_sentence_surface(
        &mut self,
        input: &[u8],
        max_index: u8,
    ) -> Result<SentenceSurface, OracleError> {
        self.reset()?;
        let (_parse_return, parsed_input_length) = self.parse_input(input)?;
        if parsed_input_length == 0 {
            return Ok(SentenceSurface::default());
        }
        // SAFETY: instance is non-null and owned by `self`.
        let guessed = unsafe { ffi::pinyin_guess_sentence(self.instance) };
        let (candidate_total, infos) = self.collect_candidates()?;
        let proven = infos
            .iter()
            .filter(|info| matches!(info.candidate_type, OracleCandidateType::NbestMatch))
            .filter_map(|info| info.nbest_index)
            .max()
            .unwrap_or(0)
            .min(max_index);
        let mut sentences = vec![self.get_sentence(0)?];
        for index in 1..=proven {
            sentences.push(self.get_sentence(index)?);
        }
        Ok(SentenceSurface {
            guessed,
            sentences,
            candidate_total,
            candidates: infos,
        })
    }

    /// One `pinyin_get_sentence` read, `None` when the oracle reports none.
    ///
    /// # Errors
    ///
    /// Returns an error if the returned string is not UTF-8.
    fn get_sentence(&mut self, index: u8) -> Result<Option<String>, OracleError> {
        let mut text: *mut core::ffi::c_char = core::ptr::null_mut();
        // SAFETY: instance is non-null and owned by `self`; the out-pointer is
        // valid and writable. On success libpinyin transfers ownership of the
        // string, released with `g_free` below exactly once.
        let ok = unsafe { ffi::pinyin_get_sentence(self.instance, index, &raw mut text) };
        if !ok || text.is_null() {
            return Ok(None);
        }
        // SAFETY: `text` is a non-null NUL-terminated GLib string we now own;
        // copied before the `g_free` below, never used after.
        let borrowed = unsafe { CStr::from_ptr(text) };
        let copied = borrowed.to_str().ok().map(str::to_owned);
        // SAFETY: ownership-transferring out-param above; freed exactly once.
        unsafe { ffi::g_free(text.cast()) };
        copied.map(Some).ok_or(OracleError::NonUtf8 {
            function: "pinyin_get_sentence",
        })
    }

    /// The oracle's own `phrase text → phrase_token_t` mapping.
    ///
    /// Returns every token spelling `phrase`, across all loaded libraries
    /// (the token's top byte identifies the library).
    ///
    /// # Errors
    ///
    /// Returns an error if `phrase` carries an interior NUL, if the GLib
    /// array cannot be allocated, or if the call reports failure.
    pub fn lookup_tokens(&mut self, phrase: &str) -> Result<Vec<u32>, OracleError> {
        let phrase_c = CString::new(phrase).map_err(|_| OracleError::InteriorNul {
            what: "phrase for token lookup",
        })?;

        // SAFETY: plain constructor call; element size is `u32`. NULL is
        // checked below.
        let array = unsafe { ffi::g_array_new(0, 0, 4) };
        if array.is_null() {
            return Err(OracleError::Call {
                function: "g_array_new",
            });
        }

        // SAFETY: instance is non-null and owned by `self`; `phrase_c` is a
        // live NUL-terminated C string; `array` is a live GArray of `u32`
        // elements that only this call appends to.
        let ok = unsafe { ffi::pinyin_lookup_tokens(self.instance, phrase_c.as_ptr(), array) };

        let tokens = if ok {
            // SAFETY: `array` is non-null and its documented public fields
            // describe `len` elements of 4 bytes each at `data`; the slice is
            // copied before `g_array_free` releases the storage.
            let raw = unsafe {
                let len = (*array).len as usize;
                core::slice::from_raw_parts((*array).data.cast::<u8>(), len * 4).to_vec()
            };
            raw.chunks_exact(4)
                .map(|chunk| u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect()
        } else {
            Vec::new()
        };

        // SAFETY: `array` is non-null and owned by this call; freed exactly
        // once with its storage, and never used after.
        unsafe { ffi::g_array_free(array, 1) };

        if ok {
            Ok(tokens)
        } else {
            Err(OracleError::Call {
                function: "pinyin_lookup_tokens",
            })
        }
    }

    /// The oracle's own `phrase_token_t → phrase text` mapping.
    ///
    /// Returns `None` when the oracle reports no phrase for `token`.
    ///
    /// # Errors
    ///
    /// Returns an error if the returned string is not UTF-8.
    pub fn token_text(&mut self, token: u32) -> Result<Option<String>, OracleError> {
        let mut len: c_uint = 0;
        let mut text: *mut core::ffi::c_char = core::ptr::null_mut();
        // SAFETY: instance is non-null and owned by `self`; the out-pointers
        // are valid, aligned and writable. On success libpinyin transfers
        // ownership of the string, freed below exactly once.
        let ok = unsafe {
            ffi::pinyin_token_get_phrase(self.instance, token, &raw mut len, &raw mut text)
        };

        if !ok || text.is_null() {
            return Ok(None);
        }

        // SAFETY: `text` is a non-null NUL-terminated GLib string we now own;
        // copied before freeing, and the pointer is not used after.
        let borrowed = unsafe { CStr::from_ptr(text) };
        let copied = borrowed.to_str().map(str::to_owned);
        // SAFETY: `text` came from an ownership-transferring out param; freed
        // exactly once here.
        unsafe { ffi::g_free(text.cast()) };

        copied.map(Some).map_err(|_| OracleError::NonUtf8 {
            function: "pinyin_token_get_phrase",
        })
    }

    /// Builds the candidate list and copies the first
    /// [`MAX_CAPTURED_CANDIDATES`] candidates, each with its string and the two
    /// public fields W2-CAND records (`pinyin_get_candidate_type`,
    /// `pinyin_get_candidate_nbest_index`).
    fn collect_candidates(&mut self) -> Result<(u32, Vec<CandidateInfo>), OracleError> {
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

            // The two extra public accessors. `candidate` is still the pointer
            // borrowed above and none of these reads mutates the instance, so it
            // stays valid across all three.
            let mut raw_type: core::ffi::c_int = 0;
            // SAFETY: `candidate` is non-null and borrowed from this unmutated
            // instance; the out-pointer is a valid, writable `c_int`.
            let ok = unsafe {
                ffi::pinyin_get_candidate_type(self.instance, candidate, &raw mut raw_type)
            };
            if !ok {
                return Err(OracleError::Call {
                    function: "pinyin_get_candidate_type",
                });
            }
            let candidate_type = OracleCandidateType::from_raw(raw_type)
                .ok_or(OracleError::UnknownCandidateType { value: raw_type })?;

            // `pinyin_get_candidate_nbest_index` asserts internally that the
            // candidate is an NBEST_MATCH_CANDIDATE and aborts otherwise —
            // observed at runtime as
            //   Assertion `NBEST_MATCH_CANDIDATE == candidate->m_candidate_type' failed
            // which reaches abort(), the same uncatchable class as
            // docs/findings/oracle-apostrophe-abort.md. The index therefore
            // exists only for n-best candidates; for every other type it is
            // `None` (written `-`) and the accessor is not called. This guard is
            // derived from the observed abort, not from reading upstream source.
            let nbest_index = if matches!(candidate_type, OracleCandidateType::NbestMatch) {
                let mut raw_index: u8 = 0;
                // SAFETY: `candidate` is a non-null NBEST_MATCH candidate borrowed
                // from this unmutated instance, which satisfies the accessor's
                // asserted precondition; the out-pointer is a valid, writable
                // `u8`. A false return means no index, recorded as `None`, never
                // a fabricated `0`.
                let has_index = unsafe {
                    ffi::pinyin_get_candidate_nbest_index(
                        self.instance,
                        candidate,
                        &raw mut raw_index,
                    )
                };
                has_index.then_some(raw_index)
            } else {
                None
            };

            candidates.push(CandidateInfo {
                text: owned,
                candidate_type,
                nbest_index,
            });
        }

        Ok((total, candidates))
    }

    /// Walks the parsed prefix, mirroring the traversal in
    /// `tools/capture/capture.c`.
    ///
    /// Offsets advance to each segment's end, or by one byte when the oracle
    /// yields nothing usable, which is what makes the walk terminate on the
    /// F-E-01 shape instead of spinning.
    fn collect_segments(
        &mut self,
        parsed_len: usize,
        input: &[u8],
    ) -> Result<Vec<OracleSegment>, OracleError> {
        // Abort guard, see docs/findings/oracle-apostrophe-abort.md. The pinned
        // oracle reports a non-empty parsed prefix for apostrophe-only input
        // while its key matrix has no columns, and a key query then trips an
        // assert() that reaches abort(). A phonetic key is built from an initial
        // and a final, both spelled with lowercase ASCII letters, so a parsed
        // prefix with no letter cannot have produced a key: skipping the walk
        // loses no information. The sentinel keeps the input visible as a
        // divergence rather than silently smoothing the defect away.
        if parsed_len > 0
            && !input
                .get(..parsed_len)
                .unwrap_or(input)
                .iter()
                .any(u8::is_ascii_lowercase)
        {
            return Ok(vec![OracleSegment::NoKeyColumns { parsed_len }]);
        }

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

/// Adapts a [`Session`] to the differential runner's producer seam.
///
/// Reuses one instance across the corpus, clearing state with `pinyin_reset`
/// between inputs. `session_reset_agrees_with_a_fresh_instance` pins that this
/// is equivalent to the frozen fresh-instance protocol, so the cheap path cannot
/// drift from the protocol the fixtures were captured under.
#[derive(Debug)]
pub struct LiveSource<'oracle> {
    session: Session<'oracle>,
}

impl<'oracle> LiveSource<'oracle> {
    /// Wraps `session`.
    #[must_use]
    pub const fn new(session: Session<'oracle>) -> Self {
        Self { session }
    }
}

impl ObservationSource for LiveSource<'_> {
    fn observe(&mut self, input: &[u8]) -> Result<OracleObservation, OracleError> {
        self.session.observe(input)
    }

    fn source(&self) -> DiffSource {
        DiffSource::Live
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
