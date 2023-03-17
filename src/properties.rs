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
        let regex = Regex::new(name_regex)?;
        if regex.is_match(&compared_file_name_full) || regex.is_match(&actual_file_name_full) {
            error!(
                "One of the files ({}, {}) matched the regex {name_regex}",
                &compared_file_name_full, &actual_file_name_full
            );
            is_error = true;
        }
    }

    if let (Ok(nominal_meta), Ok(actual_meta)) = (nominal.metadata(), actual.metadata()) {
        if let Some(size_tolerance) = config.file_size_tolerance_bytes {
            let size_diff =
                (nominal_meta.len() as i128 - actual_meta.len() as i128).unsigned_abs() as u64;
            if size_diff > size_tolerance {
                error!("File size tolerance exceeded, diff is {size_diff}, tolerance was {size_tolerance}");
                is_error = true;
            }
        }

        if let Some(mod_time_tolerance) = config.modification_date_tolerance_secs {
            if let (Ok(mod_time_act), Ok(mod_time_nom)) =
                (nominal_meta.modified(), actual_meta.modified())
            {
                let now = SystemTime::now();

                if let (Ok(nom_age), Ok(act_age)) = (
                    now.duration_since(mod_time_nom),
                    now.duration_since(mod_time_act),
                ) {
                    let time_diff = nom_age - act_age;
                    if time_diff.as_secs() > mod_time_tolerance {
                        error!("Modification times too far off difference in timestamps {}s - tolerance {mod_time_tolerance}s",time_diff.as_secs());
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
        }
    } else {
        error!(
            "Could not get file metadata for either: {} or {}",
            &compared_file_name_full, &actual_file_name_full
        );
        is_error = true;
    }

    Ok(FileCompareResult {
        compared_file_name: compared_file_name_full,
        is_error,
        detail_path: None,
    })
}
