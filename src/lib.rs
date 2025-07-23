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

use schemars::schema_for;
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{debug, error, info, span};
use vg_errortools::{fat_io_wrap_std, FatIOError};

pub use csv::CSVCompareConfig;
pub use hash::HashConfig;

use crate::directory::DirectoryConfig;
use crate::external::ExternalConfig;
pub use crate::html::HTMLCompareConfig;
pub use crate::image::ImageCompareConfig;
pub use crate::json::JsonConfig;
use crate::properties::PropertiesConfig;
use crate::report::{get_relative_path, DiffDetail, Difference};

/// comparison module for csv comparison
pub mod csv;

mod directory;
mod external;
mod hash;
mod html;
mod image;
mod pdf;
mod properties;
mod report;

mod json;

#[derive(Error, Debug)]
/// Top-Level Error class for all errors that can happen during havocompare-running
pub enum Error {
    /// Pattern used for globbing was invalid
    #[error("Failed to evaluate globbing pattern! {0}")]
    IllegalGlobbingPattern(#[from] glob::PatternError),
    /// Regex pattern requested could not be compiled
    #[error("Failed to compile regex! {0}")]
    RegexCompilationError(#[from] regex::Error),
    /// An error occurred in the csv rule checker
    #[error("CSV module error")]
    CSVModuleError(#[from] csv::Error),
    /// An error occurred in the image rule checker
    #[error("Image module error")]
    ImageModuleError(#[from] image::Error),
    /// An error occurred in the directory/file exists rule checker
    #[error("Image module error")]
    DirectoryModuleError(#[from] directory::Error),

    /// An error occurred in the reporting module
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

    /// could not extract filename from path
    #[error("File path parsing failed")]
    FilePathParsingFails(String),

    /// Different number of files matched pattern in actual and nominal
    #[error("Different number of files matched pattern in actual {0} and nominal {1}")]
    DifferentNumberOfFiles(usize, usize),

    /// Different number of files matched pattern in actual and nominal
    #[error("{0} is not a directory")]
    NotDirectory(String),
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
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
    /// Compare file-properties
    FileProperties(PropertiesConfig),

    /// Compare JSON files
    Json(JsonConfig),

    /// Run external comparison executable
    External(ExternalConfig),

    /// File exists / directory structure checker
    Directory(DirectoryConfig),
}

fn get_file_name(path: &Path) -> Option<Cow<str>> {
    path.file_name().map(|f| f.to_string_lossy())
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
/// Represents a whole configuration file consisting of several comparison rules
pub struct ConfigurationFile {
    /// A list of all rules to be checked on run
    pub rules: Vec<Rule>,
}

impl ConfigurationFile {
    /// creates a [`ConfigurationFile`] file struct from anything implementing `Read`
    pub fn from_reader(reader: impl Read) -> Result<ConfigurationFile, Error> {
        let config: ConfigurationFile = serde_yaml::from_reader(reader)?;
        Ok(config)
    }

    /// creates a [`ConfigurationFile`] from anything path-convertible
    pub fn from_file(file: impl AsRef<Path>) -> Result<ConfigurationFile, Error> {
        let config_reader = fat_io_wrap_std(file, &File::open)?;
        Self::from_reader(BufReader::new(config_reader))
    }
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
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

/// Use this to compare a single file against another file using a given rule
pub fn compare_files(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    comparison_mode: &ComparisonMode,
) -> Difference {
    let file_name_nominal = nominal.as_ref().to_string_lossy();
    let file_name_actual = actual.as_ref().to_string_lossy();
    let _file_span = span!(tracing::Level::INFO, "Processing").entered();

    info!("File: {file_name_nominal} | {file_name_actual}");

    let compare_result: Result<Difference, Box<dyn std::error::Error>> = {
        match comparison_mode {
            ComparisonMode::CSV(conf) => {
                csv::compare_paths(nominal.as_ref(), actual.as_ref(), conf).map_err(|e| e.into())
            }
            ComparisonMode::Image(conf) => {
                image::compare_paths(nominal.as_ref(), actual.as_ref(), conf).map_err(|e| e.into())
            }
            ComparisonMode::PlainText(conf) => {
                html::compare_files(nominal.as_ref(), actual.as_ref(), conf).map_err(|e| e.into())
            }
            ComparisonMode::Hash(conf) => {
                hash::compare_files(nominal.as_ref(), actual.as_ref(), conf).map_err(|e| e.into())
            }
            ComparisonMode::PDFText(conf) => {
                pdf::compare_files(nominal.as_ref(), actual.as_ref(), conf).map_err(|e| e.into())
            }
            ComparisonMode::FileProperties(conf) => {
                properties::compare_files(nominal.as_ref(), actual.as_ref(), conf)
                    .map_err(|e| e.into())
            }
            ComparisonMode::External(conf) => {
                external::compare_files(nominal.as_ref(), actual.as_ref(), conf)
                    .map_err(|e| e.into())
            }
            ComparisonMode::Json(conf) => {
                json::compare_files(nominal.as_ref(), actual.as_ref(), conf).map_err(|e| e.into())
            }
            ComparisonMode::Directory(conf) => {
                let pattern = ["**/*"];
                let exclude_pattern: Vec<String> = Vec::new();
                match get_files(nominal.as_ref(), &pattern, &exclude_pattern) {
                    Ok(n) => match get_files(actual.as_ref(), &pattern, &exclude_pattern) {
                        Ok(a) => directory::compare_paths(
                            nominal.as_ref(),
                            actual.as_ref(),
                            &n,
                            &a,
                            conf,
                        )
                        .map_err(|e| e.into()),
                        Err(e) => Err(e.into()),
                    },
                    Err(e) => Err(e.into()),
                }
            }
        }
    };
    let compare_result = match compare_result {
        Ok(r) => r,
        Err(e) => {
            let e = e.to_string();
            error!("Problem comparing the files {}", &e);
            let mut d = Difference::new_for_file(nominal, actual);
            d.error();
            d.push_detail(DiffDetail::Error(e));
            d
        }
    };

    if compare_result.is_error {
        error!("Files didn't match");
    } else {
        debug!("Files matched");
    }

    compare_result
}

pub(crate) fn get_files(
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
    compare_results: &mut Vec<Difference>,
) -> Result<bool, Error> {
    let _file_span = span!(tracing::Level::INFO, "Rule").entered();
    info!("Name: {}", rule.name.as_str());
    if !nominal.as_ref().is_dir() {
        error!(
            "Nominal folder {} is not a folder",
            nominal.as_ref().to_string_lossy()
        );
        return Err(Error::NotDirectory(
            nominal.as_ref().to_string_lossy().to_string(),
        ));
    }
    if !actual.as_ref().is_dir() {
        error!(
            "Actual folder {} is not a folder",
            actual.as_ref().to_string_lossy()
        );
        return Err(Error::NotDirectory(
            actual.as_ref().to_string_lossy().to_string(),
        ));
    }

    let exclude_patterns = rule.pattern_exclude.as_deref().unwrap_or_default();

    let nominal_cleaned_paths =
        get_files(nominal.as_ref(), &rule.pattern_include, exclude_patterns)?;
    let actual_cleaned_paths = get_files(actual.as_ref(), &rule.pattern_include, exclude_patterns)?;

    let mut all_okay = true;
    match &rule.file_type {
        ComparisonMode::Directory(config) => {
            match directory::compare_paths(
                nominal.as_ref(),
                actual.as_ref(),
                &nominal_cleaned_paths,
                &actual_cleaned_paths,
                config,
            ) {
                Ok(diff) => {
                    all_okay = !diff.is_error;
                    compare_results.push(diff);
                }
                Err(e) => {
                    error!("Problem comparing the files {}", &e);
                    return Err(e.into());
                }
            }
        }
        _ => {
            info!(
                "Found {} files matching includes in actual, {} files in nominal",
                actual_cleaned_paths.len(),
                nominal_cleaned_paths.len()
            );
            let actual_files = actual_cleaned_paths.len();
            let nominal_files = nominal_cleaned_paths.len();

            if actual_files != nominal_files {
                error!(
                    "Different number of files matched pattern in actual {} and nominal {}",
                    actual_files, nominal_files
                );
                return Err(Error::DifferentNumberOfFiles(actual_files, nominal_files));
            }

            nominal_cleaned_paths
                .into_iter()
                .zip(actual_cleaned_paths)
                .for_each(|(n, a)| {
                    let compare_result = compare_files(n, a, &rule.file_type);
                    all_okay &= !compare_result.is_error;
                    compare_results.push(compare_result);
                });
        }
    }

    Ok(all_okay)
}

/// Use this function if you don't want this crate to load and parse a config file but provide a custom rules struct yourself
pub fn compare_folders_cfg(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    config_struct: ConfigurationFile,
    report_path: impl AsRef<Path>,
) -> Result<bool, Error> {
    let mut rule_results: Vec<report::RuleDifferences> = Vec::new();

    let results: Vec<bool> = config_struct
        .rules
        .into_iter()
        .map(|rule| {
            let mut compare_results: Vec<Difference> = Vec::new();
            let okay = process_rule(
                nominal.as_ref(),
                actual.as_ref(),
                &rule,
                &mut compare_results,
            );

            let rule_name = rule.name.as_str();

            let result = match okay {
                Ok(res) => res,
                Err(e) => {
                    compare_results.push(Difference {
                        nominal_file: nominal.as_ref().to_path_buf(),
                        actual_file: actual.as_ref().to_path_buf(),
                        relative_file_path: get_relative_path(actual.as_ref(), nominal.as_ref())
                            .to_string_lossy()
                            .to_string(),
                        is_error: true,
                        detail: vec![DiffDetail::Error(e.to_string())],
                    });
                    error!("Error occurred during rule-processing for rule {rule_name}: {e}");
                    false
                }
            };

            rule_results.push(report::RuleDifferences {
                rule,
                diffs: compare_results,
            });

            result
        })
        .collect();

    let all_okay = results.iter().all(|result| *result);
    report::create_reports(&rule_results, &report_path)?;
    Ok(all_okay)
}

/// The main function for comparing folders. It will parse a config file in yaml format, create a report in report_path and compare the folders nominal and actual.
pub fn compare_folders(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    config_file: impl AsRef<Path>,
    report_path: impl AsRef<Path>,
) -> Result<bool, Error> {
    let config = ConfigurationFile::from_file(config_file)?;
    compare_folders_cfg(nominal, actual, config, report_path)
}

/// Create the jsonschema for the current configuration file format
pub fn get_schema() -> Result<String, Error> {
    let schema = schema_for!(ConfigurationFile);
    Ok(serde_json::to_string_pretty(&schema)?)
}

/// Try to load config yaml and check whether it is a valid one. Returns true if file can be loaded, otherwise false
pub fn validate_config(config_file: impl AsRef<Path>) -> bool {
    let config_file = config_file.as_ref();
    let config_file_string = config_file.to_string_lossy();
    if !config_file.exists() {
        error!("Could not find config file: {config_file_string}");
        return false;
    }

    match ConfigurationFile::from_file(config_file) {
        Ok(_) => {
            info!("Config file {config_file_string} loaded successfully");
            true
        }
        Err(e) => {
            error!(
                "Could not load config file {config_file_string}: {}",
                e.to_string()
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::image::{CompareMode, RGBCompareMode};

    use super::*;

    #[test]
    fn folder_not_found_is_error() {
        let rule = Rule {
            name: "test rule".to_string(),
            file_type: ComparisonMode::Image(ImageCompareConfig {
                threshold: 1.0,
                mode: CompareMode::RGB(RGBCompareMode::Hybrid),
            }),
            pattern_include: vec!["*.".to_string()],
            pattern_exclude: None,
        };
        let mut result = Vec::new();
        assert!(process_rule("NOT_EXISTING", ".", &rule, &mut result).is_err());
        assert!(process_rule(".", "NOT_EXISTING", &rule, &mut result).is_err());
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
