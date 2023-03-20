use crate::report::FileCompareResult;
use crate::Error;
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::time::SystemTime;
use tracing::error;

/// the configuration struct for file property comparison
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PropertiesConfig {
    /// Compare the file size, difference must be smaller then given value
    file_size_tolerance_bytes: Option<u64>,

    /// Compare the modification date, difference must be smaller then the given value
    modification_date_tolerance_secs: Option<u64>,

    /// Fail if the name contains that regex
    forbid_name_regex: Option<String>,
}

fn regex_matches_any_path(
    nominal_path: &str,
    actual_path: &str,
    regex: &str,
) -> Result<bool, Error> {
    let regex = Regex::new(regex)?;
    if regex.is_match(nominal_path) || regex.is_match(actual_path) {
        error!("One of the files ({nominal_path}, {actual_path}) matched the regex {regex}");
        return Ok(true);
    }
    Ok(false)
}

fn file_size_out_of_tolerance(nominal: &Path, actual: &Path, tolerance: u64) -> bool {
    let mut is_error = false;
    if let (Ok(nominal_meta), Ok(actual_meta)) = (nominal.metadata(), actual.metadata()) {
        let size_diff =
            (nominal_meta.len() as i128 - actual_meta.len() as i128).unsigned_abs() as u64;
        if size_diff > tolerance {
            error!("File size tolerance exceeded, diff is {size_diff}, tolerance was {tolerance}");
            is_error = true;
        }
    } else {
        error!(
            "Could not get file metadata for either: {} or {}",
            &nominal.to_string_lossy(),
            &actual.to_string_lossy()
        );
        is_error = true;
    }
    is_error
}

fn file_modification_time_out_of_tolerance(nominal: &Path, actual: &Path, tolerance: u64) -> bool {
    let mut is_error = false;
    if let (Ok(nominal_meta), Ok(actual_meta)) = (nominal.metadata(), actual.metadata()) {
        if let (Ok(mod_time_act), Ok(mod_time_nom)) =
            (nominal_meta.modified(), actual_meta.modified())
        {
            let now = SystemTime::now();

            if let (Ok(nom_age), Ok(act_age)) = (
                now.duration_since(mod_time_nom),
                now.duration_since(mod_time_act),
            ) {
                let time_diff =
                    (nom_age.as_secs() as i128 - act_age.as_secs() as i128).unsigned_abs() as u64;
                if time_diff > tolerance {
                    error!("Modification times too far off difference in timestamps {time_diff} s - tolerance {tolerance} s");
                    is_error = true;
                }
            } else {
                error!("Could not calculate duration between modification timestamps");
                is_error = true;
            }
        } else {
            error!("Could not read file modification timestamps");
            is_error = true;
        }
    } else {
        error!(
            "Could not get file metadata for either: {} or {}",
            &nominal.to_string_lossy(),
            &actual.to_string_lossy()
        );
        is_error = true;
    }
    is_error
}

pub fn compare_files<P: AsRef<Path>>(
    nominal: P,
    actual: P,
    config: &PropertiesConfig,
) -> Result<FileCompareResult, Error> {
    let nominal = nominal.as_ref();
    let actual = actual.as_ref();
    let mut is_error = false;
    let compared_file_name_full = nominal.to_string_lossy().into_owned();
    let actual_file_name_full = nominal.to_string_lossy();

    if let Some(name_regex) = config.forbid_name_regex.as_deref() {
        is_error |=
            regex_matches_any_path(&compared_file_name_full, &actual_file_name_full, name_regex)?;
    }

    if let Some(tolerance) = config.file_size_tolerance_bytes {
        is_error |= file_size_out_of_tolerance(nominal, actual, tolerance);
    }

    if let Some(tolerance) = config.modification_date_tolerance_secs {
        is_error |= file_modification_time_out_of_tolerance(nominal, actual, tolerance);
    }

    Ok(FileCompareResult {
        compared_file_name: compared_file_name_full,
        is_error,
        detail_path: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_regex_works() {
        let file_name_mock = "/dev/urandom";
        let file_name_cap_mock = "/proc/cpuInfo";
        let regex_no_capitals = r"[A-Z]";
        let regex_no_spaces = r"[\s]";
        assert!(
            regex_matches_any_path(file_name_mock, file_name_cap_mock, regex_no_capitals).unwrap()
        );
        assert!(
            !regex_matches_any_path(file_name_mock, file_name_cap_mock, regex_no_spaces).unwrap()
        );
    }

    #[test]
    fn file_size() {
        let toml_file = "Cargo.toml";
        let lock_file = "Cargo.lock";
        assert!(!file_size_out_of_tolerance(
            Path::new(toml_file),
            Path::new(toml_file),
            0
        ));
        assert!(file_size_out_of_tolerance(
            Path::new(toml_file),
            Path::new(lock_file),
            0
        ));
    }
    #[test]
    fn modification_timestamps() {
        let toml_file = "Cargo.toml";
        let lock_file = "Cargo.lock";
        assert!(!file_modification_time_out_of_tolerance(
            Path::new(toml_file),
            Path::new(toml_file),
            0
        ));
        assert!(file_modification_time_out_of_tolerance(
            Path::new(toml_file),
            Path::new(lock_file),
            0
        ));
    }
}
