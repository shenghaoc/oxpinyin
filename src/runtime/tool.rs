//! Heap-loaded lookup data for the nonshipping REPL, assembled beside the runtime readers.
use super::*;
use crate::data::{PhraseLibraries, SystemTables};

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
    model: Option<crate::data::BigramTables>,
}
impl Lookup {
    // Native index or model construction failures terminate at the utility boundary. The
    // pin's NULL user-bigram attach fails before native I/O.
    /// Opens the selected phonetic or phrase index and loads the system libraries named in
    /// the configuration. Set `bigram` to attach the system model for decoding.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError`] for the first required phrase chunk that could not be loaded.
    pub fn open(
        data: &Path,
        conf: &TableConf,
        phonetic: bool,
        bigram: bool,
    ) -> Result<Self, LoadError> {
        let path = data.join(if phonetic {
            "pinyin_index.bin"
        } else {
            "phrase_index.bin"
        });
        let tables = SystemTables::for_lookup(&path, phonetic)
            .unwrap_or_else(|_| crate::input::tool::abort());
        let mut libraries = PhraseLibraries::empty();
        for (index, row) in conf.system_libraries() {
            let name = row.system.as_ref().map(|name| PathBuf::from(name.as_ref()));
            let path = crate::input::build_path(
                data,
                name.as_deref().unwrap_or(Path::new("")).as_os_str(),
            );
            libraries
                .load_heap(index, &path)
                .map_err(|cause| LoadError { name, cause })?;
        }
        let model = bigram.then(|| {
            crate::data::BigramTables::attach(&data.join("bigram.db"), conf.lambda_f32())
                .unwrap_or_else(|_| crate::input::tool::abort())
        });
        Ok(Self {
            tables,
            libraries,
            model,
        })
    }
    // The matrix driver reads SEARCH_CONTINUED and aborts on a failed table query.
    /// Reports whether the index has a continuation row for `keys`.
    #[must_use]
    pub fn key_exists(&self, keys: &[crate::parser::ChewingKey]) -> bool {
        self.tables
            .key_exists(keys)
            .unwrap_or_else(|_| crate::input::tool::abort())
    }
    /// Runs n-best lookup with no user overlay. Returns an empty result when
    /// the lookup was opened with bigrams disabled.
    ///
    /// # Errors
    /// Returns decoder/model errors.
    pub fn phonetic(
        &self,
        prepared: &crate::engine::tool::PreparedLookup,
    ) -> Result<Vec<crate::engine::tool::LookupRow>, EngineError> {
        let Some(model) = &self.model else {
            return Ok(Vec::new());
        };
        crate::engine::tool::lookup(
            prepared,
            &self.tables.read(&self.libraries),
            &model.read_all(&self.libraries),
        )
    }
    /// Runs phrase segmentation with no user overlay. Returns an empty result
    /// when the lookup was opened with bigrams disabled.
    ///
    /// # Errors
    /// Returns decoder/model errors.
    pub fn phrase(&self, words: &[u32]) -> Result<Vec<PhraseToken>, EngineError> {
        let Some(model) = &self.model else {
            return Ok(Vec::new());
        };
        crate::engine::tool::phrase_lookup(
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

#[cfg(test)]
mod tests;
