//! libpinyin's `FlexibleBigram` training container, on the store's backends.
//!
//! The pin's k-mixture-model tools keep their working model in a DBM whose
//! records are raw, host-native bytes: an eight-byte magic record and one
//! four-byte-token-keyed single gram per predecessor token. This module
//! reproduces that container class and that byte layout on whichever
//! backend the crate compiles, so a file it writes is a file the pin's own
//! `gen_k_mixture_model`, `export_k_mixture_model` and
//! `validate_k_mixture_model` read, and the reverse.
//!
//! # On-disk layout
//!
//! The pin (`src/storage/flexible_ngram_bdb.h`,
//! `flexible_ngram_kyotodb.h`, `flexible_ngram_tkrzwdb.h` at 074a2219)
//! stores two kinds of record in one flat keyspace.
//!
//! **The magic record.** Its key is `m_magic_header_index`, two
//! `phrase_token_t`s both set to `null_token`, so eight zero bytes. Its
//! value is the four-byte magic number (`KMMP` for the k-mixture model)
//! followed by the caller's magic header (`KMixtureModelMagicHeader`, three
//! native `guint32`s). A fresh container is created with the magic number
//! alone — the pin's Berkeley DB `DB_DBT_PARTIAL` put at offset 0 and the
//! KC/Tkrzw whole-value set both leave a four-byte record
//! (`flexible_ngram_bdb.h:118-131`, `flexible_ngram_kyotodb.h:157-161`,
//! `flexible_ngram_tkrzwdb.h:161-167`) — and `set_magic_header` rewrites
//! the value as the magic number plus the header
//! (`flexible_ngram_bdb.h:338-356`, `flexible_ngram_kyotodb.h:319-342`,
//! `flexible_ngram_tkrzwdb.h:333-357`).
//!
//! **A single gram.** Its key is one `phrase_token_t` in host byte order.
//! Its value is the caller's array header followed by a token-ascending
//! array of `ArrayItemWithToken` records: a four-byte `phrase_token_t`
//! followed by the caller's array item
//! (`flexible_single_gram.h:57-60,75-78,151-181`). On the little-endian
//! linux/amd64 reference this is the k-mixture model's 8-byte
//! `KMixtureModelArrayHeader` and 20-byte records (4-byte token plus the
//! 16-byte `KMixtureModelArrayItem`); the layout is the pin's
//! `memcpy`/struct layout, not a normalised encoding, so no length prefix
//! or padding is added here.
//!
//! # Container class
//!
//! All three backends open a hash container with the pin's own class and
//! flags: Berkeley DB `DB_HASH` at mode 0644
//! (`flexible_ngram_bdb.h:110-114`), Kyoto Cabinet `HashDB`
//! (`flexible_ngram_kyotodb.h:143-155`) and tkrzw `HashDBM`
//! (`flexible_ngram_tkrzwdb.h:145-159`). [`FlexibleBigram::open`] opens or
//! creates that container through [`WriteStore::create_hash`] and
//! [`FlexibleBigram::open_read_only`] through
//! [`RawReadStore::open_hash_read_only`], exactly the constructors datagen's
//! `write_hash_with` already uses for libpinyin's `bigram.db`.
//!
//! # Iteration order
//!
//! [`FlexibleBigram::get_all_items`] walks the container in its **native
//! order** through [`RawReadStore::native_keys`], as the pin's
//! `get_all_items` does: a Berkeley DB `DB_NEXT` cursor
//! (`flexible_ngram_bdb.h:245-291`), a Kyoto Cabinet `iterate`
//! (`flexible_ngram_kyotodb.h:262-272`) or a tkrzw `ProcessEach`
//! (`flexible_ngram_tkrzwdb.h:274-282`). The order differs between
//! backends and between files, so it is not sorted here; the KMM caller
//! must not rely on it, and the pin's tools do not.
//!
//! # Pin error behaviour
//!
//! A missing key is a miss: [`FlexibleBigram::load`] answers `None`, the
//! header getters `None`. A closed handle answers `false`/`None` through the
//! `Option`/`bool` results. [`FlexibleBigram::get_array_header`] asserts the
//! record is at least one array header long, the pin's
//! `assert(db_data.size == sizeof(ArrayHeader))` and
//! `assert(retsize >= vsiz)` (`flexible_ngram_bdb.h:388`,
//! `flexible_ngram_kyotodb.h:371`, `flexible_ngram_tkrzwdb.h:388`). A
//! failing cursor walk in `get_all_items` prints the pin's
//! `training db error, exit!` (no newline) to stderr and exits with the
//! `EIO` status (`flexible_ngram_bdb.h:279-286`) — the error cannot be
//! recovered in the pin either.

use std::marker::PhantomData;
use std::path::Path;

use crate::{BackendError, DefaultStore, RawReadStore, StoreError, WriteStore, WriteTxn};

/// The magic record's eight-byte key: two `null_token` `phrase_token_t`s
/// (`flexible_ngram_bdb.h:118-121`, `flexible_ngram_kyotodb.h:157-158`,
/// `flexible_ngram_tkrzwdb.h:161-162`). Every token key is four bytes, so
/// this is the one record the pin's `get_all_items` visitors skip.
pub const MAGIC_KEY: [u8; 8] = [0; 8];

/// The pin's magic-number field length (`char m_magic_number[4]`).
const MAGIC_NUMBER_LEN: usize = 4;

/// The pin's `phrase_token_t` field length.
const TOKEN_LEN: usize = 4;

/// `EIO`'s status on every host the pin builds for; `exit(EIO)` is the pin's
/// unrecoverable `get_all_items` answer (`flexible_ngram_bdb.h:285`).
const EIO_EXIT_CODE: i32 = 5;

/// One fixed-layout record of the pin's structs, encoded and decoded as the
/// pin's `memcpy` does.
///
/// Implementors define the k-mixture model's magic header, array header and
/// array item. [`Self::LEN`] must be the pin's `sizeof` for that type and
/// [`Self::encode`] must produce exactly `LEN` bytes in the pin's
/// host-native layout — do not add a length prefix, padding or a normalised
/// byte order.
pub trait FixedRecord: Clone {
    /// The record's `sizeof`, in bytes.
    const LEN: usize;

    /// The value the pin's `memset(&record, 0, sizeof(record))` produces.
    fn zeroed() -> Self;

    /// The record as the pin `memcpy`s it, exactly [`Self::LEN`] bytes.
    fn encode(&self) -> Vec<u8>;

    /// Read a record from exactly [`Self::LEN`] bytes of the pin's layout.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `bytes` does not hold a valid record.
    fn decode(bytes: &[u8]) -> Result<Self, StoreError>;
}

/// The three k-mixture-model record types and the container's magic number.
///
/// The pin's `FlexibleBigram` is a template over the same three types and
/// takes the four-byte magic number in its constructor
/// (`flexible_ngram.h:27-29`, `k_mixture_model.h:110-166`). A KMM caller
/// names `KMMP` and the three structs here.
pub trait FlexibleGramSpec {
    /// The four-byte magic number, e.g. the k-mixture model's `KMMP`.
    const MAGIC_NUMBER: [u8; 4];
    /// The pin's `MagicHeader`.
    type MagicHeader: FixedRecord;
    /// The pin's `ArrayHeader`.
    type ArrayHeader: FixedRecord;
    /// The pin's `ArrayItem`.
    type ArrayItem: FixedRecord;
}

/// One array item with its token — the pin's `ArrayItemWithToken`
/// (`flexible_single_gram.h:57-60`).
///
/// The pin stores the token first, then the item, both host-native. The
/// record order in a single gram is ascending by [`Self::token`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayItemWithToken<I> {
    /// The successor `phrase_token_t`, host-native.
    pub token: u32,
    /// The caller's array item.
    pub item: I,
}

/// The pin's `FlexibleSingleGram`: an array header and a token-ascending
/// item array (`flexible_single_gram.h:37-321`).
///
/// This is the in-memory value of one single gram. The item operations
/// mirror the pin's in-memory `FlexibleSingleGram` methods; persistence is
/// [`FlexibleBigram::store`], as the pin's tools do
/// (`utils/training/gen_k_mixture_model.cpp:252-271`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleGram<H, I> {
    header: H,
    items: Vec<ArrayItemWithToken<I>>,
}

impl<H: FixedRecord, I: FixedRecord> Default for SingleGram<H, I> {
    fn default() -> Self {
        Self::new()
    }
}

impl<H: FixedRecord, I: FixedRecord> SingleGram<H, I> {
    /// A gram with the pin's zeroed array header and no items
    /// (`flexible_single_gram.h:75-78`).
    #[must_use]
    pub fn new() -> Self {
        Self {
            header: H::zeroed(),
            items: Vec::new(),
        }
    }

    /// A gram from an encoded header and items already in token order.
    ///
    /// The caller is responsible for the pin's ascending order; this does
    /// not sort or reject a file the pin would read.
    #[must_use]
    pub fn from_parts(header: H, items: Vec<ArrayItemWithToken<I>>) -> Self {
        Self { header, items }
    }

    /// The array header.
    #[must_use]
    pub fn header(&self) -> &H {
        &self.header
    }

    /// Replace the array header.
    pub fn set_header(&mut self, header: H) {
        self.header = header;
    }

    /// The items in their stored, token-ascending order.
    #[must_use]
    pub fn items(&self) -> &[ArrayItemWithToken<I>] {
        &self.items
    }

    /// The item of `token`, or `None` when the gram has none.
    #[must_use]
    pub fn get_array_item(&self, token: u32) -> Option<I> {
        self.position(token)
            .ok()
            .map(|index| self.items[index].item.clone())
    }

    /// Insert the item of `token`; `false` when the gram already has one
    /// (`flexible_single_gram.h:151-181`).
    pub fn insert_array_item(&mut self, token: u32, item: I) -> bool {
        match self.position(token) {
            Ok(_) => false,
            Err(index) => {
                self.items.insert(index, ArrayItemWithToken { token, item });
                true
            }
        }
    }

    /// Remove and answer the item of `token`, or `None` when absent
    /// (`flexible_single_gram.h:193-221`).
    pub fn remove_array_item(&mut self, token: u32) -> Option<I> {
        self.position(token)
            .ok()
            .map(|index| self.items.remove(index).item)
    }

    /// Replace the item of `token`; `false` when absent
    /// (`flexible_single_gram.h:268-290`).
    pub fn set_array_item(&mut self, token: u32, item: I) -> bool {
        match self.position(token) {
            Ok(index) => {
                self.items[index].item = item;
                true
            }
            Err(_) => false,
        }
    }

    /// The index of `token`, or where it would be inserted.
    fn position(&self, token: u32) -> Result<usize, usize> {
        self.items.binary_search_by_key(&token, |entry| entry.token)
    }

    /// The pin's raw value: header then token-then-item records.
    fn encode(&self) -> Vec<u8> {
        encode_value(&self.header, &self.items)
    }
}

/// The pin's `FlexibleBigram` over the compiled-in store backend.
///
/// `S` names the three record types and the magic number; `B` is the store
/// backend (`DefaultStore` through [`DefaultFlexibleBigram`], or an
/// explicitly named peer for a cross-backend caller).
pub struct FlexibleBigram<B, S> {
    db: Option<B>,
    read_only: bool,
    _spec: PhantomData<fn() -> S>,
}

/// One single gram of `S`'s array header and array item types.
pub type SpecGram<S> =
    SingleGram<<S as FlexibleGramSpec>::ArrayHeader, <S as FlexibleGramSpec>::ArrayItem>;

/// [`FlexibleBigram`] on the backend this build selected.
pub type DefaultFlexibleBigram<S> = FlexibleBigram<DefaultStore, S>;

impl<B, S> FlexibleBigram<B, S>
where
    B: RawReadStore + WriteStore,
    S: FlexibleGramSpec,
{
    /// Open the working model read-write, creating it when the path is
    /// absent or empty.
    ///
    /// This is the pin's `attach(path, ATTACH_READWRITE | ATTACH_CREATE)`:
    /// a hash container in the backend's own class
    /// ([`WriteStore::create_hash`]), a fresh one carrying the four-byte
    /// magic number, an existing one checked against it. An existing
    /// container whose magic record is missing or different is refused, as
    /// the pin's signature check refuses it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the container cannot be opened or
    /// created, or does not carry [`FlexibleGramSpec::MAGIC_NUMBER`].
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let fresh = match std::fs::metadata(path) {
            Ok(metadata) => metadata.len() == 0,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Err(error) => return Err(StoreError::Io(error)),
        };
        let this = Self {
            db: Some(B::create_hash(path)?),
            read_only: false,
            _spec: PhantomData,
        };
        match this.read_value(&MAGIC_KEY)? {
            Some(value) => {
                if !this.matches_magic(&value) {
                    return Err(signature_error());
                }
            }
            None if fresh => {
                if !this.write_value(&MAGIC_KEY, &S::MAGIC_NUMBER)? {
                    return Err(StoreError::Backend(BackendError::from(
                        "flexible bigram container was not writable while creating its magic record",
                    )));
                }
            }
            None => return Err(signature_error()),
        }
        Ok(this)
    }

    /// Open an existing working model read-only, as the pin's
    /// `attach(path, ATTACH_READONLY)` does.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the container cannot be opened or does
    /// not carry [`FlexibleGramSpec::MAGIC_NUMBER`].
    pub fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        let this = Self {
            db: Some(B::open_hash_read_only(path)?),
            read_only: true,
            _spec: PhantomData,
        };
        match this.read_value(&MAGIC_KEY)? {
            Some(value) if this.matches_magic(&value) => Ok(this),
            _ => Err(signature_error()),
        }
    }

    /// Close the backend handle — the pin's `reset()`, which synchronises
    /// and closes. Dropping the handle performs the backend's own close
    /// (`DB->sync` then close on Berkeley DB, `close` on Kyoto Cabinet and
    /// tkrzw); every write this layer made has already been committed.
    pub fn close(&mut self) {
        self.db = None;
    }

    /// Whether a handle is open (the pin's `m_db != NULL`).
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.db.is_some()
    }

    /// Load one single gram; `None` when the token has no row, or when the
    /// handle is closed (`flexible_ngram_bdb.h:165-187` and its peers).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the record read fails or the stored
    /// value is not a whole pin single gram.
    pub fn load(&self, token: u32) -> Result<Option<SpecGram<S>>, StoreError> {
        let Some(value) = self.read_gram(&token.to_ne_bytes())? else {
            return Ok(None);
        };
        let (header, items) = decode_value(&value)?;
        Ok(Some(SingleGram::from_parts(header, items)))
    }

    /// Store one single gram; `false` when the handle is closed or
    /// read-only (the pin returns false when `m_db` is null or the put
    /// fails, `flexible_ngram_bdb.h:198-214` and its peers).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend write fails.
    pub fn store(&self, token: u32, gram: &SpecGram<S>) -> Result<bool, StoreError> {
        self.write_value(&token.to_ne_bytes(), &gram.encode())
    }

    /// Remove one single gram; `false` when the token has no row, or when
    /// the handle is closed or read-only (`flexible_ngram_bdb.h:224-235`).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the existence read or the delete fails.
    pub fn remove(&self, token: u32) -> Result<bool, StoreError> {
        let key = token.to_ne_bytes();
        if self.db.is_none() || self.read_only {
            return Ok(false);
        }
        if self.read_value(&key)?.is_none() {
            return Ok(false);
        }
        self.delete_value(&key)
    }

    /// Retrieve every item of one single gram; `None` when the token has no
    /// row (`flexible_single_gram.h:88-105` on the loaded gram).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the load fails.
    pub fn retrieve_all(
        &self,
        token: u32,
    ) -> Result<Option<Vec<ArrayItemWithToken<S::ArrayItem>>>, StoreError> {
        Ok(self.load(token)?.map(|gram| gram.items().to_vec()))
    }

    /// Get the magic header; `None` when the record is absent or its value
    /// is not the four-byte magic number plus a whole header
    /// (`flexible_ngram_bdb.h:302-328`, `flexible_ngram_kyotodb.h:282-309`,
    /// `flexible_ngram_tkrzwdb.h:292-323`).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the read fails or the header bytes do
    /// not decode.
    pub fn get_magic_header(&self) -> Result<Option<S::MagicHeader>, StoreError> {
        let Some(value) = self.read_value(&MAGIC_KEY)? else {
            return Ok(None);
        };
        let header_len = <S::MagicHeader as FixedRecord>::LEN;
        if value.len() != MAGIC_NUMBER_LEN + header_len {
            return Ok(None);
        }
        Ok(Some(S::MagicHeader::decode(&value[MAGIC_NUMBER_LEN..])?))
    }

    /// Set the magic header, writing the magic number in front of it as the
    /// pin's KC and tkrzw implementations do (`flexible_ngram_kyotodb.h:319-342`,
    /// `flexible_ngram_tkrzwdb.h:333-357`); the Berkeley DB partial put
    /// writes the same final bytes (`flexible_ngram_bdb.h:338-356`).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend write fails.
    pub fn set_magic_header(&self, header: &S::MagicHeader) -> Result<bool, StoreError> {
        if self.db.is_none() || self.read_only {
            return Ok(false);
        }
        let encoded = header.encode();
        debug_assert_eq!(encoded.len(), <S::MagicHeader as FixedRecord>::LEN);
        let mut value = Vec::with_capacity(MAGIC_NUMBER_LEN + encoded.len());
        value.extend_from_slice(&S::MAGIC_NUMBER);
        value.extend_from_slice(&encoded);
        self.write_value(&MAGIC_KEY, &value)
    }

    /// Get one single gram's array header; `None` when the token has no
    /// row (`flexible_ngram_bdb.h:367-391`, `flexible_ngram_kyotodb.h:353-374`,
    /// `flexible_ngram_tkrzwdb.h:368-391`).
    ///
    /// # Panics
    ///
    /// Panics when the stored value is shorter than the pin's array header:
    /// the pin asserts the partial read's size
    /// (`flexible_ngram_bdb.h:388`) or `retsize >= vsiz`
    /// (`flexible_ngram_kyotodb.h:371`, `flexible_ngram_tkrzwdb.h:388`).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the read fails or the header bytes do
    /// not decode.
    #[expect(
        clippy::panic_in_result_fn,
        reason = "the pin's get_array_header asserts the record's array header length \
                  (flexible_ngram_bdb.h:388, flexible_ngram_kyotodb.h:371, \
                  flexible_ngram_tkrzwdb.h:388); the panic is the required behaviour"
    )]
    pub fn get_array_header(&self, token: u32) -> Result<Option<S::ArrayHeader>, StoreError> {
        let Some(value) = self.read_value(&token.to_ne_bytes())? else {
            return Ok(None);
        };
        let header_len = <S::ArrayHeader as FixedRecord>::LEN;
        assert!(
            value.len() >= header_len,
            "flexible bigram array header is {} bytes, shorter than the pin's {header_len}",
            value.len(),
        );
        Ok(Some(S::ArrayHeader::decode(&value[..header_len])?))
    }

    /// Set one single gram's array header without disturbing its items.
    ///
    /// The pin's Berkeley DB implementation writes the header in place with
    /// a `DB_DBT_PARTIAL` put (`flexible_ngram_bdb.h:402-420`); KC and tkrzw
    /// load the whole value, overwrite the header and store it back
    /// (`flexible_ngram_kyotodb.h:385-408`,
    /// `flexible_ngram_tkrzwdb.h:402-430`). Both leave every item byte as it
    /// was; a missing record is created with the header alone in all three.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the read or write fails.
    pub fn set_array_header(
        &self,
        token: u32,
        header: &S::ArrayHeader,
    ) -> Result<bool, StoreError> {
        if self.db.is_none() || self.read_only {
            return Ok(false);
        }
        let key = token.to_ne_bytes();
        let header_len = <S::ArrayHeader as FixedRecord>::LEN;
        let encoded = header.encode();
        debug_assert_eq!(encoded.len(), header_len);
        let mut value = self.read_value(&key)?.unwrap_or_default();
        if value.len() < header_len {
            value.resize(header_len, 0);
        }
        value[..header_len].copy_from_slice(&encoded);
        self.write_value(&key, &value)
    }

    /// Every token key in the container's **native** walk order, skipping
    /// the magic record (`flexible_ngram_bdb.h:245-291` and its peers).
    ///
    /// The order is the backend's, not sorted: [`RawReadStore::native_keys`]
    /// is the shared seam, and its hash-container arms are the pin's own
    /// walks — a Berkeley DB `DB_NEXT` cursor, a Kyoto Cabinet `iterate`
    /// and a tkrzw `ProcessEach`, each in its library's order. The
    /// eight-byte magic key and every other non-token key are skipped by
    /// length, as the pin's visitors skip them. `None` when the handle is
    /// closed (the pin returns false after clearing the array).
    ///
    /// # Panics
    ///
    /// Exits the process after printing `training db error, exit!` to
    /// stderr when the backend walk fails, as the pin does
    /// (`flexible_ngram_bdb.h:279-286`).
    #[must_use]
    pub fn get_all_items(&self) -> Option<Vec<u32>> {
        let db = self.db.as_ref()?;
        match db.native_keys() {
            Ok(keys) => Some(
                keys.into_iter()
                    .filter_map(|key| <[u8; 4]>::try_from(key.as_slice()).ok())
                    .map(u32::from_ne_bytes)
                    .collect(),
            ),
            Err(_) => training_db_error(),
        }
    }

    /// The magic record's value starts with this container's magic number.
    fn matches_magic(&self, value: &[u8]) -> bool {
        value.len() >= MAGIC_NUMBER_LEN && value[..MAGIC_NUMBER_LEN] == S::MAGIC_NUMBER
    }

    /// Read `key` from the raw keyspace; `None` on a miss or a closed
    /// handle.
    fn read_value(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let Some(db) = self.db.as_ref() else {
            return Ok(None);
        };
        let mut value = Vec::new();
        Ok(db.get_raw(key, &mut value)?.then_some(value))
    }

    /// Read one single gram the way the pin's `load` does: Kyoto Cabinet
    /// checks the size and then reads the body, which
    /// [`RawReadStore::get_bigram_raw`] reproduces (and reports a changed
    /// size as [`StoreError::ReadAfterCheck`], where the pin's
    /// `check_result(…)` asserts, `flexible_ngram_kyotodb.h:202-210`). The
    /// other two backends read the value once.
    fn read_gram(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let Some(db) = self.db.as_ref() else {
            return Ok(None);
        };
        let mut value = Vec::new();
        Ok(db.get_bigram_raw(key, &mut value)?.then_some(value))
    }

    /// Write `key` to the raw keyspace; `false` on a closed or read-only
    /// handle.
    fn write_value(&self, key: &[u8], value: &[u8]) -> Result<bool, StoreError> {
        let Some(db) = self.db.as_ref() else {
            return Ok(false);
        };
        if self.read_only {
            return Ok(false);
        }
        db.write(|txn| txn.put_raw(key, value))?;
        Ok(true)
    }

    /// Remove `key` from the raw keyspace; `false` on a closed or
    /// read-only handle.
    fn delete_value(&self, key: &[u8]) -> Result<bool, StoreError> {
        let Some(db) = self.db.as_ref() else {
            return Ok(false);
        };
        if self.read_only {
            return Ok(false);
        }
        db.write(|txn| txn.remove_raw(key))?;
        Ok(true)
    }
}

/// The value of a single gram: the encoded header then the token-then-item
/// records.
fn encode_value<H: FixedRecord, I: FixedRecord>(
    header: &H,
    items: &[ArrayItemWithToken<I>],
) -> Vec<u8> {
    let mut value = Vec::with_capacity(H::LEN + items.len() * (TOKEN_LEN + I::LEN));
    value.extend_from_slice(&header.encode());
    for entry in items {
        value.extend_from_slice(&entry.token.to_ne_bytes());
        value.extend_from_slice(&entry.item.encode());
    }
    value
}

/// [`encode_value`]'s inverse, with the pin's length check made explicit so
/// a truncated record is a `Result` instead of an out-of-bounds read.
fn decode_value<H: FixedRecord, I: FixedRecord>(
    value: &[u8],
) -> Result<(H, Vec<ArrayItemWithToken<I>>), StoreError> {
    let header_len = H::LEN;
    let item_len = TOKEN_LEN + I::LEN;
    if value.len() < header_len || !(value.len() - header_len).is_multiple_of(item_len) {
        return Err(StoreError::Backend(BackendError::from(format!(
            "flexible bigram value is {} bytes: a {header_len}-byte header plus whole \
             {item_len}-byte item records expected",
            value.len(),
        ))));
    }
    let header = H::decode(&value[..header_len])?;
    let mut items = Vec::with_capacity((value.len() - header_len) / item_len);
    for record in value[header_len..].chunks_exact(item_len) {
        let mut token = [0u8; TOKEN_LEN];
        token.copy_from_slice(&record[..TOKEN_LEN]);
        let item = I::decode(&record[TOKEN_LEN..])?;
        items.push(ArrayItemWithToken {
            token: u32::from_ne_bytes(token),
            item,
        });
    }
    Ok((header, items))
}

/// A container that does not carry this flexible gram's magic number.
fn signature_error() -> StoreError {
    StoreError::Backend(BackendError::from(
        "flexible bigram container does not carry the expected magic number",
    ))
}

/// The pin's unrecoverable `get_all_items` cursor failure
/// (`flexible_ngram_bdb.h:279-286`): the message with no trailing newline,
/// then `exit(EIO)`.
fn training_db_error() -> ! {
    use std::io::Write as _;
    std::io::stderr()
        .write_all(b"training db error, exit!")
        .ok();
    std::process::exit(EIO_EXIT_CODE);
}

#[cfg(test)]
mod tests {
    use super::{
        ArrayItemWithToken, FixedRecord, FlexibleGramSpec, MAGIC_KEY, SingleGram, StoreError,
    };

    /// A stand-in for the KMM magic header.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    struct Magic(u32);

    /// A stand-in for the KMM array header.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    struct Header(u32, u32);

    /// A stand-in for the KMM array item.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    struct Item(u32, u32, u32, u32);

    impl FixedRecord for Magic {
        const LEN: usize = 4;

        fn zeroed() -> Self {
            Self(0)
        }

        fn encode(&self) -> Vec<u8> {
            self.0.to_ne_bytes().to_vec()
        }

        fn decode(bytes: &[u8]) -> Result<Self, StoreError> {
            Ok(Self(u32::from_ne_bytes(bytes.try_into().map_err(
                |_| StoreError::Backend("magic header is not four bytes".into()),
            )?)))
        }
    }

    impl FixedRecord for Header {
        const LEN: usize = 8;

        fn zeroed() -> Self {
            Self(0, 0)
        }

        fn encode(&self) -> Vec<u8> {
            let mut bytes = Vec::with_capacity(Self::LEN);
            bytes.extend_from_slice(&self.0.to_ne_bytes());
            bytes.extend_from_slice(&self.1.to_ne_bytes());
            bytes
        }

        fn decode(bytes: &[u8]) -> Result<Self, StoreError> {
            if bytes.len() != Self::LEN {
                return Err(StoreError::Backend(
                    "array header is not eight bytes".into(),
                ));
            }
            let word = |at: usize| u32::from_ne_bytes(bytes[at..at + 4].try_into().unwrap());
            Ok(Self(word(0), word(4)))
        }
    }

    impl FixedRecord for Item {
        const LEN: usize = 16;

        fn zeroed() -> Self {
            Self(0, 0, 0, 0)
        }

        fn encode(&self) -> Vec<u8> {
            let mut bytes = Vec::with_capacity(Self::LEN);
            for word in [self.0, self.1, self.2, self.3] {
                bytes.extend_from_slice(&word.to_ne_bytes());
            }
            bytes
        }

        fn decode(bytes: &[u8]) -> Result<Self, StoreError> {
            if bytes.len() != Self::LEN {
                return Err(StoreError::Backend(
                    "array item is not sixteen bytes".into(),
                ));
            }
            let word = |at: usize| u32::from_ne_bytes(bytes[at..at + 4].try_into().unwrap());
            Ok(Self(word(0), word(4), word(8), word(12)))
        }
    }

    struct Spec;

    impl super::FlexibleGramSpec for Spec {
        const MAGIC_NUMBER: [u8; 4] = *b"TEST";
        type MagicHeader = Magic;
        type ArrayHeader = Header;
        type ArrayItem = Item;
    }

    #[test]
    fn magic_key_is_two_null_tokens() {
        assert_eq!(MAGIC_KEY, [0u8; 8]);
    }

    #[test]
    fn a_spec_names_the_magic_number_and_the_record_layouts() {
        assert_eq!(Spec::MAGIC_NUMBER, *b"TEST");
        assert_eq!(Magic::zeroed(), Magic(0));
        assert_eq!(Magic::decode(&Magic(9).encode()).unwrap(), Magic(9));
        assert_eq!(Magic::LEN, 4);
        assert_eq!(Header::LEN, 8);
        assert_eq!(Item::LEN, 16);
    }

    #[test]
    fn a_fresh_gram_is_a_zeroed_header_and_no_items() {
        let gram = SingleGram::<Header, Item>::new();
        assert_eq!(*gram.header(), Header(0, 0));
        assert!(gram.items().is_empty());
    }

    #[test]
    fn items_stay_token_ascending_and_are_unique() {
        let mut gram = SingleGram::<Header, Item>::new();
        assert!(gram.insert_array_item(7, Item(7, 0, 0, 0)));
        assert!(gram.insert_array_item(3, Item(3, 0, 0, 0)));
        assert!(gram.insert_array_item(5, Item(5, 0, 0, 0)));
        let tokens: Vec<u32> = gram.items().iter().map(|entry| entry.token).collect();
        assert_eq!(tokens, [3, 5, 7]);
        assert!(!gram.insert_array_item(5, Item(99, 0, 0, 0)));
        assert_eq!(gram.get_array_item(5), Some(Item(5, 0, 0, 0)));
        assert!(gram.set_array_item(5, Item(50, 0, 0, 0)));
        assert_eq!(gram.get_array_item(5), Some(Item(50, 0, 0, 0)));
        assert!(!gram.set_array_item(4, Item(4, 0, 0, 0)));
        assert_eq!(gram.remove_array_item(5), Some(Item(50, 0, 0, 0)));
        assert_eq!(gram.remove_array_item(5), None);
        assert_eq!(gram.get_array_item(5), None);
        assert_eq!(gram.items().len(), 2);
    }

    #[test]
    fn a_single_gram_value_is_the_pins_raw_layout() {
        let mut gram = SingleGram::<Header, Item>::new();
        gram.set_header(Header(1, 2));
        gram.insert_array_item(0x0100_0001, Item(3, 4, 5, 6));
        let value = gram.encode();
        assert_eq!(value.len(), 8 + 4 + 16);
        let mut expected = Vec::new();
        expected.extend_from_slice(&1u32.to_ne_bytes());
        expected.extend_from_slice(&2u32.to_ne_bytes());
        expected.extend_from_slice(&0x0100_0001u32.to_ne_bytes());
        for word in [3u32, 4, 5, 6] {
            expected.extend_from_slice(&word.to_ne_bytes());
        }
        assert_eq!(value, expected);
        let (header, items) = super::decode_value::<Header, Item>(&value).unwrap();
        assert_eq!(header, Header(1, 2));
        assert_eq!(
            items,
            vec![ArrayItemWithToken {
                token: 0x0100_0001,
                item: Item(3, 4, 5, 6),
            }]
        );
    }

    #[test]
    fn a_truncated_single_gram_is_refused_by_length() {
        for value in [&[][..], &[0u8; 7][..], &[0u8; 9][..], &[0u8; 24][..]] {
            assert!(
                super::decode_value::<Header, Item>(value).is_err(),
                "{value:?}"
            );
        }
    }
}
