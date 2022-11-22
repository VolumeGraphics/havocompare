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
}

impl Preprocessor {
    pub fn process(&self, table: &mut Table) {
        match self {
            Preprocessor::ExtractHeaders => extract_headers(table),
            Preprocessor::DeleteColumnByNumber(id) => delete_column_number(table, *id),
            Preprocessor::DeleteColumnByName(name) => delete_column_name(table, name.as_str()),
            Preprocessor::SortByColumnName(name) => sort_by_column_name(table, name.as_str()),
        }
    }
}

fn sort_by_column_name(table: &mut Table, name: &str) {
    let sort_master_col = table
        .columns
        .iter()
        .find(|c| c.header.as_deref().unwrap_or_default() == name)
        .unwrap();
    let mut permutation = permutation::sort_by(&sort_master_col.rows, |a, b| {
        b.get_quantity()
            .unwrap()
            .value
            .partial_cmp(&a.get_quantity().unwrap().value)
            .unwrap_or(Equal)
    });
    table.columns.iter_mut().for_each(|c| {
        permutation.apply_slice_in_place(&mut c.rows);
    });
}

fn delete_column_name(table: &mut Table, name: &str) {
    table
        .columns
        .retain(|col| col.header.as_deref().unwrap_or_default() != name);
}

fn delete_column_number(table: &mut Table, id: usize) {
    table.columns.remove(id);
}

fn extract_headers(table: &mut Table) {
    debug!("Extracting headers...");
    table.columns.iter_mut().for_each(|col| {
        let title = col.rows.drain(0..1).next().unwrap();
        if let Value::String(title) = title {
            col.header = Some(title);
        } else {
            warn!("First entry in column was not a string!");
        }
    });
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
}
