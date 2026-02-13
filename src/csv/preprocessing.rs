use crate::csv;
use crate::csv::value::Value;
use crate::csv::Table;
use glob::Pattern;
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering::Equal;
use tracing::{debug, info, warn};

#[derive(JsonSchema, Deserialize, Serialize, Debug, Clone)]
/// Preprocessor options
pub enum Preprocessor {
    /// Try to extract the headers from the first row - fallible if first row contains a number
    ExtractHeaders,
    /// Replace all fields in column by number by a deleted marker
    DeleteColumnByNumber(usize),
    /// Replace all fields in column by name by a deleted marker
    DeleteColumnByName(String),
    /// Replace all fields in column whose header matches a glob pattern by a deleted marker
    DeleteColumnByNameG(String),
    /// Replace with deleted marker: all fields in all columns that do not match any of the specified name
    KeepColumnsByName(Vec<String>),
    /// Replace with deleted marker: all fields in all columns that do not match any of the specified glob patterns
    KeepColumnsByNameG(Vec<String>),
    /// Sort rows by column with given name. Fails if no headers were extracted or column name is not found, or if any row has no numbers there
    SortByColumnName(String),
    /// Sort rows by column with given number. Fails if any row has no numbers there or if out of bounds.
    SortByColumnNumber(usize),
    /// Replace all fields in row with given number by a deleted marker
    DeleteRowByNumber(usize),
    /// Replace all fields in row  where at least a single field matches regex by a deleted marker
    DeleteRowByRegex(String),
    /// replace found cell using row and column index by a deleted marker
    DeleteCellByNumber {
        /// column number
        column: usize,
        /// row number
        row: usize,
    },
    /// replace found cell using column header and row index by a deleted marker
    DeleteCellByName {
        /// column with given name
        column: String,
        /// row number
        row: usize,
    },
}

impl Preprocessor {
    pub(crate) fn process(&self, table: &mut Table) -> Result<(), csv::Error> {
        match self {
            Preprocessor::ExtractHeaders => extract_headers(table),
            Preprocessor::DeleteColumnByNumber(id) => delete_column_number(table, *id),
            Preprocessor::DeleteColumnByName(name) => delete_column_name(table, name.as_str()),
            Preprocessor::DeleteColumnByNameG(name) => {
                delete_column_name_glob(table, name.as_str())
            }
            Preprocessor::KeepColumnsByName(names) => keep_columns_matching_any_names(table, names),
            Preprocessor::KeepColumnsByNameG(names) => {
                keep_columns_matching_any_names_glob(table, names)
            }
            Preprocessor::SortByColumnName(name) => sort_by_column_name(table, name.as_str()),
            Preprocessor::SortByColumnNumber(id) => sort_by_column_id(table, *id),
            Preprocessor::DeleteRowByNumber(id) => delete_row_by_number(table, *id),
            Preprocessor::DeleteRowByRegex(regex) => delete_row_by_regex(table, regex),
            Preprocessor::DeleteCellByNumber { column, row } => {
                delete_cell_by_number(table, *column, *row)
            }
            Preprocessor::DeleteCellByName { column, row } => {
                delete_cell_by_column_name_and_row_number(table, column, *row)
            }
        }
    }
}

fn delete_row_by_regex(table: &mut Table, regex: &str) -> Result<(), csv::Error> {
    let regex = regex::Regex::new(regex)?;
    table
        .rows_mut()
        .filter(|row| row.iter().any(|v| regex.is_match(v.to_string().as_str())))
        .for_each(|mut row| row.iter_mut().for_each(|v| **v = Value::deleted()));
    Ok(())
}

fn delete_row_by_number(table: &mut Table, id: usize) -> Result<(), csv::Error> {
    if let Some(mut v) = table.rows_mut().nth(id) {
        v.iter_mut().for_each(|v| **v = Value::deleted())
    }
    Ok(())
}

fn delete_cell_by_number(table: &mut Table, column: usize, row: usize) -> Result<(), csv::Error> {
    let value = table
        .columns
        .get_mut(column)
        .ok_or_else(|| {
            csv::Error::InvalidAccess(format!("Cell with column number {} not found.", column))
        })?
        .rows
        .get_mut(row)
        .ok_or_else(|| {
            csv::Error::InvalidAccess(format!("Cell with row number {} not found.", row))
        })?;

    *value = Value::deleted();

    Ok(())
}

fn delete_cell_by_column_name_and_row_number(
    table: &mut Table,
    column: &str,
    row: usize,
) -> Result<(), csv::Error> {
    let value = table
        .columns
        .iter_mut()
        .find(|col| col.header.as_deref().unwrap_or_default() == column)
        .ok_or_else(|| {
            csv::Error::InvalidAccess(format!("Cell with column name '{}' not found.", column))
        })?
        .rows
        .get_mut(row)
        .ok_or_else(|| {
            csv::Error::InvalidAccess(format!("Cell with row number {} not found.", row))
        })?;

    *value = Value::deleted();

    Ok(())
}

fn get_permutation(rows_to_sort_by: &Vec<f64>) -> permutation::Permutation {
    permutation::sort_by(rows_to_sort_by, |a, b| b.partial_cmp(a).unwrap_or(Equal))
}

fn apply_permutation(table: &mut Table, mut permutation: permutation::Permutation) {
    table.columns.iter_mut().for_each(|c| {
        permutation.apply_slice_in_place(&mut c.rows);
    });
}

fn sort_by_column_id(table: &mut Table, id: usize) -> Result<(), csv::Error> {
    let sort_master_col = table.columns.get(id).ok_or_else(|| {
        csv::Error::InvalidAccess(format!(
            "Column number sorting by id {id} requested but column not found."
        ))
    })?;
    let col_floats: Result<Vec<_>, csv::Error> = sort_master_col
        .rows
        .iter()
        .map(|v| {
            v.get_quantity().map(|q| q.value).ok_or_else(|| {
                csv::Error::UnexpectedValue(
                    v.clone(),
                    "Expected quantity while trying to sort by column id".to_string(),
                )
            })
        })
        .collect();
    let permutation = get_permutation(&col_floats?);
    apply_permutation(table, permutation);
    Ok(())
}

fn sort_by_column_name(table: &mut Table, name: &str) -> Result<(), csv::Error> {
    let sort_master_col = table
        .columns
        .iter()
        .find(|c| c.header.as_deref().unwrap_or_default() == name)
        .ok_or_else(|| {
            csv::Error::InvalidAccess(format!(
                "Requested format sorting by column'{name}' but column not found."
            ))
        })?;
    let col_floats: Result<Vec<_>, csv::Error> = sort_master_col
        .rows
        .iter()
        .map(|v| {
            v.get_quantity().map(|q| q.value).ok_or_else(|| {
                csv::Error::UnexpectedValue(
                    v.clone(),
                    "Expected quantity while trying to sort by column name".to_string(),
                )
            })
        })
        .collect();
    let permutation = get_permutation(&col_floats?);
    apply_permutation(table, permutation);
    Ok(())
}

fn delete_column_name(table: &mut Table, name: &str) -> Result<(), csv::Error> {
    if let Some(c) = table
        .columns
        .iter_mut()
        .find(|col| col.header.as_deref().unwrap_or_default() == name)
    {
        c.delete_contents();
    }
    Ok(())
}

fn delete_column_name_glob(table: &mut Table, name: &str) -> Result<(), csv::Error> {
    let pattern = Pattern::new(name).map_err(|e| {
        csv::Error::InvalidAccess(format!(
            "Invalid glob pattern in DeleteColumnByNameG '{}': {}",
            name, e
        ))
    })?;

    if let Some(c) = table.columns.iter_mut().find(|col| {
        let header = col.header.as_deref().unwrap_or_default();
        header != "DELETED" && (header == name || pattern.matches(header))
    }) {
        c.delete_contents();
    }
    Ok(())
}

fn delete_column_number(table: &mut Table, id: usize) -> Result<(), csv::Error> {
    if let Some(col) = table.columns.get_mut(id) {
        col.delete_contents();
    }
    Ok(())
}

fn keep_columns_matching_any_names(table: &mut Table, names: &[String]) -> Result<(), csv::Error> {
    table.columns.iter_mut().for_each(|col| {
        let header = col.header.as_deref().unwrap_or_default();
        if names.iter().any(|name| header == name) {
            info!("Keep header: \"{}\" (exact match)", header);
        } else {
            col.delete_contents();
        }
    });
    Ok(())
}

fn keep_columns_matching_any_names_glob(
    table: &mut Table,
    names: &[String],
) -> Result<(), csv::Error> {
    let patterns: Result<Vec<Pattern>, csv::Error> = names
        .iter()
        .map(|name| {
            Pattern::new(name).map_err(|e| {
                csv::Error::InvalidAccess(format!(
                    "Invalid glob pattern in KeepColumnsByNameG '{}': {}",
                    name, e
                ))
            })
        })
        .collect();
    let patterns = patterns?;

    table.columns.iter_mut().for_each(|col| {
        let header = col.header.as_deref().unwrap_or_default();
        if !(names.iter().zip(patterns.iter()).any(|(name, pattern)| {
            if header == name {
                info!("Keep header: \"{}\" (exact match)", header);
                true
            } else if pattern.matches(header) {
                info!("Keep header: \"{}\" (matches: \"{}\")", header, name);
                true
            } else {
                false
            }
        })) {
            col.delete_contents();
        }
    });
    Ok(())
}

fn extract_headers(table: &mut Table) -> Result<(), csv::Error> {
    debug!("Extracting headers...");
    let can_extract = table
        .columns
        .iter()
        .all(|c| matches!(c.rows.first(), Some(Value::String(_))));
    if !can_extract {
        warn!("Cannot extract header for this csv!");
        return Ok(());
    }

    for col in table.columns.iter_mut() {
        let title = col.rows.drain(0..1).next().ok_or_else(|| {
            csv::Error::InvalidAccess("Tried to extract header of empty column!".to_string())
        })?;
        if let Value::String(title) = title {
            col.header = Some(title);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csv::test_utils::*;
    use crate::csv::{Column, Delimiters, Error};
    use std::fs::File;

    macro_rules! string_vec {
        ($($x:expr),*) => (vec![$($x.to_string()),*]);
    }

    fn setup_table(delimiters: Option<Delimiters>) -> Table {
        let delimiters = delimiters.unwrap_or_default();
        Table::from_reader(
            File::open("tests/csv/data/DeviationHistogram.csv").unwrap(),
            &delimiters,
        )
        .unwrap()
    }

    fn setup_table_two(delimiters: Option<Delimiters>) -> Table {
        let delimiters = delimiters.unwrap_or_default();
        Table::from_reader(
            File::open("tests/csv/data/defects_headers.csv").unwrap(),
            &delimiters,
        )
        .unwrap()
    }

    #[test]
    fn test_extract_headers_two() {
        let mut table = setup_table_two(None);
        extract_headers(&mut table).unwrap();
        assert_eq!(
            table.columns.first().unwrap().header.as_deref().unwrap(),
            "Entry"
        );
        assert_eq!(
            table.columns.last().unwrap().header.as_deref().unwrap(),
            "Radius"
        );
    }

    #[test]
    fn test_extract_headers_from_string() {
        let content = "Header1;Header2;Header3\nValue1;Value2;Value3";
        let mut table = table_from_string(content);

        assert_eq!(table.columns.len(), 3);
        assert!(table.columns[0].header.is_none());

        extract_headers(&mut table).unwrap();

        assert_eq!(table.columns[0].header.as_deref().unwrap(), "Header1");
        assert_eq!(table.columns[1].header.as_deref().unwrap(), "Header2");
        assert_eq!(table.columns[2].header.as_deref().unwrap(), "Header3");
        assert_eq!(table.columns[0].rows.len(), 1); // One row remaining after header extraction
    }

    #[test]
    fn test_extract_headers_fails_with_numbers() {
        let content = "1.5;2.5;3.5\nValue1;Value2;Value3";
        let mut table = table_from_string(content);

        // Should not extract headers when first row contains numbers
        extract_headers(&mut table).unwrap();

        assert!(table.columns[0].header.is_none());
        assert_eq!(table.columns[0].rows.len(), 2); // Both rows still present
    }

    #[test]
    fn test_extract_headers() {
        let mut table = setup_table(None);
        extract_headers(&mut table).unwrap();
        assert_eq!(
            table.columns.first().unwrap().header.as_deref().unwrap(),
            "Deviation [mm]"
        );
        assert_eq!(
            table.columns.last().unwrap().header.as_deref().unwrap(),
            "Surface [mm²]"
        );
    }

    #[test]
    fn test_delete_column_by_id() {
        let mut table = setup_table(None);
        extract_headers(&mut table).unwrap();
        delete_column_number(&mut table, 0).unwrap();
        assert_eq!(
            table.columns.first().unwrap().header.as_deref().unwrap(),
            "DELETED"
        );
        assert!(table
            .columns
            .first()
            .unwrap()
            .rows
            .iter()
            .all(|v| *v == Value::deleted()));
    }

    #[test]
    fn test_delete_column_by_name() {
        let mut table = setup_table(None);
        extract_headers(&mut table).unwrap();
        delete_column_name(&mut table, "Surface [mm²]").unwrap();
        assert_eq!(
            table.columns.last().unwrap().header.as_deref().unwrap(),
            "DELETED"
        );
        assert!(table
            .columns
            .last()
            .unwrap()
            .rows
            .iter()
            .all(|v| *v == Value::deleted()));
    }

    #[test]
    fn test_delete_column_by_name_glob() {
        let mut table = setup_table(None);
        extract_headers(&mut table).unwrap();
        delete_column_name_glob(&mut table, "Surface*").unwrap();
        assert_eq!(
            table.columns.last().unwrap().header.as_deref().unwrap(),
            "DELETED"
        );
        assert!(table
            .columns
            .last()
            .unwrap()
            .rows
            .iter()
            .all(|v| *v == Value::deleted()));
    }

    #[test]
    fn test_keep_columns_matching_any_names() {
        let mut table = setup_table(None);
        // column headers in test table: "Deviation [mm]", "Surface [mm²]"
        extract_headers(&mut table).unwrap();

        let keep_names = string_vec![
            "Deviation [mm]",
            "Any name to be kept, which is no table header -> no effect"
        ];
        keep_columns_matching_any_names(&mut table, &keep_names).unwrap();
        assert_eq!(
            table.columns.first().unwrap().header.as_deref().unwrap(),
            "Deviation [mm]",
            "First column was deleted, although it is in the list of names to be kept!"
        );
        assert_eq!(
            table.columns.last().unwrap().header.as_deref().unwrap(),
            "DELETED",
            "Second column was not deleted, although it is not in the list of names to be kept!",
        );
        assert!(table
            .columns
            .last()
            .unwrap()
            .rows
            .iter()
            .all(|v| *v == Value::deleted()));
    }

    #[test]
    fn test_keep_columns_matching_glob_patterns() {
        let mut table = setup_table(None);
        // column headers in test table: "Deviation [mm]", "Surface [mm²]"
        extract_headers(&mut table).unwrap();

        let keep_names = string_vec![
            "Surface*"  // Should match "Surface [mm²]" via glob pattern
        ];
        keep_columns_matching_any_names_glob(&mut table, &keep_names).unwrap();
        assert_eq!(
            table.columns.first().unwrap().header.as_deref().unwrap(),
            "DELETED",
            "First column (Deviation) should be deleted as it doesn't match pattern!"
        );
        assert_eq!(
            table.columns.last().unwrap().header.as_deref().unwrap(),
            "Surface [mm²]",
            "Second column (Surface) was deleted, although it matches the glob pattern!",
        );
        assert!(table
            .columns
            .first()
            .unwrap()
            .rows
            .iter()
            .all(|v| *v == Value::deleted()));
    }

    #[test]
    fn test_delete_row_by_id() {
        let mut table = setup_table(None);
        delete_row_by_number(&mut table, 0).unwrap();
        assert_eq!(
            table
                .columns
                .first()
                .unwrap()
                .rows
                .first()
                .unwrap()
                .get_string()
                .as_deref()
                .unwrap(),
            "DELETED"
        );
    }

    #[test]
    fn test_delete_row_by_regex() {
        let mut table = setup_table(None);
        delete_row_by_regex(&mut table, "mm").unwrap();
        assert_eq!(
            table
                .columns
                .first()
                .unwrap()
                .rows
                .first()
                .unwrap()
                .get_string()
                .as_deref()
                .unwrap(),
            "DELETED"
        );
    }

    #[test]
    fn test_sort_by_name() {
        let mut table = setup_table(None);
        extract_headers(&mut table).unwrap();
        sort_by_column_name(&mut table, "Surface [mm²]").unwrap();
        let mut peekable_rows = table.rows().peekable();
        while let Some(row) = peekable_rows.next() {
            if let Some(next_row) = peekable_rows.peek() {
                assert!(
                    row.get(1).unwrap().get_quantity().unwrap().value
                        >= next_row.get(1).unwrap().get_quantity().unwrap().value
                );
            }
        }
    }

    #[test]
    fn test_sort_by_id() {
        let mut table = setup_table(None);
        extract_headers(&mut table).unwrap();
        let column = 1;
        sort_by_column_id(&mut table, column).unwrap();
        let mut peekable_rows = table.rows().peekable();
        while let Some(row) = peekable_rows.next() {
            if let Some(next_row) = peekable_rows.peek() {
                assert!(
                    row.get(column).unwrap().get_quantity().unwrap().value
                        >= next_row.get(column).unwrap().get_quantity().unwrap().value
                );
            }
        }
    }

    #[test]
    fn sorting_by_mixed_column_fails() {
        let column = Column {
            header: Some("Field".to_string()),
            rows: vec![
                Value::from_str("1.0", &None),
                Value::String("String-Value".to_string()),
            ],
        };
        let mut table = Table {
            columns: vec![column],
        };
        let order_by_name = sort_by_column_name(&mut table, "Field");
        assert!(matches!(
            order_by_name.unwrap_err(),
            Error::UnexpectedValue(_, _)
        ));

        let order_by_id = sort_by_column_id(&mut table, 0);
        assert!(matches!(
            order_by_id.unwrap_err(),
            Error::UnexpectedValue(_, _)
        ));
    }

    #[test]
    fn non_existing_table_fails() {
        let mut table = setup_table(None);
        let order_by_name = sort_by_column_name(&mut table, "Non-Existing-Field");
        assert!(matches!(
            order_by_name.unwrap_err(),
            Error::InvalidAccess(_)
        ));

        let order_by_id = sort_by_column_id(&mut table, 999);
        assert!(matches!(order_by_id.unwrap_err(), Error::InvalidAccess(_)));
    }

    #[test]
    fn test_delete_cell_by_numb() {
        let mut table = setup_table(None);
        delete_cell_by_number(&mut table, 1, 2).unwrap();

        assert_eq!(
            table
                .columns
                .get(1)
                .unwrap()
                .rows
                .get(2)
                .unwrap()
                .get_string()
                .as_deref()
                .unwrap(),
            "DELETED"
        );

        assert_ne!(
            table
                .columns
                .get(1)
                .unwrap()
                .rows
                .first()
                .unwrap()
                .get_string()
                .as_deref()
                .unwrap(),
            "DELETED"
        );

        assert_eq!(
            table
                .columns
                .first()
                .unwrap()
                .rows
                .get(1)
                .unwrap()
                .get_string(),
            None
        );
    }

    #[test]
    fn test_delete_cell_by_name() {
        let mut table = setup_table(None);
        extract_headers(&mut table).unwrap();
        delete_cell_by_column_name_and_row_number(&mut table, "Surface [mm²]", 1).unwrap();

        assert_eq!(
            table
                .columns
                .get(1)
                .unwrap()
                .rows
                .get(1)
                .unwrap()
                .get_string()
                .as_deref()
                .unwrap(),
            "DELETED"
        );

        assert_eq!(
            table
                .columns
                .get(1)
                .unwrap()
                .rows
                .get(3)
                .unwrap()
                .get_string(),
            None
        );

        assert_eq!(
            table
                .columns
                .get(0)
                .unwrap()
                .rows
                .get(1)
                .unwrap()
                .get_string(),
            None
        );
    }

    // Tests for PR #58 - Column filtering with exact match and glob patterns
    //
    // BEHAVIOR DOCUMENTATION & AMBIGUITIES FOUND:
    //
    // 1. DeleteColumnByName and DeleteColumnByNameG use find() which deletes only the FIRST matching column.
    //    - If multiple columns have the same name, only the first is deleted
    //    - PR reviewer (TheAdiWijaya) suggested using filter() instead to delete ALL matches
    //    - Why not "just fix it"?
    //      i.e. changing the behavior DeleteColumnByName* so that it removes ALL columns:
    //      - Workaround (feature?) exists: if 2,3,... identical columns "foo" in the CSV are actually
    //        intended behavior of the AUT, then the tester can add 2,3,... identical
    //        `DeleteColumnByName: "foo"` steps.
    //      - debatable behavior (feature or bug) is already released
    //      - singular in "DeleteColumnByName" implies deleting only one column
    //      - any existing test out there, using "DeleteColumnByName", would in general delete more columns
    //        and become less sensitive in a very implicit way when just upgrading havocompare.
    //    - Current tests document the FIRST-MATCH-ONLY behavior
    //
    // 2. DeleteColumnByNameG "exact match priority" is ambiguous:
    //    - PR says "first try exact match, then glob pattern"
    //    - Current implementation: find() returns first column matching (exact OR glob) by position
    //    - Example: pattern "Center *" with columns ["Center x [mm]", "Center y [mm]", "Center *"]
    //      deletes "Center x [mm]" (first glob match), NOT "Center *" (exact match at position 3)
    //    - Alternative interpretation: search for exact match first, if none found, then search by glob
    //    - This needs clarification from the PR author
    //
    // 3. KeepColumnsByName and KeepColumnsByNameG work correctly:
    //    - They iterate ALL columns and keep those matching any pattern (exact or glob)
    //    - Multiple columns with same name are all kept
    //    - This behavior matches the PR description examples

    #[test]
    fn test_delete_column_by_name_deletes_first_match_only() {
        // According to PR description: DeleteColumnByName deletes the first column that exactly matches
        let content = "Center x [mm];Center y [mm];Center z [mm];Center x [mm];Center x\n1;2;3;4;5";
        let mut table = table_from_string(content);
        extract_headers(&mut table).unwrap();

        delete_column_name(&mut table, "Center x [mm]").unwrap();

        // First "Center x [mm]" should be deleted
        assert_eq!(
            table.columns[0].header.as_deref().unwrap(),
            "DELETED",
            "First 'Center x [mm]' should be deleted"
        );
        assert!(table.columns[0].rows.iter().all(|v| *v == Value::deleted()));

        // Second "Center x [mm]" should still exist
        assert_eq!(
            table.columns[3].header.as_deref().unwrap(),
            "Center x [mm]",
            "Second 'Center x [mm]' should NOT be deleted (only first match)"
        );
        assert!(!table.columns[3].rows.iter().all(|v| *v == Value::deleted()));
    }

    #[test]
    fn test_delete_column_by_name_glob_deletes_first_match() {
        // AMBIGUITY DETECTED: PR says "first try exact match, then glob pattern"
        // Current implementation uses find() which returns FIRST column matching (exact OR glob)
        // When pattern "Center *" is used, it matches "Center x [mm]" (glob) before "Center *" (exact)
        // This test documents CURRENT behavior - may need clarification if prioritization is intended
        let content = "Center x [mm];Center y [mm];Center z [mm];Center *\n1;2;3;4";
        let mut table = table_from_string(content);
        extract_headers(&mut table).unwrap();

        // Pattern "Center *" as glob matches "Center x [mm]" first
        delete_column_name_glob(&mut table, "Center *").unwrap();

        // Current behavior: deletes first match by position (glob match on column 0)
        assert_eq!(
            table.columns[0].header.as_deref().unwrap(),
            "DELETED",
            "First column matching glob 'Center *' should be deleted"
        );
        assert!(table.columns[0].rows.iter().all(|v| *v == Value::deleted()));

        // The exact match "Center *" at column 3 is NOT deleted (only first match)
        assert_eq!(
            table.columns[3].header.as_deref().unwrap(),
            "Center *",
            "Exact match is not deleted because first glob match was found first"
        );
        assert!(!table.columns[3].rows.iter().all(|v| *v == Value::deleted()));
    }

    #[test]
    fn test_delete_column_by_name_glob_twice_does_not_rematch_deleted() {
        // Scenario: columns ["Data A", "Data B", "Other"], call DeleteColumnByNameG("D*") twice.
        // Step 1 should delete "Data A", step 2 should delete "Data B".
        // Bug: "D*" also matches "DELETED", so step 2 re-matches the already-deleted column.
        let content = "Data A;Data B;Other\n1;2;3";
        let mut table = table_from_string(content);
        extract_headers(&mut table).unwrap();

        // First call: should delete "Data A" (first match left-to-right)
        delete_column_name_glob(&mut table, "D*").unwrap();
        assert_eq!(
            table.columns[0].header.as_deref().unwrap(),
            "DELETED",
            "First call should delete 'Data A'"
        );
        assert_eq!(
            table.columns[1].header.as_deref().unwrap(),
            "Data B",
            "'Data B' should survive the first call"
        );

        // Second call: should delete "Data B" (next match), NOT re-match "DELETED"
        delete_column_name_glob(&mut table, "D*").unwrap();
        assert_eq!(
            table.columns[1].header.as_deref().unwrap(),
            "DELETED",
            "Second call should delete 'Data B', not re-match the already-deleted column"
        );

        // "Other" should be untouched
        assert_eq!(
            table.columns[2].header.as_deref().unwrap(),
            "Other",
            "'Other' should not be affected"
        );
        assert!(!table.columns[2].rows.iter().all(|v| *v == Value::deleted()));
    }

    #[test]
    fn test_delete_column_by_name_glob_pattern_match() {
        let content = "Center x [mm];Center y [mm];Center z [mm];Other column\n1;2;3;4";
        let mut table = table_from_string(content);
        extract_headers(&mut table).unwrap();

        delete_column_name_glob(&mut table, "Center *").unwrap();

        // First match by glob should be deleted
        assert_eq!(
            table.columns[0].header.as_deref().unwrap(),
            "DELETED",
            "First column matching 'Center *' glob should be deleted"
        );
        assert!(table.columns[0].rows.iter().all(|v| *v == Value::deleted()));

        // Other columns that match should NOT be deleted (only first match with find())
        assert_eq!(
            table.columns[1].header.as_deref().unwrap(),
            "Center y [mm]",
            "Second matching column should NOT be deleted"
        );
    }

    #[test]
    fn test_delete_column_by_name_glob_square_brackets() {
        // According to PR: Square brackets in glob are wildcards (match 'u' OR 'm')
        // So exact match should be tried first to handle "[mm]" correctly
        let content = "Value [mm];Value [um];Value m;Value u\n1;2;3;4";
        let mut table = table_from_string(content);
        extract_headers(&mut table).unwrap();

        // Without exact match fallback, "[um]" would match "Value u" or "Value m"
        delete_column_name_glob(&mut table, "Value [mm]").unwrap();

        // Should delete exact match "Value [mm]"
        assert_eq!(
            table.columns[0].header.as_deref().unwrap(),
            "DELETED",
            "Exact match 'Value [mm]' should be deleted"
        );
    }

    #[test]
    fn test_keep_columns_by_name_example_from_pr() {
        // Example from PR #58
        let content = "Center x [mm];Center y [mm];Center z [mm];any other string;Center x [mm];Center x\n1;2;3;4;5;6";
        let mut table = table_from_string(content);
        extract_headers(&mut table).unwrap();

        let keep_names = string_vec!["Center x [mm]", "Center y [mm]"];
        keep_columns_matching_any_names(&mut table, &keep_names).unwrap();

        // First two columns should be kept (exact matches)
        assert_eq!(
            table.columns[0].header.as_deref().unwrap(),
            "Center x [mm]",
            "First column should be kept"
        );
        assert!(!table.columns[0].rows.iter().all(|v| *v == Value::deleted()));

        assert_eq!(
            table.columns[1].header.as_deref().unwrap(),
            "Center y [mm]",
            "Second column should be kept"
        );
        assert!(!table.columns[1].rows.iter().all(|v| *v == Value::deleted()));

        // Third column should be deleted
        assert_eq!(
            table.columns[2].header.as_deref().unwrap(),
            "DELETED",
            "Center z [mm] should be deleted"
        );
        assert!(table.columns[2].rows.iter().all(|v| *v == Value::deleted()));

        // Fourth column should be deleted
        assert_eq!(
            table.columns[3].header.as_deref().unwrap(),
            "DELETED",
            "any other string should be deleted"
        );
        assert!(table.columns[3].rows.iter().all(|v| *v == Value::deleted()));

        // Fifth column (duplicate "Center x [mm]") should be kept
        assert_eq!(
            table.columns[4].header.as_deref().unwrap(),
            "Center x [mm]",
            "Second 'Center x [mm]' should also be kept"
        );
        assert!(!table.columns[4].rows.iter().all(|v| *v == Value::deleted()));

        // Sixth column should be deleted
        assert_eq!(
            table.columns[5].header.as_deref().unwrap(),
            "DELETED",
            "Center x (without [mm]) should be deleted"
        );
        assert!(table.columns[5].rows.iter().all(|v| *v == Value::deleted()));
    }

    #[test]
    fn test_keep_columns_by_name_glob_with_wildcard() {
        // Example from PR: "Center *" should match "Center x [mm]", "Center y [mm]", "Center _"
        let content = "Center x [mm];Center y [mm];Center _;Other column\n1;2;3;4";
        let mut table = table_from_string(content);
        extract_headers(&mut table).unwrap();

        let keep_names = string_vec!["Center *"];
        keep_columns_matching_any_names_glob(&mut table, &keep_names).unwrap();

        // All "Center *" columns should be kept
        assert_eq!(
            table.columns[0].header.as_deref().unwrap(),
            "Center x [mm]",
            "Center x [mm] should match glob"
        );
        assert!(!table.columns[0].rows.iter().all(|v| *v == Value::deleted()));

        assert_eq!(
            table.columns[1].header.as_deref().unwrap(),
            "Center y [mm]",
            "Center y [mm] should match glob"
        );
        assert!(!table.columns[1].rows.iter().all(|v| *v == Value::deleted()));

        assert_eq!(
            table.columns[2].header.as_deref().unwrap(),
            "Center _",
            "Center _ should match glob"
        );
        assert!(!table.columns[2].rows.iter().all(|v| *v == Value::deleted()));

        // "Other column" should be deleted
        assert_eq!(
            table.columns[3].header.as_deref().unwrap(),
            "DELETED",
            "Other column should be deleted"
        );
        assert!(table.columns[3].rows.iter().all(|v| *v == Value::deleted()));
    }

    #[test]
    fn test_keep_columns_by_name_glob_question_mark_wildcard() {
        // Example from PR: "Center ? [mm]" does NOT match "Center x [mm]"
        // because ? matches single char and space is a char
        let content = "Center x [mm];Center  [mm];Centerx[mm]\n1;2;3";
        let mut table = table_from_string(content);
        extract_headers(&mut table).unwrap();

        let keep_names = string_vec!["Center ? [mm]"];
        keep_columns_matching_any_names_glob(&mut table, &keep_names).unwrap();

        // "Center x [mm]" has space+x before [mm], so doesn't match single char '?'
        assert_eq!(
            table.columns[0].header.as_deref().unwrap(),
            "DELETED",
            "Center x [mm] should NOT match 'Center ? [mm]' pattern"
        );
        assert!(table.columns[0].rows.iter().all(|v| *v == Value::deleted()));

        // "Center  [mm]" has space+space before [mm], so doesn't match single char '?'
        assert_eq!(
            table.columns[1].header.as_deref().unwrap(),
            "DELETED",
            "Center  [mm] should NOT match 'Center ? [mm]' pattern"
        );

        // "Centerx[mm]" has no space, so doesn't match
        assert_eq!(
            table.columns[2].header.as_deref().unwrap(),
            "DELETED",
            "Centerx[mm] should NOT match 'Center ? [mm]' pattern"
        );
    }

    #[test]
    fn test_keep_columns_by_name_glob_exact_match_takes_priority() {
        // Verify that exact match is tried before glob pattern
        let content = "Center *;Center x;Center y\n1;2;3";
        let mut table = table_from_string(content);
        extract_headers(&mut table).unwrap();

        let keep_names = string_vec!["Center *"];
        keep_columns_matching_any_names_glob(&mut table, &keep_names).unwrap();

        // "Center *" should be kept (exact match)
        assert_eq!(
            table.columns[0].header.as_deref().unwrap(),
            "Center *",
            "Exact match 'Center *' should be kept"
        );
        assert!(!table.columns[0].rows.iter().all(|v| *v == Value::deleted()));

        // "Center x" should also be kept (glob match)
        assert_eq!(
            table.columns[1].header.as_deref().unwrap(),
            "Center x",
            "Center x should match glob pattern"
        );
        assert!(!table.columns[1].rows.iter().all(|v| *v == Value::deleted()));

        // "Center y" should also be kept (glob match)
        assert_eq!(
            table.columns[2].header.as_deref().unwrap(),
            "Center y",
            "Center y should match glob pattern"
        );
        assert!(!table.columns[2].rows.iter().all(|v| *v == Value::deleted()));
    }

    #[test]
    fn test_keep_columns_by_name_glob_square_brackets_as_wildcard() {
        // According to PR: [um] matches 'u' OR 'm'
        // But exact match should be tried first
        let content = "Value [mm];Value u;Value m;Value [um]\n1;2;3;4";
        let mut table = table_from_string(content);
        extract_headers(&mut table).unwrap();

        let keep_names = string_vec!["Value [mm]"];
        keep_columns_matching_any_names_glob(&mut table, &keep_names).unwrap();

        // "Value [mm]" should be kept (exact match takes priority)
        assert_eq!(
            table.columns[0].header.as_deref().unwrap(),
            "Value [mm]",
            "Exact match 'Value [mm]' should be kept"
        );
        assert!(!table.columns[0].rows.iter().all(|v| *v == Value::deleted()));

        // "Value u" should also be kept (glob [mm] matches 'm' at that position)
        // Wait - this would only work if the glob was "Value [um]" not "Value [mm]"
        // Let's check if "Value u" matches glob pattern "Value [mm]"
        // [mm] means 'm' OR 'm', so it matches single 'm'
        // "Value u" doesn't have 'm' at position after "Value ", so it shouldn't match
        assert_eq!(
            table.columns[1].header.as_deref().unwrap(),
            "DELETED",
            "Value u should NOT match 'Value [mm]' glob"
        );

        // "Value m" should be kept if glob [mm] at that position matches 'm'
        assert_eq!(
            table.columns[2].header.as_deref().unwrap(),
            "Value m",
            "Value m should match glob 'Value [mm]' (first 'm' in brackets)"
        );
        assert!(!table.columns[2].rows.iter().all(|v| *v == Value::deleted()));
    }

    #[test]
    fn test_keep_columns_multiple_patterns() {
        // Test with multiple patterns in the list
        let content = "Column A;Column B;Data X;Data Y;Other\n1;2;3;4;5";
        let mut table = table_from_string(content);
        extract_headers(&mut table).unwrap();

        let keep_names = string_vec!["Column *", "Data X"];
        keep_columns_matching_any_names_glob(&mut table, &keep_names).unwrap();

        // "Column A" and "Column B" should match first pattern
        assert_eq!(table.columns[0].header.as_deref().unwrap(), "Column A");
        assert!(!table.columns[0].rows.iter().all(|v| *v == Value::deleted()));

        assert_eq!(table.columns[1].header.as_deref().unwrap(), "Column B");
        assert!(!table.columns[1].rows.iter().all(|v| *v == Value::deleted()));

        // "Data X" should match second pattern (exact)
        assert_eq!(table.columns[2].header.as_deref().unwrap(), "Data X");
        assert!(!table.columns[2].rows.iter().all(|v| *v == Value::deleted()));

        // "Data Y" should be deleted (doesn't match any pattern)
        assert_eq!(table.columns[3].header.as_deref().unwrap(), "DELETED");
        assert!(table.columns[3].rows.iter().all(|v| *v == Value::deleted()));

        // "Other" should be deleted
        assert_eq!(table.columns[4].header.as_deref().unwrap(), "DELETED");
        assert!(table.columns[4].rows.iter().all(|v| *v == Value::deleted()));
    }

    #[test]
    fn test_keep_columns_by_name_empty_list() {
        // Empty list should delete all columns
        let content = "Column A;Column B;Column C\n1;2;3";
        let mut table = table_from_string(content);
        extract_headers(&mut table).unwrap();

        let keep_names: Vec<String> = vec![];
        keep_columns_matching_any_names(&mut table, &keep_names).unwrap();

        // All columns should be deleted
        assert!(table.columns.iter().all(|col| {
            col.header.as_deref().unwrap() == "DELETED"
                && col.rows.iter().all(|v| *v == Value::deleted())
        }));
    }

    #[test]
    fn test_delete_column_by_name_nonexistent() {
        // Deleting non-existent column should succeed (no-op)
        let content = "Column A;Column B\n1;2";
        let mut table = table_from_string(content);
        extract_headers(&mut table).unwrap();

        delete_column_name(&mut table, "Nonexistent").unwrap();

        // Both columns should still exist
        assert_eq!(table.columns[0].header.as_deref().unwrap(), "Column A");
        assert!(!table.columns[0].rows.iter().all(|v| *v == Value::deleted()));
        assert_eq!(table.columns[1].header.as_deref().unwrap(), "Column B");
        assert!(!table.columns[1].rows.iter().all(|v| *v == Value::deleted()));
    }

    #[test]
    fn test_delete_column_by_name_header_occurs_twice_only_first_column_deleted() {
        // Deleting non-existent column should succeed (no-op)
        let content = "Column A;Column B;Column A\n1;2;3";
        let mut table = table_from_string(content);
        extract_headers(&mut table).unwrap();

        delete_column_name(&mut table, "Column A").unwrap();

        // Only first column should be deleted
        assert_eq!(table.columns[0].header.as_deref().unwrap(), "DELETED");
        assert!(table.columns[0].rows.iter().all(|v| *v == Value::deleted()));
        assert_eq!(table.columns[1].header.as_deref().unwrap(), "Column B");
        assert!(!table.columns[1].rows.iter().all(|v| *v == Value::deleted()));
        assert_eq!(table.columns[2].header.as_deref().unwrap(), "Column A");
        assert!(!table.columns[2].rows.iter().all(|v| *v == Value::deleted()));

        delete_column_name(&mut table, "Column A").unwrap();
        // Now the 2nd "Column A" should be deleted as well.
        assert_eq!(table.columns[0].header.as_deref().unwrap(), "DELETED");
        assert!(table.columns[0].rows.iter().all(|v| *v == Value::deleted()));
        assert_eq!(table.columns[1].header.as_deref().unwrap(), "Column B");
        assert!(!table.columns[1].rows.iter().all(|v| *v == Value::deleted()));
        assert_eq!(table.columns[2].header.as_deref().unwrap(), "DELETED");
        assert!(table.columns[2].rows.iter().all(|v| *v == Value::deleted()));
    }

    #[test]
    fn test_delete_column_by_name_glob_invalid_pattern() {
        // Invalid glob pattern should return error
        let content = "Column A;Column B\n1;2";
        let mut table = table_from_string(content);
        extract_headers(&mut table).unwrap();

        let result = delete_column_name_glob(&mut table, "[invalid");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::InvalidAccess(_)));
    }

    #[test]
    fn test_keep_columns_by_name_glob_invalid_pattern() {
        // Invalid glob pattern should return error
        let content = "Column A;Column B\n1;2";
        let mut table = table_from_string(content);
        extract_headers(&mut table).unwrap();

        let keep_names = string_vec!["[invalid"];
        let result = keep_columns_matching_any_names_glob(&mut table, &keep_names);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::InvalidAccess(_)));
    }
}
