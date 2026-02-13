//! Shared test utilities for CSV module tests
#![cfg(test)]

use crate::csv::{Delimiters, Table};
use std::io::Cursor;

/// Helper function to create a Table from CSV string content for testing
///
/// Uses semicolon as field delimiter and dot as decimal separator.
pub fn table_from_string(content: &str) -> Table {
    let cursor = Cursor::new(content.as_bytes());

    Table::from_reader(
        cursor,
        &Delimiters {
            field_delimiter: Some(';'),
            decimal_separator: Some('.'),
        },
    )
    .unwrap()
}
