//! The segmenter-era constants and the `table.conf` λ reader the training
//! stages still share. The bigram/unigram scoring model itself moved with
//! the `ngseg` binary to `oxpinyin-utils`.

use oxpinyin_data::table_info::TableConf;

/// λ written in the pin's `table.conf` (`lambda parameter:0.312699`).
///
/// `SystemTableInfo2::load` scans that line with `fscanf(..., "%f")`
/// (`src/storage/table_info.cpp:220`); this is the nearest `f32` to the
/// decimal the pin ships.
pub const PINNED_LAMBDA: f32 = 0.312_699;

/// `null_token` (`novel_types.h:121`).
pub const NULL_TOKEN: u32 = 0;

/// `sentence_start` (`novel_types.h:122`). The trellis's initial node.
pub const SENTENCE_START: u32 = 1;

/// The λ of the `table.conf` at `path`, read by `SystemTableInfo2::load`'s
/// own scanner ([`TableConf::load_path`], `074a2219
/// src/storage/table_info.cpp:194-294`), the same reader both C facades
/// use; `None` where that load refuses the file.
#[must_use]
pub fn load_table_conf_lambda(path: &std::path::Path) -> Option<f32> {
    TableConf::load_path(path)
        .ok()
        .map(|conf| conf.lambda_f32())
}

#[cfg(test)]
mod tests {
    use super::{PINNED_LAMBDA, load_table_conf_lambda};

    const CONF: &str = "binary format version:7\nmodel data version:14\n\
        lambda parameter:0.312699\nsource table format:pinyin\n\
        database format:BerkeleyDB\n";

    fn lambda_of(text: &str) -> Option<f32> {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("table.conf");
        std::fs::write(&path, text).expect("write");
        load_table_conf_lambda(&path)
    }

    #[test]
    fn the_pinned_lambda_is_the_scanned_gfloat() {
        assert_eq!(lambda_of(CONF), Some(PINNED_LAMBDA));
        assert_eq!(lambda_of("binary format version:7\n"), None);
    }

    /// The tools read λ as both facades do: the host `fscanf("%f")`.
    #[test]
    fn the_lambda_reader_is_the_pins_scanner() {
        let with_lambda = |spelling: &str| {
            CONF.replace(
                "lambda parameter:0.312699",
                &format!("lambda parameter:{spelling}"),
            )
        };
        assert_eq!(lambda_of(&with_lambda("0x1p-1")), Some(0.5));
        assert_eq!(lambda_of(&with_lambda("5e-1")), Some(0.5));
        assert!(lambda_of(&with_lambda("nan")).is_some_and(f32::is_nan));
        // The preceding `\n` in the pin's directive matches any whitespace.
        let spaced = CONF.replace("lambda parameter:", "   lambda parameter:");
        assert_eq!(lambda_of(&spaced), Some(PINNED_LAMBDA));
    }
}
