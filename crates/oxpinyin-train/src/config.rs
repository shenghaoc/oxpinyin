//! The trainer configuration, a typed reproduction of `trainer/lib/myconfig.py`.
//!
//! Every knob the main pipeline reads from `MyConfig` is a typed field here,
//! with the same default value and the same meaning. Filenames and postfixes
//! that upstream derives from `MyConfig` getters are module constants, so the
//! orchestrator lays out a `try<name>` workspace the same way the Python
//! trainer does.

/// Postfix of a raw-corpus index file (`getIndexPostfix`).
pub const INDEX_POSTFIX: &str = ".index";
/// Postfix of a segmented corpus file (`getSegmentPostfix`).
pub const SEGMENT_POSTFIX: &str = ".segmented";
/// Postfix of a status file (`getStatusPostfix`).
pub const STATUS_POSTFIX: &str = ".status";
/// Postfix of a report file (`getReportPostfix`).
pub const REPORT_POSTFIX: &str = ".report";
/// Postfix of a KMM model file (`getModelPostfix`).
pub const MODEL_POSTFIX: &str = ".db";

/// The gather index filename (`getEstimateIndex`).
pub const ESTIMATE_INDEX: &str = "estimate.index";
/// The sorted gather index filename (`getSortedEstimateIndex`).
pub const SORTED_ESTIMATE_INDEX: &str = "estimate.sorted.index";
/// The final interpolation model filename (`getFinalModelFileName`).
pub const FINAL_MODEL_FILE_NAME: &str = "interpolation2.text";
/// The final-workspace status filename (`getFinalStatusFileName`).
pub const FINAL_STATUS_FILE_NAME: &str = "cwd.status";
/// The evaluation corpus filename (`getEvalsTextFileName`).
pub const EVALS_TEXT_FILE_NAME: &str = "evals2.text";

/// The candidate model filename for candidate number `index`
/// (`getCandidateModelName`).
#[must_use]
pub fn candidate_model_name(index: u32) -> String {
    format!("model-candidates-{index}.db")
}

/// The typed trainer configuration (`MyConfig`, main-pipeline knobs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrainConfig {
    /// Skip a segmented corpus file smaller than this, in bytes
    /// (`getMinimumFileSize`: 1200 Chinese characters × 3 + 1200 ÷ 2).
    pub minimum_file_size: u64,
    /// Roll over to a new candidate once the aggregated segmented input of the
    /// current candidate exceeds this many bytes (`getCandidateModelSize`:
    /// 28.5 MB × 2). Upstream compares the running byte total, not the model
    /// file size, so the orchestrator does too.
    pub candidate_model_size: u64,
    /// `gen_k_mixture_model --maximum-occurs-allowed` (`getMaximumOccursAllowed`).
    pub maximum_occurs_allowed: u32,
    /// `gen_k_mixture_model --maximum-increase-rates-allowed`
    /// (`getMaximumIncreaseRatesAllowed`).
    pub maximum_increase_rates_allowed: f64,
    /// Whether the generate stage trains the π-gram (`<start>` → token) row;
    /// upstream always does (there is no knob), kept here for the
    /// `--skip-pi-gram-training` parity of the individual CLI.
    pub train_pi_gram: bool,
    /// Number of top candidates to merge (`tryprune.py --merge`, default 10).
    pub merge_number: usize,
    /// `prune_k_mixture_model -k` (`tryprune.py -k`, default 3).
    pub prune_k: u32,
    /// `prune_k_mixture_model --CDF` (`tryprune.py --CDF`, default 0.99).
    pub prune_cdf: f64,
}

impl Default for TrainConfig {
    fn default() -> Self {
        Self {
            // 1200 * 3 + 1200 / 2 = 4200. Upstream computes this in float
            // (1200/2 == 600.0) and compares `<`; the value is integral.
            minimum_file_size: 1200 * 3 + 1200 / 2,
            // 28.5 * 1000 * 1000 * 2 = 57_000_000.
            candidate_model_size: 57_000_000,
            maximum_occurs_allowed: 20,
            maximum_increase_rates_allowed: 3.0,
            train_pi_gram: true,
            merge_number: 10,
            prune_k: 3,
            prune_cdf: 0.99,
        }
    }
}

impl TrainConfig {
    /// The generate-stage parameters for `oxpinyin_kmm::GenerateParams`.
    #[must_use]
    pub fn generate_params(&self) -> oxpinyin_kmm::GenerateParams {
        oxpinyin_kmm::GenerateParams {
            max_occurs: self.maximum_occurs_allowed,
            max_increase_rate: self.maximum_increase_rates_allowed,
            train_pi_gram: self.train_pi_gram,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TrainConfig, candidate_model_name};

    #[test]
    fn defaults_match_myconfig() {
        let config = TrainConfig::default();
        assert_eq!(config.minimum_file_size, 4200);
        assert_eq!(config.candidate_model_size, 57_000_000);
        assert_eq!(config.maximum_occurs_allowed, 20);
        assert_eq!(config.maximum_increase_rates_allowed, 3.0);
        assert_eq!(config.merge_number, 10);
        assert_eq!(config.prune_k, 3);
        assert_eq!(config.prune_cdf, 0.99);
    }

    #[test]
    fn candidate_names_are_numbered() {
        assert_eq!(candidate_model_name(0), "model-candidates-0.db");
        assert_eq!(candidate_model_name(7), "model-candidates-7.db");
    }

    #[test]
    fn generate_params_forwards_the_knobs() {
        let params = TrainConfig::default().generate_params();
        assert_eq!(params.max_occurs, 20);
        assert_eq!(params.max_increase_rate, 3.0);
        assert!(params.train_pi_gram);
    }
}
