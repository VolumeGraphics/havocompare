use crate::report::{DiffDetail, Difference};
use crate::Error;
use itertools::Itertools;
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::error;

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
/// configuration for the json compare module
pub struct JsonConfig {
    ignore_keys: Vec<String>,
}

pub(crate) fn compare_files<P: AsRef<Path>>(
    nominal: P,
    actual: P,
    config: &JsonConfig,
) -> Result<Difference, Error> {
    let mut diff = Difference::new_for_file(&nominal, &actual);
    let compared_file_name = nominal.as_ref().to_string_lossy().into_owned();

    let nominal = vg_errortools::fat_io_wrap_std(&nominal, &std::fs::read_to_string)?;
    let actual = vg_errortools::fat_io_wrap_std(&actual, &std::fs::read_to_string)?;

    let json_diff = json_diff::process::compare_jsons(&nominal, &actual);
    let json_diff = match json_diff {
        Ok(diff) => diff,
        Err(e) => {
            let error_message =
                format!("JSON deserialization failed for {compared_file_name} (error: {e})");
            error!("{}", error_message);
            diff.push_detail(DiffDetail::Error(error_message));
            diff.error();
            return Ok(diff);
        }
    };
    let filtered_diff: Vec<_> = json_diff
        .all_diffs()
        .into_iter()
        .filter(|(_d, v)| !config.ignore_keys.contains(v))
        .collect();

    if !filtered_diff.is_empty() {
        let all_diffs_log = filtered_diff
            .iter()
            .map(|(d, v)| format!("{d}: {v}"))
            .join("\n");
        diff.push_detail(DiffDetail::Json {
            differences: all_diffs_log,
        });

        diff.error();
    }

    for (d_type, key) in filtered_diff {
        error!("{d_type}: {key}");
    }

    Ok(diff)
}
