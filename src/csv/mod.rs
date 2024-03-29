use crate::report;
mod preprocessing;
mod tokenizer;
mod value;

pub use preprocessing::Preprocessor;
use value::Quantity;
use value::Value;

use rayon::prelude::*;
use regex::Regex;
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display, Formatter};
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;
use std::slice::{Iter, IterMut};
use thiserror::Error;
use tracing::error;
use vg_errortools::{fat_io_wrap_std, FatIOError};

#[derive(Error, Debug)]
/// Possible errors during csv parsing
pub enum Error {
    #[error("Unexpected Value found {0} - {1}")]
    /// Value type was different than expected
    UnexpectedValue(Value, String),
    #[error("Tried accessing empty field")]
    /// Tried to access a non-existing field
    InvalidAccess(String),
    #[error("Failed to compile regex {0}")]
    /// Regex compilation failed
    RegexCompilationFailed(#[from] regex::Error),
    #[error("File access failed {0}")]
    /// File access failed
    FileAccessFailed(#[from] FatIOError),
    #[error("IoError occurred {0}")]
    /// Problem involving files or readers
    IoProblem(#[from] std::io::Error),

    #[error("Format guessing failed")]
    /// Failure to guess field delimiters - decimal separator guessing is optional
    FormatGuessingFailure,

    #[error("A string literal was started but did never end")]
    /// A string literal was started but did never end
    UnterminatedLiteral,

    #[error("CSV format invalid: first row has a different column number then row {0}")]
    /// The embedded row number had a different column count than the first
    UnstableColumnCount(usize),

    #[error("The files compared have different row count. Nominal: {0}, and Actual: {1}")]
    /// Files being compared have different row numbers
    UnequalRowCount(usize, usize),
}

/// A position inside a table
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Position {
    /// row number, starting with zero
    pub row: usize,
    /// column number, starting with zero
    pub col: usize,
}

#[derive(Debug, Serialize, Clone)]
/// Difference of a table entry
pub enum DiffType {
    /// Both entries were strings, but had different contents
    UnequalStrings {
        /// nominal string
        nominal: String,
        /// actual string
        actual: String,
        /// position
        position: Position,
    },
    /// Both entries were [`Quantity`]s but exceeded tolerances
    OutOfTolerance {
        /// nominal
        nominal: Quantity,
        /// actual
        actual: Quantity,
        /// compare mode that was exceeded
        mode: Mode,
        /// position in table
        position: Position,
    },
    /// both fields had different value types
    DifferentValueTypes {
        /// nominal
        nominal: Value,
        /// actual
        actual: Value,
        /// position
        position: Position,
    },
    /// Both fields were headers but with different contents
    UnequalHeader {
        /// nominal
        nominal: String,
        /// actual
        actual: String,
    },
}

impl Display for DiffType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DiffType::DifferentValueTypes {
                nominal,
                actual,
                position,
            } => {
                write!(
                    f,
                    "Line: {}, Col: {} -- Different value types -- Expected {}, Found {}",
                    position.row, position.col, nominal, actual
                )
                .unwrap_or_default();
            }
            DiffType::OutOfTolerance {
                actual,
                nominal,
                mode,
                position,
            } => {
                write!(
                    f,
                    "Line: {}, Col: {} -- Out of tolerance -- Expected {}, Found {}, Mode {}",
                    position.row, position.col, nominal, actual, mode
                )
                .unwrap_or_default();
            }
            DiffType::UnequalStrings {
                nominal,
                actual,
                position,
            } => {
                write!(
                    f,
                    "Line: {}, Col: {} -- Different strings -- Expected {}, Found {}",
                    position.row, position.col, nominal, actual
                )
                .unwrap_or_default();
            }
            DiffType::UnequalHeader { nominal, actual } => {
                write!(
                    f,
                    "Different header strings -- Expected {}, Found {}",
                    nominal, actual
                )
                .unwrap_or_default();
            }
        };
        Ok(())
    }
}

#[derive(Copy, Clone, JsonSchema, Debug, Deserialize, Serialize, PartialEq)]
/// comparison mode for csv cells
pub enum Mode {
    /// `(a-b).abs() < threshold`
    Absolute(f64),
    /// `((a-b)/a).abs() < threshold`
    Relative(f64),
    /// always matches
    Ignore,
}

impl Display for Mode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self {
            Mode::Absolute(tolerance) => {
                write!(f, "Absolute (tol: {tolerance})").unwrap_or_default();
            }
            Mode::Relative(tolerance) => {
                write!(f, "Relative (tol: {tolerance})").unwrap_or_default();
            }
            Mode::Ignore => {
                write!(f, "Ignored").unwrap_or_default();
            }
        };
        Ok(())
    }
}

impl Mode {
    pub(crate) fn in_tolerance(&self, nominal: &Quantity, actual: &Quantity) -> bool {
        if nominal.value.is_nan() && actual.value.is_nan() {
            return true;
        }
        match self {
            Mode::Absolute(tolerance) => {
                let plain_diff = (nominal.value - actual.value).abs();
                let numerically = if plain_diff == 0.0 {
                    true
                } else if *tolerance == 0.0 {
                    false
                } else {
                    let diff = nominal.minimal_diff(actual);
                    diff <= *tolerance
                };

                let identical_units = nominal.unit == actual.unit;
                numerically && identical_units
            }
            Mode::Ignore => true,
            Mode::Relative(tolerance) => {
                let plain_diff = (nominal.value - actual.value).abs();
                let numerically = if plain_diff == 0.0 {
                    true
                } else if *tolerance == 0.0 {
                    false
                } else {
                    let diff = nominal.minimal_diff(actual);
                    let diff = (diff / nominal.value).abs();
                    diff <= *tolerance
                };
                let identical_units = nominal.unit == actual.unit;
                numerically && identical_units
            }
        }
    }
}

#[derive(JsonSchema, Deserialize, Serialize, Debug, Default, Clone)]
/// Settings for the CSV comparison module
pub struct CSVCompareConfig {
    #[serde(flatten)]
    /// delimiters for the file parsing
    pub delimiters: Delimiters,
    /// How numerical values shall be compared, strings are always checked for identity
    pub comparison_modes: Vec<Mode>,
    /// Any field matching the given regex is excluded from comparison
    pub exclude_field_regex: Option<String>,
    /// Preprocessing done to the csv files before beginning the comparison
    pub preprocessing: Option<Vec<Preprocessor>>,
}

#[derive(JsonSchema, Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Default)]
/// Delimiter configuration for file parsing
pub struct Delimiters {
    /// The delimiters of the csv fields (typically comma, semicolon or pipe)
    pub field_delimiter: Option<char>,
    /// The decimal separator for floating point numbers (typically dot or comma)
    pub decimal_separator: Option<char>,
}

impl Delimiters {
    pub(crate) fn is_empty(&self) -> bool {
        self.decimal_separator.is_none() && self.field_delimiter.is_none()
    }

    #[cfg(test)]
    pub fn autodetect() -> Delimiters {
        Delimiters {
            field_delimiter: None,
            decimal_separator: None,
        }
    }
}

#[derive(Default, Clone)]
pub(crate) struct Column {
    pub header: Option<String>,
    pub rows: Vec<Value>,
}

impl Column {
    pub fn delete_contents(&mut self) {
        self.header = Some("DELETED".to_string());
        let row_count = self.rows.len();
        self.rows = vec![Value::deleted(); row_count];
    }
}

pub(crate) struct Table {
    pub columns: Vec<Column>,
}

impl Table {
    pub(crate) fn from_reader<R: Read + Seek>(
        input: R,
        config: &Delimiters,
    ) -> Result<Table, Error> {
        let mut cols = Vec::new();
        let input = BufReader::new(input);
        let mut parser = if config.is_empty() {
            tokenizer::Parser::new_guess_format(input)?
        } else {
            tokenizer::Parser::new(input, config.clone()).ok_or(Error::FormatGuessingFailure)?
        };

        for (line_num, fields) in parser.parse_to_rows()?.enumerate() {
            if cols.is_empty() {
                cols.resize_with(fields.len(), Column::default);
            }
            if fields.len() != cols.len() {
                let message = format!("Error: Columns inconsistent! First row had {}, this row has {} (row:{line_num})", cols.len(), fields.len());
                error!("{}", message.as_str());
                return Err(Error::UnstableColumnCount(line_num));
            } else {
                fields
                    .into_iter()
                    .zip(cols.iter_mut())
                    .for_each(|(f, col)| col.rows.push(f));
            }
        }

        Ok(Table { columns: cols })
    }

    pub(crate) fn rows(&self) -> RowIterator {
        RowIterator {
            position: self.columns.iter().map(|c| c.rows.iter()).collect(),
        }
    }

    pub(crate) fn rows_mut(&mut self) -> RowIteratorMut {
        RowIteratorMut {
            position: self.columns.iter_mut().map(|c| c.rows.iter_mut()).collect(),
        }
    }
}

macro_rules! mk_next {
    ($pos: expr) => {{
        let row: Vec<_> = $pos.iter_mut().filter_map(|i| i.next()).collect();
        if row.is_empty() {
            None
        } else {
            Some(row)
        }
    }};
}

macro_rules! impl_ex_size_it {
    ($($t:ty),+) => {
        $(impl<'a> ExactSizeIterator for $t {
            fn len(&self) -> usize {
                self.position.first().unwrap().len()
            }
        })+
    };
}

impl_ex_size_it!(RowIteratorMut<'_>, RowIterator<'_>);

pub(crate) struct RowIteratorMut<'a> {
    position: Vec<IterMut<'a, Value>>,
}

impl<'a> Iterator for RowIteratorMut<'a> {
    type Item = Vec<&'a mut Value>;
    fn next(&mut self) -> Option<Self::Item> {
        mk_next!(self.position)
    }
}

pub(crate) struct RowIterator<'a> {
    position: Vec<Iter<'a, Value>>,
}

impl<'a> Iterator for RowIterator<'a> {
    type Item = Vec<&'a Value>;
    fn next(&mut self) -> Option<Self::Item> {
        mk_next!(self.position)
    }
}

pub(crate) fn compare_tables(
    nominal: &Table,
    actual: &Table,
    config: &CSVCompareConfig,
) -> Result<Vec<DiffType>, Error> {
    if nominal.rows().len() != actual.rows().len() {
        return Err(Error::UnequalRowCount(
            nominal.rows().len(),
            actual.rows().len(),
        ));
    }

    let mut diffs = Vec::new();
    for (col, (col_nom, col_act)) in nominal
        .columns
        .iter()
        .zip(actual.columns.iter())
        .enumerate()
    {
        if let (Some(nom_header), Some(act_header)) = (&col_nom.header, &col_act.header) {
            if nom_header != act_header {
                diffs.extend(vec![DiffType::UnequalHeader {
                    nominal: nom_header.to_owned(),
                    actual: act_header.to_owned(),
                }]);
            }
        }

        for (row, (val_nom, val_act)) in col_nom.rows.iter().zip(col_act.rows.iter()).enumerate() {
            let position = Position { row, col };
            let diffs_field = compare_values(val_nom, val_act, config, position)?;
            diffs.extend(diffs_field);
        }
    }
    Ok(diffs)
}

fn both_quantity<'a>(
    actual: &'a Value,
    nominal: &'a Value,
) -> Option<(&'a Quantity, &'a Quantity)> {
    if let Some(actual) = actual.get_quantity() {
        if let Some(nominal) = nominal.get_quantity() {
            return Some((actual, nominal));
        }
    }
    None
}

fn both_string(actual: &Value, nominal: &Value) -> Option<(String, String)> {
    if let Some(actual) = actual.get_string() {
        if let Some(nominal) = nominal.get_string() {
            return Some((actual, nominal));
        }
    }
    None
}

fn compare_values(
    nominal: &Value,
    actual: &Value,
    config: &CSVCompareConfig,
    position: Position,
) -> Result<Vec<DiffType>, Error> {
    // float quantity compare
    if let Some((actual_float, nominal_float)) = both_quantity(actual, nominal) {
        Ok(config
            .comparison_modes
            .iter()
            .filter_map(|cm| {
                if !cm.in_tolerance(nominal_float, actual_float) {
                    Some(DiffType::OutOfTolerance {
                        nominal: nominal_float.clone(),
                        actual: actual_float.clone(),
                        mode: *cm,
                        position,
                    })
                } else {
                    None
                }
            })
            .collect())
    } else if let Some((actual_string, nominal_string)) = both_string(actual, nominal) {
        if let Some(exclude_regex) = config.exclude_field_regex.as_deref() {
            let regex = Regex::new(exclude_regex)?;
            if regex.is_match(nominal_string.as_str()) {
                return Ok(Vec::new());
            }
        }
        if nominal_string != actual_string {
            Ok(vec![DiffType::UnequalStrings {
                position,
                nominal: nominal_string,
                actual: actual_string,
            }])
        } else {
            Ok(Vec::new())
        }
    } else {
        Ok(vec![DiffType::DifferentValueTypes {
            actual: actual.clone(),
            nominal: nominal.clone(),
            position,
        }])
    }
}

fn get_diffs_readers<R: Read + Seek + Send>(
    nominal: R,
    actual: R,
    config: &CSVCompareConfig,
) -> Result<(Table, Table, Vec<DiffType>), Error> {
    let tables: Result<Vec<Table>, Error> = [nominal, actual]
        .into_par_iter()
        .map(|r| Table::from_reader(r, &config.delimiters))
        .collect();
    let mut tables = tables?;
    if let (Some(mut actual), Some(mut nominal)) = (tables.pop(), tables.pop()) {
        if let Some(preprocessors) = config.preprocessing.as_ref() {
            for preprocessor in preprocessors.iter() {
                preprocessor.process(&mut nominal)?;
                preprocessor.process(&mut actual)?;
            }
        }
        let comparison_result = compare_tables(&nominal, &actual, config)?;
        Ok((nominal, actual, comparison_result))
    } else {
        Err(Error::UnterminatedLiteral)
    }
}

pub(crate) fn compare_paths(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    config: &CSVCompareConfig,
) -> Result<report::Difference, Error> {
    let nominal_file = fat_io_wrap_std(nominal.as_ref(), &File::open)?;
    let actual_file = fat_io_wrap_std(actual.as_ref(), &File::open)?;

    let (_, _, results) = get_diffs_readers(&nominal_file, &actual_file, config)?;
    results.iter().for_each(|error| {
        error!("{}", &error);
    });
    let is_error = !results.is_empty();
    let mut result = report::Difference::new_for_file(nominal.as_ref(), actual.as_ref());
    result.is_error = is_error;
    result.detail = results.into_iter().map(report::DiffDetail::CSV).collect();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csv::DiffType::{
        DifferentValueTypes, OutOfTolerance, UnequalHeader, UnequalStrings,
    };
    use crate::csv::Preprocessor::ExtractHeaders;
    use std::io::Cursor;

    const NOMINAL: &str = "nominal";
    const ACTUAL: &str = "actual";
    const POS_COL: usize = 1337;
    const POS_ROW: usize = 667;

    fn mk_position() -> Position {
        Position {
            col: POS_COL,
            row: POS_ROW,
        }
    }

    #[test]
    fn diff_types_readable_string() {
        let string_unequal = UnequalStrings {
            nominal: NOMINAL.to_string(),
            actual: ACTUAL.to_string(),
            position: mk_position(),
        };
        let msg = format!("{string_unequal}");
        assert!(msg.contains(NOMINAL));
        assert!(msg.contains(ACTUAL));
        assert!(msg.contains(format!("{POS_COL}").as_str()));
        assert!(msg.contains(format!("{POS_ROW}").as_str()));
    }

    #[test]
    fn diff_types_readable_out_of_tolerance() {
        let string_unequal = OutOfTolerance {
            nominal: Quantity {
                value: 10.0,
                unit: Some("mm".to_owned()),
            },
            actual: Quantity {
                value: 12.0,
                unit: Some("um".to_owned()),
            },
            mode: Mode::Absolute(11.0),
            position: mk_position(),
        };
        let msg = format!("{string_unequal}");
        assert!(msg.contains("10 mm"));
        assert!(msg.contains("11"));
        assert!(msg.contains("12 um"));
        assert!(msg.contains("Absolute"));
        assert!(msg.contains(format!("{POS_COL}").as_str()));
        assert!(msg.contains(format!("{POS_ROW}").as_str()));
    }

    #[test]
    fn diff_types_readable_different_value_types() {
        let string_unequal = DifferentValueTypes {
            nominal: Value::from_str("10.0 mm", &None),
            actual: Value::from_str(ACTUAL, &None),
            position: mk_position(),
        };
        let msg = format!("{string_unequal}");
        assert!(msg.contains("10 mm"));
        assert!(msg.contains(ACTUAL));
        assert!(msg.contains(format!("{POS_COL}").as_str()));
        assert!(msg.contains(format!("{POS_ROW}").as_str()));
    }

    #[test]
    fn table_cols_reading_correct() {
        let table = Table::from_reader(
            File::open("tests/csv/data/Annotations.csv").unwrap(),
            &Delimiters::default(),
        )
        .unwrap();
        assert_eq!(table.columns.len(), 13);
    }

    #[test]
    fn table_rows_reading_correct() {
        let table = Table::from_reader(
            File::open("tests/csv/data/Annotations.csv").unwrap(),
            &Delimiters::default(),
        )
        .unwrap();
        assert_eq!(table.rows().len(), 6);
    }

    #[test]
    fn identity_comparison_is_empty() {
        let config = CSVCompareConfig {
            exclude_field_regex: None,
            comparison_modes: vec![Mode::Absolute(0.0), Mode::Relative(0.0)],
            delimiters: Delimiters::default(),
            preprocessing: None,
        };

        let actual = File::open("tests/csv/data/Annotations.csv").unwrap();
        let nominal = File::open("tests/csv/data/Annotations.csv").unwrap();

        let (_, _, diff) = get_diffs_readers(nominal, actual, &config).unwrap();
        assert!(diff.is_empty());
    }

    #[test]
    fn diffs_on_table_level() {
        let config = CSVCompareConfig {
            preprocessing: None,
            exclude_field_regex: Some(r"Surface".to_owned()),
            comparison_modes: vec![],
            delimiters: Delimiters::default(),
        };

        let actual = Table::from_reader(
            File::open("tests/csv/data/DeviationHistogram.csv").unwrap(),
            &config.delimiters,
        )
        .unwrap();
        let nominal = Table::from_reader(
            File::open("tests/csv/data/DeviationHistogram_diff.csv").unwrap(),
            &config.delimiters,
        )
        .unwrap();

        let diff = compare_tables(&nominal, &actual, &config).unwrap();
        assert_eq!(diff.len(), 1);
        let first_diff = diff.first().unwrap();
        if let DifferentValueTypes {
            nominal,
            actual,
            position,
        } = first_diff
        {
            assert_eq!(nominal.get_string().unwrap(), "different_type_here");
            assert_eq!(actual.get_quantity().unwrap().value, 0.00204398);
            assert_eq!(position.col, 1);
            assert_eq!(position.row, 12);
        } else {
            unreachable!();
        }
    }

    #[test]
    fn header_diffs_on_table_level() {
        let config = CSVCompareConfig {
            preprocessing: Some(vec![ExtractHeaders]),
            exclude_field_regex: None,
            comparison_modes: vec![],
            delimiters: Delimiters::default(),
        };

        let mut actual = Table::from_reader(
            File::open("tests/csv/data/Annotations.csv").unwrap(),
            &config.delimiters,
        )
        .unwrap();

        ExtractHeaders.process(&mut actual).unwrap();

        let mut nominal = Table::from_reader(
            File::open("tests/csv/data/Annotations_diff.csv").unwrap(),
            &config.delimiters,
        )
        .unwrap();

        ExtractHeaders.process(&mut nominal).unwrap();

        let diff = compare_tables(&nominal, &actual, &config).unwrap();
        assert_eq!(diff.len(), 3);

        let first_diff = diff.first().unwrap();
        if let UnequalHeader { nominal, actual } = first_diff {
            assert_eq!(nominal, "Position x [mm]");
            assert_eq!(actual, "Pos. x [mm]");
        } else {
            unreachable!();
        }
    }

    #[test]
    fn different_type_search_only() {
        let config = CSVCompareConfig {
            preprocessing: None,
            exclude_field_regex: Some(r"Surface".to_owned()),
            comparison_modes: vec![],
            delimiters: Delimiters::default(),
        };

        let actual = File::open("tests/csv/data/DeviationHistogram.csv").unwrap();
        let nominal = File::open("tests/csv/data/DeviationHistogram_diff.csv").unwrap();

        let (_, _, diff) = get_diffs_readers(nominal, actual, &config).unwrap();
        assert_eq!(diff.len(), 1);
        let first_diff = diff.first().unwrap();
        if let DifferentValueTypes {
            nominal,
            actual,
            position,
        } = first_diff
        {
            assert_eq!(nominal.get_string().unwrap(), "different_type_here");
            assert_eq!(actual.get_quantity().unwrap().value, 0.00204398);
            assert_eq!(position.col, 1);
            assert_eq!(position.row, 12);
        }
    }

    #[test]
    fn numerics_test_absolute() {
        let config = CSVCompareConfig {
            preprocessing: None,
            exclude_field_regex: Some(r"Surface".to_owned()),
            comparison_modes: vec![Mode::Absolute(0.5)],
            delimiters: Delimiters::default(),
        };

        let actual = File::open("tests/csv/data/DeviationHistogram.csv").unwrap();
        let nominal = File::open("tests/csv/data/DeviationHistogram_diff.csv").unwrap();

        let (_, _, diff) = get_diffs_readers(nominal, actual, &config).unwrap();
        // the different value type is still there, but we have 2 diffs over 0.5
        assert_eq!(diff.len(), 3);
    }

    #[test]
    fn mode_formatting() {
        let abs = Mode::Absolute(0.1);
        let msg = format!("{abs}");
        assert!(msg.contains("0.1"));
        assert!(msg.contains("Absolute"));

        let abs = Mode::Relative(0.1);
        let msg = format!("{abs}");
        assert!(msg.contains("0.1"));
        assert!(msg.contains("Relative"));

        let abs = Mode::Ignore;
        let msg = format!("{abs}");
        assert!(msg.contains("Ignored"));
    }

    #[test]
    fn different_formattings() {
        let config = CSVCompareConfig {
            preprocessing: None,
            exclude_field_regex: None,
            comparison_modes: vec![Mode::Absolute(0.5)],
            delimiters: Delimiters::autodetect(),
        };

        let actual = File::open(
            "tests/integ/data/display_of_status_message_in_cm_tables/actual/Volume1.csv",
        )
        .unwrap();
        let nominal = File::open(
            "tests/integ/data/display_of_status_message_in_cm_tables/expected/Volume1.csv",
        )
        .unwrap();

        let (_, _, diff) = get_diffs_readers(nominal, actual, &config).unwrap();
        // the different value type is still there, but we have 2 diffs over 0.5
        assert_eq!(diff.len(), 0);
    }

    #[test]
    fn numerics_test_relative() {
        let config = CSVCompareConfig {
            preprocessing: None,
            exclude_field_regex: Some(r"Surface".to_owned()),
            comparison_modes: vec![Mode::Relative(0.1)],
            delimiters: Delimiters::default(),
        };

        let actual = File::open("tests/csv/data/DeviationHistogram.csv").unwrap();
        let nominal = File::open("tests/csv/data/DeviationHistogram_diff.csv").unwrap();

        let (_, _, diff) = get_diffs_readers(nominal, actual, &config).unwrap();
        // the different value type is still there, but we have 5 rel diffs over 0.1
        assert_eq!(diff.len(), 6);
    }

    #[test]
    fn string_value_parsing_works() {
        let pairs = [
            ("0.6", Quantity::new(0.6, None)),
            ("0.6 in", Quantity::new(0.6, Some("in"))),
            ("inf", Quantity::new(f64::INFINITY, None)),
            ("-0.6", Quantity::new(-0.6, None)),
            ("-0.6 mm", Quantity::new(-0.6, Some("mm"))),
        ];
        pairs.into_iter().for_each(|(string, quantity)| {
            assert_eq!(Value::from_str(string, &None), Value::Quantity(quantity));
        });

        let nan_value = Value::from_str("nan mm", &None);
        let nan_value = nan_value.get_quantity().unwrap();
        assert!(nan_value.value.is_nan());
        assert_eq!(nan_value.unit, Some("mm".to_string()));
    }

    #[test]
    fn basic_compare_modes_test_absolute() {
        let abs_mode = Mode::Absolute(1.0);
        assert!(abs_mode.in_tolerance(&Quantity::new(0.0, None), &Quantity::new(1.0, None)));
        assert!(abs_mode.in_tolerance(&Quantity::new(0.0, None), &Quantity::new(-1.0, None)));
        assert!(abs_mode.in_tolerance(&Quantity::new(1.0, None), &Quantity::new(0.0, None)));
        assert!(abs_mode.in_tolerance(&Quantity::new(-1.0, None), &Quantity::new(0.0, None)));
        assert!(abs_mode.in_tolerance(&Quantity::new(0.0, None), &Quantity::new(0.0, None)));

        assert!(!abs_mode.in_tolerance(&Quantity::new(0.0, None), &Quantity::new(1.01, None)));
        assert!(!abs_mode.in_tolerance(&Quantity::new(0.0, None), &Quantity::new(-1.01, None)));
        assert!(!abs_mode.in_tolerance(&Quantity::new(1.01, None), &Quantity::new(0.0, None)));
        assert!(!abs_mode.in_tolerance(&Quantity::new(-1.01, None), &Quantity::new(0.0, None)));
    }

    #[test]
    fn basic_compare_modes_test_relative() {
        let rel_mode = Mode::Relative(1.0);
        assert!(rel_mode.in_tolerance(&Quantity::new(1.0, None), &Quantity::new(2.0, None)));
        assert!(rel_mode.in_tolerance(&Quantity::new(2.0, None), &Quantity::new(4.0, None)));
        assert!(rel_mode.in_tolerance(&Quantity::new(-1.0, None), &Quantity::new(-2.0, None)));
        assert!(rel_mode.in_tolerance(&Quantity::new(-2.0, None), &Quantity::new(-4.0, None)));
        assert!(rel_mode.in_tolerance(&Quantity::new(0.0, None), &Quantity::new(0.0, None)));

        assert!(!rel_mode.in_tolerance(&Quantity::new(1.0, None), &Quantity::new(2.01, None)));
        assert!(!rel_mode.in_tolerance(&Quantity::new(2.0, None), &Quantity::new(4.01, None)));
    }

    #[test]
    fn check_same_numbers_different_missmatch() {
        let rel_mode = Mode::Relative(1.0);
        assert!(!rel_mode.in_tolerance(
            &Quantity::new(2.0, Some("mm")),
            &Quantity::new(2.0, Some("m"))
        ));
    }

    #[test]
    fn basic_compare_modes_test_ignored() {
        let abs_mode = Mode::Ignore;
        assert!(abs_mode.in_tolerance(
            &Quantity::new(1.0, None),
            &Quantity::new(f64::INFINITY, None)
        ));
    }

    #[test]
    fn nan_is_nan() {
        let nan = f64::NAN;
        let nominal = Quantity {
            value: nan,
            unit: None,
        };
        let actual = Quantity {
            value: nan,
            unit: None,
        };

        assert!(Mode::Relative(1.0).in_tolerance(&nominal, &actual));
        assert!(Mode::Absolute(1.0).in_tolerance(&nominal, &actual));
        assert!(Mode::Ignore.in_tolerance(&nominal, &actual))
    }

    #[test]
    fn bom_is_trimmed() {
        let str_with_bom = "\u{feff}Hallo\n\r";
        let str_no_bom = "Hallo\n";
        let cfg = CSVCompareConfig {
            preprocessing: None,
            delimiters: Delimiters::default(),
            exclude_field_regex: None,
            comparison_modes: vec![Mode::Absolute(0.0)],
        };
        let (_, _, res) =
            get_diffs_readers(Cursor::new(str_with_bom), Cursor::new(str_no_bom), &cfg).unwrap();
        assert!(res.is_empty());
    }

    fn mk_test_table() -> Table {
        let col = Column {
            rows: vec![
                Value::from_str("0.0", &None),
                Value::from_str("1.0", &None),
                Value::from_str("2.0", &None),
            ],
            header: None,
        };

        let col_two = col.clone();
        Table {
            columns: vec![col, col_two],
        }
    }

    #[test]
    fn row_iterator() {
        let table = mk_test_table();
        let mut row_iterator = table.rows();
        assert_eq!(row_iterator.len(), 3);
        let first_row = row_iterator.next().unwrap();
        assert!(first_row
            .iter()
            .all(|v| **v == Value::from_str("0.0", &None)));
        for row in row_iterator {
            assert_eq!(row.len(), 2);
        }
    }

    #[test]
    fn row_iterator_mut() {
        let mut table = mk_test_table();
        let mut row_iterator = table.rows_mut();
        assert_eq!(row_iterator.len(), 3);
        let first_row = row_iterator.next().unwrap();
        assert!(first_row
            .iter()
            .all(|v| **v == Value::from_str("0.0", &None)));
        for row in row_iterator {
            assert_eq!(row.len(), 2);
        }
        let row_iterator = table.rows_mut();
        for mut row in row_iterator {
            assert_eq!(row.len(), 2);
            row.iter_mut()
                .for_each(|v| **v = Value::from_str("4.0", &None));
        }
        let mut row_iterator = table.rows();
        assert!(row_iterator.all(|r| r.iter().all(|v| **v == Value::from_str("4.0", &None))));
    }

    #[test]
    fn loading_non_existing_folder_fails() {
        let conf = CSVCompareConfig {
            comparison_modes: vec![],
            delimiters: Delimiters::default(),
            exclude_field_regex: None,
            preprocessing: None,
        };
        let result = compare_paths("non_existing", "also_non_existing", &conf);
        assert!(matches!(result.unwrap_err(), Error::FileAccessFailed(_)));
    }

    #[test]
    fn table_with_newlines_consistent_col_lengths() {
        let table = Table::from_reader(
            File::open("tests/csv/data/defects.csv").unwrap(),
            &Delimiters::autodetect(),
        )
        .unwrap();
        for col in table.columns.iter() {
            assert_eq!(col.rows.len(), table.columns.first().unwrap().rows.len());
        }
    }

    #[test]
    fn test_float_diff_precision() {
        let magic_first = 0.03914;
        let magic_second = 0.03913;
        let tolerance = 0.00001;
        let tolerance_f64 = 0.00001;

        let single_diff: f32 = magic_first - magic_second;
        assert!(single_diff > tolerance);

        let quantity1 = Quantity::new(0.03914, None);
        let quantity2 = Quantity::new(0.03913, None);
        let modes = [
            Mode::Absolute(tolerance_f64),
            Mode::Relative(tolerance_f64 / 0.03914),
        ];
        for mode in modes {
            assert!(mode.in_tolerance(&quantity1, &quantity2));
        }
    }
}
