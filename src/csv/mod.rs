use crate::report;
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
}

#[derive(JsonSchema, Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct Delimiters {
    pub field_delimiter: Option<char>,
    pub decimal_separator: Option<char>,
}

impl Delimiters {
    pub fn is_empty(&self) -> bool {
        self.decimal_separator.is_none() && self.field_delimiter.is_none()
    }
}

#[derive(Debug, Clone, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct Quantity {
    value: f32,
    unit: Option<String>,
}

impl Quantity {
    #[cfg(test)]
    pub(crate) fn new(value: f32, unit: Option<&str>) -> Self {
        Self {
            unit: unit.map(|s| s.to_owned()),
            value,
        }
    }
}

impl Display for Quantity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(unit) = self.unit.as_deref() {
            write!(f, "{} {}", self.value, unit)
        } else {
            write!(f, "{}", self.value)
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Value {
    Quantity(Quantity),
    String(String),
}

impl Display for Value {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self {
            Value::Quantity(val) => {
                write!(f, "{}", val).unwrap();
            }
            Value::String(val) => {
                write!(f, "'{}'s", val).unwrap();
            }
        }
        Ok(())
    }
}

impl Value {
    fn get_numerical_value(field_split: &[&str]) -> Option<f32> {
        if field_split.len() == 1 || field_split.len() == 2 {
            if let Ok(float_value) = field_split.first().unwrap().parse::<f32>() {
                return Some(float_value);
            }
        }
        None
    }

    pub fn from_str(s: &str, decimal_separator: &Option<char>) -> Value {
        let field_string: String = if let Some(delim) = decimal_separator {
            s.replace(*delim, ".")
        } else {
            s.into()
        };

        let field_split: Vec<_> = field_string.trim().split(' ').collect();

        if let Some(float_value) = Self::get_numerical_value(field_split.as_slice()) {
            Value::Quantity(Quantity {
                value: float_value,
                unit: field_split.get(1).map(|&s| s.to_owned()),
            })
        } else {
            Value::String(s.to_owned())
        }
    }

    pub fn get_quantity(&self) -> Option<&Quantity> {
        match self {
            Value::Quantity(quantity) => Some(quantity),
            _ => None,
        }
    }

    pub fn get_string(&self) -> Option<String> {
        match self {
            Value::String(string) => Some(string.to_owned()),
            _ => None,
        }
    }
}

pub struct Field {
    pub position: Position,
    pub value: Value,
}

fn split_row(row: String, config: &Delimiters, row_num: usize) -> Vec<Field> {
    if let Some(row_delimiter) = config.field_delimiter.as_ref() {
        row.split(*row_delimiter)
            .enumerate()
            .map(|(column, field)| Field {
                position: Position {
                    row: row_num,
                    col: column,
                },
                value: Value::from_str(field, &config.decimal_separator),
            })
            .collect()
    } else {
        let field = Field {
            position: Position {
                row: row_num,
                col: 0,
            },
            value: Value::from_str(row.as_str(), &config.decimal_separator),
        };
        vec![field]
    }
}

pub fn split_to_fields<R: Read + Seek>(mut input: R, config: &Delimiters) -> Vec<Field> {
    let delimiters = match config.is_empty() {
        false => Cow::Borrowed(config),
        true => Cow::Owned(guess_format_from_reader(&mut input)),
    };
    debug!("Final delimiters: {:?}", delimiters);
    let input = BufReader::new(input);
    input
        .lines()
        .filter_map(|l| l.ok())
        .enumerate()
        .flat_map(|(row_num, row_value)| split_row(row_value, &delimiters, row_num))
        .collect()
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

fn compare_fields(nominal: Field, actual: Field, config: &CSVCompareConfig) -> Vec<DiffType> {
    // float quantity compare
    if let Some((actual_float, nominal_float)) = both_quantity(&actual.value, &nominal.value) {
        config
            .comparison_modes
            .iter()
            .filter_map(|cm| {
                if !cm.in_tolerance(nominal_float, actual_float) {
                    Some(DiffType::OutOfTolerance {
                        nominal: nominal_float.clone(),
                        actual: actual_float.clone(),
                        mode: *cm,
                        position: nominal.position,
                    })
                } else {
                    None
                }
            })
            .collect()
    } else if let Some((actual_string, nominal_string)) = both_string(&actual.value, &nominal.value)
    {
        if let Some(exclude_regex) = config.exclude_field_regex.as_deref() {
            let regex = Regex::new(exclude_regex).expect("Specified exclusion regex invalid!");
            if regex.is_match(nominal_string.as_str()) {
                return Vec::new();
            }
        }
        if nominal_string != actual_string {
            vec![DiffType::UnequalStrings {
                position: nominal.position,
                nominal: nominal_string,
                actual: actual_string,
            }]
        } else {
            Vec::new()
        }
    } else {
        vec![DiffType::DifferentValueTypes {
            actual: actual.value,
            nominal: nominal.value,
            position: nominal.position,
        }]
    }
}

fn get_diffs_readers<R: Read + Seek>(
    nominal: R,
    actual: R,
    config: &CSVCompareConfig,
) -> Vec<DiffType> {
    let nominal_fields = split_to_fields(nominal, &config.delimiters);
    let actual_fields = split_to_fields(actual, &config.delimiters);
    nominal_fields
        .into_iter()
        .zip(actual_fields.into_iter())
        .flat_map(|(nominal, actual)| compare_fields(nominal, actual, config))
        .collect()
}

pub fn compare_paths(
    nominal: impl AsRef<Path>,
    actual: impl AsRef<Path>,
    config: &CSVCompareConfig,
    rule_name: &str,
) -> report::FileCompareResult {
    let nominal_file = File::open(nominal.as_ref()).expect("Could not open nominal file");
    let actual_file = File::open(actual.as_ref()).expect("Could not open nominal file");

    let result = get_diffs_readers(&nominal_file, &actual_file, config);
    result.iter().for_each(|error| {
        error!("{}", &error);
    });

    report::write_csv_detail(
        nominal.as_ref(),
        actual.as_ref(),
        &result,
        rule_name,
        &config.delimiters,
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
    use std::fs::File;
    use test_log::test;

    #[test]
    fn identity_comparison_is_empty() {
        let config = CSVCompareConfig {
            exclude_field_regex: None,
            comparison_modes: vec![Mode::Absolute(0.0), Mode::Relative(0.0)],
            delimiters: Delimiters::default(),
        };

        let actual = File::open("tests/csv/data/Annotations.csv").unwrap();
        let nominal = File::open("tests/csv/data/Annotations.csv").unwrap();

        let diff = get_diffs_readers(nominal, actual, &config);
        assert!(diff.is_empty());
    }

    #[test]
    fn different_type_search_only() {
        let config = CSVCompareConfig {
            exclude_field_regex: Some(r"Surface".to_owned()),
            comparison_modes: vec![],
            delimiters: Delimiters {
                decimal_separator: Some('.'),
                field_delimiter: Some(','),
            },
        };

        let actual = File::open("tests/csv/data/DeviationHistogram.csv").unwrap();
        let nominal = File::open("tests/csv/data/DeviationHistogram_diff.csv").unwrap();

        let diff = get_diffs_readers(nominal, actual, &config);
        assert_eq!(diff.len(), 1);
        let first_diff = diff.first().unwrap();
        if let DiffType::DifferentValueTypes {
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
            exclude_field_regex: Some(r"Surface".to_owned()),
            comparison_modes: vec![Mode::Absolute(0.5)],
            delimiters: Delimiters {
                decimal_separator: Some('.'),
                field_delimiter: Some(','),
            },
        };

        let actual = File::open("tests/csv/data/DeviationHistogram.csv").unwrap();
        let nominal = File::open("tests/csv/data/DeviationHistogram_diff.csv").unwrap();

        let diff = get_diffs_readers(nominal, actual, &config);
        // the different value type is still there, but we have 2 diffs over 0.5
        assert_eq!(diff.len(), 3);
    }

    #[test]
    fn different_formattings() {
        let config = CSVCompareConfig {
            exclude_field_regex: None,
            comparison_modes: vec![Mode::Absolute(0.5)],
            delimiters: Delimiters::default(),
        };

        let actual = File::open(
            "tests/integ/data/display_of_status_message_in_cm_tables/actual/Volume1.csv",
        )
        .unwrap();
        let nominal = File::open(
            "tests/integ/data/display_of_status_message_in_cm_tables/expected/Volume1.csv",
        )
        .unwrap();

        let diff = get_diffs_readers(nominal, actual, &config);
        // the different value type is still there, but we have 2 diffs over 0.5
        assert_eq!(diff.len(), 0);
    }

    #[test]
    fn numerics_test_relative() {
        let config = CSVCompareConfig {
            exclude_field_regex: Some(r"Surface".to_owned()),
            comparison_modes: vec![Mode::Relative(0.1)],
            delimiters: Delimiters {
                decimal_separator: Some('.'),
                field_delimiter: Some(','),
            },
        };

        let actual = File::open("tests/csv/data/DeviationHistogram.csv").unwrap();
        let nominal = File::open("tests/csv/data/DeviationHistogram_diff.csv").unwrap();

        let diff = get_diffs_readers(nominal, actual, &config);
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
}
