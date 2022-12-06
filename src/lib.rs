#![crate_name = "havocompare"]
//! # Comparing folders and files by rules
//! Havocompare allows to compare folders (or to be more exact: the files inside the folders) following user definable rules.
//! A self contained html report is generated. To use it without the CLI, the main method is: [`compare_folders`].
//!
#![warn(missing_docs)]
#![warn(unused_qualifications)]
#![deny(deprecated)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

mod csv;
mod hash;
mod html;
mod image;
mod pdf;
mod report;

use crate::hash::HashConfig;
use crate::html::HTMLCompareConfig;
use crate::report::FileCompareResult;
use schemars::schema_for;
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{debug, error, info};

#[derive(Error, Debug)]
/// Top-Level Error class for all errors that can happen during havocompare-running
pub enum Error {
    /// Attempted to glob an illegal pattern
    #[error("Failed to evaluate globbing pattern! {0}")]
    IllegalGlobbingPattern(#[from] glob::PatternError),
    #[error("Failed to compile regex! {0}")]
    RegexCompilationError(#[from] regex::Error),
    #[error("CSV module error")]
    CSVModuleError(#[from] csv::Error),
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[allow(clippy::upper_case_acronyms)]
enum ComparisonMode {
    /// smart CSV compare
    CSV(csv::CSVCompareConfig),
    /// thresholds comparison
    Image(image::ImageCompareConfig),
    /// plain text compare
    PlainText(HTMLCompareConfig),
    /// Compare using file hashes
    Hash(HashConfig),
    /// PDF text compare
    PDFText(HTMLCompareConfig),
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct ConfigurationFile {
    rules: Vec<Rule>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct Rule {
    name: String,
    pattern_include: String,
    pattern_exclude: Option<String>,
    #[serde(flatten)]
    file_type: ComparisonMode,
}

fn glob_files(path: impl AsRef<Path>, pattern: Option<&str>) -> Result<Vec<PathBuf>, Error> {
    if let Some(pattern) = pattern {
        let path_prefix = path.as_ref().join(pattern);
        let path_pattern = path_prefix.to_string_lossy();
        debug!("Globbing: {}", path_pattern);
        Ok(glob::glob(path_pattern.as_ref())?
            .filter_map(|c| c.ok())
            .collect())
    } else {
        Ok(Vec::new())
    }
}

fn filter_exclude(paths: Vec<PathBuf>, excludes: Vec<PathBuf>) -> Vec<PathBuf> {
    debug!(
        "Filtering paths {:#?} with exclusion list {:#?}",
        &paths, &excludes
    );
    paths
        .into_iter()
        .filter_map(|p| if excludes.contains(&p) { None } else { Some(p) })
        .collect()
}

fn process_file(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    rule: &Rule,
) -> FileCompareResult {
    info!(
        "Processing files: {} vs {}...",
        nominal.as_ref().to_string_lossy(),
        actual.as_ref().to_string_lossy()
    );

    let compare_result = match &rule.file_type {
        ComparisonMode::CSV(conf) => {
            csv::compare_paths(nominal.as_ref(), actual.as_ref(), conf, &rule.name)
        }
        ComparisonMode::Image(conf) => {
            image::compare_paths(nominal.as_ref(), actual.as_ref(), conf, &rule.name)
        }
        ComparisonMode::PlainText(conf) => {
            html::compare_files(nominal.as_ref(), actual.as_ref(), conf, &rule.name)
        }
        ComparisonMode::Hash(conf) => {
            hash::compare_files(nominal.as_ref(), actual.as_ref(), conf, &rule.name)
        }
        ComparisonMode::PDFText(conf) => {
            pdf::compare_files(nominal.as_ref(), actual.as_ref(), conf, &rule.name)
        }
    };

    if compare_result.is_error {
        error!("Files didn't match");
    } else {
        info!("Files matched!");
    }

    compare_result
}

fn process_rule(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    rule: &Rule,
    compare_results: &mut Vec<FileCompareResult>,
) -> Result<bool, Error> {
    info!("Processing rule: {}", rule.name.as_str());
    if !nominal.as_ref().is_dir() {
        error!(
            "Nominal folder {} is not a folder",
            nominal.as_ref().to_string_lossy()
        );
        return Ok(false);
    }
    if !actual.as_ref().is_dir() {
        error!(
            "Actual folder {} is not a folder",
            actual.as_ref().to_string_lossy()
        );
        return Ok(false);
    }

    let nominal_files_exclude = glob_files(nominal.as_ref(), rule.pattern_exclude.as_deref())?;
    let nominal_paths: Vec<_> = glob_files(nominal.as_ref(), Some(rule.pattern_include.as_str()))?;
    let nominal_cleaned_paths = filter_exclude(nominal_paths, nominal_files_exclude);

    let actual_files_exclude = glob_files(actual.as_ref(), rule.pattern_exclude.as_deref())?;
    let actual_paths: Vec<_> = glob_files(actual.as_ref(), Some(rule.pattern_include.as_str()))?;
    let actual_cleaned_paths = filter_exclude(actual_paths, actual_files_exclude);

    info!(
        "Found {} files matching includes in actual, {} files in nominal",
        actual_cleaned_paths.len(),
        nominal_cleaned_paths.len()
    );

    let mut all_okay = true;
    nominal_cleaned_paths
        .into_iter()
        .zip(actual_cleaned_paths.into_iter())
        .for_each(|(n, a)| {
            let compare_result = process_file(n, a, rule);

            all_okay &= !compare_result.is_error;

            compare_results.push(compare_result);
        });

    Ok(all_okay)
}

/// The main function for comparing folders. It will parse a config file in yaml format, create a report in report_path and compare the folders nominal and actual.
pub fn compare_folders(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    config_file: impl AsRef<Path>,
    report_path: impl AsRef<Path>,
) -> Result<bool, Error> {
    let config: ConfigurationFile =
        serde_yaml::from_reader(File::open(config_file).expect("Could not open config file"))
            .expect("Could not parse config file");
    let mut rule_results: Vec<report::RuleResult> = Vec::new();

    let mut results = config.rules.into_iter().map(|rule| {
        let mut compare_results: Vec<FileCompareResult> = Vec::new();
        let okay = process_rule(
            nominal.as_ref(),
            actual.as_ref(),
            &rule,
            &mut compare_results,
        );

        let rule_name = rule.name.as_str();

        let result = match okay {
            Ok(result) => result,
            Err(e) => {
                println!(
                    "Error occured during rule-processing for rule {}: {}",
                    rule_name, e
                );
                false
            }
        };
        rule_results.push(report::RuleResult {
            rule,
            compare_results,
        });

        result
    });
    let all_okay = results.all(|result| result);
    report::create(&rule_results, report_path)?;
    Ok(all_okay)
}

/// Create the jsonschema for the current configuration file format
pub fn get_schema() -> String {
    let schema = schema_for!(ConfigurationFile);
    serde_json::to_string_pretty(&schema).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::ImageCompareConfig;
    #[test]
    fn folder_not_found_is_false() {
        let rule = Rule {
            name: "test rule".to_string(),
            file_type: ComparisonMode::Image(ImageCompareConfig { threshold: 1.0 }),
            pattern_include: "*.".to_string(),
            pattern_exclude: None,
        };
        let mut result = Vec::new();
        assert!(!process_rule("NOT_EXISTING", ".", &rule, &mut result).unwrap());
        assert!(!process_rule(".", "NOT_EXISTING", &rule, &mut result).unwrap());
    }
}
