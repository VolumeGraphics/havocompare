use super::Error;
use crate::csv::Delimiters;
use itertools::Itertools;
use regex::Regex;
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Seek};
use tracing::{debug, error, info};

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
        error!("Could not guess field delimiter, bailing out.");
        return Err(Error::FormatGuessingFailure);
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
pub enum Token {
    Field(String),
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
            println!("Running remainder search: {}", remainder);
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
    buffer: VecDeque<Token>,
}

fn generate_tokens(input: &str, field_sep: char) -> Result<VecDeque<Token>, Error> {
    let mut tokens = VecDeque::new();
    let mut pos = 0;
    loop {
        let remainder = &input[pos..];
        if let Some(special_char) = find_next_special_char(remainder, field_sep) {
            let mut end_pos = special_char.get_position();
            match special_char {
                SpecialCharacter::FieldStop(_) => {
                    tokens.push_back(Token::Field(remainder[..end_pos].to_string()));
                }
                SpecialCharacter::NewLine(_) => {
                    tokens.push_back(Token::Field(remainder[..end_pos].to_string()));
                    tokens.push_back(Token::LineBreak);
                }
                SpecialCharacter::Quote(_) => {
                    let after_first_quote = &remainder[1..];
                    let quote_end =
                        find_next_quote(after_first_quote).ok_or(Error::UnterminatedLiteral)?;
                    let after_quote = quote_end.get_position() + 1;
                    let inner_remainder = &remainder[after_quote..];
                    let field_end = find_next_field_stop(inner_remainder, field_sep)
                        .map(|sc| sc.get_position())
                        .unwrap_or(inner_remainder.len());
                    tokens.push_back(Token::Field(
                        remainder[..after_quote + field_end].to_string(),
                    ));
                    end_pos += after_quote + field_end;
                }
                SpecialCharacter::Tick(_) => {
                    let after_first_quote = &remainder[1..];
                    let quote_end =
                        find_next_tick(after_first_quote).ok_or(Error::UnterminatedLiteral)?;
                    let after_quote = quote_end.get_position() + 1;
                    let inner_remainder = &remainder[after_quote..];
                    let field_end = find_next_field_stop(inner_remainder, field_sep)
                        .map(|sc| sc.get_position())
                        .unwrap_or(inner_remainder.len());
                    tokens.push_back(Token::Field(
                        remainder[..after_quote + field_end].to_string(),
                    ));
                    end_pos += after_quote + field_end;
                }
            };
            pos += end_pos + 1;
        } else {
            break;
        }
    }
    if pos < input.len() {
        tokens.push_back(Token::Field(input[pos..].to_string()));
    }
    Ok(tokens)
}

impl<R: Read + Seek> Tokenizer<R> {
    pub fn new_guess_format(mut reader: R) -> Option<Self> {
        guess_format_from_reader(&mut reader)
            .ok()
            .map(|delimiters| Tokenizer {
                reader,
                delimiters,
                buffer: VecDeque::new(),
            })
    }

    pub fn new(reader: R, delimiters: Delimiters) -> Option<Self> {
        delimiters.field_delimiter?;
        Some(Tokenizer {
            reader,
            delimiters,
            buffer: VecDeque::new(),
        })
    }

    fn generate_tokens(&mut self) -> Result<(), Error> {
        let mut string_buffer = String::new();
        self.reader.read_to_string(&mut string_buffer)?;
        let string_buffer = string_buffer.trim_start_matches('\u{feff}');
        let string_buffer = string_buffer.replace('\r', "");
        let field_sep = self.delimiters.field_delimiter.unwrap_or(',');
        self.buffer = generate_tokens(string_buffer.as_str(), field_sep)?;
        Ok(())
    }
}

#[cfg(test)]
mod tokenizer_tests {
    use super::*;

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
        assert_eq!(tokens.pop_front().unwrap(), Token::Field("bla".to_owned()));
        assert_eq!(
            tokens.pop_front().unwrap(),
            Token::Field("blubb".to_owned())
        );
        assert_eq!(tokens.pop_front().unwrap(), Token::Field("2.0".to_owned()));
    }

    #[test]
    fn tokenization_with_literals() {
        let str = "bla,\"bla,bla\",2.0";
        let mut tokens = generate_tokens(str, ',').unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens.pop_front().unwrap(), Token::Field("bla".to_owned()));
        assert_eq!(
            tokens.pop_front().unwrap(),
            Token::Field("\"bla,bla\"".to_owned())
        );
        assert_eq!(tokens.pop_front().unwrap(), Token::Field("2.0".to_owned()));
    }

    #[test]
    fn tokenization_with_multi_line_literals() {
        let str = "bla,\"bla\nbla\",2.0";
        let mut tokens = generate_tokens(str, ',').unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens.pop_front().unwrap(), Token::Field("bla".to_owned()));
        assert_eq!(
            tokens.pop_front().unwrap(),
            Token::Field("\"bla\nbla\"".to_owned())
        );
        assert_eq!(tokens.pop_front().unwrap(), Token::Field("2.0".to_owned()));
    }
    #[test]
    fn tokenization_with_multi_line_with_escape_break_literals() {
        let str = "\\\"bla,\"'bla\\\"\nbla'\",2.0";
        let mut tokens = generate_tokens(str, ',').unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(
            tokens.pop_front().unwrap(),
            Token::Field("\\\"bla".to_owned())
        );
        assert_eq!(
            tokens.pop_front().unwrap(),
            Token::Field("\"'bla\\\"\nbla'\"".to_owned())
        );
        assert_eq!(tokens.pop_front().unwrap(), Token::Field("2.0".to_owned()));
    }

    #[test]
    fn tokenization_new_lines() {
        let str = "bla,bla\nbla,bla";
        let mut tokens = generate_tokens(str, ',').unwrap();
        println!("{:?}", tokens);
        assert_eq!(tokens.len(), 5);
        assert_eq!(tokens.pop_front().unwrap(), Token::Field("bla".to_owned()));
        assert_eq!(tokens.pop_front().unwrap(), Token::Field("bla".to_owned()));
        assert_eq!(tokens.pop_front().unwrap(), Token::LineBreak);
        assert_eq!(tokens.pop_front().unwrap(), Token::Field("bla".to_owned()));
        assert_eq!(tokens.pop_front().unwrap(), Token::Field("bla".to_owned()));
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
