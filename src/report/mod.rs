mod template;

use crate::csv::{DiffType, Position, Table};
use crate::properties::MetaDataPropertyDiff;
use crate::Rule;
use serde::Serialize;
use std::borrow::Cow;
use std::ffi::OsStr;
use std::fs;
use std::fs::File;
use std::iter::zip;
use std::path::{Path, PathBuf};
use tera::{Context, Tera};
use thiserror::Error;
use tracing::{debug, error, info, span};
use vg_errortools::{fat_io_wrap_std, FatIOError};

#[derive(Error, Debug)]
pub enum Error {
    #[error("Failed to evaluate globbing pattern! {0}")]
    IllegalGlobbingPattern(#[from] glob::PatternError),
    #[error("File access failed {0}")]
    FileAccessFailed(#[from] FatIOError),
    #[error("Tera templating error {0}")]
    TeraIssue(#[from] tera::Error),
    #[error("Problem processing file name {0}")]
    FileNameParsing(String),
    #[error("IO error {0}")]
    IOIssue(#[from] std::io::Error),
    #[error("fs_extra crate error {0}")]
    FsExtraFailed(#[from] fs_extra::error::Error),
    #[error("JSON serialization failed {0}")]
    Serde(#[from] serde_json::Error),
}

#[derive(Serialize, Debug)]
pub struct FileCompareResult {
    pub compared_file_name: String,
    pub is_error: bool,
    pub detail_path: Option<DetailPath>,
    pub additional_columns: Vec<AdditionalOverviewColumn>,
}

#[derive(Serialize, Debug, Default)]
pub struct AdditionalOverviewColumn {
    pub nominal_value: String,
    pub actual_value: String,
    pub is_error: bool,
    pub diff_value: String,
}

#[derive(Serialize, Debug)]
pub struct DetailPath {
    pub temp_path: PathBuf,
    pub path_name: String,
}

#[derive(Serialize, Debug)]
pub(crate) struct RuleResult {
    pub rule: Rule,
    pub compare_results: Vec<FileCompareResult>,
}

#[derive(Serialize, Debug, Clone)]
pub struct CSVReportColumn {
    pub nominal_value: String,
    pub actual_value: String,
    pub diffs: Vec<String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct CSVReportRow {
    pub columns: Vec<CSVReportColumn>,
    pub has_diff: bool,
    pub has_error: bool,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Report {
    pub diffs: Vec<Difference>,
    pub rules: Vec<Rule>,
}

#[derive(Serialize, Debug, Clone)]
pub struct RuleDifferences {
    pub rule: Rule,
    pub diffs: Vec<Difference>,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Difference {
    pub nominal_file: PathBuf,
    pub actual_file: PathBuf,
    pub is_error: bool,
    pub detail: Vec<DiffDetail>,
}

impl Difference {
    pub fn new_for_file(nominal: impl AsRef<Path>, actual: impl AsRef<Path>) -> Self {
        Self {
            nominal_file: nominal.as_ref().to_path_buf(),
            actual_file: actual.as_ref().to_path_buf(),
            ..Default::default()
        }
    }

    pub fn error(&mut self) {
        self.is_error = true;
    }

    pub fn push_detail(&mut self, detail: DiffDetail) {
        self.detail.push(detail);
    }

    pub fn join(&mut self, other: Self) -> bool {
        if self.nominal_file != other.nominal_file {
            return false;
        }
        self.is_error |= other.is_error;
        self.detail.extend(other.detail.into_iter());
        true
    }
}

#[derive(Serialize, Debug, Clone)]
#[allow(clippy::upper_case_acronyms)]
pub enum DiffDetail {
    CSV(DiffType),
    Image { score: f64, diff_image: String },
    Text { line: usize, score: f64 },
    Hash { actual: String, nominal: String },
    External { stdout: String, stderr: String },
    Properties(MetaDataPropertyDiff),
    Error(String),
}

pub fn create_sub_folder() -> Result<DetailPath, Error> {
    let temp_path = tempfile::Builder::new()
        .prefix("havocompare-")
        .tempdir()?
        .into_path();

    let path_name = temp_path
        .file_name()
        .ok_or_else(|| {
            Error::FileNameParsing(format!(
                "Could not extract filename from {}",
                temp_path.to_string_lossy()
            ))
        })?
        .to_string_lossy()
        .to_string();

    Ok(DetailPath {
        temp_path,
        path_name,
    })
}

pub fn write_html_detail(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    diffs: &[String],
) -> Result<FileCompareResult, Error> {
    let mut result = FileCompareResult {
        compared_file_name: get_relative_path(actual.as_ref(), nominal.as_ref())
            .to_string_lossy()
            .to_string(),
        is_error: false,
        detail_path: None,
        additional_columns: vec![],
    };

    if diffs.is_empty() {
        return Ok(result);
    }

    let sub_folder = create_sub_folder()?;

    let detail_file = sub_folder.temp_path.join(template::DETAIL_FILENAME);

    let mut tera = Tera::default();
    tera.add_raw_template(
        &detail_file.to_string_lossy(),
        template::PLAIN_TEXT_DETAIL_TEMPLATE,
    )?;

    let mut ctx = Context::new();
    ctx.insert("actual", &actual.as_ref().to_string_lossy());
    ctx.insert("nominal", &nominal.as_ref().to_string_lossy());

    ctx.insert("errors", diffs);

    let file = fat_io_wrap_std(&detail_file, &File::create)?;

    debug!("detail html {:?} created", &detail_file);

    tera.render_to(&detail_file.to_string_lossy(), &ctx, file)?;

    result.is_error = true;
    result.detail_path = Some(sub_folder);

    Ok(result)
}

pub(crate) fn write_csv_detail(
    nominal_table: Table,
    actual_table: Table,
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    diffs: &[DiffType],
) -> Result<FileCompareResult, Error> {
    let mut result = FileCompareResult {
        compared_file_name: get_relative_path(actual.as_ref(), nominal.as_ref())
            .to_string_lossy()
            .to_string(),
        is_error: false,
        detail_path: None,
        additional_columns: vec![],
    };

    let mut headers: CSVReportRow = CSVReportRow {
        columns: vec![],
        has_diff: false,
        has_error: false,
    };

    nominal_table
        .columns
        .iter()
        .zip(actual_table.columns.iter())
        .for_each(|(n, a)| {
            let a_header = a.header.as_deref();
            let n_header = n.header.as_deref();

            if let (Some(a_header), Some(n_header)) = (a_header, n_header) {
                let actual_value = a_header.to_owned();
                let nominal_value = n_header.to_owned();

                if nominal_value != actual_value {
                    headers.has_diff = true;
                }

                headers.columns.push(CSVReportColumn {
                    actual_value,
                    nominal_value,
                    diffs: Vec::new(),
                });
            }
        });

    let rows: Vec<CSVReportRow> = nominal_table
        .rows()
        .zip(actual_table.rows())
        .enumerate()
        .map(|(row, (n, a))| {
            let mut has_diff = false;
            let mut has_error = false;

            let columns: Vec<CSVReportColumn> = n
                .into_iter()
                .zip(a.into_iter())
                .enumerate()
                .map(|(col, (n, a))| {
                    let current_pos = Position { col, row };
                    let csv_report = CSVReportColumn {
                        nominal_value: n.to_string(),
                        actual_value: a.to_string(),
                        diffs: diffs
                            .iter()
                            .filter(|diff| {
                                let position = match diff {
                                    DiffType::UnequalStrings { position, .. } => position,
                                    DiffType::OutOfTolerance { position, .. } => position,
                                    DiffType::DifferentValueTypes { position, .. } => position,
                                    _ => {
                                        return false;
                                    }
                                };

                                position.row == current_pos.row && position.col == current_pos.col
                            })
                            .map(|diff| match diff {
                                DiffType::UnequalStrings { .. } => "Different strings".to_owned(),
                                DiffType::OutOfTolerance { mode, .. } => {
                                    format!("Out of tolerance. Mode: {mode}")
                                }
                                DiffType::DifferentValueTypes { .. } => {
                                    "Different value types".to_owned()
                                }
                                _ => "Unknown difference".to_owned(),
                            })
                            .collect(),
                    };

                    if !csv_report.diffs.is_empty() {
                        has_error = true;
                    }

                    if csv_report.nominal_value != csv_report.actual_value {
                        has_diff = true;
                    }

                    csv_report
                })
                .collect();

            CSVReportRow {
                has_error,
                has_diff,
                columns,
            }
        })
        .collect();

    let sub_folder = create_sub_folder()?;

    let detail_file = sub_folder.temp_path.join(template::DETAIL_FILENAME);

    let mut tera = Tera::default();
    tera.add_raw_template(
        &detail_file.to_string_lossy(),
        template::PLAIN_CSV_DETAIL_TEMPLATE,
    )?;

    let mut ctx = Context::new();
    ctx.insert("actual", &actual.as_ref().to_string_lossy());
    ctx.insert("nominal", &nominal.as_ref().to_string_lossy());
    ctx.insert("rows", &rows);
    ctx.insert("headers", &headers);

    let file = fat_io_wrap_std(&detail_file, &File::create)?;
    debug!("detail html {:?} created", &detail_file);

    tera.render_to(&detail_file.to_string_lossy(), &ctx, file)?;

    result.is_error = !diffs.is_empty();
    result.detail_path = Some(sub_folder);

    Ok(result)
}

pub fn write_image_detail(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    diffs: &[String],
) -> Result<FileCompareResult, Error> {
    let mut result = FileCompareResult {
        compared_file_name: get_relative_path(actual.as_ref(), nominal.as_ref())
            .to_string_lossy()
            .to_string(),
        is_error: false,
        detail_path: None,
        additional_columns: vec![],
    };

    if diffs.is_empty() {
        return Ok(result);
    }

    let sub_folder = create_sub_folder()?;

    let detail_file = sub_folder.temp_path.join(template::DETAIL_FILENAME);

    let mut tera = Tera::default();
    tera.add_raw_template(
        &detail_file.to_string_lossy(),
        template::PLAIN_IMAGE_DETAIL_TEMPLATE,
    )?;

    let mut ctx = Context::new();
    ctx.insert("actual", &actual.as_ref().to_string_lossy());
    ctx.insert("nominal", &nominal.as_ref().to_string_lossy());

    fn get_file_name(path: &Path) -> Result<Cow<str>, Error> {
        path.file_name()
            .map(|f| f.to_string_lossy())
            .ok_or_else(|| {
                Error::FileNameParsing(format!(
                    "Could not extract filename from {}",
                    path.to_string_lossy()
                ))
            })
    }

    let actual_image = format!("actual_image_{}", get_file_name(actual.as_ref())?);
    let nominal_image = format!("nominal_image_.{}", get_file_name(nominal.as_ref())?);

    fs::copy(actual.as_ref(), sub_folder.temp_path.join(&actual_image))
        .map_err(|e| FatIOError::from_std_io_err(e, actual.as_ref().to_path_buf()))?;
    fs::copy(nominal.as_ref(), sub_folder.temp_path.join(&nominal_image))
        .map_err(|e| FatIOError::from_std_io_err(e, nominal.as_ref().to_path_buf()))?;

    let diff_image = &diffs[1];
    let img_target = sub_folder.temp_path.join(diff_image);
    fs::copy(diff_image, &img_target)
        .map_err(|e| FatIOError::from_std_io_err(e, img_target.to_path_buf()))?;

    ctx.insert("error", &diffs[0]);
    ctx.insert("diff_image", diff_image);
    ctx.insert("actual_image", &actual_image);
    ctx.insert("nominal_image", &nominal_image);

    let file = fat_io_wrap_std(&detail_file, &File::create)?;
    debug!("detail html {:?} created", &detail_file);

    tera.render_to(&detail_file.to_string_lossy(), &ctx, file)?;

    result.is_error = true;
    result.detail_path = Some(sub_folder);

    Ok(result)
}

pub fn write_pdf_detail(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    nominal_string: &String,
    actual_string: &String,
    diffs: &[(usize, String)],
) -> Result<FileCompareResult, Error> {
    let mut result = FileCompareResult {
        compared_file_name: get_relative_path(actual.as_ref(), nominal.as_ref())
            .to_string_lossy()
            .to_string(),
        is_error: false,
        detail_path: None,
        additional_columns: vec![],
    };

    let sub_folder = create_sub_folder()?;

    let nominal_extracted_filename = "nominal_extracted_text.txt";
    let actual_extracted_filename = "actual_extracted_text.txt";

    let nominal_extracted_file = sub_folder.temp_path.join(nominal_extracted_filename);
    fs::write(&nominal_extracted_file, nominal_string.as_bytes())
        .map_err(|e| FatIOError::from_std_io_err(e, nominal_extracted_file))?;

    let actual_extracted_file = sub_folder.temp_path.join(actual_extracted_filename);
    fs::write(&actual_extracted_file, actual_string.as_bytes())
        .map_err(|e| FatIOError::from_std_io_err(e, actual_extracted_file))?;
    info!("Extracted text written to files");

    let detail_file = sub_folder.temp_path.join(template::DETAIL_FILENAME);

    let mut tera = Tera::default();
    tera.add_raw_template(
        &detail_file.to_string_lossy(),
        template::PLAIN_PDF_DETAIL_TEMPLATE,
    )?;

    let combined_lines: Vec<CSVReportColumn> = actual_string
        .lines()
        .enumerate()
        .zip(nominal_string.lines())
        .map(|((l, a), n)| {
            let mut result = CSVReportColumn {
                nominal_value: n.replace(' ', "&nbsp;"),
                actual_value: a.replace(' ', "&nbsp;"),
                diffs: vec![],
            };

            if let Some(diff) = diffs.iter().find(|(i, _msg)| *i == l) {
                result.diffs.push(diff.1.clone());
            };

            result
        })
        .collect();

    let mut ctx = Context::new();
    ctx.insert("actual", &actual.as_ref().to_string_lossy());
    ctx.insert("nominal", &nominal.as_ref().to_string_lossy());
    ctx.insert("diffs", &diffs);
    ctx.insert("combined_lines", &combined_lines);
    ctx.insert("nominal_extracted_filename", nominal_extracted_filename);
    ctx.insert("actual_extracted_filename", actual_extracted_filename);

    ctx.insert("errors", diffs);
    let file = fat_io_wrap_std(&detail_file, &File::create)?;
    debug!("detail html {:?} created", &detail_file);

    tera.render_to(&detail_file.to_string_lossy(), &ctx, file)?;

    result.is_error = !diffs.is_empty();
    result.detail_path = Some(sub_folder);

    Ok(result)
}

pub fn write_external_detail(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    is_error: bool,
    stdout: &str,
    stderr: &str,
    message: &str,
) -> Result<FileCompareResult, Error> {
    let mut result = FileCompareResult {
        compared_file_name: get_relative_path(actual.as_ref(), nominal.as_ref())
            .to_string_lossy()
            .to_string(),
        is_error,
        detail_path: None,
        additional_columns: vec![],
    };

    let sub_folder = create_sub_folder()?;
    let detail_file = sub_folder.temp_path.join(template::DETAIL_FILENAME);

    let mut tera = Tera::default();
    tera.add_raw_template(
        &detail_file.to_string_lossy(),
        template::PLAIN_EXTERNAL_DETAIL_TEMPLATE,
    )?;

    let mut ctx = Context::new();
    ctx.insert("actual", &actual.as_ref().to_string_lossy());
    ctx.insert("nominal", &nominal.as_ref().to_string_lossy());
    ctx.insert("stdout", stdout);
    ctx.insert("stderr", stderr);
    ctx.insert("message", message);

    let file = fat_io_wrap_std(&detail_file, &File::create)?;
    debug!("detail html {:?} created", &detail_file);

    tera.render_to(&detail_file.to_string_lossy(), &ctx, file)?;

    result.detail_path = Some(sub_folder);

    Ok(result)
}

fn create_error_detail(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    error: Box<dyn std::error::Error>,
) -> Result<DetailPath, Error> {
    let sub_folder = create_sub_folder()?;
    let detail_file = sub_folder.temp_path.join(template::DETAIL_FILENAME);

    let mut tera = Tera::default();
    tera.add_raw_template(
        &detail_file.to_string_lossy(),
        template::ERROR_DETAIL_TEMPLATE,
    )?;

    let mut ctx = Context::new();
    ctx.insert("actual", &actual.as_ref().to_string_lossy());
    ctx.insert("nominal", &nominal.as_ref().to_string_lossy());
    ctx.insert("error", &error.to_string());

    let file = fat_io_wrap_std(&detail_file, &File::create)?;

    tera.render_to(&detail_file.to_string_lossy(), &ctx, file)?;

    Ok(sub_folder)
}

pub fn write_error_detail(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    error: Box<dyn std::error::Error>,
) -> FileCompareResult {
    let mut result = FileCompareResult {
        compared_file_name: get_relative_path(actual.as_ref(), nominal.as_ref())
            .to_string_lossy()
            .to_string(),
        is_error: true,
        detail_path: None,
        additional_columns: vec![],
    };

    if let Ok(sub_folder) = create_error_detail(nominal, actual, error) {
        result.detail_path = Some(sub_folder);
    } else {
        error!("Could not create error detail");
    }

    result
}

pub(crate) fn create_reports(
    rule_results: &[RuleDifferences],
    report_path: impl AsRef<Path>,
) -> Result<(), Error> {
    let _reporting_span = span!(tracing::Level::INFO, "Reporting");
    let _reporting_span = _reporting_span.enter();
    let report_dir = report_path.as_ref();
    if report_dir.is_dir() {
        info!("Delete report folder");
        fat_io_wrap_std(&report_dir, &fs::remove_dir_all)?;
    }
    info!("create report folder");
    fat_io_wrap_std(&report_dir, &fs::create_dir)?;

    create_json(rule_results, &report_path)?;
    create_html(rule_results, &report_path)?;

    Ok(())
}

pub(crate) fn create_json(
    rule_results: &[RuleDifferences],
    report_path: impl AsRef<Path>,
) -> Result<(), Error> {
    let _reporting_span = span!(tracing::Level::INFO, "JSON");
    let _reporting_span = _reporting_span.enter();
    let report_dir = report_path.as_ref();
    let writer = report_dir.join("report.json");
    let writer = fat_io_wrap_std(writer, &File::create)?;
    serde_json::to_writer_pretty(writer, &rule_results)?;
    Ok(())
}

pub(crate) fn create_html(
    rule_results: &[RuleDifferences],
    report_path: impl AsRef<Path>,
) -> Result<(), Error> {
    let _reporting_span = span!(tracing::Level::INFO, "HTML");
    let _reporting_span = _reporting_span.enter();
    let report_dir = report_path.as_ref();

    for rule_result in rule_results.iter() {
        let sub_folder = report_dir.join(&rule_result.rule.name);
        debug!("Create subfolder {:?}", &sub_folder);
        fat_io_wrap_std(&sub_folder, &fs::create_dir)?;
        for file in rule_result.diffs.iter() {
            let cmp_errors: Vec<&DiffDetail> = file
                .detail
                .iter()
                .filter(|r| !matches!(r, DiffDetail::Error(_)))
                .collect();
            let diffs: Vec<&DiffDetail> = file
                .detail
                .iter()
                .filter(|r| matches!(r, DiffDetail::Error(_)))
                .collect();
            // TODO: Write cmp_errors to report
            // TODO: Write diffs to report
        }
    }

    Ok(())
}

pub(crate) fn create(
    rule_results: &[RuleResult],
    report_path: impl AsRef<Path>,
) -> Result<(), Error> {
    let _reporting_span = span!(tracing::Level::INFO, "Reporting");
    let _reporting_span = _reporting_span.enter();
    let report_dir = report_path.as_ref();
    if report_dir.is_dir() {
        info!("Delete report folder");
        fat_io_wrap_std(&report_dir, &fs::remove_dir_all)?;
    }

    info!("create report folder");
    fat_io_wrap_std(&report_dir, &fs::create_dir)?;

    //move folders
    for rule_result in rule_results.iter() {
        let sub_folder = report_dir.join(&rule_result.rule.name);
        debug!("Create subfolder {:?}", &sub_folder);
        fat_io_wrap_std(&sub_folder, &fs::create_dir)?;
        for file_result in rule_result.compare_results.iter() {
            if let Some(detail_path) = &file_result.detail_path {
                debug!(
                    "moving subfolder {:?} to {:?}",
                    &detail_path.temp_path, &sub_folder
                );

                let options = fs_extra::dir::CopyOptions::new();
                fs_extra::dir::copy(&detail_path.temp_path, &sub_folder, &options)?;
            }
        }
    }

    write_index(report_dir, rule_results)
}

pub(crate) fn write_index(
    report_dir: impl AsRef<Path>,
    rule_results: &[RuleResult],
) -> Result<(), Error> {
    let index_file = report_dir.as_ref().join(template::INDEX_FILENAME);

    let mut tera = Tera::default();

    tera.add_raw_template(&index_file.to_string_lossy(), template::INDEX_TEMPLATE)?;

    let mut ctx = Context::new();
    ctx.insert("rule_results", rule_results);
    ctx.insert("detail_filename", template::DETAIL_FILENAME);

    let file = fat_io_wrap_std(&index_file, &File::create)?;
    tera.render_to(&index_file.to_string_lossy(), &ctx, file)?;

    debug!("Report.html created");
    Ok(())
}

///Find the relative path between two files
/// compare both files n reversed order (from bottom to top), and returns only the part which are the same on both files
pub(crate) fn get_relative_path(
    actual_path: impl AsRef<Path>,
    nominal_path: impl AsRef<Path>,
) -> PathBuf {
    let actual_iter = actual_path.as_ref().iter().rev();
    let nominal_iter = nominal_path.as_ref().iter().rev();
    let zipped_path = zip(nominal_iter, actual_iter);

    let mut is_the_same = true;
    let mut paths: Vec<&OsStr> = Vec::new();
    for (n, a) in zipped_path {
        if n != a {
            is_the_same = false;
        }

        if is_the_same {
            paths.push(n);
        }
    }

    paths.reverse();

    PathBuf::from_iter(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_relative_path() {
        let result = get_relative_path(
            "tests/integ/data/display_of_status_message_in_cm_tables/expected/Volume1.csv",
            "tests/integ/data/display_of_status_message_in_cm_tables/actual/Volume1.csv",
        );
        assert_eq!(PathBuf::from("Volume1.csv"), result);

        let result = get_relative_path(
            "tests/act/something/csv/test.csv",
            "tests/exp/something/csv/test.csv",
        );
        assert_eq!(PathBuf::from("something/csv/test.csv"), result);

        let result = get_relative_path(
            "tests/integ/data/display_of_status_message_in_cm_tables/expected/Volume1.csv",
            "C:/Users/someuser/Documents/git/havocompare/tests/actual/Volume1.csv",
        );
        assert_eq!(PathBuf::from("Volume1.csv"), result);

        let result = get_relative_path(
            "tests/actual/csv/Volume1.csv",
            "tests/integ/data/display_of_status_message_in_cm_tables/expected/csv/Volume1.csv",
        );
        assert_eq!(PathBuf::from("csv/Volume1.csv"), result);

        let result = get_relative_path(
            "csv/Volume1.csv",
            "tests/integ/data/display_of_status_message_in_cm_tables/expected/csv/Volume1.csv",
        );
        assert_eq!(PathBuf::from("csv/Volume1.csv"), result);

        let result = get_relative_path(
            "csv/Volume1.csv",
            "tests/integ/data/display_of_status_message_in_cm_tables/expected/Volume1.csv",
        );
        assert_eq!(PathBuf::from("Volume1.csv"), result);
    }

    #[test]
    fn test_create_sub_folder() {
        let sub_folder = create_sub_folder().unwrap();
        assert!(sub_folder.temp_path.is_dir());
        assert!(!sub_folder.path_name.is_empty());
    }
}
