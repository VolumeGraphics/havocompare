mod template;

use crate::csv::{Delimiters, DiffType, Position, Table};
use crate::properties::MetaDataPropertyDiff;
use crate::{ComparisonMode, Rule};
use pdf_extract::extract_text;
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
    #[error("CSV failed {0}")]
    Csv(#[from] crate::csv::Error),
    #[error("PDF Extract failed {0}")]
    PdfExtract(#[from] pdf_extract::OutputError),
}

#[derive(Serialize, Debug, Default, Clone)]
pub struct AdditionalOverviewColumn {
    pub nominal_value: String,
    pub actual_value: String,
    pub is_error: bool,
}

#[derive(Serialize, Debug, Clone)]
pub struct DetailPath {
    pub path: PathBuf,
    pub name: String,
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
    pub has_diff: bool,  //tolerable error
    pub has_error: bool, //intolerable error
}

#[derive(Serialize, Debug, Clone)]
pub struct RuleDifferences {
    pub rule: Rule,
    pub diffs: Vec<Difference>,
}

#[derive(Serialize, Debug, Clone)]
pub struct RenderToHtmlRuleDifferences {
    pub rule: Rule,
    pub diffs: Vec<RenderToHtmlDifference>,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Difference {
    pub nominal_file: PathBuf,
    pub actual_file: PathBuf,
    pub relative_file_path: String,
    pub is_error: bool,
    pub detail: Vec<DiffDetail>,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct RenderToHtmlDifference {
    #[serde(flatten)]
    pub diff: Difference,
    pub detail_path: Option<DetailPath>,
    pub additional_columns: Vec<AdditionalOverviewColumn>,
}

impl Difference {
    pub fn new_for_file(nominal: impl AsRef<Path>, actual: impl AsRef<Path>) -> Self {
        Self {
            relative_file_path: get_relative_path(actual.as_ref(), nominal.as_ref())
                .to_string_lossy()
                .to_string(),
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

pub fn create_detail_folder(report_dir: impl AsRef<Path>) -> Result<DetailPath, Error> {
    let temp_path = tempfile::Builder::new()
        .prefix("havocompare-")
        .tempdir_in(report_dir.as_ref())?
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
        path: temp_path,
        name: path_name,
    })
}

pub fn write_html_detail(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    diffs: &[String],
    report_dir: impl AsRef<Path>,
) -> Result<Option<DetailPath>, Error> {
    if diffs.is_empty() {
        return Ok(None);
    }

    let detail_path = create_detail_folder(report_dir.as_ref())?;

    let detail_file = detail_path.path.join(template::DETAIL_FILENAME);

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

    Ok(Some(detail_path))
}

pub(crate) fn write_csv_detail(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    diffs: &[&DiffType],
    config: &Delimiters,
    report_dir: impl AsRef<Path>,
) -> Result<Option<DetailPath>, Error> {
    let mut headers: CSVReportRow = CSVReportRow {
        columns: vec![],
        has_diff: false,
        has_error: false,
    };

    let nominal_table = Table::from_reader(File::open(nominal.as_ref())?, config)?;
    let actual_table = Table::from_reader(File::open(actual.as_ref())?, config)?;
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

    let detail_path = create_detail_folder(report_dir)?;

    let detail_file = detail_path.path.join(template::DETAIL_FILENAME);

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

    Ok(Some(detail_path))
}

pub fn write_image_detail(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    diffs: &[(&f64, &String)],
    report_dir: impl AsRef<Path>,
) -> Result<Option<DetailPath>, Error> {
    if diffs.is_empty() {
        return Ok(None);
    }

    let detail_path = create_detail_folder(report_dir.as_ref())?;

    let detail_file = detail_path.path.join(template::DETAIL_FILENAME);

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

    fs::copy(actual.as_ref(), detail_path.path.join(&actual_image))
        .map_err(|e| FatIOError::from_std_io_err(e, actual.as_ref().to_path_buf()))?;
    fs::copy(nominal.as_ref(), detail_path.path.join(&nominal_image))
        .map_err(|e| FatIOError::from_std_io_err(e, nominal.as_ref().to_path_buf()))?;

    let (score, diff_image) = diffs[0];
    let img_target = detail_path.path.join(diff_image);
    fs::copy(diff_image, &img_target)
        .map_err(|e| FatIOError::from_std_io_err(e, img_target.to_path_buf()))?;

    ctx.insert("error", score); //TODO: rename this correctly
    ctx.insert("diff_image", diff_image);
    ctx.insert("actual_image", &actual_image);
    ctx.insert("nominal_image", &nominal_image);

    let file = fat_io_wrap_std(&detail_file, &File::create)?;
    debug!("detail html {:?} created", &detail_file);

    tera.render_to(&detail_file.to_string_lossy(), &ctx, file)?;

    Ok(Some(detail_path))
}

pub fn write_pdf_detail(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    diffs: &[(&usize, &f64)],
    report_dir: impl AsRef<Path>,
) -> Result<Option<DetailPath>, Error> {
    let detail_path = create_detail_folder(report_dir.as_ref())?;

    let nominal_string = extract_text(nominal.as_ref())?;
    let actual_string = extract_text(actual.as_ref())?;

    let nominal_extracted_filename = "nominal_extracted_text.txt";
    let actual_extracted_filename = "actual_extracted_text.txt";

    let nominal_extracted_file = detail_path.path.join(nominal_extracted_filename);
    fs::write(&nominal_extracted_file, nominal_string.as_bytes())
        .map_err(|e| FatIOError::from_std_io_err(e, nominal_extracted_file))?;

    let actual_extracted_file = detail_path.path.join(actual_extracted_filename);
    fs::write(&actual_extracted_file, actual_string.as_bytes())
        .map_err(|e| FatIOError::from_std_io_err(e, actual_extracted_file))?;
    info!("Extracted text written to files");

    let detail_file = detail_path.path.join(template::DETAIL_FILENAME);

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

            if let Some(diff) = diffs.iter().find(|(i, t)| **i == l) {
                result.diffs.push(format!(
                    "Missmatch in line {}, threshold: {}",
                    diff.0, diff.1
                ));
            };

            result
        })
        .collect();

    let mut ctx = Context::new();
    ctx.insert("actual", &actual.as_ref().to_string_lossy());
    ctx.insert("nominal", &nominal.as_ref().to_string_lossy());
    ctx.insert("combined_lines", &combined_lines);
    ctx.insert("nominal_extracted_filename", nominal_extracted_filename);
    ctx.insert("actual_extracted_filename", actual_extracted_filename);

    ctx.insert("errors", diffs);
    let file = fat_io_wrap_std(&detail_file, &File::create)?;
    debug!("detail html {:?} created", &detail_file);

    tera.render_to(&detail_file.to_string_lossy(), &ctx, file)?;

    Ok(Some(detail_path))
}

pub fn write_external_detail(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    stdout: &str,
    stderr: &str,
    message: &str,
    report_dir: impl AsRef<Path>,
) -> Result<Option<DetailPath>, Error> {
    let detail_path = create_detail_folder(report_dir.as_ref())?;
    let detail_file = detail_path.path.join(template::DETAIL_FILENAME);

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

    Ok(Some(detail_path))
}

fn create_error_detail(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    error: Box<dyn std::error::Error>,
    report_dir: impl AsRef<Path>,
) -> Result<DetailPath, Error> {
    let sub_folder = create_detail_folder(report_dir.as_ref())?;
    let detail_file = sub_folder.path.join(template::DETAIL_FILENAME);

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
    report_dir: impl AsRef<Path>,
) -> RenderToHtmlDifference {
    let mut result = RenderToHtmlDifference {
        diff: Default::default(),
        detail_path: None,
        additional_columns: vec![],
    };

    if let Ok(sub_folder) = create_error_detail(nominal, actual, error, report_dir) {
        result.detail_path = Some(sub_folder);
    } else {
        error!("Could not create error detail");
    }

    result
}

pub(crate) fn create_reports(
    rule_differences: &[RuleDifferences],
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

    create_json(rule_differences, &report_path)?;
    create_html(rule_differences, &report_path)?;

    Ok(())
}

pub(crate) fn create_json(
    rule_differences: &[RuleDifferences],
    report_path: impl AsRef<Path>,
) -> Result<(), Error> {
    let _reporting_span = span!(tracing::Level::INFO, "JSON");
    let _reporting_span = _reporting_span.enter();
    let report_dir = report_path.as_ref();
    let writer = report_dir.join("report.json");
    let writer = fat_io_wrap_std(writer, &File::create)?;
    serde_json::to_writer_pretty(writer, &rule_differences)?;
    Ok(())
}

pub(crate) fn create_html(
    rule_differences: &[RuleDifferences],
    report_path: impl AsRef<Path>,
) -> Result<(), Error> {
    let _reporting_span = span!(tracing::Level::INFO, "HTML");
    let _reporting_span = _reporting_span.enter();
    let report_dir = report_path.as_ref();

    let mut html_rule_differences: Vec<RenderToHtmlRuleDifferences> = Vec::new();
    for rule_difference in rule_differences.iter() {
        let sub_folder = report_dir.join(&rule_difference.rule.name);
        debug!("Create subfolder {:?}", &sub_folder);
        fat_io_wrap_std(&sub_folder, &fs::create_dir)?;

        let render_diffs: Vec<_> = rule_difference
            .diffs
            .iter()
            .map(|file| {
                // let diffs: Vec<&DiffDetail> = file
                //     .detail
                //     .iter()
                //     .filter(|r| matches!(r, DiffDetail::Error(_)))
                //     .collect();

                // TODO: Write diffs to report -> this is crate error?

                //TODO: use Unwrap_or_else display error in report + console
                let detail_path = match &rule_difference.rule.file_type {
                    ComparisonMode::CSV(config) => {
                        let diffs: Vec<&DiffType> = file
                            .detail
                            .iter()
                            .filter_map(|r| match r {
                                DiffDetail::CSV(d) => Some(d),
                                _ => None,
                            })
                            .collect();

                        write_csv_detail(
                            &file.nominal_file,
                            &file.actual_file,
                            &diffs,
                            &config.delimiters,
                            &sub_folder,
                        )
                        .unwrap_or_default()
                    }
                    ComparisonMode::PlainText(_) => {
                        let diffs: Vec<String> = file
                            .detail
                            .iter()
                            .filter_map(|r| match r {
                                DiffDetail::Text { line, score } => {
                                    Some(format!("{} {}", line, score)) //TODO: want the text difference
                                }
                                _ => None,
                            })
                            .collect();

                        write_html_detail(
                            &file.nominal_file,
                            &file.actual_file,
                            &diffs,
                            &sub_folder,
                        )
                        .unwrap_or_default()
                    }
                    ComparisonMode::PDFText(_) => {
                        let diffs: Vec<(&usize, &f64)> = file
                            .detail
                            .iter()
                            .filter_map(|r| match r {
                                DiffDetail::Text { line, score } => {
                                    //TODO: displayed line should be + 1
                                    Some((line, score)) //TODO: want the text difference
                                }
                                _ => None,
                            })
                            .collect();

                        write_pdf_detail(&file.nominal_file, &file.actual_file, &diffs, &sub_folder)
                            .unwrap_or_default()
                    }
                    ComparisonMode::Image(_) => {
                        let diffs: Vec<(&f64, &String)> = file
                            .detail
                            .iter()
                            .filter_map(|r| match r {
                                DiffDetail::Image { score, diff_image } => {
                                    Some((score, diff_image))
                                }
                                _ => None,
                            })
                            .collect();

                        write_image_detail(
                            &file.nominal_file,
                            &file.actual_file,
                            &diffs, //should actually only 1 image per file compare
                            &sub_folder,
                        )
                        .unwrap_or_default()
                    }
                    ComparisonMode::External(_) => {
                        if let Some((stdout, stderr)) = file
                            .detail
                            .iter()
                            .filter_map(|r| match r {
                                DiffDetail::External { stdout, stderr } => Some((stdout, stderr)),
                                _ => None,
                            })
                            .next()
                        {
                            write_external_detail(
                                &file.nominal_file,
                                &file.actual_file,
                                stdout,
                                stderr,
                                "", //TODO: this is DiffDetail::Error
                                &sub_folder,
                            )
                            .unwrap_or_default()
                        } else {
                            None
                        }
                    }
                    ComparisonMode::FileProperties(_) => None, //we need only additional columns in the index.html
                    ComparisonMode::Hash(_) => {
                        let diffs: Vec<String> = file
                            .detail
                            .iter()
                            .filter_map(|r| match r {
                                DiffDetail::Hash { actual, nominal } => Some(format!(
                                    "Nominal file's hash is '{}' actual is '{}'",
                                    nominal, actual
                                )),
                                _ => None,
                            })
                            .collect();

                        write_html_detail(
                            &file.nominal_file,
                            &file.actual_file,
                            &diffs,
                            &sub_folder,
                        )
                        .unwrap_or_default()
                    }
                };

                let additional_columns: Vec<AdditionalOverviewColumn> =
                    match &rule_difference.rule.file_type {
                        ComparisonMode::FileProperties(_) => {
                            let mut additional_columns: Vec<AdditionalOverviewColumn> = Vec::new();

                            let diffs: Vec<&MetaDataPropertyDiff> = file
                                .detail
                                .iter()
                                .filter_map(|r| match r {
                                    DiffDetail::Properties(diff) => Some(diff),
                                    _ => None,
                                })
                                .collect();

                            let result: AdditionalOverviewColumn = if diffs
                                .iter()
                                .any(|d| matches!(d, MetaDataPropertyDiff::IllegalName))
                            {
                                AdditionalOverviewColumn {
                                    nominal_value: file.nominal_file.to_string_lossy().to_string(),
                                    actual_value: file.actual_file.to_string_lossy().to_string(),
                                    is_error: true,
                                }
                            } else {
                                Default::default()
                            };
                            additional_columns.push(result);

                            let result: AdditionalOverviewColumn =
                                if let Some(MetaDataPropertyDiff::Size { nominal, actual }) = diffs
                                    .iter()
                                    .find(|d| matches!(d, MetaDataPropertyDiff::Size { .. }))
                                {
                                    AdditionalOverviewColumn {
                                        nominal_value: format!("{nominal}"),
                                        actual_value: format!("{actual}"),
                                        is_error: true,
                                    }
                                } else {
                                    Default::default()
                                };
                            additional_columns.push(result);

                            let result: AdditionalOverviewColumn =
                                if let Some(MetaDataPropertyDiff::CreationDate {
                                    nominal,
                                    actual,
                                }) = diffs.iter().find(|d| {
                                    matches!(d, MetaDataPropertyDiff::CreationDate { .. })
                                }) {
                                    AdditionalOverviewColumn {
                                        nominal_value: nominal.clone(),
                                        actual_value: actual.clone(),
                                        is_error: true,
                                    }
                                } else {
                                    Default::default()
                                };
                            additional_columns.push(result);

                            additional_columns
                        }
                        _ => Vec::new(),
                    };

                RenderToHtmlDifference {
                    diff: file.clone(),
                    detail_path,
                    additional_columns,
                }
            })
            .collect();

        html_rule_differences.push(RenderToHtmlRuleDifferences {
            rule: rule_difference.rule.clone(),
            diffs: render_diffs,
        });
    }

    write_index(report_dir, &html_rule_differences)?;

    Ok(())
}

pub(crate) fn write_index(
    report_dir: impl AsRef<Path>,
    rule_results: &[RenderToHtmlRuleDifferences],
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
        let report_dir = tempfile::tempdir().unwrap();
        let sub_folder = create_detail_folder(&report_dir).unwrap();
        assert!(sub_folder.path.is_dir());
        assert!(!sub_folder.name.is_empty());
    }
}
