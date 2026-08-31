//! The main-pipeline stages as pure functions over in-memory data
//! (`segment.py` → `generate.py` → `estimate.py` → `tryprune.py` →
//! `evaluate.py`). The persistent orchestrator ([`crate::workspace`]) drives
//! these and adds status files, candidate files, and cleanup; keeping the
//! transforms pure here makes the whole pipeline unit-testable from a raw
//! corpus without touching a filesystem.

use oxpinyin_core::{Dictionary, PhraseEntry, SyllableKey};
use oxpinyin_data::Lambda;
use oxpinyin_eval::{
    EvalReport, PhraseSource, build_model, correction_rate, estimate_lambda, parse_eval_corpus,
    parse_interpolation2,
};
use oxpinyin_kmm::{
    KMixtureModel, estimate, export, kmm_text_to_interpolation, merge_into, prune, validate,
};
use oxpinyin_lambda::DeletedCounts;
use oxpinyin_segment::Segmenter;

use crate::candidate::{Candidate, CandidateIndex};
use crate::config::{TrainConfig, candidate_model_name};
use crate::error::TrainError;

/// How the segment stage segments (`segment.py`: default `ngseg`, `--fast`
/// `spseg`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SegmentMethod {
    /// `ngseg` — the bigram-scored segmentation (the pipeline default).
    #[default]
    Ngseg,
    /// `spseg` — the fewest-words shortest path (`--fast`).
    Spseg,
}

/// A segmented corpus document. `size` is the segmented byte length — the
/// quantity both the minimum-file-size gate and the candidate-rollover
/// accumulator weigh (`get_file_length(infile + '.segmented')`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentedDoc {
    /// The document title (from the corpus index).
    pub title: String,
    /// The segmented token stream (`token phrase` / `null_token` lines).
    pub text: String,
    /// The segmented byte length.
    pub size: u64,
}

/// Segments raw corpus documents (`title`, raw bytes) into token streams.
///
/// # Errors
///
/// Returns [`TrainError::Segment`] when the segmenter's bigram backend fails.
pub fn segment_documents(
    segmenter: &Segmenter,
    method: SegmentMethod,
    documents: &[(String, Vec<u8>)],
) -> Result<Vec<SegmentedDoc>, TrainError> {
    let mut out = Vec::with_capacity(documents.len());
    for (title, raw) in documents {
        // ngseg/spseg read a whole file; `extra_enter = false` matches the
        // `ngseg -o` the segment stage runs (its differential pins this).
        let text = match method {
            SegmentMethod::Ngseg => {
                segmenter
                    .segment_bytes(raw, false)
                    .map_err(|error| TrainError::Segment {
                        detail: error.to_string(),
                    })?
            }
            SegmentMethod::Spseg => segmenter.spseg_bytes(raw, false),
        };
        let size = text.len() as u64;
        out.push(SegmentedDoc {
            title: title.clone(),
            text,
            size,
        });
    }
    Ok(out)
}

/// A generated candidate model with the half-open text-index range it covers
/// (`GenerateStart`/`GenerateEnd`).
#[derive(Clone, Debug)]
pub struct CandidateModel {
    /// Candidate number (`getCandidateModelName`).
    pub number: u32,
    /// The accumulated K-mixture model.
    pub model: KMixtureModel,
    /// First text index covered (`GenerateStart`).
    pub text_start: u64,
    /// One past the last text index covered (`GenerateEnd`).
    pub text_end: u64,
    /// The aggregated segmented byte size of the covered documents.
    pub aggregated_size: u64,
}

/// Generates candidate models from segmented documents, reproducing
/// `generate.py`'s accumulate-and-roll-over loop: documents are added into the
/// current candidate in index order; a segmented file below the minimum size
/// is skipped; once the aggregated segmented size exceeds
/// `candidate_model_size` the candidate is closed and the next begins. The
/// document that trips the threshold is part of the candidate it closes.
///
/// Unlike upstream, an empty trailing candidate (when the last document closed
/// one exactly) is not emitted: it has no model file, so it contributes no
/// score and no merge input — the observable candidate set is identical. The
/// numbering of the non-empty candidates is unchanged.
///
/// # Errors
///
/// Returns [`TrainError::Kmm`] when `add_document` rejects a segmented stream.
pub fn generate_candidates(
    config: &TrainConfig,
    documents: &[SegmentedDoc],
) -> Result<Vec<CandidateModel>, TrainError> {
    let params = config.generate_params();
    let mut candidates = Vec::new();

    let mut current = KMixtureModel::new();
    let mut has_document = false;
    let mut text_start: u64 = 0;
    let mut aggregated: u64 = 0;
    let mut number: u32 = 0;

    for (index, document) in documents.iter().enumerate() {
        let index = index as u64;
        // Minimum-file-size filter (`infilesize < getMinimumFileSize()`).
        if document.size < config.minimum_file_size {
            continue;
        }
        current
            .add_document(&document.text, params)
            .map_err(|error| TrainError::Kmm {
                stage: "generate",
                detail: error.to_string(),
            })?;
        has_document = true;
        aggregated = aggregated.saturating_add(document.size);

        if aggregated > config.candidate_model_size {
            let text_end = index + 1;
            candidates.push(CandidateModel {
                number,
                model: std::mem::replace(&mut current, KMixtureModel::new()),
                text_start,
                text_end,
                aggregated_size: aggregated,
            });
            number += 1;
            text_start = text_end;
            aggregated = 0;
            has_document = false;
        }
    }

    // The trailing partial candidate (only when it actually holds a document).
    if has_document {
        candidates.push(CandidateModel {
            number,
            model: current,
            text_start,
            text_end: documents.len() as u64,
            aggregated_size: aggregated,
        });
    }

    Ok(candidates)
}

/// A scored candidate: its record and its model, kept together so the merge
/// stage can act on the sorted order.
#[derive(Clone, Debug)]
pub struct ScoredCandidate {
    /// The gather-index record (`subdir#model#score`), with `subdir` empty for
    /// the single in-memory index.
    pub candidate: Candidate,
    /// The candidate model.
    pub model: KMixtureModel,
}

/// Scores each candidate with `estimate_k_mixture_model` against the deleted
/// model — the candidate's `EstimateScore` (average λ) — and gathers the
/// records (`estimate.py`'s `walkThroughModels` + `gatherModels`).
///
/// # Errors
///
/// Returns [`TrainError::Kmm`] when estimation fails.
pub fn score_candidates(
    candidates: Vec<CandidateModel>,
    deleted: &KMixtureModel,
) -> Result<Vec<ScoredCandidate>, TrainError> {
    let mut scored = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let score = estimate(&candidate.model, deleted)
            .map_err(|error| TrainError::Kmm {
                stage: "estimate",
                detail: error.to_string(),
            })?
            .average;
        scored.push(ScoredCandidate {
            candidate: Candidate {
                subdir: String::new(),
                model_name: candidate_model_name(candidate.number),
                score,
            },
            model: candidate.model,
        });
    }
    Ok(scored)
}

/// The gather + sort of scored candidates (`gatherModels` → `sortModels`):
/// the unsorted index, the sorted index, and the scored candidates in sorted
/// (descending) order carrying their models for the merge.
pub struct SortedCandidates {
    /// `estimate.index` — records in gather order.
    pub gathered: CandidateIndex,
    /// `estimate.sorted.index` — records by score descending.
    pub sorted: CandidateIndex,
    /// The scored candidates in the same descending order, with their models.
    pub models: Vec<ScoredCandidate>,
}

/// Gathers and sorts scored candidates by score descending (stable), matching
/// `sortModels`. The models travel with the records so the merge stage never
/// has to re-associate a sorted record with its model.
#[must_use]
pub fn gather_and_sort(scored: Vec<ScoredCandidate>) -> SortedCandidates {
    let gathered =
        CandidateIndex::from_candidates(scored.iter().map(|s| s.candidate.clone()).collect());
    let mut models = scored;
    // Stable, descending — the same order `CandidateIndex::sorted_by_score_desc`
    // produces for the persisted index.
    models.sort_by(|a, b| b.candidate.score.total_cmp(&a.candidate.score));
    let sorted =
        CandidateIndex::from_candidates(models.iter().map(|s| s.candidate.clone()).collect());
    SortedCandidates {
        gathered,
        sorted,
        models,
    }
}

/// The final pruned model in every form the workflow emits (`tryprune.py`).
#[derive(Clone, Debug)]
pub struct FinalModel {
    /// `kmm_merged.text` — the merged model before pruning.
    pub kmm_merged_text: String,
    /// `kmm_pruned.text` — the merged model after pruning.
    pub kmm_pruned_text: String,
    /// `interpolation2.text` — the final model (the pruned model converted).
    pub interpolation2: String,
    /// How many candidates were merged (`PruneMergeNumber`).
    pub merge_number: usize,
    /// The final interpolation model size in bytes (`PruneModelSize`).
    pub model_size: u64,
}

/// Merges the top `merge_number` sorted candidates, validates, exports,
/// prunes, validates, exports, and converts to `interpolation2.text`
/// (`tryprune.py`'s `mergeSomeModels` → validate → export → prune → validate →
/// export → convert). The sorted models must be in descending score order,
/// which [`gather_and_sort`] guarantees and [`CandidateIndex::top_n`] re-checks.
///
/// # Errors
///
/// Returns [`TrainError`] when there are too few candidates, the order is not
/// descending, or a KMM stage fails.
pub fn merge_prune_convert(
    config: &TrainConfig,
    sorted: &SortedCandidates,
) -> Result<FinalModel, TrainError> {
    // Re-check the descending/≤1 order over exactly the top N (mergeSomeModels).
    let top = sorted.sorted.top_n(config.merge_number)?;
    debug_assert_eq!(top.len(), config.merge_number);

    // Merge in sorted order (validate each first, as tryprune does).
    let mut merged = KMixtureModel::new();
    for scored in sorted.models.iter().take(config.merge_number) {
        validate(&scored.model).map_err(|error| TrainError::Kmm {
            stage: "validate(candidate)",
            detail: error.to_string(),
        })?;
        merge_into(&mut merged, &scored.model).map_err(|error| TrainError::Kmm {
            stage: "merge",
            detail: error.to_string(),
        })?;
    }
    validate(&merged).map_err(|error| TrainError::Kmm {
        stage: "validate(merged)",
        detail: error.to_string(),
    })?;
    let kmm_merged_text = export(&merged);

    // Prune a copy, validate, export.
    let mut pruned = merged.clone();
    prune(&mut pruned, config.prune_k, config.prune_cdf).map_err(|error| TrainError::Kmm {
        stage: "prune",
        detail: error.to_string(),
    })?;
    validate(&pruned).map_err(|error| TrainError::Kmm {
        stage: "validate(pruned)",
        detail: error.to_string(),
    })?;
    let kmm_pruned_text = export(&pruned);

    // Convert the pruned KMM text to the final interpolation model.
    let interpolation2 =
        kmm_text_to_interpolation(&kmm_pruned_text).map_err(|error| TrainError::Kmm {
            stage: "to-interpolation",
            detail: error.to_string(),
        })?;
    let model_size = interpolation2.len() as u64;

    Ok(FinalModel {
        kmm_merged_text,
        kmm_pruned_text,
        interpolation2,
        merge_number: config.merge_number,
        model_size,
    })
}

/// The evaluation outcome: the applied λ and the correction-rate report
/// (`evaluate.py`'s `estimateModel` → `modifyLambda` → `evaluateModel`).
#[derive(Clone, Debug)]
pub struct EvalOutcome {
    /// The estimated-and-applied average λ (`EvaluateAverageLambda`).
    pub average_lambda: Lambda,
    /// The correction-rate report (`EvaluateCorrectionRate` is `report.rate`).
    pub report: EvalReport,
}

/// Estimates λ over the final model against the deleted counts, applies it,
/// and decodes the evaluation corpus to a correction rate — the native
/// `evaluate.py`. Reuses `oxpinyin-eval` end to end.
///
/// # Errors
///
/// Returns [`TrainError::Eval`] when λ cannot be estimated or the corpus is
/// malformed.
pub fn evaluate_model<D, P>(
    interpolation2: &str,
    deleted: &DeletedCounts,
    dictionary: &D,
    source: &P,
    evals_text: &str,
) -> Result<EvalOutcome, TrainError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
    P: PhraseSource,
{
    let counts = parse_interpolation2(interpolation2);
    let lambda = estimate_lambda(&counts, deleted).map_err(|error| TrainError::Eval {
        detail: error.to_string(),
    })?;
    // Floored over the system lexicon, as `evaluate.py`'s `make` rebuild is.
    let model = build_model(&counts, lambda, source.lexicon_tokens());
    let sentences = parse_eval_corpus(evals_text).map_err(|error| TrainError::Eval {
        detail: error.to_string(),
    })?;
    let report = correction_rate(dictionary, &model, source, &sentences).map_err(|error| {
        TrainError::Eval {
            detail: error.to_string(),
        }
    })?;
    Ok(EvalOutcome {
        average_lambda: lambda,
        report,
    })
}

/// The whole end-to-end outcome (`oxpinyin-train`'s authoritative result).
#[derive(Clone, Debug)]
pub struct TrainOutcome {
    /// The final `interpolation2.text`.
    pub interpolation2: String,
    /// The applied average λ.
    pub average_lambda: Lambda,
    /// The measured correction rate (`passed / tested`).
    pub correction_rate: f64,
    /// The full evaluation report.
    pub report: EvalReport,
    /// The number of candidates generated.
    pub candidate_count: usize,
}

/// The assembled type bundle a caller passes to [`run_pipeline`] for the
/// evaluate stage: the system dictionary, the phrase source over it, and the
/// evaluation corpus.
pub struct EvalInputs<'a, D, P> {
    /// The system dictionary the decode ranks phrases against.
    pub dictionary: &'a D,
    /// The phrase source (best pronunciation + text per token).
    pub source: &'a P,
    /// The evaluation corpus (`evals2.text`).
    pub evals_text: &'a str,
    /// The deleted counts for the final λ estimation.
    pub deleted: &'a DeletedCounts,
}

/// Runs the whole main pipeline in memory from segmented documents to the
/// final model, λ, and correction rate — segment is done by the caller (it
/// owns the [`Segmenter`]); this drives generate → estimate → sort → merge →
/// prune → convert → evaluate. The persistent orchestrator adds files and
/// resumability around the same calls.
///
/// # Errors
///
/// Propagates any stage's [`TrainError`].
pub fn run_pipeline<D, P>(
    config: &TrainConfig,
    documents: &[SegmentedDoc],
    scoring_deleted: &KMixtureModel,
    eval: &EvalInputs<'_, D, P>,
) -> Result<TrainOutcome, TrainError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
    P: PhraseSource,
{
    let candidates = generate_candidates(config, documents)?;
    let candidate_count = candidates.len();
    let scored = score_candidates(candidates, scoring_deleted)?;
    let sorted = gather_and_sort(scored);
    let final_model = merge_prune_convert(config, &sorted)?;
    let outcome = evaluate_model(
        &final_model.interpolation2,
        eval.deleted,
        eval.dictionary,
        eval.source,
        eval.evals_text,
    )?;
    Ok(TrainOutcome {
        interpolation2: final_model.interpolation2,
        average_lambda: outcome.average_lambda,
        correction_rate: outcome.report.rate,
        report: outcome.report,
        candidate_count,
    })
}
