use crate::report;
use crate::report::{DiffDetail, Difference};
use regex::Regex;
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use strsim::normalized_damerau_levenshtein;
use thiserror::Error;
use tracing::error;
use vg_errortools::fat_io_wrap_std;
use vg_errortools::FatIOError;

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
/// Plain text comparison config, also used for PDF
pub struct HTMLCompareConfig {
    /// Normalized Damerau-Levenshtein distance, 0.0 = bad, 1.0 = identity
    pub threshold: f64,
    /// Lines matching any of the given regex will be excluded from comparison
    pub ignore_lines: Option<Vec<String>>,
}

impl HTMLCompareConfig {
    pub(crate) fn get_ignore_list(&self) -> Result<Vec<Regex>, regex::Error> {
        let exclusion_list: Option<Result<Vec<_>, regex::Error>> = self
            .ignore_lines
            .as_ref()
            .map(|v| v.iter().map(|exc| Regex::new(exc)).collect());
        let exclusion_list = match exclusion_list {
            Some(r) => r?,
            None => Vec::new(),
        };
        Ok(exclusion_list)
    }
}

impl Default for HTMLCompareConfig {
    fn default() -> Self {
        HTMLCompareConfig {
            threshold: 1.0,
            ignore_lines: None,
        }
    }
}

#[derive(Debug, Error)]
/// Errors during html / plain text checking
pub enum Error {
    #[error("Failed to compile regex {0}")]
    RegexCompilationFailure(#[from] regex::Error),
    #[error("Problem creating hash report {0}")]
    ReportingProblem(#[from] report::Error),
    #[error("File access failed {0}")]
    FileAccessFailure(#[from] FatIOError),
}

pub fn compare_files<P: AsRef<Path>>(
    nominal_path: P,
    actual_path: P,
    config: &HTMLCompareConfig,
) -> Result<Difference, Error> {
    let actual = BufReader::new(fat_io_wrap_std(actual_path.as_ref(), &File::open)?);
    let nominal = BufReader::new(fat_io_wrap_std(nominal_path.as_ref(), &File::open)?);

    let exclusion_list = config.get_ignore_list()?;
    let mut difference = Difference::new_for_file(nominal_path, actual_path);
    actual
        .lines()
        .enumerate()
        .filter_map(|l| l.1.ok().map(|a| (l.0, a)))
        .zip(nominal.lines().map_while(Result::ok))
        .filter(|((_, a), n)|
            exclusion_list.iter().all(|exc| !exc.is_match(a)) && exclusion_list.iter().all(|exc| !exc.is_match(n))
        )
        .for_each(|((l, a), n)| {
            let distance = normalized_damerau_levenshtein(a.as_str(),n.as_str());
            if  distance < config.threshold {

                let error =  format!(
                    "Mismatch in HTML-file in line {}. Expected: '{}' found '{}' (diff: {}, threshold: {})",
                    l, n, a, distance, config.threshold
                );

                error!("{}" , &error);
                difference.push_detail(DiffDetail::Text {actual: a, nominal: n, score: distance, line: l});
                difference.error();
            }
        });

    Ok(difference)
}

#[cfg(test)]
mod test {
    use super::*;
    use test_log::test;
    #[test]
    fn test_identity() {
        assert!(
            !compare_files(
                "tests/html/test.html",
                "tests/html/test.html",
                &HTMLCompareConfig::default(),
            )
            .unwrap()
            .is_error
        );
    }

    #[test]
    fn test_modified() {
        let actual = "tests/html/test.html";
        let nominal = "tests/html/html_changed.html";

        let result = compare_files(actual, nominal, &HTMLCompareConfig::default()).unwrap();

        assert!(result.is_error);
    }

    #[test]
    fn test_allow_modified_threshold() {
        assert!(
            !compare_files(
                "tests/html/test.html",
                "tests/html/html_changed.html",
                &HTMLCompareConfig {
                    threshold: 0.9,
                    ignore_lines: None
                },
            )
            .unwrap()
            .is_error
        );
    }

    #[test]
    fn test_ignore_lines_regex() {
        assert!(
            !compare_files(
                "tests/html/test.html",
                "tests/html/html_changed.html",
                &HTMLCompareConfig {
                    threshold: 1.0,
                    ignore_lines: Some(vec!["stylesheet".to_owned()])
                },
            )
            .unwrap()
            .is_error
        );
    }
}
