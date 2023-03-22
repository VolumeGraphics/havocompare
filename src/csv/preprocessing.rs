use crate::csv;
use crate::csv::value::Value;
use crate::csv::Table;
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering::Equal;
use tracing::{debug, warn};

#[derive(JsonSchema, Deserialize, Serialize, Debug)]
/// Preprocessor options
pub enum Preprocessor {
    /// Try to extract the headers from the first row - fallible if first row contains a number
    ExtractHeaders,
    /// Replace all fields in column by number by a deleted marker
    DeleteColumnByNumber(usize),
    /// Replace all fields in column by name by a deleted marker
    DeleteColumnByName(String),
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

fn delete_column_number(table: &mut Table, id: usize) -> Result<(), csv::Error> {
    if let Some(col) = table.columns.get_mut(id) {
        col.delete_contents();
    }
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
    use crate::csv::{Column, Delimiters, Error};
    use std::fs::File;

    fn setup_table(delimiters: Option<Delimiters>) -> Table {
        let delimiters = delimiters.unwrap_or_default();
        Table::from_reader(
            File::open("tests/csv/data/DeviationHistogram.csv").unwrap(),
            &delimiters,
        )
        .unwrap()
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
            .all(|v| *v == csv::Value::deleted()));
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
            .all(|v| *v == csv::Value::deleted()));
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
}
