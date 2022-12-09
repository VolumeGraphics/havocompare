use crate::html::HTMLCompareConfig;
use crate::report;
use pdf_extract::extract_text;
use std::path::Path;
use strsim::normalized_damerau_levenshtein;
use thiserror::Error;
use tracing::{error, info};
use vg_errortools::FatIOError;

#[derive(Debug, Error)]
/// Errors during html / plain text checking
pub enum Error {
    #[error("Failed to compile regex {0}")]
    RegexCompilationError(#[from] regex::Error),
    #[error("Problem creating hash report {0}")]
    ReportingError(#[from] report::Error),
    #[error("File access failed {0}")]
    FileAccessError(#[from] FatIOError),
    #[error("PDF text extraction error {0}")]
    PdfTextExtractionFailed(#[from] pdf_extract::OutputError),
}

pub fn compare_files<P: AsRef<Path>>(
    nominal_path: P,
    actual_path: P,
    config: &HTMLCompareConfig,
    rule_name: &str,
) -> Result<report::FileCompareResult, Error> {
    info!("Extracting text from actual pdf");
    let actual = extract_text(actual_path.as_ref())?;

    info!("Extracting text from nominal pdf");
    let nominal = extract_text(nominal_path.as_ref())?;

    let mut diffs: Vec<(usize, String)> = Vec::new();

    let exclusion_list = config.get_ignore_list()?;

    actual
        .lines()
        .enumerate()
        .into_iter()
        .zip(nominal.lines().into_iter())
        .filter(|((_, a), n)|
            exclusion_list.iter().all(|exc| !exc.is_match(a)) && exclusion_list.iter().all(|exc| !exc.is_match(n))
        )
        .for_each(|((l, a), n)| {
            let distance = normalized_damerau_levenshtein(a,n);
            if  distance < config.threshold {

                let error =  format!(
                    "Missmatch in PDF-Text-file in line {}. Expected: '{}' found '{}' (diff: {}, threshold: {})",
                    l, n, a, distance, config.threshold
                );

                error!("{}" , &error);

                diffs.push((l, error));
            }
        });

    Ok(report::write_pdf_detail(
        nominal_path.as_ref(),
        actual_path.as_ref(),
        &nominal,
        &actual,
        &diffs,
        rule_name,
    )?)
}

#[cfg(test)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
mod test {
    use super::*;

    #[test]
    fn test_compare_pdf() {
        let result = compare_files(
            "tests/pdf/actual.pdf",
            "tests/pdf/expected.pdf",
            &HTMLCompareConfig::default(),
            "",
        )
        .unwrap();
        assert!(result.is_error);

        let result = compare_files(
            "tests/pdf/actual.pdf",
            "tests/pdf/actual.pdf",
            &HTMLCompareConfig::default(),
            "",
        )
        .unwrap();
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
        )
        .unwrap();
        assert!(!result.is_error);
    }
}
