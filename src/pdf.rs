use crate::html::compare_files as html_compare_files;
use crate::html::HTMLCompareConfig;
use crate::report;
use pdf_extract::extract_text;
use std::fs;
use std::path::Path;
use tracing::info;

const TXT_EXTENSION: &str = "txt";

pub fn compare_files<P: AsRef<Path>>(
    actual_path: P,
    nominal_path: P,
    config: &HTMLCompareConfig,
    rule_name: &str,
) -> report::FileCompareResult {
    let temp_dir =
        tempdir::TempDir::new("hvc_pdf").expect("Could not generate temporary directory for pdf");
    let temp_dir = temp_dir.path();

    info!("Extracting text from actual pdf");
    let actual_string =
        extract_text(actual_path.as_ref()).expect("Could not extract text from actual pdf");
    let actual_file_name = actual_path
        .as_ref()
        .file_name()
        .expect("Could not get pdf name")
        .to_string_lossy()
        .to_string();
    let actual_file_name = format!("{}.actual.{}", actual_file_name, TXT_EXTENSION);
    let actual_file = temp_dir.join(actual_file_name);

    info!("Writing temporary actual text file");
    fs::write(&actual_file, actual_string.as_bytes()).expect("Could not create file");

    info!("Extracting text from nominal pdf");
    let nominal_string =
        extract_text(nominal_path.as_ref()).expect("Could not extract text from nominal pdf");
    let nominal_file_name = nominal_path
        .as_ref()
        .file_name()
        .expect("Could not get pdf name")
        .to_string_lossy()
        .to_string();
    let nominal_file_name = format!("{}.nominal.{}", nominal_file_name, TXT_EXTENSION);

    info!("Writing temporary nominal text file");
    let nominal_file = temp_dir.join(nominal_file_name);

    fs::write(&nominal_file, nominal_string.as_bytes()).expect("Could not create file");

    html_compare_files(actual_file, nominal_file, config, rule_name)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_compare_pdf() {
        let result = compare_files(
            "tests/pdf/actual.pdf",
            "tests/pdf/expected.pdf",
            &HTMLCompareConfig::default(),
            "",
        );
        assert!(result.is_error);

        let result = compare_files(
            "tests/pdf/actual.pdf",
            "tests/pdf/actual.pdf",
            &HTMLCompareConfig::default(),
            "",
        );
        assert!(!result.is_error);
    }

    #[test]
    fn test_ignore_line_pdf() {
        let result = compare_files(
            "tests/pdf/actual.pdf",
            "tests/pdf/expected.pdf",
            &HTMLCompareConfig {
                threshold: 1.0,
                ignore_lines: Some(vec!["/ w o r k s p a c e /".to_owned()]),
            },
            "",
        );
        assert!(!result.is_error);
    }
}
