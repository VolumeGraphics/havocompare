use crate::report;
mod preprocessing;
mod value;

use preprocessing::Preprocessor;
use value::Quantity;
use value::Value;

use itertools::Itertools;
use regex::Regex;
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek};
use std::path::Path;
use tracing::{debug, error, info};

#[derive(Clone, Copy, Debug)]
pub struct Position {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug)]
pub enum DiffType {
    UnequalStrings {
        nominal: String,
        actual: String,
        position: Position,
    },
    OutOfTolerance {
        nominal: Quantity,
        actual: Quantity,
        mode: Mode,
        position: Position,
    },
    DifferentValueTypes {
        nominal: Value,
        actual: Value,
        position: Position,
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
                .unwrap();
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
                .unwrap();
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
                .unwrap();
            }
        };
        Ok(())
    }
}

#[derive(Copy, Clone, JsonSchema, Debug, Deserialize, Serialize)]
pub enum Mode {
    Absolute(f32),
    Relative(f32),
    Ignore,
}

impl Display for Mode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self {
            Mode::Absolute(tolerance) => {
                write!(f, "Absolute (tol: {})", tolerance).unwrap();
            }
            Mode::Relative(tolerance) => {
                write!(f, "Relative (tol: {})", tolerance).unwrap();
            }
            Mode::Ignore => {
                write!(f, "Ignored").unwrap();
            }
        };
        Ok(())
    }
}

impl Mode {
    pub fn in_tolerance(&self, nominal: &Quantity, actual: &Quantity) -> bool {
        if nominal.value.is_nan() && actual.value.is_nan() {
            return true;
        }

        match self {
            Mode::Absolute(tolerance) => {
                let numerically = (nominal.value - actual.value).abs() <= *tolerance;
                let identical_units = nominal.unit == actual.unit;
                numerically && identical_units
            }
            Mode::Ignore => true,
            Mode::Relative(tolerance) => {
                let diff = (nominal.value - actual.value).abs();
                let numerically = if diff == 0.0 {
                    true
                } else {
                    (diff / nominal.value).abs() <= *tolerance
                };
                let identical_units = nominal.unit == actual.unit;
                numerically && identical_units
            }
        }
    }
}

#[derive(JsonSchema, Deserialize, Serialize, Debug)]
pub struct CSVCompareConfig {
    #[serde(flatten)]
    pub delimiters: Delimiters,
    pub comparison_modes: Vec<Mode>,
    pub exclude_field_regex: Option<String>,
    pub preprocessing: Option<Vec<Preprocessor>>,
}

#[derive(JsonSchema, Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Delimiters {
    pub field_delimiter: Option<char>,
    pub decimal_separator: Option<char>,
}

impl Default for Delimiters {
    fn default() -> Self {
        Delimiters {
            field_delimiter: Some(','),
            decimal_separator: Some('.'),
        }
    }
}

impl Delimiters {
    pub fn is_empty(&self) -> bool {
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

#[derive(Default)]
pub struct Column {
    pub header: Option<String>,
    pub rows: Vec<Value>,
}

pub struct Table {
    pub columns: Vec<Column>,
}

impl Table {
    pub fn from_reader<R: Read + Seek>(mut input: R, config: &Delimiters) -> Table {
        let delimiters = match config.is_empty() {
            false => Cow::Borrowed(config),
            true => Cow::Owned(guess_format_from_reader(&mut input)),
        };
        debug!("Final delimiters: {:?}", delimiters);
        let mut cols = Vec::new();
        let input = BufReader::new(input);
        input.lines().filter_map(|l| l.ok()).for_each(|row| {
            let row = row.trim_start_matches('\u{feff}');
            let fields = split_row(row, &delimiters);
            if cols.is_empty() {
                cols.resize_with(fields.len(), Column::default);
            }
            fields
                .into_iter()
                .zip(cols.iter_mut())
                .for_each(|(f, col)| col.rows.push(f));
        });

        Table { columns: cols }
    }

    pub fn rows(&self) -> RowIterator {
        RowIterator {
            table: self,
            current_idx: 0,
        }
    }
}

pub struct RowIterator<'a> {
    current_idx: usize,
    table: &'a Table,
}

impl<'a> ExactSizeIterator for RowIterator<'a> {
    fn len(&self) -> usize {
        self.table
            .columns
            .first()
            .map(|c| c.rows.len())
            .unwrap_or(0)
    }
}

impl<'a> Iterator for RowIterator<'a> {
    type Item = Vec<&'a Value>;
    fn next(&mut self) -> Option<Self::Item> {
        let row: Vec<_> = self
            .table
            .columns
            .iter()
            .filter_map(|c| c.rows.get(self.current_idx))
            .collect();
        self.current_idx += 1;
        if row.len() == self.table.columns.len() {
            Some(row)
        } else {
            None
        }
    }
}

pub fn compare_tables(nominal: &Table, actual: &Table, config: &CSVCompareConfig) -> Vec<DiffType> {
    nominal
        .columns
        .iter()
        .zip(actual.columns.iter())
        .enumerate()
        .flat_map(|(col, (col_nom, col_act))| {
            col_nom
                .rows
                .iter()
                .zip(col_act.rows.iter())
                .enumerate()
                .flat_map(move |(row, (val_nom, val_act))| {
                    let position = Position { row, col };
                    compare_values(val_nom, val_act, config, position)
                })
        })
        .collect()
}

fn split_row(row: &str, config: &Delimiters) -> Vec<Value> {
    if let Some(row_delimiter) = config.field_delimiter.as_ref() {
        row.split(*row_delimiter)
            .enumerate()
            .map(|(_, field)| Value::from_str(field, &config.decimal_separator))
            .collect()
    } else {
        vec![Value::from_str(row, &config.decimal_separator)]
    }
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
) -> Vec<DiffType> {
    // float quantity compare
    if let Some((actual_float, nominal_float)) = both_quantity(actual, nominal) {
        config
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
            .collect()
    } else if let Some((actual_string, nominal_string)) = both_string(actual, nominal) {
        if let Some(exclude_regex) = config.exclude_field_regex.as_deref() {
            let regex = Regex::new(exclude_regex).expect("Specified exclusion regex invalid!");
            if regex.is_match(nominal_string.as_str()) {
                return Vec::new();
            }
        }
        if nominal_string != actual_string {
            vec![DiffType::UnequalStrings {
                position,
                nominal: nominal_string,
                actual: actual_string,
            }]
        } else {
            Vec::new()
        }
    } else {
        vec![DiffType::DifferentValueTypes {
            actual: actual.clone(),
            nominal: nominal.clone(),
            position,
        }]
    }
}

fn get_diffs_readers<R: Read + Seek>(
    nominal: R,
    actual: R,
    config: &CSVCompareConfig,
) -> (Table, Table, Vec<DiffType>) {
    let mut nominal = Table::from_reader(nominal, &config.delimiters);
    let mut actual = Table::from_reader(actual, &config.delimiters);
    info!("Running preprocessing steps");
    if let Some(preprocessors) = config.preprocessing.as_ref() {
        preprocessors.iter().for_each(|preprocessor| {
            preprocessor.process(&mut nominal);
            preprocessor.process(&mut actual);
        });
    }
    let comparison_result = compare_tables(&nominal, &actual, config);
    (nominal, actual, comparison_result)
}

pub fn compare_paths(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    config: &CSVCompareConfig,
    rule_name: &str,
) -> report::FileCompareResult {
    let nominal_file = File::open(nominal.as_ref()).expect("Could not open nominal file");
    let actual_file = File::open(actual.as_ref()).expect("Could not open nominal file");

    let (nominal_table, actual_table, results) =
        get_diffs_readers(&nominal_file, &actual_file, config);
    results.iter().for_each(|error| {
        error!("{}", &error);
    });

    report::write_csv_detail(
        nominal_table,
        actual_table,
        nominal.as_ref(),
        actual.as_ref(),
        results.as_slice(),
        rule_name,
    )
}

fn guess_format_from_line(
    line: &str,
    field_separator_hint: Option<char>,
) -> (Option<char>, Option<char>) {
    let mut field_separator = field_separator_hint;

    if field_separator.is_none() {
        if line.find(';').is_some() {
            field_separator = Some(';');
        } else {
            let field_sep_regex = Regex::new(r"\w([,|])[\W\w]").unwrap();
            let capture = field_sep_regex.captures_iter(line).next();
            if let Some(cap) = capture {
                field_separator = Some(cap[1].chars().next().unwrap());
            }
        }
    }

    let decimal_separator_candidates = [',', '.'];
    let context_acceptable_candidates = if let Some(field_separator) = field_separator {
        decimal_separator_candidates
            .into_iter()
            .filter(|c| *c != field_separator)
            .join("")
    } else {
        decimal_separator_candidates.into_iter().join("")
    };

    let decimal_separator_regex_string = format!(r"\d([{}])\d", context_acceptable_candidates);
    debug!(
        "Regex for decimal sep: '{}'",
        decimal_separator_regex_string.as_str()
    );
    let decimal_separator_regex = Regex::new(decimal_separator_regex_string.as_str()).unwrap();
    let mut separators: HashMap<char, usize> = HashMap::new();

    for capture in decimal_separator_regex.captures_iter(line) {
        let sep = capture[1].chars().next().unwrap();
        if let Some(entry) = separators.get_mut(&sep) {
            *entry += 1;
        } else {
            separators.insert(sep, 1);
        }
    }

    debug!(
        "Found separator candidates with occurrence count: {:?}",
        separators
    );

    let decimal_separator = separators
        .iter()
        .sorted_by(|a, b| b.1.cmp(a.1))
        .map(|s| s.0.to_owned())
        .next();

    (field_separator, decimal_separator)
}

fn guess_format_from_reader<R: Read + Seek>(mut input: &mut R) -> Delimiters {
    let mut format = (None, None);

    let bufreader = BufReader::new(&mut input);
    debug!("Guessing format from reader...");
    for line in bufreader.lines().filter_map(|l| l.ok()) {
        debug!("Guessing format from line: '{}'", line.as_str());
        format = guess_format_from_line(line.as_str(), format.0);
        debug!("Current format: {:?}", format);
        if format.0.is_some() && format.1.is_some() {
            break;
        }
    }

    input.rewind().expect("Could not rewind the file");

    let delim = Delimiters {
        field_delimiter: format.0,
        decimal_separator: format.1,
    };
    info!(
        "Inferring of csv delimiters resulted in decimal separators: '{:?}', field delimiter: '{:?}'",
        delim.decimal_separator, delim.field_delimiter
    );
    delim
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csv::DiffType::{DifferentValueTypes, OutOfTolerance, UnequalStrings};
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
        let msg = format!("{}", string_unequal);
        assert!(msg.contains(NOMINAL));
        assert!(msg.contains(ACTUAL));
        assert!(msg.contains(format!("{}", POS_COL).as_str()));
        assert!(msg.contains(format!("{}", POS_ROW).as_str()));
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
        let msg = format!("{}", string_unequal);
        assert!(msg.contains("10 mm"));
        assert!(msg.contains("11"));
        assert!(msg.contains("12 um"));
        assert!(msg.contains("Absolute"));
        assert!(msg.contains(format!("{}", POS_COL).as_str()));
        assert!(msg.contains(format!("{}", POS_ROW).as_str()));
    }

    #[test]
    fn diff_types_readable_different_value_types() {
        let string_unequal = DifferentValueTypes {
            nominal: Value::from_str("10.0 mm", &None),
            actual: Value::from_str(ACTUAL, &None),
            position: mk_position(),
        };
        let msg = format!("{}", string_unequal);
        println!("{}", msg);
        assert!(msg.contains("10 mm"));
        assert!(msg.contains(ACTUAL));
        assert!(msg.contains(format!("{}", POS_COL).as_str()));
        assert!(msg.contains(format!("{}", POS_ROW).as_str()));
    }

    #[test]
    fn table_cols_reading_correct() {
        let table = Table::from_reader(
            File::open("tests/csv/data/Annotations.csv").unwrap(),
            &Delimiters::default(),
        );
        assert_eq!(table.columns.len(), 13);
    }

    #[test]
    fn table_rows_reading_correct() {
        let table = Table::from_reader(
            File::open("tests/csv/data/Annotations.csv").unwrap(),
            &Delimiters::default(),
        );
        assert_eq!(table.rows().len(), 7);
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

        let (_, _, diff) = get_diffs_readers(nominal, actual, &config);
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
        );
        let nominal = Table::from_reader(
            File::open("tests/csv/data/DeviationHistogram_diff.csv").unwrap(),
            &config.delimiters,
        );

        let diff = compare_tables(&nominal, &actual, &config);
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
    fn different_type_search_only() {
        let config = CSVCompareConfig {
            preprocessing: None,
            exclude_field_regex: Some(r"Surface".to_owned()),
            comparison_modes: vec![],
            delimiters: Delimiters::default(),
        };

        let actual = File::open("tests/csv/data/DeviationHistogram.csv").unwrap();
        let nominal = File::open("tests/csv/data/DeviationHistogram_diff.csv").unwrap();

        let (_, _, diff) = get_diffs_readers(nominal, actual, &config);
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

        let (_, _, diff) = get_diffs_readers(nominal, actual, &config);
        // the different value type is still there, but we have 2 diffs over 0.5
        assert_eq!(diff.len(), 3);
    }

    #[test]
    fn mode_formatting() {
        let abs = Mode::Absolute(0.1);
        let msg = format!("{}", abs);
        assert!(msg.contains("0.1"));
        assert!(msg.contains("Absolute"));

        let abs = Mode::Relative(0.1);
        let msg = format!("{}", abs);
        assert!(msg.contains("0.1"));
        assert!(msg.contains("Relative"));

        let abs = Mode::Ignore;
        let msg = format!("{}", abs);
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

        let (_, _, diff) = get_diffs_readers(nominal, actual, &config);
        println!("{:?}", diff);
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

        let (_, _, diff) = get_diffs_readers(nominal, actual, &config);
        // the different value type is still there, but we have 5 rel diffs over 0.1
        assert_eq!(diff.len(), 6);
    }

    #[test]
    fn string_value_parsing_works() {
        let pairs = [
            ("0.6", Quantity::new(0.6, None)),
            ("0.6 in", Quantity::new(0.6, Some("in"))),
            ("inf", Quantity::new(f32::INFINITY, None)),
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
        let abs_mode = Mode::Relative(1.0);
        assert!(abs_mode.in_tolerance(&Quantity::new(1.0, None), &Quantity::new(2.0, None)));
        assert!(abs_mode.in_tolerance(&Quantity::new(2.0, None), &Quantity::new(4.0, None)));
        assert!(abs_mode.in_tolerance(&Quantity::new(-1.0, None), &Quantity::new(-2.0, None)));
        assert!(abs_mode.in_tolerance(&Quantity::new(-2.0, None), &Quantity::new(-4.0, None)));
        assert!(abs_mode.in_tolerance(&Quantity::new(0.0, None), &Quantity::new(0.0, None)));

        assert!(!abs_mode.in_tolerance(&Quantity::new(1.0, None), &Quantity::new(2.01, None)));
        assert!(!abs_mode.in_tolerance(&Quantity::new(2.0, None), &Quantity::new(4.01, None)));
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
            &Quantity::new(f32::INFINITY, None)
        ));
    }

    #[test]
    fn format_detection_basics() {
        let format = guess_format_from_line(
            "-0.969654597744788,-0.215275534510198,0.115869999295192,7.04555232210696",
            None,
        );
        assert_eq!(format, (Some(','), Some('.')));

        let format = guess_format_from_line(
            "-0.969654597744788;-0.215275534510198;0.115869999295192;7.04555232210696",
            None,
        );
        assert_eq!(format, (Some(';'), Some('.')));

        let format = guess_format_from_line(
            "-0.969654597744788,-0.215275534510198,0.115869999295192,7.04555232210696",
            None,
        );
        assert_eq!(format, (Some(','), Some('.')));
    }

    #[test]
    fn format_detection_from_file() {
        let format =
            guess_format_from_reader(&mut File::open("tests/csv/data/Annotations.csv").unwrap());
        assert_eq!(
            format,
            Delimiters {
                field_delimiter: Some(','),
                decimal_separator: Some('.')
            }
        );
    }

    #[test]
    fn format_detection_from_file_metrology_special() {
        let format = guess_format_from_reader(
            &mut File::open("tests/csv/data/Multi_Apply_Rotation.csv").unwrap(),
        );
        assert_eq!(
            format,
            Delimiters {
                field_delimiter: Some(','),
                decimal_separator: Some('.')
            }
        );
    }

    #[test]
    fn format_detection_from_file_metrology_other_special() {
        let format = guess_format_from_reader(
            &mut File::open("tests/csv/data/CM_quality_threshold.csv").unwrap(),
        );
        assert_eq!(
            format,
            Delimiters {
                field_delimiter: Some(','),
                decimal_separator: None
            }
        );
    }

    #[test]
    fn format_detection_from_file_analysis_pia_table() {
        let format = guess_format_from_reader(
            &mut File::open("tests/csv/data/easy_pore_export_annoration_table_result.csv").unwrap(),
        );
        assert_eq!(
            format,
            Delimiters {
                field_delimiter: Some(';'),
                decimal_separator: Some(',')
            }
        );
    }

    #[test]
    fn format_detection_from_file_no_field_sep() {
        let format =
            guess_format_from_reader(&mut File::open("tests/csv/data/no_field_sep.csv").unwrap());
        assert_eq!(
            format,
            Delimiters {
                field_delimiter: None,
                decimal_separator: Some('.')
            }
        );
    }
    #[test]
    fn format_detection_from_file_semicolon_formatting() {
        let format = guess_format_from_reader(
            &mut File::open(
                "tests/integ/data/display_of_status_message_in_cm_tables/expected/Volume1.csv",
            )
            .unwrap(),
        );
        assert_eq!(
            format,
            Delimiters {
                field_delimiter: Some(';'),
                decimal_separator: Some(',')
            }
        );
    }

    #[test]
    fn format_detection_from_file_dot_comma_formatting() {
        let format = guess_format_from_reader(
            &mut File::open(
                "tests/integ/data/display_of_status_message_in_cm_tables/actual/Volume1.csv",
            )
            .unwrap(),
        );
        assert_eq!(
            format,
            Delimiters {
                field_delimiter: Some(','),
                decimal_separator: Some('.')
            }
        );
    }

    #[test]
    fn nan_is_nan() {
        let nan = f32::NAN;
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
    fn no_delimiter_whole_row_is_field() {
        let row = "my - cool - row - has - strange - delimiters";
        let delimiters = Delimiters {
            field_delimiter: None,
            decimal_separator: None,
        };
        let split_result = split_row(row, &delimiters);
        assert_eq!(split_result.len(), 1);
        let value = split_result.first().unwrap();
        assert_eq!(value.get_string().as_deref().unwrap(), row);
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
            get_diffs_readers(Cursor::new(str_with_bom), Cursor::new(str_no_bom), &cfg);
        assert!(res.is_empty());
    }
}
