use std::path::Path;

use itertools::Itertools;
use json_diff_ng::DiffType;
use regex::Regex;
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::report::{DiffDetail, Difference};
use crate::Error;

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
/// configuration for the json compare module
pub struct JsonConfig {
    #[serde(default)]
    ignore_keys: Vec<String>,
    #[serde(default)]
    sort_arrays: bool,
}
impl JsonConfig {
    pub(crate) fn get_ignore_list(&self) -> Result<Vec<Regex>, regex::Error> {
        self.ignore_keys.iter().map(|v| Regex::new(v)).collect()
    }
}

pub(crate) fn compare_files<P: AsRef<Path>>(
    nominal: P,
    actual: P,
    config: &JsonConfig,
) -> Result<Difference, Error> {
    let mut diff = Difference::new_for_file(&nominal, &actual);
    let compared_file_name = nominal.as_ref().to_string_lossy().into_owned();

    let nominal: String = vg_errortools::fat_io_wrap_std(&nominal, &std::fs::read_to_string)?;
    let actual: String = vg_errortools::fat_io_wrap_std(&actual, &std::fs::read_to_string)?;
    let ignores = config.get_ignore_list()?;

    let json_diff = json_diff_ng::compare_strs(&nominal, &actual, config.sort_arrays, &ignores);
    let json_diff = match json_diff {
        Ok(diff) => diff,
        Err(e) => {
            let error_message =
                format!("JSON comparison failed for {compared_file_name} (error: {e})");
            error!("{}", error_message);
            diff.push_detail(DiffDetail::Error(error_message));
            diff.error();
            return Ok(diff);
        }
    };
    let filtered_diff: Vec<_> = json_diff.all_diffs();

    if !filtered_diff.is_empty() {
        for (d_type, key) in filtered_diff.iter() {
            error!("{d_type}: {key}");
        }
        let left = filtered_diff
            .iter()
            .filter_map(|(k, v)| {
                if matches!(k, DiffType::LeftExtra) {
                    Some(v.to_string())
                } else {
                    None
                }
            })
            .join("\n");
        let right = filtered_diff
            .iter()
            .filter_map(|(k, v)| {
                if matches!(k, DiffType::RightExtra) {
                    Some(v.to_string())
                } else {
                    None
                }
            })
            .join("\n");
        let differences = filtered_diff
            .iter()
            .filter_map(|(k, v)| {
                if matches!(k, DiffType::Mismatch) {
                    Some(v.to_string())
                } else {
                    None
                }
            })
            .join("\n");
        let root_mismatch = filtered_diff
            .iter()
            .find(|(k, _v)| matches!(k, DiffType::RootMismatch))
            .map(|(_, v)| v.to_string());

        diff.push_detail(DiffDetail::Json {
            differences,
            left,
            right,
            root_mismatch,
        });

        diff.error();
    }

    Ok(diff)
}

#[cfg(test)]
mod test {
    use super::*;

    fn trim_split(list: &str) -> Vec<&str> {
        list.split('\n').map(|e| e.trim()).collect()
    }

    #[test]
    fn no_filter() {
        let cfg = JsonConfig {
            ignore_keys: vec![],
            sort_arrays: false,
        };
        let result = compare_files(
            "tests/integ/data/json/expected/guy.json",
            "tests/integ/data/json/actual/guy.json",
            &cfg,
        )
        .unwrap();
        if let DiffDetail::Json {
            differences,
            left,
            right,
            root_mismatch,
        } = result.detail.first().unwrap()
        {
            let differences = trim_split(differences);

            assert!(differences.contains(&".car.(\"RX7\" != \"Panda Trueno\")"));
            assert!(differences.contains(&".age.(21 != 18)"));
            assert!(differences.contains(&".name.(\"Keisuke\" != \"Takumi\")"));
            assert_eq!(differences.len(), 3);

            assert_eq!(left.as_str(), ".brothers");
            assert!(right.is_empty());
            assert!(root_mismatch.is_none());
        } else {
            panic!("wrong diffdetail");
        }
    }

    #[test]
    fn filter_works() {
        let cfg = JsonConfig {
            ignore_keys: vec!["name".to_string(), "brother(s?)".to_string()],
            sort_arrays: false,
        };
        let result = compare_files(
            "tests/integ/data/json/expected/guy.json",
            "tests/integ/data/json/actual/guy.json",
            &cfg,
        )
        .unwrap();
        if let DiffDetail::Json {
            differences,
            left,
            right,
            root_mismatch,
        } = result.detail.first().unwrap()
        {
            let differences = trim_split(differences);
            assert!(differences.contains(&".car.(\"RX7\" != \"Panda Trueno\")"));
            assert!(differences.contains(&".age.(21 != 18)"));
            assert_eq!(differences.len(), 2);
            assert!(right.is_empty());
            assert!(left.is_empty());
            assert!(root_mismatch.is_none());
        } else {
            panic!("wrong diffdetail");
        }
    }
}
