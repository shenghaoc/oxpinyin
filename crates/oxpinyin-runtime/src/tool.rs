//! Heap-loaded test-program data, assembled beside the runtime's other readers.
use super::*;
use oxpinyin_data::{PhraseLibraries, dict::SystemTables};

/// Failure of the test program's required chunk load.
pub struct LoadError {
    /// The raw configured filename (`None` means the pin's NULL column).
    pub name: Option<PathBuf>,
    /// Why loading stopped.
    pub cause: LibraryError,
}

/// A single native index, heap-loaded phrase facade and optional system bigram.
pub struct Lookup {
    tables: SystemTables,
    libraries: PhraseLibraries,
    model: Option<oxpinyin_data::lm::BigramTables>,
}
impl Lookup {
    /// Attach the index, load each SYSTEM_FILE, then attach the system bigram.
    /// The pin's NULL user-bigram attach fails before native I/O.
    ///
    /// # Errors
    /// Returns the first required chunk failure. Native construction failures
    /// before a handle exists terminate through the shared boundary.
    pub fn open(conf: &TableConf, phonetic: bool, bigram: bool) -> Result<Self, LoadError> {
        let path = Path::new(if phonetic {
            "../../data/pinyin_index.bin"
        } else {
            "../../data/phrase_index.bin"
        });
        let tables = SystemTables::for_lookup(path, phonetic)
            .unwrap_or_else(|_| oxpinyin_input::tool::abort());
        let mut libraries = PhraseLibraries::empty();
        for (index, row) in conf.system_libraries() {
            let name = row.system.as_ref().map(|name| PathBuf::from(name.as_ref()));
            let path = oxpinyin_input::path::build_path(
                Path::new("../../data"),
                name.as_deref().unwrap_or(Path::new("")).as_os_str(),
            );
            libraries
                .load_heap(index, &path)
                .map_err(|cause| LoadError { name, cause })?;
        }
        let model = bigram.then(|| {
            let mut model =
                oxpinyin_data::lm::BigramTables::attach(Path::new("../../data/bigram.db"))
                    .unwrap_or_else(|_| oxpinyin_input::tool::abort());
            model.set_lambda_from_conf(conf);
            model
        });
        Ok(Self {
            tables,
            libraries,
            model,
        })
    }
    /// The matrix driver's SEARCH_CONTINUED table probe.
    #[must_use]
    pub fn key_exists(&self, keys: &[oxpinyin_core::ChewingKey]) -> bool {
        self.tables
            .key_exists(keys)
            .unwrap_or_else(|_| oxpinyin_input::tool::abort())
    }
    /// Existing n-best lookup with no user overlay.
    ///
    /// # Errors
    /// Returns decoder/model errors.
    pub fn phonetic(
        &self,
        prepared: &oxpinyin_engine::tool::PreparedLookup,
    ) -> Result<Vec<oxpinyin_engine::tool::LookupRow>, EngineError> {
        let Some(model) = &self.model else {
            return Ok(Vec::new());
        };
        oxpinyin_engine::tool::lookup(
            prepared,
            &self.tables.read(&self.libraries),
            &model.read_all(&self.libraries),
        )
    }
    /// Existing phrase span decoder with no user overlay.
    ///
    /// # Errors
    /// Returns decoder/model errors.
    pub fn phrase(&self, words: &[u32]) -> Result<Vec<PhraseToken>, EngineError> {
        let Some(model) = &self.model else {
            return Ok(Vec::new());
        };
        oxpinyin_engine::tool::phrase_lookup(
            words,
            &self.tables.read(&self.libraries),
            &model.read_all(&self.libraries),
        )
    }
    /// Resolve a result token to its native stored phrase.
    #[must_use]
    pub fn text(&self, token: u32) -> Option<PhraseText> {
        self.libraries.phrase_text(token)
    }
}
