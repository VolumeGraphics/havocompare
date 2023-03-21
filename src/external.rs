use crate::report::FileCompareResult;
use crate::Error;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{error, info};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ExternalConfig {
    /// The executable to call - will be started like: `#executable #(#extra_params)* #nominal #actual`
    executable: String,
    /// Extra parameters to pass
    extra_params: Vec<String>,
}

pub(crate) fn compare_files<P: AsRef<Path>>(
    nominal: P,
    actual: P,
    config: &ExternalConfig,
) -> Result<FileCompareResult, Error> {
    let compared_file_name = nominal.as_ref().to_string_lossy().into_owned();
    let mut is_error = false;
    let output = std::process::Command::new(&config.executable)
        .args(&config.extra_params)
        .arg(nominal.as_ref())
        .arg(actual.as_ref())
        .output();
    if let Ok(output) = output {
        info!(
            "External stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        info!(
            "External stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        if !output.status.success() {
            error!("External checker denied file {}", &compared_file_name);
            is_error = true;
        }
    } else {
        error!(
            "External checker execution failed for file {}",
            &compared_file_name
        );
        is_error = true;
    }
    Ok(FileCompareResult {
        compared_file_name,
        is_error,
        detail_path: None,
        extra_columns: vec![],
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use test_log::test;

    #[test]
    fn test_non_existent_exe() {
        let result = compare_files(
            Path::new("file1"),
            Path::new("file2"),
            &ExternalConfig {
                extra_params: Vec::new(),
                executable: "non_existent".to_owned(),
            },
        )
        .unwrap();
        assert!(result.is_error);
    }

    #[test]
    fn test_bad_output() {
        let result = compare_files(
            Path::new("file1"),
            Path::new("file2"),
            &ExternalConfig {
                extra_params: vec![
                    "run".to_owned(),
                    "--bin".to_owned(),
                    "print_args".to_owned(),
                    "--".to_owned(),
                    "--exit-with-error".to_owned(),
                ],
                executable: "cargo".to_owned(),
            },
        )
        .unwrap();
        assert!(result.is_error);
    }
}
