use crate::csv::value::Value;
use crate::csv::Table;
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

#[derive(JsonSchema, Deserialize, Serialize, Debug)]
pub enum Preprocessor {
    ExtractHeaders,
    DeleteColumnByNumber(usize),
    DeleteColumnByName(String),
}

impl Preprocessor {
    pub fn process(&self, table: &mut Table) {
        match self {
            Preprocessor::ExtractHeaders => extract_headers(table),
            Preprocessor::DeleteColumnByNumber(id) => delete_column_number(table, *id),
            Preprocessor::DeleteColumnByName(name) => delete_column_name(table, name.as_str()),
        }
    }
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
