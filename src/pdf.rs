use crate::html::HTMLCompareConfig;
use crate::report;
use crate::report::{DiffDetail, Difference};
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
    RegexCompilationFailure(#[from] regex::Error),
    #[error("Problem creating hash report {0}")]
    ReportingFailure(#[from] report::Error),
    #[error("File access failed {0}")]
    FileAccessProblem(#[from] FatIOError),
    #[error("PDF text extraction error {0}")]
    PdfTextExtractionFailed(#[from] pdf_extract::OutputError),
}

pub fn compare_files<P: AsRef<Path>>(
    nominal_path: P,
    actual_path: P,
    config: &HTMLCompareConfig,
) -> Result<Difference, Error> {
    info!("Extracting text from actual pdf");
    let actual = extract_text(actual_path.as_ref())?;

    info!("Extracting text from nominal pdf");
    let nominal = extract_text(nominal_path.as_ref())?;

    let exclusion_list = config.get_ignore_list()?;
    let mut difference = Difference::new_for_file(&nominal_path, &actual_path);
    actual
        .lines()
        .enumerate()
        .zip(nominal.lines())
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
                difference.push_detail(DiffDetail::Text {actual:a.to_owned(), nominal:n.to_owned(), score: distance, line: l});
                difference.error();
            }
        });

    Ok(difference)
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
        )
        .unwrap();
        assert!(result.is_error);

        let result = compare_files(
            "tests/pdf/actual.pdf",
            "tests/pdf/actual.pdf",
            &HTMLCompareConfig::default(),
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
                ignore_lines: Some(vec!["/workspace/".to_owned()]),
            },
        )
        .unwrap();
        assert!(!result.is_error);
    }
}
