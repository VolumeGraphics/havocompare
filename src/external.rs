use crate::report::FileCompareResult;
use crate::{report, Error};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{error, info};

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
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
    let (stdout_string, stderr_string, error_message) = if let Ok(output) = output {
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        info!("External stdout: {}", stdout.as_str());
        info!("External stderr: {}", stderr.as_str());
        let error_message = if !output.status.success() {
            let message = format!("External checker denied file {}", &compared_file_name);
            error!("{}", &message);
            is_error = true;
            message
        } else {
            "".to_owned()
        };

        (stdout, stderr, error_message)
    } else {
        let error_message = format!(
            "External checker execution failed for file {}",
            &compared_file_name
        );
        error!("{}", error_message);
        is_error = true;
        ("".to_owned(), "".to_owned(), error_message)
    };
    Ok(report::write_external_detail(
        nominal,
        actual,
        is_error,
        &stdout_string,
        &stderr_string,
        &error_message,
    )?)
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
