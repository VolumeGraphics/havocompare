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

/// comparison module for csv comparison
pub mod csv;
pub use csv::CSVCompareConfig;
mod hash;
pub use hash::HashConfig;
mod html;
mod image;
pub use crate::image::ImageCompareConfig;
mod pdf;
mod report;

pub use crate::html::HTMLCompareConfig;
use crate::report::FileCompareResult;
use schemars::schema_for;
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{debug, error, info};
use vg_errortools::{fat_io_wrap_std, FatIOError};

#[derive(Error, Debug)]
/// Top-Level Error class for all errors that can happen during havocompare-running
pub enum Error {
    /// Pattern used for globbing was invalid
    #[error("Failed to evaluate globbing pattern! {0}")]
    IllegalGlobbingPattern(#[from] glob::PatternError),
    /// Regex pattern requested could not be compiled
    #[error("Failed to compile regex! {0}")]
    RegexCompilationError(#[from] regex::Error),
    /// An error occured in the csv rule checker
    #[error("CSV module error")]
    CSVModuleError(#[from] csv::Error),
    /// An error occured in the reporting module
    #[error("Error occurred during report creation {0}")]
    ReportingError(#[from] report::Error),
    /// An error occurred during reading yaml
    #[error("Serde error, loading a yaml: {0}")]
    SerdeYamlFail(#[from] serde_yaml::Error),
    /// An error occurred during writing json
    #[error("Serde error, writing json: {0}")]
    SerdeJsonFail(#[from] serde_json::Error),
    /// A problem happened while accessing a file
    #[error("File access failed {0}")]
    FileAccessError(#[from] FatIOError),
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[allow(clippy::upper_case_acronyms)]
/// Representing the comparison mode
pub enum ComparisonMode {
    /// smart CSV compare
    CSV(CSVCompareConfig),
    /// thresholds comparison
    Image(ImageCompareConfig),
    /// plain text compare
    PlainText(HTMLCompareConfig),
    /// Compare using file hashes
    Hash(HashConfig),
    /// PDF text compare
    PDFText(HTMLCompareConfig),
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
/// Represents a whole configuration file consisting of several comparison rules
pub struct ConfigurationFile {
    /// A list of all rules to be checked on run
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
/// Representing a single comparison rule
pub struct Rule {
    /// The name of the rule - will be displayed in logs
    pub name: String,
    /// A list of glob-patterns to include
    pub pattern_include: Vec<String>,
    /// A list of glob-patterns to exclude - optional
    pub pattern_exclude: Option<Vec<String>>,
    /// How these files shall be compared
    #[serde(flatten)]
    pub file_type: ComparisonMode,
}

fn glob_files(
    path: impl AsRef<Path>,
    patterns: &[impl AsRef<str>],
) -> Result<Vec<PathBuf>, glob::PatternError> {
    let mut files = Vec::new();
    for pattern in patterns {
        let path_prefix = path.as_ref().join(pattern.as_ref());
        let path_pattern = path_prefix.to_string_lossy();
        debug!("Globbing: {}", path_pattern);
        files.extend(glob::glob(path_pattern.as_ref())?.filter_map(|p| p.ok()));
    }
    Ok(files)
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
) -> Result<FileCompareResult, Box<dyn std::error::Error>> {
    info!(
        "Processing files: {} vs {}...",
        nominal.as_ref().to_string_lossy(),
        actual.as_ref().to_string_lossy()
    );

    let compare_result: Result<FileCompareResult, Box<dyn std::error::Error>> =
        match &rule.file_type {
            ComparisonMode::CSV(conf) => {
                csv::compare_paths(nominal.as_ref(), actual.as_ref(), conf, &rule.name)
                    .map_err(|e| e.into())
            }
            ComparisonMode::Image(conf) => {
                image::compare_paths(nominal.as_ref(), actual.as_ref(), conf, &rule.name)
                    .map_err(|e| e.into())
            }
            ComparisonMode::PlainText(conf) => {
                html::compare_files(nominal.as_ref(), actual.as_ref(), conf, &rule.name)
                    .map_err(|e| e.into())
            }
            ComparisonMode::Hash(conf) => {
                hash::compare_files(nominal.as_ref(), actual.as_ref(), conf, &rule.name)
                    .map_err(|e| e.into())
            }
            ComparisonMode::PDFText(conf) => {
                pdf::compare_files(nominal.as_ref(), actual.as_ref(), conf, &rule.name)
                    .map_err(|e| e.into())
            }
        };

    if let Ok(compare_result) = &compare_result {
        if compare_result.is_error {
            error!("Files didn't match");
        } else {
            info!("Files matched");
        }
    } else {
        error!("Problem comparing the files");
    }

    compare_result
}

fn get_files(
    path: impl AsRef<Path>,
    patterns_include: &[impl AsRef<str>],
    patterns_exclude: &[impl AsRef<str>],
) -> Result<Vec<PathBuf>, glob::PatternError> {
    let files_exclude = glob_files(path.as_ref(), patterns_exclude)?;
    let files_include: Vec<_> = glob_files(path.as_ref(), patterns_include)?;
    Ok(filter_exclude(files_include, files_exclude))
}

fn process_rule(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    rule: &Rule,
    compare_results: &mut Vec<Result<FileCompareResult, Box<dyn std::error::Error>>>,
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

    let exclude_patterns = rule.pattern_exclude.as_deref().unwrap_or_default();

    let nominal_cleaned_paths =
        get_files(nominal.as_ref(), &rule.pattern_include, exclude_patterns)?;
    let actual_cleaned_paths = get_files(actual.as_ref(), &rule.pattern_include, exclude_patterns)?;

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

            all_okay &= compare_result
                .as_ref()
                .map(|r| !r.is_error)
                .unwrap_or(false);
            compare_results.push(compare_result);
        });

    Ok(all_okay)
}

/// Use this function if you don't want this crate to load and parse a config file but provide a custom rules struct yourself
pub fn compare_folders_cfg(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    config_struct: ConfigurationFile,
    report_path: impl AsRef<Path>,
) -> Result<bool, Error> {
    let mut rule_results: Vec<report::RuleResult> = Vec::new();

    let mut results = config_struct.rules.into_iter().map(|rule| {
        let mut compare_results: Vec<Result<FileCompareResult, Box<dyn std::error::Error>>> =
            Vec::new();
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
            compare_results: compare_results.into_iter().filter_map(|r| r.ok()).collect(),
        });

        result
    });
    let all_okay = results.all(|result| result);
    report::create(&rule_results, report_path)?;
    Ok(all_okay)
}

/// The main function for comparing folders. It will parse a config file in yaml format, create a report in report_path and compare the folders nominal and actual.
pub fn compare_folders(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    config_file: impl AsRef<Path>,
    report_path: impl AsRef<Path>,
) -> Result<bool, Error> {
    let config_reader = fat_io_wrap_std(config_file, &File::open)?;
    let config: ConfigurationFile = serde_yaml::from_reader(config_reader)?;
    compare_folders_cfg(nominal, actual, config, report_path)
}

/// Create the jsonschema for the current configuration file format
pub fn get_schema() -> Result<String, Error> {
    let schema = schema_for!(ConfigurationFile);
    Ok(serde_json::to_string_pretty(&schema)?)
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
            pattern_include: vec!["*.".to_string()],
            pattern_exclude: None,
        };
        let mut result = Vec::new();
        assert!(!process_rule("NOT_EXISTING", ".", &rule, &mut result).unwrap());
        assert!(!process_rule(".", "NOT_EXISTING", &rule, &mut result).unwrap());
    }

    #[test]
    fn multiple_include_exclude_works() {
        let pattern_include = vec![
            "**/Components.csv".to_string(),
            "**/CumulatedHistogram.csv".to_string(),
        ];
        let empty = vec![""];
        let result =
            get_files("tests/csv/data/", &pattern_include, &empty).expect("could not glob");
        assert_eq!(result.len(), 2);
        let excludes = vec!["**/Components.csv".to_string()];
        let result =
            get_files("tests/csv/data/", &pattern_include, &excludes).expect("could not glob");
        assert_eq!(result.len(), 1);
        let excludes = vec![
            "**/Components.csv".to_string(),
            "**/CumulatedHistogram.csv".to_string(),
        ];
        let result =
            get_files("tests/csv/data/", &pattern_include, &excludes).expect("could not glob");
        assert!(result.is_empty());
    }
}
