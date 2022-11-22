mod template;

use crate::csv::{DiffType, Position, Table};
use serde::Serialize;
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use tera::{Context, Tera};
use tracing::{debug, info};

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
pub struct CSVReport {
    pub nominal_value: String,
    pub actual_value: String,
    pub diffs: Vec<String>,
}

pub fn create_sub_folder(
    rule_name: &str,
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
) -> PathBuf {
    let joined_file_names = nominal
        .as_ref()
        .join(rule_name)
        .join(actual.as_ref().to_string_lossy().to_string())
        .to_string_lossy()
        .to_string();

    let key = format!("havoc-{:x}", md5::compute(joined_file_names.as_bytes()));

    let sub_folder = PathBuf::from(&key);

    if sub_folder.is_dir() {
        fs::remove_dir_all(&sub_folder).expect("Can't delete sub folder");
    }

    debug!("create sub folder {}", &key);

    fs::create_dir(&key).expect("Can't create sub folder");

    sub_folder
}

pub fn write_html_detail(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    diffs: &[String],
    rule_name: &str,
) -> FileCompareResult {
    let mut result = FileCompareResult {
        nominal: nominal.as_ref().to_string_lossy().to_string(),
        actual: actual.as_ref().to_string_lossy().to_string(),
        is_error: false,
        detail_path: None,
    };

    if diffs.is_empty() {
        return result;
    }

    let sub_folder = create_sub_folder(rule_name, nominal.as_ref(), actual.as_ref());

    let detail_file = sub_folder.join(template::DETAIL_FILENAME);

    let mut tera = Tera::default();
    tera.add_raw_template(
        &detail_file.to_string_lossy(),
        template::PLAIN_TEXT_DETAIL_TEMPLATE,
    )
    .expect("Can't add raw template for detail.html");

    let mut ctx = Context::new();
    ctx.insert("actual", &actual.as_ref().to_string_lossy());
    ctx.insert("nominal", &nominal.as_ref().to_string_lossy());

    ctx.insert("errors", diffs);

    let file = File::create(&detail_file).expect("Can't create detail.html");

    info!("detail html {:?} created", &detail_file);

    tera.render_to(&detail_file.to_string_lossy(), &ctx, file)
        .expect("Can't render to detail.html");

    result.is_error = true;
    result.detail_path = Some(sub_folder);

    result
}

pub fn write_csv_detail(
    nominal_table: Table,
    actual_table: Table,
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    diffs: &[DiffType],
    rule_name: &str,
) -> FileCompareResult {
    let mut result = FileCompareResult {
        nominal: nominal.as_ref().to_string_lossy().to_string(),
        actual: actual.as_ref().to_string_lossy().to_string(),
        is_error: false,
        detail_path: None,
    };

    let headers: Vec<_> = nominal_table
        .columns
        .iter()
        .zip(actual_table.columns.iter())
        .map(|(n, a)| CSVReport {
            actual_value: a
                .header
                .as_deref()
                .unwrap_or("Header preprocessing not enabled in config")
                .to_owned(),
            nominal_value: n
                .header
                .as_deref()
                .unwrap_or("Header preprocessing not enabled in config")
                .to_owned(),
            diffs: Vec::new(),
        })
        .collect();

    let rows: Vec<Vec<_>> = nominal_table
        .rows()
        .zip(actual_table.rows())
        .enumerate()
        .map(|(row, (n, a))| {
            n.into_iter()
                .zip(a.into_iter())
                .enumerate()
                .map(|(col, (n, a))| {
                    let current_pos = Position { col, row };
                    CSVReport {
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
                    }
                })
                .collect()
        })
        .collect();

    let sub_folder = create_sub_folder(rule_name, nominal.as_ref(), actual.as_ref());

    let detail_file = sub_folder.join(template::DETAIL_FILENAME);

    let mut tera = Tera::default();
    tera.add_raw_template(
        &detail_file.to_string_lossy(),
        template::PLAIN_CSV_DETAIL_TEMPLATE,
    )
    .expect("Can't add raw template for detail.html");

    let mut ctx = Context::new();
    ctx.insert("actual", &actual.as_ref().to_string_lossy());
    ctx.insert("nominal", &nominal.as_ref().to_string_lossy());
    ctx.insert("rows", &rows);
    ctx.insert("headers", &headers);

    let file = File::create(&detail_file).expect("Can't create detail.html");

    info!("detail html {:?} created", &detail_file);

    tera.render_to(&detail_file.to_string_lossy(), &ctx, file)
        .expect("Can't render to detail.html");

    result.is_error = !diffs.is_empty();
    result.detail_path = Some(sub_folder);

    result
}

pub fn write_image_detail(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    diffs: &[String],
    rule_name: &str,
) -> FileCompareResult {
    let mut result = FileCompareResult {
        nominal: nominal.as_ref().to_string_lossy().to_string(),
        actual: actual.as_ref().to_string_lossy().to_string(),
        is_error: false,
        detail_path: None,
    };

    if diffs.is_empty() {
        return result;
    }

    let sub_folder = create_sub_folder(rule_name, nominal.as_ref(), actual.as_ref());

    let detail_file = sub_folder.join(template::DETAIL_FILENAME);

    let mut tera = Tera::default();
    tera.add_raw_template(
        &detail_file.to_string_lossy(),
        template::PLAIN_IMAGE_DETAIL_TEMPLATE,
    )
    .expect("Can't add raw template for detail.html");

    let mut ctx = Context::new();
    ctx.insert("actual", &actual.as_ref().to_string_lossy());
    ctx.insert("nominal", &nominal.as_ref().to_string_lossy());

    let actual_file_extension = actual
        .as_ref()
        .file_name()
        .expect("Can't get actual image extenstion")
        .to_string_lossy();
    let nominal_file_extension = nominal
        .as_ref()
        .file_name()
        .expect("Can't get nominal image file name")
        .to_string_lossy();

    let actual_image = format!("actual_image_{}", actual_file_extension);
    let nominal_image = format!("nominal_image_.{}", nominal_file_extension);

    fs::copy(actual.as_ref(), sub_folder.join(&actual_image)).expect("Can't copy actual image");
    fs::copy(nominal.as_ref(), sub_folder.join(&nominal_image)).expect("Can't copy nominal image");

    let diff_image = &diffs[1];
    fs::copy(diff_image, sub_folder.join(diff_image)).expect("Can't copy diff image");

    ctx.insert("error", &diffs[0]);
    ctx.insert("diff_image", diff_image);
    ctx.insert("actual_image", &actual_image);
    ctx.insert("nominal_image", &nominal_image);

    let file = File::create(&detail_file).expect("Can't create detail.html");

    info!("detail html {:?} created", &detail_file);

    tera.render_to(&detail_file.to_string_lossy(), &ctx, file)
        .expect("Can't render to detail.html");

    result.is_error = true;
    result.detail_path = Some(sub_folder);

    result
}

pub(crate) fn create(rule_results: &[RuleResult], report_path: impl AsRef<Path>) {
    let report_dir = report_path.as_ref();
    if report_dir.is_dir() {
        info!("Delete report folder");
        fs::remove_dir_all(&report_dir).expect("Can't delete report folder");
    }

    info!("create report folder");
    fs::create_dir(&report_dir).expect("Can't create report folder");

    //move folders
    for rule_result in rule_results.iter() {
        let sub_folder = report_dir.join(&rule_result.rule.name);
        info!("Create subfolder {:?}", &sub_folder);
        fs::create_dir(&sub_folder).expect("Can't create report sub folder");

        for file_result in rule_result.compare_results.iter() {
            if let Some(detail) = &file_result.detail_path {
                let target = &sub_folder.join(detail);
                info!("moving subfolder {:?} to {:?}", &detail, &target);

                let files = crate::glob_files(detail, Some("*"));
                files.iter().for_each(|file| {
                    if let Some(file_name) = file.file_name() {
                        if !target.exists() || !target.is_dir() {
                            debug!(
                                "creating target subfolder {} in the report dir ",
                                target.to_string_lossy()
                            );
                            fs::create_dir(target).expect("can't create dir");
                        }
                        debug!("copying file to target {}", file.to_string_lossy());
                        fs::copy(file, target.join(file_name)).expect("can't copy file");
                    }
                });
                debug!("removing temporary subfolder {}", detail.to_string_lossy());
                fs::remove_dir_all(detail).expect("can't delete dir");
            }
        }
    }

    write_index(report_dir, rule_results);
}

pub(crate) fn write_index(report_dir: impl AsRef<Path>, rule_results: &[RuleResult]) {
    let index_file = report_dir.as_ref().join(template::INDEX_FILENAME);

    let mut tera = Tera::default();
    tera.add_raw_template(&index_file.to_string_lossy(), template::INDEX_TEMPLATE)
        .expect("Can't add raw template for index.html");

    let mut ctx = Context::new();
    ctx.insert("rule_results", rule_results);
    ctx.insert("detail_filename", template::DETAIL_FILENAME);

    let file = File::create(&index_file).expect("Can't create index.html");
    tera.render_to(&index_file.to_string_lossy(), &ctx, file)
        .expect("Can't render to index.html");

    info!("Report.html created");
}
