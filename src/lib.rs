#![crate_name = "havocompare"]
//! # Comparing folders and files by rules
//! Havocompare allows to compare folders (or to be more exact: the files inside the folders) following user definable rules.
//! A self contained html report is generated. To use it without the CLI, the main method is: [`compare_folders`].
//!
#![warn(missing_docs)]
#![warn(unused_qualifications)]
#![deny(deprecated)]

mod csv;
mod html;
mod image;
mod report;

use crate::html::HTMLCompareConfig;
use crate::report::FileCompareResult;
use schemars::schema_for;
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::{Path, PathBuf};
use tracing::{debug, error, info};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[allow(clippy::upper_case_acronyms)]
enum ComparisonMode {
    /// smart CSV compare
    CSV(csv::CSVCompareConfig),
    /// thresholds comparison
    Image(image::ImageCompareConfig),
    /// plain text compare
    PlainText(HTMLCompareConfig),
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

fn glob_files(path: impl AsRef<Path>, pattern: Option<&str>) -> Vec<PathBuf> {
    if let Some(pattern) = pattern {
        let path_prefix = path.as_ref().join(pattern);
        let path_pattern = path_prefix.to_string_lossy();
        debug!("Globbing: {}", path_pattern);
        glob::glob(path_pattern.as_ref())
            .unwrap()
            .filter_map(|c| c.ok())
            .collect()
    } else {
        Vec::new()
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
) -> bool {
    info!("Processing rule: {}", rule.name.as_str());
    if !nominal.as_ref().is_dir() {
        error!(
            "Nominal folder {} is not a folder",
            nominal.as_ref().to_string_lossy()
        );
        return false;
    }
    if !actual.as_ref().is_dir() {
        error!(
            "Actual folder {} is not a folder",
            actual.as_ref().to_string_lossy()
        );
        return false;
    }

    let nominal_files_exclude = glob_files(nominal.as_ref(), rule.pattern_exclude.as_deref());
    let nominal_paths: Vec<_> = glob_files(nominal.as_ref(), Some(rule.pattern_include.as_str()));
    let nominal_cleaned_paths = filter_exclude(nominal_paths, nominal_files_exclude);

    let actual_files_exclude = glob_files(actual.as_ref(), rule.pattern_exclude.as_deref());
    let actual_paths: Vec<_> = glob_files(actual.as_ref(), Some(rule.pattern_include.as_str()));
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

    all_okay
}

/// The main function for comparing folders. It will parse a config file in yaml format, create a report in report_path and compare the folders nominal and actual.
pub fn compare_folders(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    config_file: impl AsRef<Path>,
    report_path: impl AsRef<Path>,
) -> bool {
    let config: ConfigurationFile =
        serde_yaml::from_reader(File::open(config_file).expect("Could not open config file"))
            .expect("Could not parse config file");
    let mut all_okay = true;
    let mut rule_results: Vec<report::RuleResult> = Vec::new();

    config.rules.into_iter().for_each(|rule| {
        let mut compare_results: Vec<FileCompareResult> = Vec::new();
        let okay = process_rule(
            nominal.as_ref(),
            actual.as_ref(),
            &rule,
            &mut compare_results,
        );

        rule_results.push(report::RuleResult {
            rule,
            compare_results,
        });

        all_okay &= okay;
    });

    report::create(&rule_results, report_path);

    all_okay
}

/// Create the jsonschema for the current configuration file format
pub fn get_schema() -> String {
    let schema = schema_for!(ConfigurationFile);
    serde_json::to_string_pretty(&schema).unwrap()
}
