mod template;

use crate::csv::{DiffType, Position, Table};
use serde::Serialize;
use std::borrow::Cow;
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use tera::{Context, Tera};
use thiserror::Error;
use tracing::{debug, info};
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
}

#[derive(Serialize, Debug)]
pub struct FileCompareResult {
    pub nominal: String,
    pub actual: String,
    pub is_error: bool,
    pub detail_path: Option<PathBuf>,
}

#[derive(Serialize, Debug)]
pub(crate) struct RuleResult {
    pub rule: crate::Rule,
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

pub fn create_sub_folder(
    rule_name: &str,
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
) -> Result<PathBuf, Error> {
    let mut joined_file_names = nominal.as_ref().to_string_lossy().to_string();

    joined_file_names.push_str(actual.as_ref().to_string_lossy().to_string().as_str());
    joined_file_names.push_str(rule_name);

    debug!("Joined name: {}", &joined_file_names);

    let key = format!("havoc-{:x}", md5::compute(joined_file_names.as_bytes()));

    let sub_folder = PathBuf::from(&key);

    if sub_folder.is_dir() {
        fat_io_wrap_std(&sub_folder, &fs::remove_dir_all)?;
    }

    debug!("create sub folder {}", &key);
    fat_io_wrap_std(&key, &fs::create_dir)?;

    Ok(sub_folder)
}

pub fn write_html_detail(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    diffs: &[String],
    rule_name: &str,
) -> Result<FileCompareResult, Error> {
    let nominal_file_name = get_file_name(nominal.as_ref())?.to_string();
    let actual_file_name = get_file_name(actual.as_ref())?.to_string();

    let mut result = FileCompareResult {
        nominal: nominal_file_name,
        actual: actual_file_name,
        is_error: false,
        detail_path: None,
    };

    if diffs.is_empty() {
        return Ok(result);
    }

    let sub_folder = create_sub_folder(rule_name, nominal.as_ref(), actual.as_ref())?;

    let detail_file = sub_folder.join(template::DETAIL_FILENAME);

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

    info!("detail html {:?} created", &detail_file);

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
    rule_name: &str,
) -> Result<FileCompareResult, Error> {
    let nominal_file_name = get_file_name(nominal.as_ref())?.to_string();
    let actual_file_name = get_file_name(actual.as_ref())?.to_string();

    let mut result = FileCompareResult {
        nominal: nominal_file_name,
        actual: actual_file_name,
        is_error: false,
        detail_path: None,
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

            if a_header.is_some() && n_header.is_some() {
                let actual_value = a_header
                    .unwrap_or("Header preprocessing not enabled in config")
                    .to_owned();

                let nominal_value = n_header
                    .unwrap_or("Header preprocessing not enabled in config")
                    .to_owned();

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
                                };

                                position.row == current_pos.row && position.col == current_pos.col
                            })
                            .map(|diff| match diff {
                                DiffType::UnequalStrings { .. } => "Different strings".to_owned(),
                                DiffType::OutOfTolerance { mode, .. } => {
                                    format!("Out of tolerance. Mode: {}", mode)
                                }
                                DiffType::DifferentValueTypes { .. } => {
                                    "Different value types".to_owned()
                                }
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

    let sub_folder = create_sub_folder(rule_name, nominal.as_ref(), actual.as_ref())?;

    let detail_file = sub_folder.join(template::DETAIL_FILENAME);

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
    info!("detail html {:?} created", &detail_file);

    tera.render_to(&detail_file.to_string_lossy(), &ctx, file)?;

    result.is_error = !diffs.is_empty();
    result.detail_path = Some(sub_folder);

    Ok(result)
}

pub fn write_image_detail(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    diffs: &[String],
    rule_name: &str,
) -> Result<FileCompareResult, Error> {
    let nominal_file_name = get_file_name(nominal.as_ref())?.to_string();
    let actual_file_name = get_file_name(actual.as_ref())?.to_string();

    let mut result = FileCompareResult {
        nominal: nominal_file_name,
        actual: actual_file_name,
        is_error: false,
        detail_path: None,
    };

    if diffs.is_empty() {
        return Ok(result);
    }

    let sub_folder = create_sub_folder(rule_name, nominal.as_ref(), actual.as_ref())?;

    let detail_file = sub_folder.join(template::DETAIL_FILENAME);

    let mut tera = Tera::default();
    tera.add_raw_template(
        &detail_file.to_string_lossy(),
        template::PLAIN_IMAGE_DETAIL_TEMPLATE,
    )?;

    let mut ctx = Context::new();
    ctx.insert("actual", &actual.as_ref().to_string_lossy());
    ctx.insert("nominal", &nominal.as_ref().to_string_lossy());

    let actual_file_extension = get_file_name(actual.as_ref())?;
    let nominal_file_extension = get_file_name(nominal.as_ref())?;

    let actual_image = format!("actual_image_{}", actual_file_extension);
    let nominal_image = format!("nominal_image_.{}", nominal_file_extension);

    fs::copy(actual.as_ref(), sub_folder.join(&actual_image))
        .map_err(|e| FatIOError::from_std_io_err(e, actual.as_ref().to_path_buf()))?;
    fs::copy(nominal.as_ref(), sub_folder.join(&nominal_image))
        .map_err(|e| FatIOError::from_std_io_err(e, nominal.as_ref().to_path_buf()))?;

    let diff_image = &diffs[1];
    let img_target = sub_folder.join(diff_image);
    fs::copy(diff_image, &img_target)
        .map_err(|e| FatIOError::from_std_io_err(e, img_target.to_path_buf()))?;

    ctx.insert("error", &diffs[0]);
    ctx.insert("diff_image", diff_image);
    ctx.insert("actual_image", &actual_image);
    ctx.insert("nominal_image", &nominal_image);

    let file = fat_io_wrap_std(&detail_file, &File::create)?;
    info!("detail html {:?} created", &detail_file);

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
    rule_name: &str,
) -> Result<FileCompareResult, Error> {
    let nominal_file_name = get_file_name(nominal.as_ref())?.to_string();
    let actual_file_name = get_file_name(actual.as_ref())?.to_string();

    let mut result = FileCompareResult {
        nominal: nominal_file_name,
        actual: actual_file_name,
        is_error: false,
        detail_path: None,
    };

    let sub_folder = create_sub_folder(rule_name, nominal.as_ref(), actual.as_ref())?;

    let nominal_extracted_filename = "nominal_extracted_text.txt";
    let actual_extracted_filename = "actual_extracted_text.txt";

    let nominal_extracted_file = sub_folder.join(nominal_extracted_filename);
    fs::write(&nominal_extracted_file, nominal_string.as_bytes())
        .map_err(|e| FatIOError::from_std_io_err(e, nominal_extracted_file))?;

    let actual_extracted_file = sub_folder.join(actual_extracted_filename);
    fs::write(&actual_extracted_file, actual_string.as_bytes())
        .map_err(|e| FatIOError::from_std_io_err(e, actual_extracted_file))?;
    info!("Extracted text written to files");

    let detail_file = sub_folder.join(template::DETAIL_FILENAME);

    let mut tera = Tera::default();
    tera.add_raw_template(
        &detail_file.to_string_lossy(),
        template::PLAIN_PDF_DETAIL_TEMPLATE,
    )?;

    let combined_lines: Vec<CSVReportColumn> = actual_string
        .lines()
        .enumerate()
        .into_iter()
        .zip(nominal_string.lines().into_iter())
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
    info!("detail html {:?} created", &detail_file);

    tera.render_to(&detail_file.to_string_lossy(), &ctx, file)?;

    result.is_error = !diffs.is_empty();
    result.detail_path = Some(sub_folder);

    Ok(result)
}

pub(crate) fn create(
    rule_results: &[RuleResult],
    report_path: impl AsRef<Path>,
) -> Result<(), Error> {
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
        info!("Create subfolder {:?}", &sub_folder);
        fat_io_wrap_std(&sub_folder, &fs::create_dir)?;
        for file_result in rule_result.compare_results.iter() {
            if let Some(detail) = &file_result.detail_path {
                let target = &sub_folder.join(detail);
                info!("moving subfolder {:?} to {:?}", &detail, &target);

                let files = crate::glob_files(detail, &["*"])?;
                for file in files.iter() {
                    if let Some(file_name) = file.file_name() {
                        if !target.exists() || !target.is_dir() {
                            debug!(
                                "creating target subfolder {} in the report dir ",
                                target.to_string_lossy()
                            );
                            fat_io_wrap_std(&target, &fs::create_dir)?;
                        }
                        debug!("copying file to target {}", file.to_string_lossy());
                        fs::copy(file, target.join(file_name))
                            .map_err(|e| FatIOError::from_std_io_err(e, file.clone()))?;
                    }
                }
                debug!("removing temporary subfolder {}", detail.to_string_lossy());
                fat_io_wrap_std(detail, &fs::remove_dir_all)?;
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

    info!("Report.html created");
    Ok(())
}
