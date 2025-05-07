use crate::report::{DiffDetail, Difference};
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::error;

#[derive(Debug, Error)]
/// Errors during html / plain text checking
pub enum Error {
    #[error("Failed to remove path's prefix")]
    StripPrefixError(#[from] path::StripPrefixError),
}

#[derive(JsonSchema, Deserialize, Serialize, Debug, Clone)]
pub struct DirectoryConfig {
    pub mode: Mode,
}

#[derive(JsonSchema, Deserialize, Serialize, Debug, Clone)]
pub enum Mode {
    /// check whether both paths are really the same: whether entry is missing in actual, and/or if entry exists in actual but not in nominal
    Identical,
    /// check only if entry is missing in actual, ignoring entries that exist in actual but not in nominal
    MissingOnly,
}

pub(crate) fn compare_paths<P: AsRef<Path>>(
    nominal: P,
    actual: P,
    nominal_entries: &[PathBuf],
    actual_entries: &[PathBuf],
    config: &DirectoryConfig,
) -> Result<Difference, Error> {
    let nominal_path = nominal.as_ref();
    let actual_path = actual.as_ref();

    let mut difference = Difference::new_for_file(nominal_path, actual_path);

    //remove root paths!
    let nominal_entries: Result<Vec<_>, path::StripPrefixError> = nominal_entries
        .iter()
        .map(|path| path.strip_prefix(nominal_path))
        .collect();
    let nominal_entries = nominal_entries?;

    let actual_entries: Result<Vec<_>, path::StripPrefixError> = actual_entries
        .iter()
        .map(|path| path.strip_prefix(actual_path))
        .collect();
    let actual_entries = actual_entries?;

    let mut is_the_same = true;
    if matches!(config.mode, Mode::Identical | Mode::MissingOnly) {
        nominal_entries.iter().for_each(|entry| {
            let detail = if let Some(f) = actual_entries.iter().find(|a| *a == entry) {
                (f.to_string_lossy().to_string(), false)
            } else {
                error!("{:?} doesn't exists in the actual folder", entry);
                is_the_same = false;
                ("".to_owned(), true)
            };

            difference.push_detail(DiffDetail::File {
                nominal: entry.to_string_lossy().to_string(),
                actual: detail.0,
                error: detail.1,
            });
        });
    }

    if matches!(config.mode, Mode::Identical) {
        actual_entries.iter().for_each(|entry| {
            if !nominal_entries.iter().any(|n| n == entry) {
                difference.push_detail(DiffDetail::File {
                    nominal: "".to_owned(),
                    actual: entry.to_string_lossy().to_string(),
                    error: true,
                });

                error!("Additional entry {:?} found in the actual folder", entry);
                is_the_same = false;
            }
        });
    }

    if !is_the_same {
        difference.error();
    }

    Ok(difference)
}

#[cfg(test)]

mod test {
    use super::*;

    #[test]
    fn test_compare_directories() {
        let nominal_dir = tempfile::Builder::new()
            .prefix("my-nominal")
            // .keep(true)
            .rand_bytes(1)
            .tempdir_in("tests")
            .expect("");

        std::fs::create_dir_all(nominal_dir.path().join("dir/a/aa")).expect("");
        std::fs::create_dir_all(nominal_dir.path().join("dir/b")).expect("");
        std::fs::create_dir_all(nominal_dir.path().join("dir/c")).expect("");

        let actual_dir = tempfile::Builder::new()
            .prefix("my-actual")
            // .keep(true)
            .rand_bytes(1)
            .tempdir_in("tests")
            .expect("");

        std::fs::create_dir_all(actual_dir.path().join("dir/a/aa")).expect("");
        std::fs::create_dir_all(actual_dir.path().join("dir/b")).expect("");
        std::fs::create_dir_all(actual_dir.path().join("dir/c")).expect("");

        let pattern_include = ["**/*/"];
        let pattern_exclude: Vec<String> = Vec::new();

        let nominal_entries =
            crate::get_files(&nominal_dir, &pattern_include, &pattern_exclude).expect("");
        let actual_entries =
            crate::get_files(&actual_dir, &pattern_include, &pattern_exclude).expect("");

        let result = compare_paths(
            nominal_dir.path(),
            actual_dir.path(),
            &nominal_entries,
            &actual_entries,
            &DirectoryConfig {
                mode: Mode::Identical,
            },
        )
        .expect("");

        assert!(!result.is_error);

        std::fs::create_dir_all(actual_dir.path().join("dir/d")).expect("");

        let nominal_entries =
            crate::get_files(&nominal_dir, &pattern_include, &pattern_exclude).expect("");
        let actual_entries =
            crate::get_files(&actual_dir, &pattern_include, &pattern_exclude).expect("");

        let result = compare_paths(
            nominal_dir.path(),
            actual_dir.path(),
            &nominal_entries,
            &actual_entries,
            &DirectoryConfig {
                mode: Mode::Identical,
            },
        )
        .expect("");

        assert!(result.is_error);

        let result = compare_paths(
            nominal_dir.path(),
            actual_dir.path(),
            &nominal_entries,
            &actual_entries,
            &DirectoryConfig {
                mode: Mode::MissingOnly,
            },
        )
        .expect("");

        assert!(!result.is_error);

        std::fs::create_dir_all(nominal_dir.path().join("dir/d")).expect("");
        std::fs::create_dir_all(nominal_dir.path().join("dir/e")).expect("");

        let nominal_entries =
            crate::get_files(&nominal_dir, &pattern_include, &pattern_exclude).expect("");
        let actual_entries =
            crate::get_files(&actual_dir, &pattern_include, &pattern_exclude).expect("");

        let result = compare_paths(
            nominal_dir.path(),
            actual_dir.path(),
            &nominal_entries,
            &actual_entries,
            &DirectoryConfig {
                mode: Mode::Identical,
            },
        )
        .expect("");

        assert!(result.is_error);

        let result = compare_paths(
            nominal_dir.path(),
            actual_dir.path(),
            &nominal_entries,
            &actual_entries,
            &DirectoryConfig {
                mode: Mode::MissingOnly,
            },
        )
        .expect("");

        assert!(result.is_error);
    }
}
