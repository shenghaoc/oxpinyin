//! Declarative marshalling macros shared by the two C-ABI facades
//! (`oxpinyin-capi` → `libpinyin.so.15`, `oxpinyin-zhuyin-capi` →
//! `libzhuyin.so.15`).
//!
//! The two C crates expand the pointer casts, allocation and output writers
//! at the ABI boundary. The marshalling module forbids unsafe code. The macro definitions
//! compile here and the generated unsafe blocks compile at their call sites.
//!
//! The safe [`input`] and [`phrase_text`] modules compile here: they convert
//! borrowed input to Rust values and encode [`crate::PhraseText`]
//! as the pin's C-facing bytes. Only pointer borrowing and output allocation
//! remain unsafe edges in the adapters.
//!
//! No exported C symbol is generated here: the `#[unsafe(no_mangle)]`
//! entry points keep their hand-written bodies in each C crate, so the
//! `libpinyin.ver` (79) / `libzhuyin.ver` (52) export gates and the C++
//! header smoke test see an unchanged surface. These macros stamp only the
//! crate-internal Rust helpers those entry points call.
/// Stamps the opaque-handle marshalling helpers behind a facade's three C
/// handle types: the context, the instance, and the borrowed lookup
/// candidate.
///
/// Each facade exposes zero-sized `#[repr(C)]` marker types across the C
/// ABI (`pinyin_context_t *` etc.) whose pointers actually address a
/// heap-allocated backing struct (`CapiContext`/`CapiInstance`/
/// `CapiCandidate`). This macro centralises the eight cast/box helpers that
/// bridge the two representations so each call site stays readable and the
/// two facades cannot drift:
///
/// - `context_ref` / `context_mut` — `*mut Marker` → `&`/`&mut Backing`
/// - `instance_ref` / `instance_mut` — likewise for the instance handle
/// - `candidate_ref` — `*mut CandMarker` → `&CandBacking`
/// - `candidate_ptr` — `&CandBacking` → `*mut CandMarker`
/// - `box_context` / `box_instance` — `Backing` → `*mut Marker` (leaks a
///   `Box` for the caller to reclaim through its destructor entry point)
///
/// `$vis` is applied to every generated item, so a facade whose modules
/// reach these through `crate::state::*` passes `pub` and one that keeps
/// them crate-internal passes `pub(crate)`.
///
/// # Example
///
/// ```ignore
/// oxpinyin::opaque_handle_casts! {
///     vis: pub,
///     context: PinyinContext => CapiContext,
///     instance: PinyinInstance => CapiInstance,
///     candidate: LookupCandidate => CapiCandidate,
/// }
/// ```
#[macro_export]
macro_rules! opaque_handle_casts {
    (
        vis: $vis:vis,
        context: $ctx_marker:ty => $ctx_backing:ty,
        instance: $inst_marker:ty => $inst_backing:ty,
        candidate: $cand_marker:ty => $cand_backing:ty $(,)?
    ) => {
        /// Casts a context handle pointer to a shared backing reference.
        ///
        /// # Safety
        ///
        /// `ptr` must be non-null and produced by
        /// `box_context` (`Box::into_raw(Box::new(<backing> { .. }))`). The
        /// returned reference must not outlive that `Box` (i.e. must not be
        /// used after the `fini` entry point reconstructs and drops it) and
        /// must not be stored in a longer-lived location.
        $vis unsafe fn context_ref<'a>(ptr: *mut $ctx_marker) -> &'a $ctx_backing {
            // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
            unsafe { &*(ptr.cast::<$ctx_backing>()) }
        }

        /// Casts a context handle pointer to a unique backing reference.
        ///
        /// # Safety
        ///
        /// `ptr` must be non-null and produced by `box_context`. No other
        /// reference to the same context may exist, and the returned
        /// reference must not outlive the backing `Box` (must not be used
        /// after the `fini` entry point reconstructs and drops it) or be
        /// stored in a longer-lived location.
        $vis unsafe fn context_mut<'a>(ptr: *mut $ctx_marker) -> &'a mut $ctx_backing {
            // SAFETY: Caller guarantees the pointer is valid and unique for the chosen lifetime.
            unsafe { &mut *(ptr.cast::<$ctx_backing>()) }
        }

        /// Casts an instance handle pointer to a shared backing reference.
        ///
        /// # Safety
        ///
        /// `ptr` must be non-null and produced by `box_instance`. The
        /// returned reference must not outlive that `Box` (must not be used
        /// after the `free_instance` entry point reconstructs and drops it)
        /// and must not be stored in a longer-lived location.
        $vis unsafe fn instance_ref<'a>(ptr: *mut $inst_marker) -> &'a $inst_backing {
            // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
            unsafe { &*(ptr.cast::<$inst_backing>()) }
        }

        /// Casts an instance handle pointer to a unique backing reference.
        ///
        /// # Safety
        ///
        /// `ptr` must be non-null and produced by `box_instance`. No other
        /// reference to the same instance may exist, and the returned
        /// reference must not outlive the backing `Box` (must not be used
        /// after the `free_instance` entry point reconstructs and drops it)
        /// or be stored in a longer-lived location.
        $vis unsafe fn instance_mut<'a>(ptr: *mut $inst_marker) -> &'a mut $inst_backing {
            // SAFETY: Caller guarantees the pointer is valid and unique for the chosen lifetime.
            unsafe { &mut *(ptr.cast::<$inst_backing>()) }
        }

        /// Converts a backing context into a handle pointer for return to C.
        $vis fn box_context(ctx: $ctx_backing) -> *mut $ctx_marker {
            ::std::boxed::Box::into_raw(::std::boxed::Box::new(ctx)).cast()
        }

        /// Converts a backing instance into a handle pointer for return to C.
        $vis fn box_instance(inst: $inst_backing) -> *mut $inst_marker {
            ::std::boxed::Box::into_raw(::std::boxed::Box::new(inst)).cast()
        }

        /// Casts a lookup-candidate handle pointer back to its backing.
        ///
        /// # Safety
        ///
        /// `ptr` must be non-null and point into an active backing
        /// `candidates` vec (produced by `candidate_ptr`).
        $vis unsafe fn candidate_ref<'a>(ptr: *mut $cand_marker) -> &'a $cand_backing {
            // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
            unsafe { &*(ptr.cast::<$cand_backing>()) }
        }

        /// Returns a lookup-candidate handle pointer to a backing candidate.
        $vis const fn candidate_ptr(cand: &$cand_backing) -> *mut $cand_marker {
            (cand as *const $cand_backing as *mut $cand_backing).cast()
        }
    };
}

pub mod input;
pub mod phrase_text;
pub mod ucs4;

/// Defines the shared GLib output and allocation helpers in each C facade.
/// The expanding crate supplies its existing GLib, libc and key-text names.
#[macro_export]
macro_rules! ffi_output_helpers {
    (glib: $glib:ident, key_text: $key_text:ident, malloc: $malloc:ident, free: $free:ident, domain: $domain:expr) => {
        /// `text` with a terminating NUL, the C string a GLib call reads. C reads
        /// it up to its first NUL, as it reads the pin's argument.
        fn c_arg(text: &[u8]) -> Vec<u8> {
            let mut arg = Vec::with_capacity(text.len() + 1);
            arg.extend_from_slice(text);
            arg.push(0);
            arg
        }

        /// `g_strdup(text)`: the pin's duplication of a returned string. GLib
        /// allocates with `g_malloc`, so a failed allocation aborts as the pin's
        /// does. The caller owns the result and frees it with `g_free`.
        pub fn strdup(text: &str) -> *mut c_char {
            let arg = c_arg(text.as_bytes());
            // SAFETY: `arg` is NUL-terminated and outlives the call.
            unsafe { $glib::g_strdup(arg.as_ptr().cast()) }
        }

        /// `g_strdup(text)` for a C string that may be NULL: GLib answers NULL for
        /// NULL, the pin's `g_strdup` of a failed `g_ucs4_to_utf8` result
        /// (`lookup.cpp:63`, `pinyin.cpp:904`).
        ///
        /// # Safety
        ///
        /// `text` is NULL or a NUL-terminated string.
        pub unsafe fn strdup_raw(text: *const c_char) -> *mut c_char {
            // SAFETY: The caller guarantees `text` is NULL or NUL-terminated;
            // `g_strdup` accepts NULL.
            unsafe { $glib::g_strdup(text) }
        }

        /// A key rendering made with the call `_ChewingKey::get_*_string` makes
        /// (`chewing_key.cpp:47-121`): `g_strdup` of the spelling, or
        /// `g_strdup_printf` with the pin's format and arguments. GLib formats and
        /// allocates, so a failed allocation fails as the pin's does.
        pub fn key_text(text: $key_text) -> *mut c_char {
            match text {
                $key_text::Plain(base) => strdup(base),
                $key_text::Digit(base, tone) => {
                    let base = c_arg(base.as_bytes());
                    // SAFETY: `%s%d` consumes one NUL-terminated `const char *`,
                    // which outlives the call, and one `int`.
                    unsafe {
                        $glib::g_strdup_printf(
                            c"%s%d".as_ptr(),
                            base.as_ptr().cast::<c_char>(),
                            c_int::from(tone),
                        )
                    }
                }
                $key_text::Mark(base, mark) => {
                    let (base, mark) = (c_arg(base.as_bytes()), c_arg(mark.as_bytes()));
                    // SAFETY: `%s%s` consumes two NUL-terminated `const char *`
                    // arguments, both of which outlive the call.
                    unsafe {
                        $glib::g_strdup_printf(
                            c"%s%s".as_ptr(),
                            base.as_ptr().cast::<c_char>(),
                            mark.as_ptr().cast::<c_char>(),
                        )
                    }
                }
            }
        }

        /// The keyboard symbol vector as the pin's `in_chewing_scheme` builds it
        /// (`zhuyin_parser2.cpp:301-332`, `:496-544`, `:798-851`): a
        /// `g_ptr_array_new`, one `g_strdup` and `g_ptr_array_add` per symbol, then
        /// the terminating NULL and `g_ptr_array_free(array, FALSE)`, whose
        /// segment the caller frees with `g_strfreev`. No symbol answers NULL after
        /// `g_ptr_array_free(array, TRUE)`, as the pin's `false` does.
        pub fn symbol_vector(items: &[impl AsRef<str>]) -> *mut *mut c_char {
            // SAFETY: `g_ptr_array_new` takes no arguments and aborts rather than
            // answering NULL.
            let array = unsafe { $glib::g_ptr_array_new() };
            for item in items {
                let symbol = strdup(item.as_ref());
                // SAFETY: `array` is the live array made above; it takes ownership
                // of the GLib-allocated `symbol`.
                unsafe { $glib::g_ptr_array_add(array, symbol.cast()) };
            }
            if items.is_empty() {
                // SAFETY: `array` is live and holds no elements; freeing the segment
                // frees nothing else.
                unsafe { $glib::g_ptr_array_free(array, $glib::GTRUE) };
                return ptr::null_mut();
            }
            // SAFETY: `array` is live. Adding NULL terminates the vector, and
            // `g_ptr_array_free(…, FALSE)` releases the array and hands its
            // segment, now owned by the caller, back.
            unsafe {
                $glib::g_ptr_array_add(array, ptr::null_mut());
                $glib::g_ptr_array_free(array, $glib::GFALSE).cast()
            }
        }

        /// The pin's `convert_to_utf8` over the sentence row's phrases
        /// (`lookup/lookup.cpp:27-69`), call for call. Each phrase is converted by
        /// this facade's fallible encoder, which stands for `g_ucs4_to_utf8`
        /// (`:58`) and answers NULL when its allocation fails. While the result is
        /// still NULL it becomes `g_strdup(phrase)` (`:62-63`), which is NULL again
        /// for a NULL phrase; otherwise `g_strconcat(result, "", phrase, NULL)`
        /// (`:65`, the NULL delimiter made `""` at `:33-34`), which stops at a NULL
        /// phrase and so copies the result unchanged. The phrase and the previous
        /// result are then freed (`:66-67`). GLib aborts if its own allocation
        /// fails. NULL when every conversion failed.
        pub fn sentence_text(
            phrases: impl IntoIterator<
                Item = Option<impl core::borrow::Borrow<$crate::PhraseText>>,
            >,
        ) -> *mut c_char {
            let mut result: *mut c_char = ptr::null_mut();
            for phrase in phrases {
                // A phrase whose words GLib does not convert is NULL here.
                let converted = phrase.map_or(ptr::null_mut(), |phrase| {
                    let bytes =
                        $crate::marshal::phrase_text::phrase_text_bytes(phrase.borrow());
                    malloc_cstr_with(bytes.len(), |out| {
                        out.copy_from_slice(&bytes);
                        bytes.len()
                    })
                });
                let previous = result;
                result = if previous.is_null() {
                    // SAFETY: `converted` is NULL or this call's NUL-terminated
                    // buffer; `g_strdup` answers NULL for NULL.
                    unsafe { strdup_raw(converted) }
                } else {
                    // SAFETY: `previous` is the NUL-terminated GLib string this loop
                    // made, `""` is a static C string, `converted` is NULL (which
                    // ends the list) or this call's NUL-terminated buffer, and the
                    // list ends with NULL.
                    unsafe {
                        $glib::g_strconcat(
                            previous.cast_const(),
                            c"".as_ptr(),
                            converted.cast_const(),
                            ptr::null::<c_char>(),
                        )
                    }
                };
                // SAFETY: `converted` is NULL or this call's encoder buffer, and
                // `previous` NULL or the GLib string this loop owns; nothing else
                // holds either, and `g_free` releases both.
                unsafe {
                    $glib::g_free(converted.cast());
                    $glib::g_free(previous.cast());
                }
            }
            result
        }

        /// Hands C one `malloc`-allocated, NUL-terminated string of `len` bytes
        /// that `fill` writes into the buffer it is given, returning how many it
        /// wrote: the encoder that stands for the pin's `g_ucs4_to_utf8`
        /// conversions (`pinyin.cpp:716`, `:822`, `:828`, `:1644`, `lookup.cpp:54`,
        /// `:58`). Like that function it answers NULL when its allocation fails,
        /// with no warning and no abort; it also answers NULL when `fill` wrote
        /// another length or the bytes carry a NUL. The buffer is zeroed before
        /// `fill` sees it, so the slice it writes through is initialized memory.
        /// The caller frees the result with `g_free`.
        pub fn malloc_cstr_with(len: usize, fill: impl FnOnce(&mut [u8]) -> usize) -> *mut c_char {
            let Some(size) = len.checked_add(1) else {
                return ptr::null_mut();
            };
            // SAFETY: `malloc` returns `size` writable bytes or null.
            let dst = unsafe { $malloc(size) }.cast::<u8>();
            if dst.is_null() {
                return ptr::null_mut();
            }
            // SAFETY: `dst` points to `size` writable bytes that nothing else
            // references; zeroing them makes every byte an initialized `u8`.
            unsafe {
                ptr::write_bytes(dst, 0, size);
            }
            // SAFETY: `dst` points to `size` initialized bytes this function alone
            // owns until it returns, so the exclusive slice is valid for its scope.
            let buffer = unsafe { std::slice::from_raw_parts_mut(dst, size) };
            let written = fill(&mut buffer[..len]);
            if written != len || buffer[..len].contains(&0) {
                // SAFETY: `dst` came from `malloc` above and was handed out nowhere.
                unsafe { $free(dst.cast()) };
                return ptr::null_mut();
            }
            buffer[len] = 0;
            dst.cast::<c_char>()
        }

        /// Logs `message` through `GLib` at warning level under the facade's
        /// domain. The library's only diagnostic channel: the C ABI's frozen
        /// return shapes (`false` / NULL) carry no reason, and glib is already
        /// linked for the ABI's `GArray`s. A message with an interior NUL is
        /// dropped rather than truncated.
        pub fn log_warning(message: &str) {
            let Ok(message) = CString::new(message) else {
                return;
            };
            // SAFETY: `g_log` is variadic; the `%s` format consumes exactly one
            // `const char *`, and both the domain and the message are
            // NUL-terminated buffers that outlive the call.
            unsafe {
                $glib::g_log(
                    ($domain).as_ptr(),
                    $glib::G_LOG_LEVEL_WARNING,
                    c"%s".as_ptr(),
                    message.as_ptr(),
                );
            }
        }
    };
}

/// Defines the shared display_string_getter helper at each C ABI boundary.
#[macro_export]
macro_rules! display_string_getter {
    (key: $key:ident, char: $char:ident) => {
        /// The shared body of the two index-spelling display getters: NULL the
        /// out-param, refuse the zero key on `get_table_index() == 0`, then hand
        /// out the rendering made with the pin's `g_strdup`/`g_strdup_printf`
        /// call (`chewing_key.cpp:97-114`), caller-owned (`g_free`).
        ///
        /// # Safety
        ///
        /// Under the crate-level rules, Out-parameters and Borrowed objects: a
        /// non-NULL output is aligned and writable, and a non-NULL key is valid
        /// for reads. Access must not conflict with live borrows.
        unsafe fn display_string_getter(
            key: *mut $key,
            utf8_str: *mut *mut $char,
            render: impl FnOnce($crate::$key) -> $crate::KeyText,
        ) -> bool {
            // The pin never reads `instance` (`pinyin.cpp:2729-2749`); it writes
            // `*utf8_str` before any check, so a NULL out stays guarded (row 41).
            if key.is_null() || utf8_str.is_null() {
                return false;
            }

            // SAFETY: Out-parameters guarantees aligned writable storage; NULL was checked.
            unsafe {
                *utf8_str = ptr::null_mut();
            }
            // SAFETY: Borrowed objects guarantees a readable key; NULL was checked.
            let core = unsafe { *key }.to_core();
            if !core.elements_in_dims() {
                std::process::abort();
            }
            if core.table_index() == 0 {
                return false;
            }
            if !core.tone_in_dims() {
                std::process::abort();
            }
            // SAFETY: Out-parameters guarantees aligned writable storage; NULL was checked.
            unsafe {
                *utf8_str = key_text(render(core));
            }
            true
        }
    };
}

/// Defines the shared append_stored_words helper at each C ABI boundary.
#[macro_export]
macro_rules! append_stored_words {
    (array: $array:ident, append: $append:ident) => {
        /// Append one reading's stored packed words to the caller's array, or the
        /// pin's zeroed past-the-end words when the item has no such reading.
        ///
        /// # Safety
        /// `keys` must be a non-NULL `GArray` whose element size is two bytes, as
        /// the caller of `pinyin_token_get_nth_pronunciation` creates it.
        unsafe fn append_stored_words(keys: *mut $array, length: usize, words: Option<&[u8]>) {
            match words {
                None => {
                    // SAFETY: the caller guarantees a real $array; `zeroed` outlives
                    // the append call, which copies from it.
                    let zeroed = vec![0_u16; length];
                    // SAFETY: as above; element size is two bytes.
                    unsafe {
                        $append(
                            keys,
                            zeroed.as_ptr().cast::<c_void>(),
                            c_uint::try_from(zeroed.len()).unwrap_or(0),
                        );
                    }
                }
                Some(words) => {
                    // SAFETY: the caller guarantees a real $array with element size
                    // two bytes; `words` holds `2 * phrase_length` stored bytes and
                    // outlives the append call, which copies from it.
                    unsafe {
                        $append(
                            keys,
                            words.as_ptr().cast::<c_void>(),
                            c_uint::try_from(words.len() / 2).unwrap_or(0),
                        );
                    }
                }
            }
        }
    };
}

/// Defines facade handle ownership, borrows and instance views at the C boundary.
/// The optional context field preserves the pinyin instance back-pointer.
#[macro_export]
macro_rules! state_handle_helpers {
    (context: $ctx:ident, instance: $inst:ident, instance_binding: $binding:ident, error: $error:ty, context_field: { $($context_field:tt)* }) => {
        /// Borrow storage retained by a handle, including after context fini.
        /// # Safety
        /// The C caller must serialize calls on this context and all its handles;
        /// no mutation may overlap the returned operation-scoped borrow.
        pub(crate) unsafe fn owner_ref(owner: &ContextOwner) -> &CapiContext {
            // SAFETY: the owner keeps the allocation alive, and the caller's
            // one-call-at-a-time contract excludes mutation during this borrow.
            unsafe { &*owner.get() }
        }

        /// Borrow the shared allocation exclusively for one C call.
        /// # Safety
        /// No other call or retained Rust borrow may access this allocation until
        /// the returned borrow ends, including through another owning handle.
        pub(crate) unsafe fn owner_mut(owner: &mut ContextOwner) -> &mut CapiContext {
            // SAFETY: ownership guarantees liveness; the C caller's one-call-at-a-time
            // contract, not uniqueness of the reference count, guarantees exclusivity.
            unsafe { &mut *owner.get() }
        }

        /// Retain the backing allocation while the public context handle is live.
        /// # Safety
        /// `ptr` must be a live context; the caller serializes operations on it.
        pub(crate) unsafe fn context_owner(ptr: *mut $ctx) -> ContextOwner {
            // SAFETY: the public handle is live for this call; clone only its owner.
            Rc::clone(unsafe { handles::context_ref(ptr) })
        }

        /// Box the context's reference-counted owner as the public C handle.
        pub(crate) fn box_context(context: CapiContext) -> *mut $ctx {
            handles::box_context(Rc::new(UnsafeCell::new(context)))
        }

        /// Borrow a live public context for one operation.
        /// # Safety
        /// `ptr` is live and the caller excludes mutation during the borrow.
        pub(crate) unsafe fn context_ref<'a>(ptr: *mut $ctx) -> &'a CapiContext {
            // SAFETY: the caller keeps the public handle live and serializes its calls.
            unsafe { owner_ref(handles::context_ref(ptr)) }
        }

        /// Borrow a live public context exclusively for one operation.
        /// # Safety
        /// `ptr` is live; no call or borrow through any handle overlaps this one.
        pub(crate) unsafe fn context_mut<'a>(ptr: *mut $ctx) -> &'a mut CapiContext {
            // SAFETY: the caller keeps the public handle live and grants exclusive access.
            unsafe { owner_mut(handles::context_mut(ptr)) }
        }

        /// Borrow the runtime retained by an instance, without its public context.
        /// # Safety
        /// `ptr` is live; all earlier readers have ended and the C caller grants
        /// exclusive access across every handle sharing this runtime for this call.
        pub(crate) unsafe fn instance_context_mut<'a>(ptr: *mut $inst) -> &'a mut CapiContext {
            // SAFETY: the instance owns the allocation; serialized C calls exclude
            // access through any other instance, iterator or the public context.
            unsafe { owner_mut(&mut handles::instance_mut(ptr).owner) }
        }

        /// Borrow this call's instance and its live context.
        /// # Safety
        /// The instance must be live and all calls sharing its runtime serialized.
        /// No instance or context mutation may overlap the returned reader.
        pub unsafe fn instance_ref<'a>(ptr: *mut $inst) -> CapiInstanceRead<'a> {
            // SAFETY: the caller supplies a live instance and serializes all shared-handle calls.
            let $binding = unsafe { handles::instance_ref(ptr) };
            // SAFETY: the instance retains the allocation; the C caller excludes concurrent mutation.
            let context = unsafe { owner_ref(&$binding.owner) };
            let runtime = &context.core.runtime;
            CapiInstance {
                $($context_field)*
                owner: &$binding.owner,
                core: $binding.core.read(runtime),
                state: &$binding.state,
            }
        }

        /// Borrow composition exclusively and phrase storage for this call only.
        /// # Safety
        /// The instance must be live, with no overlapping access through any shared handle.
        pub unsafe fn instance_mut<'a>(ptr: *mut $inst) -> CapiInstanceMut<'a> {
            // SAFETY: the caller grants exclusive access to this live instance.
            let $binding = unsafe { handles::instance_mut(ptr) };
            // SAFETY: the instance retains the allocation; the C caller excludes runtime mutation.
            let context = unsafe { owner_ref(&$binding.owner) };
            let runtime = &context.core.runtime;
            CapiInstance {
                $($context_field)*
                owner: &$binding.owner,
                core: $binding.core.bind(runtime),
                state: &mut $binding.state,
            }
        }

        /// Train with exclusive access to the context, after all read views have ended.
        /// # Safety
        /// The caller supplies a live instance and exclusive access across all shared handles.
        pub(crate) unsafe fn train_instance(
            ptr: *mut $inst,
            index: u8,
        ) -> Result<bool, $error> {
            // SAFETY: the caller grants this operation exclusive access to the live instance.
            let $binding = unsafe { handles::instance_mut(ptr) };
            // SAFETY: the owner retains the allocation; serialized C calls exclude other readers.
            let context = unsafe { owner_mut(&mut $binding.owner) };
            context.core.train(&$binding.core, index)
        }

    };
}

/// Stamps a facade's `write_owned_sentence` helper: the `char **`-out
/// sentence writer. The facade passes its sentence allocator,
/// `fn(&[PhraseText]) -> *mut c_char`, which makes the pin's
/// `convert_to_utf8` calls over the row's phrases (`lookup.cpp:27-69` at
/// 074a2219): a fallible conversion per phrase, `g_strdup` of the first
/// result, `g_strconcat` for each later one. As in the pin, the answer is
/// `true` whether or not the result is NULL.
///
/// # Example
///
/// ```ignore
/// oxpinyin::write_owned_sentence!(crate::ffi::sentence_text);
/// ```
#[macro_export]
macro_rules! write_owned_sentence {
    ($sentence_text:path) => {
        /// Writes the allocator's result for `phrases` through the
        /// caller-owned out-param, NULL where the pin's would be, and
        /// answers `true` (`lookup.cpp:69`).
        ///
        /// # Safety
        ///
        /// Under the crate-level rules, Out-parameters: a non-NULL sentence
        /// destination is aligned and writable without conflicting with live
        /// borrows, including the phrase inputs. Its contents need not be
        /// initialized.
        unsafe fn write_owned_sentence(
            phrases: impl IntoIterator<
                Item = Option<impl ::core::borrow::Borrow<$crate::PhraseText>>,
            >,
            sentence: *mut *mut ::std::os::raw::c_char,
        ) -> bool {
            if !sentence.is_null() {
                let owned = $sentence_text(phrases);
                // SAFETY: Out-parameters guarantees aligned writable storage; NULL was checked.
                unsafe {
                    *sentence = owned;
                }
            }
            true
        }
    };
}
