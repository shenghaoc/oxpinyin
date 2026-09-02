//! Typed status files and the epoch mechanism (`trainer/lib/utils.py`).
//!
//! Upstream persists each stage's progress in a JSON status file — a flat
//! `dict` of `"<Stage>Epoch"` markers and a handful of scalar fields — and
//! gates re-runs with `check_epoch`/`sign_epoch`: a stage whose epoch in the
//! file equals the config epoch is done (skip), a smaller or absent epoch is
//! not done (run), and a larger epoch is a hard error (the file came from a
//! newer trainer). This module is the typed reproduction: [`Stage`] is the
//! enum of main-pipeline passes, [`Status`] is a struct of explicit typed
//! fields (no untyped dict), and it round-trips the same flat-JSON shape so a
//! run started by the Python trainer and one started here are mutually
//! resumable.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::error::TrainError;

/// A main-pipeline pass, in workflow order. Each has a fixed config epoch —
/// all `1` in `myconfig.py`'s `m_current_epoch` — that [`Status::sign`]
/// stamps and [`Status::is_done`] checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// `ngseg`/`spseg` raw → segmented (`SegmentEpoch`).
    Segment,
    /// `gen_k_mixture_model` candidate generation (`GenerateEpoch`).
    Generate,
    /// `estimate_k_mixture_model` scoring + gather + sort (`EstimateEpoch`).
    Estimate,
    /// merge → prune → export → convert (`PruneEpoch`).
    Prune,
    /// `estimate_interpolation` + `eval_correction_rate` (`EvaluateEpoch`).
    Evaluate,
}

impl Stage {
    /// The `"<Stage>Epoch"` status key.
    #[must_use]
    pub const fn epoch_key(self) -> &'static str {
        match self {
            Self::Segment => "SegmentEpoch",
            Self::Generate => "GenerateEpoch",
            Self::Estimate => "EstimateEpoch",
            Self::Prune => "PruneEpoch",
            Self::Evaluate => "EvaluateEpoch",
        }
    }

    /// The config epoch this build signs and checks against
    /// (`myconfig.py getEpochs`; every main-pipeline pass is `1`).
    #[must_use]
    pub const fn config_epoch(self) -> u32 {
        1
    }

    /// This pass's human name for diagnostics.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Segment => "Segment",
            Self::Generate => "Generate",
            Self::Estimate => "Estimate",
            Self::Prune => "Prune",
            Self::Evaluate => "Evaluate",
        }
    }
}

/// Whether a stage is done in a status file (`check_epoch`'s tri-state, minus
/// the panic: the "newer epoch" case is a typed error the caller surfaces).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpochState {
    /// The stage's epoch is absent or below the config epoch — not done, run.
    NotDone,
    /// The stage's epoch equals the config epoch — done, skip.
    Done,
}

/// A typed status file. Every field upstream stores in the flat status `dict`
/// is an explicit typed slot; unset fields are absent from the file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// Signed epoch per stage (`"<Stage>Epoch"` → epoch number).
    epochs: BTreeMap<Stage, u32>,
    /// `GenerateStart` — first text index this candidate covers (per-model).
    pub generate_start: Option<u64>,
    /// `GenerateEnd` — one past the last text index this candidate covers.
    pub generate_end: Option<u64>,
    /// `GenerateTextEnd` — resume point: texts consumed so far (index-level).
    pub generate_text_end: Option<u64>,
    /// `GenerateModelEnd` — resume point: candidates emitted so far.
    pub generate_model_end: Option<u32>,
    /// `EstimateScore` — the candidate's average λ (its ranking score).
    pub estimate_score: Option<f64>,
    /// `PruneMergeNumber` — how many candidates were merged.
    pub prune_merge_number: Option<u64>,
    /// `PruneK` — the prune `-k` used.
    pub prune_k: Option<u64>,
    /// `PruneCDF` — the prune `--CDF` used.
    pub prune_cdf: Option<f64>,
    /// `PruneModelSize` — the final interpolation model size, in bytes.
    pub prune_model_size: Option<u64>,
    /// `EvaluateAverageLambda` — the final applied λ.
    pub evaluate_average_lambda: Option<f64>,
    /// `EvaluateCorrectionRate` — the measured correction rate.
    pub evaluate_correction_rate: Option<f64>,
}

impl Status {
    /// A fresh, empty status (`load_status` on a missing file returns `{}`).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `check_epoch(obj, stage)` without the panic branch.
    ///
    /// # Errors
    ///
    /// Returns [`TrainError::EpochTooNew`] when the file's epoch exceeds this
    /// build's config epoch (upstream raises `EpochError`).
    pub fn epoch_state(&self, stage: Stage) -> Result<EpochState, TrainError> {
        let known = stage.config_epoch();
        match self.epochs.get(&stage).copied() {
            None => Ok(EpochState::NotDone),
            Some(found) if found < known => Ok(EpochState::NotDone),
            Some(found) if found == known => Ok(EpochState::Done),
            Some(found) => Err(TrainError::EpochTooNew {
                stage: stage.name(),
                found,
                known,
            }),
        }
    }

    /// Whether `stage` is signed at the current epoch (`check_epoch` == True).
    ///
    /// # Errors
    ///
    /// Propagates [`TrainError::EpochTooNew`].
    pub fn is_done(&self, stage: Stage) -> Result<bool, TrainError> {
        Ok(self.epoch_state(stage)? == EpochState::Done)
    }

    /// `sign_epoch(obj, stage)`: stamp the stage at this build's config epoch.
    pub fn sign(&mut self, stage: Stage) {
        self.epochs.insert(stage, stage.config_epoch());
    }

    /// The signed epoch of `stage`, if any.
    #[must_use]
    pub fn epoch(&self, stage: Stage) -> Option<u32> {
        self.epochs.get(&stage).copied()
    }

    /// Loads a status file, returning an empty status when it does not exist
    /// (`load_status`: an unreadable file yields `{}`).
    ///
    /// # Errors
    ///
    /// Returns [`TrainError`] when the file exists but cannot be read or is
    /// not the flat-JSON status shape.
    pub fn load(path: &Path) -> Result<Self, TrainError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::new()),
            Err(error) => Err(TrainError::io(path, error)),
        }
    }

    /// Writes the status file (`store_status`: `json.dumps` of the flat dict).
    /// The bytes go to a temporary sibling that is then renamed over the
    /// target (atomic within one directory), so an interruption mid-write
    /// leaves the previous status intact instead of a truncated file the
    /// next run would refuse as malformed.
    ///
    /// # Errors
    ///
    /// Returns [`TrainError::Io`] when the file cannot be written or renamed.
    pub fn store(&self, path: &Path) -> Result<(), TrainError> {
        let mut temp = path.as_os_str().to_owned();
        temp.push(".tmp");
        let temp = PathBuf::from(temp);
        std::fs::write(&temp, self.to_json()).map_err(|error| TrainError::io(&temp, error))?;
        std::fs::rename(&temp, path).map_err(|error| TrainError::io(path, error))
    }

    /// The status path for a base file (`<file><STATUS_POSTFIX>`).
    #[must_use]
    pub fn path_for(base: &Path) -> PathBuf {
        let mut name = base.as_os_str().to_owned();
        name.push(crate::config::STATUS_POSTFIX);
        PathBuf::from(name)
    }
}

/// A status scalar: the two JSON number shapes the trainer stores. Integers
/// print without a decimal point (`json.dumps(3)` → `3`); floats print with
/// Python's `repr`, which for the values here is the shortest round-tripping
/// decimal — Rust's `{}` for `f64` matches that shortest-round-trip rule.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Scalar {
    Int(u64),
    Float(f64),
}

impl fmt::Display for Scalar {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(value) => write!(formatter, "{value}"),
            // Emit a decimal point so the value round-trips as a float, the
            // way `json.dumps` renders a Python float (e.g. `1.0`, not `1`).
            Self::Float(value) => {
                if value.fract() == 0.0 && value.is_finite() {
                    write!(formatter, "{value:.1}")
                } else {
                    write!(formatter, "{value}")
                }
            }
        }
    }
}

impl Status {
    /// The flat-JSON body, keys in a fixed canonical order for determinism.
    fn to_json(&self) -> String {
        let mut fields: Vec<(String, Scalar)> = Vec::new();
        // Epochs first, in stage order, then the scalar fields in a stable
        // canonical order. Order is immaterial to `json.loads`, but a fixed
        // order keeps the output a pure function of the state.
        for stage in [
            Stage::Segment,
            Stage::Generate,
            Stage::Estimate,
            Stage::Prune,
            Stage::Evaluate,
        ] {
            if let Some(epoch) = self.epochs.get(&stage) {
                fields.push((stage.epoch_key().to_owned(), Scalar::Int(u64::from(*epoch))));
            }
        }
        let mut push_int = |key: &str, value: Option<u64>| {
            if let Some(value) = value {
                fields.push((key.to_owned(), Scalar::Int(value)));
            }
        };
        push_int("GenerateStart", self.generate_start);
        push_int("GenerateEnd", self.generate_end);
        push_int("GenerateTextEnd", self.generate_text_end);
        push_int("GenerateModelEnd", self.generate_model_end.map(u64::from));
        push_int("PruneMergeNumber", self.prune_merge_number);
        push_int("PruneK", self.prune_k);
        push_int("PruneModelSize", self.prune_model_size);
        let mut push_float = |key: &str, value: Option<f64>| {
            if let Some(value) = value {
                fields.push((key.to_owned(), Scalar::Float(value)));
            }
        };
        push_float("EstimateScore", self.estimate_score);
        push_float("PruneCDF", self.prune_cdf);
        push_float("EvaluateAverageLambda", self.evaluate_average_lambda);
        push_float("EvaluateCorrectionRate", self.evaluate_correction_rate);

        let mut json = String::from("{");
        for (index, (key, value)) in fields.iter().enumerate() {
            if index > 0 {
                json.push_str(", ");
            }
            json.push_str(&format!("\"{key}\": {value}"));
        }
        json.push('}');
        json
    }

    /// Parses the flat-JSON status shape (`{"key": number, ...}`) the trainer
    /// writes. Tolerant of whitespace; unknown keys are ignored so a status
    /// file carrying word-recognizer or punctuation markers still loads.
    fn parse(text: &str) -> Result<Self, TrainError> {
        let mut status = Self::new();
        for (key, scalar) in parse_flat_object(text)? {
            status.assign(&key, scalar);
        }
        Ok(status)
    }

    /// Dispatches one parsed `key: number` pair into its typed slot. Unknown
    /// keys (and epoch keys for non-main-pipeline stages) are ignored.
    fn assign(&mut self, key: &str, scalar: Scalar) {
        let as_u64 = || match scalar {
            Scalar::Int(value) => value,
            Scalar::Float(value) => value as u64,
        };
        let as_f64 = || match scalar {
            Scalar::Int(value) => value as f64,
            Scalar::Float(value) => value,
        };
        match key {
            "SegmentEpoch" => drop(self.epochs.insert(Stage::Segment, as_u64() as u32)),
            "GenerateEpoch" => drop(self.epochs.insert(Stage::Generate, as_u64() as u32)),
            "EstimateEpoch" => drop(self.epochs.insert(Stage::Estimate, as_u64() as u32)),
            "PruneEpoch" => drop(self.epochs.insert(Stage::Prune, as_u64() as u32)),
            "EvaluateEpoch" => drop(self.epochs.insert(Stage::Evaluate, as_u64() as u32)),
            "GenerateStart" => self.generate_start = Some(as_u64()),
            "GenerateEnd" => self.generate_end = Some(as_u64()),
            "GenerateTextEnd" => self.generate_text_end = Some(as_u64()),
            "GenerateModelEnd" => self.generate_model_end = Some(as_u64() as u32),
            "EstimateScore" => self.estimate_score = Some(as_f64()),
            "PruneMergeNumber" => self.prune_merge_number = Some(as_u64()),
            "PruneK" => self.prune_k = Some(as_u64()),
            "PruneCDF" => self.prune_cdf = Some(as_f64()),
            "PruneModelSize" => self.prune_model_size = Some(as_u64()),
            "EvaluateAverageLambda" => self.evaluate_average_lambda = Some(as_f64()),
            "EvaluateCorrectionRate" => self.evaluate_correction_rate = Some(as_f64()),
            _ => {} // ignore unknown keys
        }
    }
}

/// Parses a flat JSON object of string keys to numeric values — exactly the
/// shape `json.dumps` produces for the trainer's status dict. Panic-free;
/// anything outside that shape is a [`TrainError::Malformed`].
fn parse_flat_object(text: &str) -> Result<Vec<(String, Scalar)>, TrainError> {
    let bytes = text.trim();
    let inner = bytes
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .ok_or_else(|| TrainError::Malformed {
            detail: format!("status is not a JSON object: {text:?}"),
        })?;
    let inner = inner.trim();
    if inner.is_empty() {
        return Ok(Vec::new());
    }

    let mut pairs = Vec::new();
    for entry in split_top_level(inner) {
        let entry = entry.trim();
        let (raw_key, raw_value) = entry.split_once(':').ok_or_else(|| TrainError::Malformed {
            detail: format!("status entry has no ':' : {entry:?}"),
        })?;
        let key = parse_json_string(raw_key.trim())?;
        let value = parse_scalar(raw_value.trim())?;
        pairs.push((key, value));
    }
    Ok(pairs)
}

/// Splits object members on top-level commas. The trainer's values are bare
/// numbers, so a comma only ever separates members — but quotes are honoured
/// so a stray comma inside a (future) string key never splits mid-token.
fn split_top_level(inner: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;
    for character in inner.chars() {
        match character {
            '"' if !escaped => {
                in_string = !in_string;
                current.push(character);
            }
            '\\' if in_string => {
                escaped = !escaped;
                current.push(character);
                continue;
            }
            ',' if !in_string => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(character),
        }
        escaped = false;
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

/// Parses a JSON string literal (the key). Keys the trainer writes are ASCII
/// identifiers with no escapes; this handles the simple-escape subset and
/// rejects anything unterminated.
fn parse_json_string(token: &str) -> Result<String, TrainError> {
    let inner = token
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .ok_or_else(|| TrainError::Malformed {
            detail: format!("status key is not a string: {token:?}"),
        })?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => {
                    return Err(TrainError::Malformed {
                        detail: format!("unsupported escape \\{other} in status key"),
                    });
                }
                None => {
                    return Err(TrainError::Malformed {
                        detail: "status key ends in a backslash".to_owned(),
                    });
                }
            }
        } else {
            out.push(character);
        }
    }
    Ok(out)
}

/// Parses a JSON number into an integer or float scalar. Panic-free.
fn parse_scalar(token: &str) -> Result<Scalar, TrainError> {
    let looks_float = token.contains('.') || token.contains('e') || token.contains('E');
    if !looks_float && let Ok(value) = token.parse::<u64>() {
        return Ok(Scalar::Int(value));
    }
    token
        .parse::<f64>()
        .map(Scalar::Float)
        .map_err(|_| TrainError::Malformed {
            detail: format!("status value is not a number: {token:?}"),
        })
}

#[cfg(test)]
mod tests {
    use super::{EpochState, Stage, Status};

    #[test]
    fn absent_epoch_is_not_done_and_signing_makes_it_done() {
        let mut status = Status::new();
        assert_eq!(
            status.epoch_state(Stage::Segment).expect("state"),
            EpochState::NotDone
        );
        status.sign(Stage::Segment);
        assert_eq!(
            status.epoch_state(Stage::Segment).expect("state"),
            EpochState::Done
        );
        assert!(status.is_done(Stage::Segment).expect("done"));
    }

    #[test]
    fn a_newer_epoch_is_a_typed_error_not_a_panic() {
        let status = Status::parse("{\"SegmentEpoch\": 2}").expect("parse");
        let error = status.epoch_state(Stage::Segment).unwrap_err();
        assert!(matches!(
            error,
            crate::error::TrainError::EpochTooNew {
                stage: "Segment",
                found: 2,
                known: 1
            }
        ));
    }

    #[test]
    fn round_trips_through_flat_json() {
        let mut status = Status::new();
        status.sign(Stage::Generate);
        status.generate_text_end = Some(42);
        status.generate_model_end = Some(3);
        status.estimate_score = Some(0.312699);
        status.prune_k = Some(3);
        status.prune_cdf = Some(0.99);
        status.evaluate_correction_rate = Some(0.5);

        let json = status.to_json();
        let parsed = Status::parse(&json).expect("re-parse");
        assert_eq!(parsed, status, "json: {json}");
    }

    #[test]
    fn parses_a_python_written_status() {
        // json.dumps({"GenerateEpoch": 1, "EstimateScore": 0.87}) shape.
        let status =
            Status::parse("{\"GenerateEpoch\": 1, \"EstimateScore\": 0.87}").expect("parse");
        assert!(status.is_done(Stage::Generate).expect("done"));
        assert_eq!(status.estimate_score, Some(0.87));
    }

    #[test]
    fn an_empty_object_is_an_empty_status() {
        assert_eq!(Status::parse("{}").expect("parse"), Status::new());
        assert_eq!(Status::parse("  {  }  ").expect("parse"), Status::new());
    }

    #[test]
    fn malformed_status_is_an_error_not_a_panic() {
        assert!(Status::parse("not json").is_err());
        assert!(Status::parse("{\"k\": }").is_err());
        assert!(Status::parse("{\"k\" 3}").is_err());
        assert!(Status::parse("{\"k\": abc}").is_err());
    }

    #[test]
    fn unknown_keys_are_ignored() {
        // A word-recognizer status marker must not break main-pipeline load.
        let status = Status::parse("{\"PopulateEpoch\": 1, \"PruneK\": 3}").expect("parse");
        assert_eq!(status.prune_k, Some(3));
    }

    #[test]
    fn integers_print_without_and_floats_with_a_decimal() {
        let mut status = Status::new();
        status.prune_k = Some(3);
        status.evaluate_average_lambda = Some(1.0);
        let json = status.to_json();
        assert!(json.contains("\"PruneK\": 3"), "{json}");
        assert!(json.contains("\"EvaluateAverageLambda\": 1.0"), "{json}");
    }
}
