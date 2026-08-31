//! The persistent orchestrator: the on-disk `try<name>` layout, status files,
//! candidate model files, intermediate cleanup, and stage-level resumability
//! that wrap the pure [`crate::pipeline`] stages into the full trainer run
//! (`segment.py` → `generate.py` → `estimate.py` → `tryprune.py` →
//! `evaluate.py`).
//!
//! Directory layout, mirroring `myconfig.py`:
//!
//! ```text
//! text_dir/   <doc>            raw corpus document (input)
//!             <doc>.segmented  segmented stream            + <doc>.segmented.status
//! model_dir/  model-candidates-N.db  candidate KMM text    + .status (Generate/Estimate)
//!             estimate.index         gathered scores
//!             estimate.sorted.index  scores, descending
//!             corpus.index.status    index-level resume markers
//! final_dir/  try<name>/  cwd.status  merged.db kmm_merged.text
//!                         pruned.db   kmm_pruned.text  interpolation2.text
//! ```
//!
//! Each stage is gated by its epoch in a status file: a signed stage is
//! skipped and its artifacts reloaded, so an interrupted run resumes at the
//! first unfinished stage.

use std::path::{Path, PathBuf};

use oxpinyin_core::{Dictionary, PhraseEntry, SyllableKey};
use oxpinyin_kmm::{KMixtureModel, import};
use oxpinyin_segment::Segmenter;

use crate::config::{
    self, ESTIMATE_INDEX, FINAL_MODEL_FILE_NAME, FINAL_STATUS_FILE_NAME, SORTED_ESTIMATE_INDEX,
    TrainConfig, candidate_model_name,
};
use crate::corpus::CorpusIndex;
use crate::error::TrainError;
use crate::pipeline::{
    self, CandidateModel, EvalInputs, SegmentMethod, SegmentedDoc, TrainOutcome,
};
use crate::status::{Stage, Status};

/// Where the trainer keeps corpus, candidates, and final workspaces.
#[derive(Clone, Debug)]
pub struct TrainerPaths {
    /// Raw + segmented corpus documents.
    pub text_dir: PathBuf,
    /// Candidate models and the estimate indexes.
    pub model_dir: PathBuf,
    /// `try<name>` final workspaces.
    pub final_dir: PathBuf,
}

/// The persistent trainer over one corpus index.
pub struct Trainer {
    config: TrainConfig,
    paths: TrainerPaths,
    method: SegmentMethod,
}

impl Trainer {
    /// A trainer with the given config, paths, and segmentation method.
    #[must_use]
    pub fn new(config: TrainConfig, paths: TrainerPaths, method: SegmentMethod) -> Self {
        Self {
            config,
            paths,
            method,
        }
    }

    /// Runs the whole workflow for one corpus index and `try<name>` workspace,
    /// returning the authoritative outcome (final model, λ, correction rate).
    ///
    /// Raw documents are read from `text_dir` via the index; the deleted model
    /// scores the candidates; `eval` supplies the system dictionary, phrase
    /// source, evaluation corpus, and deleted counts for the final λ.
    ///
    /// # Errors
    ///
    /// Returns the first stage's [`TrainError`].
    pub fn run<D, P>(
        &self,
        segmenter: &Segmenter,
        index: &CorpusIndex,
        scoring_deleted: &KMixtureModel,
        eval: &EvalInputs<'_, D, P>,
        tryname: &str,
    ) -> Result<TrainOutcome, TrainError>
    where
        D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
        D::Error: core::fmt::Display,
        P: oxpinyin_eval::PhraseSource,
    {
        create_dir(&self.paths.model_dir)?;
        create_dir(&self.paths.final_dir)?;

        let segmented = self.segment_stage(segmenter, index)?;
        let candidates = self.generate_stage(&segmented)?;
        let sorted = self.estimate_stage(candidates, scoring_deleted)?;
        let final_model = self.prune_stage(&sorted, tryname)?;
        self.evaluate_stage(&final_model, eval, tryname, sorted.models.len())
    }

    /// Segment stage: segment each raw document to `<doc>.segmented`, gated by
    /// the per-document `Segment` epoch. Returns the segmented documents.
    fn segment_stage(
        &self,
        segmenter: &Segmenter,
        index: &CorpusIndex,
    ) -> Result<Vec<SegmentedDoc>, TrainError> {
        let mut out = Vec::with_capacity(index.len());
        for entry in index.entries() {
            let raw_path = entry.raw_path(&self.paths.text_dir);
            let seg_path = entry.segmented_path(&self.paths.text_dir);
            let status_path = Status::path_for(&seg_path);

            let mut status = Status::load(&status_path)?;
            let text = if status.is_done(Stage::Segment)? && seg_path.is_file() {
                read(&seg_path)?
            } else {
                let raw = read_bytes(&raw_path)?;
                let text = match self.method {
                    SegmentMethod::Ngseg => {
                        segmenter.segment_bytes(&raw, false).map_err(|error| {
                            TrainError::Segment {
                                detail: error.to_string(),
                            }
                        })?
                    }
                    SegmentMethod::Spseg => segmenter.spseg_bytes(&raw, false),
                };
                write(&seg_path, &text)?;
                status.sign(Stage::Segment);
                status.store(&status_path)?;
                text
            };
            let size = text.len() as u64;
            out.push(SegmentedDoc {
                title: entry.title.clone(),
                text,
                size,
            });
        }
        Ok(out)
    }

    /// The index-level status path (resume markers for the generate stage).
    fn index_status_path(&self) -> PathBuf {
        Status::path_for(&self.paths.model_dir.join("corpus.index"))
    }

    /// Generate stage: build candidate models with rollover + the min-size
    /// filter, numbering and persisting each as `model-candidates-N.db` with a
    /// `Generate` status carrying its `GenerateStart`/`GenerateEnd`. Gated by
    /// the index-level `Generate` epoch; when signed, candidates are reloaded.
    fn generate_stage(
        &self,
        segmented: &[SegmentedDoc],
    ) -> Result<Vec<CandidateModel>, TrainError> {
        let index_status_path = self.index_status_path();
        let mut index_status = Status::load(&index_status_path)?;

        if index_status.is_done(Stage::Generate)? {
            return self.reload_candidates(index_status.generate_model_end.unwrap_or(0));
        }

        let candidates = pipeline::generate_candidates(&self.config, segmented)?;

        // Clean any stale candidate/report files past what we now emit, then
        // persist each candidate with its per-model Generate status.
        self.cleanup_candidates_from(candidates.len() as u32)?;
        for candidate in &candidates {
            let model_path = self.candidate_path(candidate.number);
            write(&model_path, &oxpinyin_kmm::export(&candidate.model))?;
            let mut status = Status::new();
            status.generate_start = Some(candidate.text_start);
            status.generate_end = Some(candidate.text_end);
            status.sign(Stage::Generate);
            status.store(&Status::path_for(&model_path))?;
        }

        index_status.generate_text_end = Some(segmented.len() as u64);
        index_status.generate_model_end = Some(candidates.len() as u32);
        index_status.sign(Stage::Generate);
        index_status.store(&index_status_path)?;

        Ok(candidates)
    }

    /// Reloads persisted candidate models `0..count` (resume path).
    fn reload_candidates(&self, count: u32) -> Result<Vec<CandidateModel>, TrainError> {
        let mut out = Vec::with_capacity(count as usize);
        for number in 0..count {
            let model_path = self.candidate_path(number);
            let model = import(&read(&model_path)?).map_err(|error| TrainError::Kmm {
                stage: "reload-candidate",
                detail: error.to_string(),
            })?;
            let status = Status::load(&Status::path_for(&model_path))?;
            out.push(CandidateModel {
                number,
                model,
                text_start: status.generate_start.unwrap_or(0),
                text_end: status.generate_end.unwrap_or(0),
                aggregated_size: 0,
            });
        }
        Ok(out)
    }

    /// Removes candidate model + report + status files numbered `from..` until
    /// a gap, so a re-run never leaves a higher-numbered stale candidate
    /// behind (`cleanupFiles`).
    fn cleanup_candidates_from(&self, from: u32) -> Result<(), TrainError> {
        let mut number = from;
        loop {
            let model_path = self.candidate_path(number);
            let existed = remove_if_present(&model_path)?
                | remove_if_present(&report_path(&model_path))?
                | remove_if_present(&Status::path_for(&model_path))?;
            if !existed {
                break;
            }
            number += 1;
        }
        Ok(())
    }

    /// Estimate stage: score each candidate, persist its `EstimateScore`,
    /// gather `estimate.index`, sort `estimate.sorted.index`. Gated by the
    /// index-level `Estimate` epoch.
    fn estimate_stage(
        &self,
        candidates: Vec<CandidateModel>,
        deleted: &KMixtureModel,
    ) -> Result<pipeline::SortedCandidates, TrainError> {
        let index_status_path = self.index_status_path();
        let mut index_status = Status::load(&index_status_path)?;

        let scored = pipeline::score_candidates(candidates, deleted)?;
        // Persist each candidate's score into its status file.
        for candidate in &scored {
            let number = candidate_number(&candidate.candidate.model_name);
            if let Some(number) = number {
                let model_path = self.candidate_path(number);
                let status_path = Status::path_for(&model_path);
                let mut status = Status::load(&status_path)?;
                status.estimate_score = Some(candidate.candidate.score);
                status.sign(Stage::Estimate);
                status.store(&status_path)?;
            }
        }

        let sorted = pipeline::gather_and_sort(scored);
        write(
            &self.paths.model_dir.join(ESTIMATE_INDEX),
            &sorted.gathered.to_index_text(),
        )?;
        write(
            &self.paths.model_dir.join(SORTED_ESTIMATE_INDEX),
            &sorted.sorted.to_index_text(),
        )?;

        index_status.sign(Stage::Estimate);
        index_status.store(&index_status_path)?;
        Ok(sorted)
    }

    /// Prune stage: create `try<name>`, merge the top N, validate, export,
    /// prune, validate, export, convert to `interpolation2.text`. Stores the
    /// prune parameters and model size, signs `Prune`.
    fn prune_stage(
        &self,
        sorted: &pipeline::SortedCandidates,
        tryname: &str,
    ) -> Result<pipeline::FinalModel, TrainError> {
        let trydir = self.try_dir(tryname);
        let cwd_status_path = trydir.join(FINAL_STATUS_FILE_NAME);

        // Resume: a fully pruned workspace is reused.
        if cwd_status_path.is_file() {
            let status = Status::load(&cwd_status_path)?;
            let interp_path = trydir.join(FINAL_MODEL_FILE_NAME);
            if status.is_done(Stage::Prune)? && interp_path.is_file() {
                let interpolation2 = read(&interp_path)?;
                let model_size = interpolation2.len() as u64;
                return Ok(pipeline::FinalModel {
                    kmm_merged_text: read(&trydir.join("kmm_merged.text")).unwrap_or_default(),
                    kmm_pruned_text: read(&trydir.join("kmm_pruned.text")).unwrap_or_default(),
                    interpolation2,
                    merge_number: status.prune_merge_number.unwrap_or(0) as usize,
                    model_size,
                });
            }
        }

        create_dir(&trydir)?;
        let mut status = Status::new();
        status.prune_merge_number = Some(self.config.merge_number as u64);
        status.prune_k = Some(u64::from(self.config.prune_k));
        status.prune_cdf = Some(self.config.prune_cdf);
        status.store(&cwd_status_path)?;

        let final_model = pipeline::merge_prune_convert(&self.config, sorted)?;

        write(
            &trydir.join("kmm_merged.text"),
            &final_model.kmm_merged_text,
        )?;
        write(
            &trydir.join("kmm_pruned.text"),
            &final_model.kmm_pruned_text,
        )?;
        write(
            &trydir.join(FINAL_MODEL_FILE_NAME),
            &final_model.interpolation2,
        )?;

        status.prune_model_size = Some(final_model.model_size);
        status.sign(Stage::Prune);
        status.store(&cwd_status_path)?;
        Ok(final_model)
    }

    /// Evaluate stage: estimate λ over the final model, apply it, decode the
    /// evaluation corpus. Stores `EvaluateAverageLambda`/`EvaluateCorrectionRate`,
    /// signs `Evaluate`.
    fn evaluate_stage<D, P>(
        &self,
        final_model: &pipeline::FinalModel,
        eval: &EvalInputs<'_, D, P>,
        tryname: &str,
        candidate_count: usize,
    ) -> Result<TrainOutcome, TrainError>
    where
        D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
        D::Error: core::fmt::Display,
        P: oxpinyin_eval::PhraseSource,
    {
        let trydir = self.try_dir(tryname);
        let cwd_status_path = trydir.join(FINAL_STATUS_FILE_NAME);
        let mut status = Status::load(&cwd_status_path)?;

        let outcome = pipeline::evaluate_model(
            &final_model.interpolation2,
            eval.deleted,
            eval.dictionary,
            eval.source,
            eval.evals_text,
        )?;

        status.evaluate_average_lambda = Some(outcome.average_lambda.as_f64());
        status.evaluate_correction_rate = Some(outcome.report.rate);
        status.sign(Stage::Evaluate);
        status.store(&cwd_status_path)?;

        Ok(TrainOutcome {
            interpolation2: final_model.interpolation2.clone(),
            average_lambda: outcome.average_lambda,
            correction_rate: outcome.report.rate,
            report: outcome.report,
            candidate_count,
        })
    }

    fn candidate_path(&self, number: u32) -> PathBuf {
        self.paths.model_dir.join(candidate_model_name(number))
    }

    fn try_dir(&self, tryname: &str) -> PathBuf {
        self.paths.final_dir.join(format!("try{tryname}"))
    }
}

/// The candidate number encoded in a `model-candidates-N.db` filename.
fn candidate_number(model_name: &str) -> Option<u32> {
    model_name
        .strip_prefix("model-candidates-")?
        .strip_suffix(config::MODEL_POSTFIX)?
        .parse()
        .ok()
}

fn report_path(model_path: &Path) -> PathBuf {
    let mut name = model_path.as_os_str().to_owned();
    name.push(config::REPORT_POSTFIX);
    PathBuf::from(name)
}

fn create_dir(path: &Path) -> Result<(), TrainError> {
    std::fs::create_dir_all(path).map_err(|error| TrainError::io(path, error))
}

fn read(path: &Path) -> Result<String, TrainError> {
    std::fs::read_to_string(path).map_err(|error| TrainError::io(path, error))
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, TrainError> {
    std::fs::read(path).map_err(|error| TrainError::io(path, error))
}

fn write(path: &Path, text: &str) -> Result<(), TrainError> {
    if let Some(parent) = path.parent() {
        create_dir(parent)?;
    }
    std::fs::write(path, text).map_err(|error| TrainError::io(path, error))
}

/// Removes a file if it exists; returns whether it did. A not-found is not an
/// error (`os.access` then `os.unlink`).
fn remove_if_present(path: &Path) -> Result<bool, TrainError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(TrainError::io(path, error)),
    }
}
