use crate::html::HTMLCompareConfig;
use crate::report;
use pdf_extract::extract_text;
use regex::Regex;
use std::path::Path;
use strsim::normalized_damerau_levenshtein;
use tracing::{error, info};

pub fn compare_files<P: AsRef<Path>>(
    nominal_path: P,
    actual_path: P,
    config: &HTMLCompareConfig,
    rule_name: &str,
) -> report::FileCompareResult {
    info!("Extracting text from actual pdf");
    let actual =
        extract_text(actual_path.as_ref()).expect("Could not extract text from actual pdf");

    info!("Extracting text from nominal pdf");
    let nominal =
        extract_text(nominal_path.as_ref()).expect("Could not extract text from nominal pdf");

    let mut diffs: Vec<(usize, String)> = Vec::new();

    let exclusion_list: Vec<_> = config
        .ignore_lines
        .as_ref()
        .map(|v| {
            v.iter()
                .map(|exc| Regex::new(exc).expect("Plaintext exclusion regex broken!"))
                .collect()
        })
        .unwrap_or_default();

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

    report::write_pdf_detail(
        nominal_path.as_ref(),
        actual_path.as_ref(),
        &nominal,
        &actual,
        &diffs,
        rule_name,
    )
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
