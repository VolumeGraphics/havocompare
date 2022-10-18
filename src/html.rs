use crate::report;
use regex::Regex;
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use strsim::normalized_damerau_levenshtein;
use tracing::error;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HTMLCompareConfig {
    threshold: f64,
    ignore_lines: Option<Vec<String>>,
}

impl Default for HTMLCompareConfig {
    fn default() -> Self {
        HTMLCompareConfig {
            threshold: 1.0,
            ignore_lines: None,
        }
    }
}

pub fn compare_files<P: AsRef<Path>>(
    actual_path: P,
    nominal_path: P,
    config: &HTMLCompareConfig,
    rule_name: &str,
) -> report::FileCompareResult {
    let actual =
        BufReader::new(File::open(actual_path.as_ref()).expect("Could not open actual file"));
    let nominal =
        BufReader::new(File::open(nominal_path.as_ref()).expect("Could not open actual file"));

    let mut diffs: Vec<String> = Vec::new();

    let exclusion_list: Vec<_> = config
        .ignore_lines
        .as_ref()
        .map(|v| {
            v.iter()
                .map(|exc| Regex::new(exc).expect("Plaintext exclusion regex broken!"))
                .collect()
        })
        .unwrap_or_default();

    actual
        .lines()
        .enumerate()
        .into_iter()
        .filter_map(|l| l.1.ok().map(|a| (l.0, a)))
        .zip(nominal.lines().into_iter().filter_map(|l| l.ok()))
        .filter(|((_, a), n)|
            exclusion_list.iter().all(|exc| !exc.is_match(a)) && exclusion_list.iter().all(|exc| !exc.is_match(n))
        )
        .for_each(|((l, a), n)| {
            let distance = normalized_damerau_levenshtein(a.as_str(),n.as_str());
            if  distance < config.threshold {

                let error =  format!(
                    "Missmatch in HTML-file in line {}. Expected: '{}' found '{}' (diff: {}, threshold: {})",
                    l, n, a, distance, config.threshold
                );

                error!("{}" , &error);

                diffs.push(error);
            }
        });

    report::write_html_detail(
        nominal_path.as_ref(),
        actual_path.as_ref(),
        &diffs,
        rule_name,
    )
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
                ""
            )
            .is_error
        );
    }

    #[test]
    fn test_modified() {
        let actual = "tests/html/test.html";
        let nominal = "tests/html/html_changed.html";
        let rule_name = "";

        let result = compare_files(actual, nominal, &HTMLCompareConfig::default(), rule_name);

        assert!(result.is_error);

        assert!(result.detail_path.is_some());

        std::fs::remove_dir_all(
            result
                .detail_path
                .ok_or("detail_path has None value")
                .unwrap(),
        )
        .unwrap();
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
                ""
            )
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
                ""
            )
            .is_error
        );
    }
}
