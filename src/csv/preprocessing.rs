use crate::csv;
use crate::csv::value::Value;
use crate::csv::Table;
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering::Equal;
use tracing::{debug, warn};

#[derive(JsonSchema, Deserialize, Serialize, Debug)]
pub enum Preprocessor {
    ExtractHeaders,
    DeleteColumnByNumber(usize),
    DeleteColumnByName(String),
    SortByColumnName(String),
    SortByColumnNumber(usize),
    DeleteRowByNumber(usize),
    DeleteRowByRegex(String),
}

impl Preprocessor {
    pub fn process(&self, table: &mut Table) -> Result<(), csv::Error> {
        match self {
            Preprocessor::ExtractHeaders => extract_headers(table),
            Preprocessor::DeleteColumnByNumber(id) => delete_column_number(table, *id),
            Preprocessor::DeleteColumnByName(name) => delete_column_name(table, name.as_str()),
            Preprocessor::SortByColumnName(name) => sort_by_column_name(table, name.as_str()),
            Preprocessor::SortByColumnNumber(id) => sort_by_column_id(table, *id),
            Preprocessor::DeleteRowByNumber(id) => delete_row_by_number(table, *id),
            Preprocessor::DeleteRowByRegex(regex) => delete_row_by_regex(table, regex),
        }
    }
}

fn delete_row_by_regex(table: &mut Table, regex: &str) -> Result<(), csv::Error> {
    let regex = regex::Regex::new(regex)?;
    table
        .rows_mut()
        .filter(|row| row.iter().any(|v| regex.is_match(v.to_string().as_str())))
        .for_each(|mut row| {
            row.iter_mut()
                .for_each(|v| **v = Value::from_str("DELETED", &None))
        });
    Ok(())
}

fn delete_row_by_number(table: &mut Table, id: usize) -> Result<(), csv::Error> {
    if let Some(mut v) = table.rows_mut().nth(id) {
        v.iter_mut()
            .for_each(|v| **v = Value::from_str("DELETED", &None))
    }
    Ok(())
}

fn get_permutation(rows_to_sort_by: &Vec<f32>) -> permutation::Permutation {
    permutation::sort_by(rows_to_sort_by, |a, b| b.partial_cmp(a).unwrap_or(Equal))
}

fn apply_permutation(table: &mut Table, mut permutation: permutation::Permutation) {
    table.columns.iter_mut().for_each(|c| {
        permutation.apply_slice_in_place(&mut c.rows);
    });
}

fn sort_by_column_id(table: &mut Table, id: usize) -> Result<(), csv::Error> {
    let sort_master_col = table.columns.get(id).ok_or_else(|| {
        csv::Error::AccessError(format!(
            "Column number sorting by id {} requested but column not found.",
            id
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
            csv::Error::AccessError(format!(
                "Requested format sorting by column'{}' but column not found.",
                name
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
    table
        .columns
        .retain(|col| col.header.as_deref().unwrap_or_default() != name);
    Ok(())
}

fn delete_column_number(table: &mut Table, id: usize) -> Result<(), csv::Error> {
    table.columns.remove(id);
    Ok(())
}

fn extract_headers(table: &mut Table) -> Result<(), csv::Error> {
    debug!("Extracting headers...");
    for col in table.columns.iter_mut() {
        let title = col.rows.drain(0..1).next().ok_or_else(|| {
            csv::Error::AccessError("Tried to extract header of empty column!".to_string())
        })?;
        if let Value::String(title) = title {
            col.header = Some(title);
        } else {
            warn!("First entry in column was not a string!");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csv::Delimiters;
    use std::fs::File;

    fn setup_table(delimiters: Option<Delimiters>) -> Table {
        let delimiters = delimiters.unwrap_or_default();
        Table::from_reader(
            File::open("tests/csv/data/DeviationHistogram.csv").unwrap(),
            &delimiters,
        )
    }

    #[test]
    fn test_extract_headers() {
        let mut table = setup_table(None);
        extract_headers(&mut table);
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
        extract_headers(&mut table);
        delete_column_number(&mut table, 0);
        assert_eq!(
            table.columns.first().unwrap().header.as_deref().unwrap(),
            "Surface [mm²]"
        );
    }

    #[test]
    fn test_delete_column_by_name() {
        let mut table = setup_table(None);
        extract_headers(&mut table);
        delete_column_name(&mut table, "Surface [mm²]");
        assert_eq!(
            table.columns.first().unwrap().header.as_deref().unwrap(),
            "Deviation [mm]"
        );
    }

    #[test]
    fn test_delete_row_by_id() {
        let mut table = setup_table(None);
        delete_row_by_number(&mut table, 0);
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
        delete_row_by_regex(&mut table, "mm");
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
        extract_headers(&mut table);
        sort_by_column_name(&mut table, "Surface [mm²]");
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
        extract_headers(&mut table);
        let column = 1;
        sort_by_column_id(&mut table, column);
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
}
