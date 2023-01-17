use super::Error;
use crate::csv::value::Value;
use crate::csv::Delimiters;
use itertools::Itertools;
use regex::Regex;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek};
use tracing::{debug, info, warn};

fn guess_format_from_line(
    line: &str,
    field_separator_hint: Option<char>,
) -> Result<(Option<char>, Option<char>), Error> {
    let mut field_separator = field_separator_hint;

    if field_separator.is_none() {
        if line.find(';').is_some() {
            field_separator = Some(';');
        } else {
            let field_sep_regex = Regex::new(r"\w([,|])[\W\w]")?;
            let capture = field_sep_regex.captures_iter(line).next();
            if let Some(cap) = capture {
                field_separator = Some(cap[1].chars().next().ok_or_else(|| {
                    Error::InvalidAccess(format!(
                        "Could not capture field separator for guessing from '{}'",
                        line
                    ))
                })?);
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
    let decimal_separator_regex = Regex::new(decimal_separator_regex_string.as_str())?;
    let mut separators: HashMap<char, usize> = HashMap::new();

    for capture in decimal_separator_regex.captures_iter(line) {
        let sep = capture[1].chars().next().ok_or_else(|| {
            Error::InvalidAccess(format!(
                "Could not capture decimal separator for guessing from '{}'",
                line
            ))
        })?;
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

    Ok((field_separator, decimal_separator))
}

pub(crate) fn guess_format_from_reader<R: Read + Seek>(
    mut input: &mut R,
) -> Result<Delimiters, Error> {
    let mut format = (None, None);

    let bufreader = BufReader::new(&mut input);
    debug!("Guessing format from reader...");
    for line in bufreader.lines().filter_map(|l| l.ok()) {
        debug!("Guessing format from line: '{}'", line.as_str());
        format = guess_format_from_line(line.as_str(), format.0)?;
        debug!("Current format: {:?}", format);
        if format.0.is_some() && format.1.is_some() {
            break;
        }
    }

    input.rewind()?;

    if format.0.is_none() {
        warn!("Could not guess field delimiter, setting to default");
        format.0 = Delimiters::default().field_delimiter;
    }

    let delim = Delimiters {
        field_delimiter: format.0,
        decimal_separator: format.1,
    };
    info!(
        "Inferring of csv delimiters resulted in decimal separators: '{:?}', field delimiter: '{:?}'",
        delim.decimal_separator, delim.field_delimiter
    );
    Ok(delim)
}

#[derive(PartialEq, Eq, Debug)]
pub enum Token<'a> {
    Field(&'a str),
    LineBreak,
}

#[derive(PartialEq, Eq, Debug)]
enum SpecialCharacter {
    NewLine(usize),
    Quote(usize),
    Tick(usize),
    FieldStop(usize),
}

impl SpecialCharacter {
    pub fn get_position(&self) -> usize {
        match self {
            SpecialCharacter::NewLine(pos) => *pos,
            SpecialCharacter::Quote(pos) => *pos,
            SpecialCharacter::Tick(pos) => *pos,
            SpecialCharacter::FieldStop(pos) => *pos,
        }
    }
}

fn find_next_char_unescaped(string: &str, pat: char) -> Option<usize> {
    let pos = string.find(pat);
    if let Some(pos) = pos {
        if pos > 0 && &string[pos - 1..pos] == "\\" {
            let remainder = &string[pos + 1..];
            return find_next_char_unescaped(remainder, pat).map(|ipos| ipos + pos + 1);
        }
        Some(pos)
    } else {
        None
    }
}

fn find_next_quote(string: &str) -> Option<SpecialCharacter> {
    find_next_char_unescaped(string, '"').map(SpecialCharacter::Quote)
}

fn find_next_tick(string: &str) -> Option<SpecialCharacter> {
    find_next_char_unescaped(string, '\'').map(SpecialCharacter::Tick)
}

fn find_next_new_line(string: &str) -> Option<SpecialCharacter> {
    find_next_char_unescaped(string, '\n').map(SpecialCharacter::NewLine)
}

fn find_next_field_stop(string: &str, field_sep: char) -> Option<SpecialCharacter> {
    find_next_char_unescaped(string, field_sep).map(SpecialCharacter::FieldStop)
}

fn find_next_special_char(string: &str, field_sep: char) -> Option<SpecialCharacter> {
    let chars = [
        find_next_quote(string),
        find_next_tick(string),
        find_next_new_line(string),
        find_next_field_stop(string, field_sep),
    ];
    chars
        .into_iter()
        .flatten()
        .sorted_by(|a, b| a.get_position().cmp(&b.get_position()))
        .next()
}

pub(crate) struct Tokenizer<R: Read + Seek> {
    reader: R,
    delimiters: Delimiters,
    line_buffer: Vec<Vec<Value>>,
}

fn generate_tokens(input: &str, field_sep: char) -> Result<Vec<Token>, Error> {
    let mut tokens = Vec::new();
    let mut pos = 0;
    loop {
        let remainder = &input[pos..];
        if let Some(special_char) = find_next_special_char(remainder, field_sep) {
            let mut end_pos = special_char.get_position();
            match special_char {
                SpecialCharacter::FieldStop(_) => {
                    tokens.push(Token::Field(&remainder[..end_pos]));
                }
                SpecialCharacter::NewLine(_) => {
                    let field_value = &remainder[..end_pos].trim();
                    if !field_value.is_empty() {
                        tokens.push(Token::Field(field_value));
                    }
                    tokens.push(Token::LineBreak);
                }
                SpecialCharacter::Quote(_) => {
                    let (token, literal_end_pos) =
                        parse_literal(field_sep, remainder, find_next_quote)?;
                    end_pos += literal_end_pos;
                    tokens.push(token);
                }
                SpecialCharacter::Tick(_) => {
                    let (token, literal_end_pos) =
                        parse_literal(field_sep, remainder, find_next_tick)?;
                    end_pos += literal_end_pos;
                    tokens.push(token);
                }
            };
            pos += end_pos + 1;
        } else {
            break;
        }
    }
    if pos < input.len() {
        tokens.push(Token::Field(&input[pos..]));
    }
    Ok(tokens)
}

fn parse_literal<N: Fn(&str) -> Option<SpecialCharacter>>(
    field_sep: char,
    remainder: &str,
    literal_stop_finder: N,
) -> Result<(Token, usize), Error> {
    let after_first_quote = &remainder[1..];
    let quote_end = literal_stop_finder(after_first_quote).ok_or(Error::UnterminatedLiteral)?;
    let after_quote = quote_end.get_position() + 1;
    let inner_remainder = &remainder[after_quote..];
    let field_end = find_next_field_stop(inner_remainder, field_sep)
        .map(|sc| sc.get_position())
        .unwrap_or(inner_remainder.len());
    let line_end = find_next_new_line(inner_remainder)
        .map(|sc| sc.get_position())
        .unwrap_or(inner_remainder.len());
    if line_end < field_end {
        let token = Token::Field(&remainder[..after_quote]);
        Ok((token, after_quote))
    } else {
        let token = Token::Field(&remainder[..after_quote + field_end]);
        Ok((token, after_quote + field_end))
    }
}

impl<R: Read + Seek> Tokenizer<R> {
    pub fn new_guess_format(mut reader: R) -> Result<Self, Error> {
        guess_format_from_reader(&mut reader).map(|delimiters| Tokenizer {
            reader,
            delimiters,
            line_buffer: Vec::new(),
        })
    }

    pub fn new(reader: R, delimiters: Delimiters) -> Option<Self> {
        delimiters.field_delimiter?;
        Some(Tokenizer {
            reader,
            delimiters,
            line_buffer: Vec::new(),
        })
    }

    pub fn generate_tokens(&mut self) -> Result<(), Error> {
        info!(
            "Generating tokens with field delimiter: {:?}",
            self.delimiters.field_delimiter
        );

        let mut string_buffer = String::new();
        self.reader.read_to_string(&mut string_buffer)?;
        let string_buffer = string_buffer.trim_start_matches('\u{feff}');
        let string_buffer = string_buffer.replace('\r', "");
        let field_sep = self.delimiters.field_delimiter.unwrap_or(',');
        let tokens = generate_tokens(string_buffer.as_str(), field_sep)?;
        let mut buffer = Vec::new();
        buffer.push(Vec::new());
        for token in tokens.into_iter() {
            match token {
                Token::Field(input_str) => {
                    if let Some(current_line) = buffer.last_mut() {
                        current_line.push(Value::from_str(
                            input_str,
                            &self.delimiters.decimal_separator,
                        ));
                    }
                }
                Token::LineBreak => buffer.push(Vec::new()),
            }
        }
        'RemoveEmpty: loop {
            if let Some(back) = buffer.last() {
                if back.is_empty() {
                    buffer.pop();
                } else {
                    break 'RemoveEmpty;
                }
            }
        }
        self.line_buffer = buffer;
        Ok(())
    }

    pub(crate) fn into_lines_iter(self) -> impl Iterator<Item = Vec<Value>> {
        self.line_buffer.into_iter()
    }
}

#[cfg(test)]
mod tokenizer_tests {
    use super::*;
    use std::fs::File;
    use std::io::Cursor;

    #[test]
    fn unescaped() {
        let str = "...\\,...,";
        let next = find_next_char_unescaped(str, ',').unwrap();
        assert_eq!(next, 8);
    }

    #[test]
    fn next_special_char_finds_first_quote() {
        let str = ".....\"..',.";
        let next = find_next_special_char(str, ',').unwrap();
        assert_eq!(next, SpecialCharacter::Quote(5));
    }

    #[test]
    fn next_special_char_finds_first_unescaped_quote() {
        let str = "..\\\".\"..',.";
        let next = find_next_special_char(str, ',').unwrap();
        assert_eq!(next, SpecialCharacter::Quote(5));
    }

    #[test]
    fn tokenization_simple() {
        let str = "bla,blubb,2.0";
        let mut tokens = generate_tokens(str, ',').unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens.pop().unwrap(), Token::Field("2.0"));
        assert_eq!(tokens.pop().unwrap(), Token::Field("blubb"));
        assert_eq!(tokens.pop().unwrap(), Token::Field("bla"));
    }

    #[test]
    fn tokenization_with_literals() {
        let str = "bla,\"bla,bla\",2.0";
        let mut tokens = generate_tokens(str, ',').unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens.pop().unwrap(), Token::Field("2.0"));
        assert_eq!(tokens.pop().unwrap(), Token::Field("\"bla,bla\""));
        assert_eq!(tokens.pop().unwrap(), Token::Field("bla"));
    }

    #[test]
    fn tokenization_with_multi_line_literals() {
        let str = "bla,\"bla\nbla\",2.0";
        let mut tokens = generate_tokens(str, ',').unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens.pop().unwrap(), Token::Field("2.0"));
        assert_eq!(tokens.pop().unwrap(), Token::Field("\"bla\nbla\""));
        assert_eq!(tokens.pop().unwrap(), Token::Field("bla"));
    }

    #[test]
    fn tokenize_to_values_cuts_last_nl() {
        let str = "bla\n2.0\n\n";
        let mut parser = Tokenizer::new_guess_format(Cursor::new(str)).unwrap();
        parser.generate_tokens().unwrap();
        let lines: Vec<_> = parser.into_lines_iter().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn tokenization_with_multi_line_with_escape_break_literals() {
        let str = "\\\"bla,\"'bla\\\"\nbla'\",2.0";
        let mut tokens = generate_tokens(str, ',').unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens.pop().unwrap(), Token::Field("2.0"));
        assert_eq!(tokens.pop().unwrap(), Token::Field("\"'bla\\\"\nbla'\""));
        assert_eq!(tokens.pop().unwrap(), Token::Field("\\\"bla"));
    }

    #[test]
    fn tokenization_new_lines() {
        let str = "bla,bla\nbla,bla";
        let mut tokens = generate_tokens(str, ',').unwrap();
        assert_eq!(tokens.len(), 5);
        assert_eq!(tokens.pop().unwrap(), Token::Field("bla"));
        assert_eq!(tokens.pop().unwrap(), Token::Field("bla"));
        assert_eq!(tokens.pop().unwrap(), Token::LineBreak);
        assert_eq!(tokens.pop().unwrap(), Token::Field("bla"));
        assert_eq!(tokens.pop().unwrap(), Token::Field("bla"));
    }

    #[test]
    fn tokenizer_smoke() {
        let actual = File::open(
            "tests/integ/data/display_of_status_message_in_cm_tables/actual/Volume1.csv",
        )
        .unwrap();
        let mut parser = Tokenizer::new_guess_format(actual).unwrap();
        parser.generate_tokens().unwrap();

        let nominal = File::open(
            "tests/integ/data/display_of_status_message_in_cm_tables/expected/Volume1.csv",
        )
        .unwrap();
        let mut parser = Tokenizer::new_guess_format(nominal).unwrap();
        parser.generate_tokens().unwrap();
    }

    #[test]
    fn tokenizer_semicolon_test() {
        let nominal =
            File::open("tests/csv/data/easy_pore_export_annoration_table_result.csv").unwrap();
        let mut parser = Tokenizer::new_guess_format(nominal).unwrap();
        parser.generate_tokens().unwrap();
        for line in parser.into_lines_iter() {
            assert_eq!(line.len(), 5);
        }
    }
}

#[cfg(test)]
mod format_guessing_tests {
    use super::*;
    use std::fs::File;
    #[test]
    fn format_detection_basics() {
        let format = guess_format_from_line(
            "-0.969654597744788,-0.215275534510198,0.115869999295192,7.04555232210696",
            None,
        )
        .unwrap();
        assert_eq!(format, (Some(','), Some('.')));

        let format = guess_format_from_line(
            "-0.969654597744788;-0.215275534510198;0.115869999295192;7.04555232210696",
            None,
        )
        .unwrap();
        assert_eq!(format, (Some(';'), Some('.')));

        let format = guess_format_from_line(
            "-0.969654597744788,-0.215275534510198,0.115869999295192,7.04555232210696",
            None,
        )
        .unwrap();
        assert_eq!(format, (Some(','), Some('.')));
    }

    #[test]
    fn format_detection_from_file() {
        let format =
            guess_format_from_reader(&mut File::open("tests/csv/data/Annotations.csv").unwrap())
                .unwrap();
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
        )
        .unwrap();
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
        )
        .unwrap();
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
        )
        .unwrap();
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
            guess_format_from_reader(&mut File::open("tests/csv/data/no_field_sep.csv").unwrap())
                .unwrap();
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
        )
        .unwrap();
        assert_eq!(
            format,
            Delimiters {
                field_delimiter: Some(';'),
                decimal_separator: Some(',')
            }
        );
    }
    #[test]
    fn format_detection_from_file_semicolon_separators() {
        let format =
            guess_format_from_reader(&mut File::open("tests/csv/data/Components.csv").unwrap())
                .unwrap();
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
        )
        .unwrap();
        assert_eq!(
            format,
            Delimiters {
                field_delimiter: Some(','),
                decimal_separator: Some('.')
            }
        );
    }
}
