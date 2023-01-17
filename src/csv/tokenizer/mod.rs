use super::Error;
use crate::csv::tokenizer::guess_format::guess_format_from_reader;
use crate::csv::value::Value;
use crate::csv::Delimiters;
use itertools::Itertools;
use std::cmp::Ordering;
use std::io::{Read, Seek};
use tracing::info;

mod guess_format;
const BOM: char = '\u{feff}';
const DEFAULT_FIELD_SEPARATOR: char = ',';
const ESCAPE_SEQUENCE: &str = "\\";
const QUOTE: char = '\"';
const TICK: char = '\'';
const NEW_LINE: char = '\n';
const CARRIAGE_RETURN: char = '\r';

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

impl PartialOrd<Self> for SpecialCharacter {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.get_position().partial_cmp(&other.get_position())
    }
}

impl Ord for SpecialCharacter {
    fn cmp(&self, other: &Self) -> Ordering {
        self.get_position().cmp(&other.get_position())
    }
}

fn find_next_unescaped(string: &str, pat: char) -> Option<usize> {
    let pos = string.find(pat);
    if let Some(pos) = pos {
        if pos > 0 && &string[pos - 1..pos] == ESCAPE_SEQUENCE {
            let remainder = &string[pos + 1..];
            return find_next_unescaped(remainder, pat).map(|ipos| ipos + pos + 1);
        }
        Some(pos)
    } else {
        None
    }
}

fn find_quote(string: &str) -> Option<SpecialCharacter> {
    find_next_unescaped(string, QUOTE).map(SpecialCharacter::Quote)
}

fn find_tick(string: &str) -> Option<SpecialCharacter> {
    find_next_unescaped(string, TICK).map(SpecialCharacter::Tick)
}

fn find_new_line(string: &str) -> Option<SpecialCharacter> {
    find_next_unescaped(string, NEW_LINE).map(SpecialCharacter::NewLine)
}

fn find_field_stop(string: &str, field_sep: char) -> Option<SpecialCharacter> {
    find_next_unescaped(string, field_sep).map(SpecialCharacter::FieldStop)
}

fn find_special_char(string: &str, field_sep: char) -> Option<SpecialCharacter> {
    let chars = [
        find_quote(string),
        find_tick(string),
        find_new_line(string),
        find_field_stop(string, field_sep),
    ];
    chars.into_iter().flatten().sorted().next()
}

// stronk-type for rows.. is this a good idea?
struct RowBuffer(Vec<Vec<Value>>);
impl RowBuffer {
    pub fn new() -> RowBuffer {
        RowBuffer(vec![Vec::new()])
    }

    pub fn push_field(&mut self, value: Value) {
        if let Some(current_row) = self.0.last_mut() {
            current_row.push(value);
        }
    }

    pub fn new_row(&mut self) {
        self.0.push(Vec::new());
    }

    pub fn into_iter(mut self) -> std::vec::IntoIter<Vec<Value>> {
        self.trim_end();
        self.0.into_iter()
    }

    fn trim_end(&mut self) {
        'PopEmpty: loop {
            if let Some(back) = self.0.last() {
                if back.is_empty() {
                    self.0.pop();
                } else {
                    break 'PopEmpty;
                }
            }
        }
    }
}

pub(crate) struct Parser<R: Read + Seek> {
    reader: R,
    delimiters: Delimiters,
}

fn tokenize(input: &str, field_sep: char) -> Result<Vec<Token>, Error> {
    let mut tokens = Vec::new();
    let mut pos = 0;
    loop {
        let remainder = &input[pos..];
        if let Some(special_char) = find_special_char(remainder, field_sep) {
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
                    let (token, literal_end_pos) = parse_literal(field_sep, remainder, find_quote)?;
                    end_pos += literal_end_pos;
                    tokens.push(token);
                }
                SpecialCharacter::Tick(_) => {
                    let (token, literal_end_pos) = parse_literal(field_sep, remainder, find_tick)?;
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
    let field_end = find_field_stop(inner_remainder, field_sep)
        .map(|sc| sc.get_position())
        .unwrap_or(inner_remainder.len());
    let line_end = find_new_line(inner_remainder)
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

impl<R: Read + Seek> Parser<R> {
    pub fn new_guess_format(mut reader: R) -> Result<Self, Error> {
        guess_format_from_reader(&mut reader).map(|delimiters| Parser { reader, delimiters })
    }

    pub fn new(reader: R, delimiters: Delimiters) -> Option<Self> {
        delimiters.field_delimiter?;
        Some(Parser { reader, delimiters })
    }

    pub(crate) fn parse_to_rows(&mut self) -> Result<std::vec::IntoIter<Vec<Value>>, Error> {
        info!(
            "Generating tokens with field delimiter: {:?}",
            self.delimiters.field_delimiter
        );

        let mut string_buffer = String::new();
        self.reader.read_to_string(&mut string_buffer)?;
        // remove BoM & windows line endings to linux line endings
        string_buffer.retain(|c| ![BOM, CARRIAGE_RETURN].contains(&c));

        let field_sep = self
            .delimiters
            .field_delimiter
            .unwrap_or(DEFAULT_FIELD_SEPARATOR);

        let mut buffer = RowBuffer::new();

        tokenize(string_buffer.as_str(), field_sep)?
            .into_iter()
            .for_each(|t| match t {
                Token::Field(input_str) => {
                    buffer.push_field(Value::from_str(
                        input_str,
                        &self.delimiters.decimal_separator,
                    ));
                }
                Token::LineBreak => buffer.new_row(),
            });

        Ok(buffer.into_iter())
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
        let next = find_next_unescaped(str, ',').unwrap();
        assert_eq!(next, 8);
    }

    #[test]
    fn next_special_char_finds_first_quote() {
        let str = ".....\"..',.";
        let next = find_special_char(str, ',').unwrap();
        assert_eq!(next, SpecialCharacter::Quote(5));
    }

    #[test]
    fn next_special_char_finds_first_unescaped_quote() {
        let str = "..\\\".\"..',.";
        let next = find_special_char(str, ',').unwrap();
        assert_eq!(next, SpecialCharacter::Quote(5));
    }

    #[test]
    fn tokenization_simple() {
        let str = "bla,blubb,2.0";
        let mut tokens = tokenize(str, ',').unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens.pop().unwrap(), Token::Field("2.0"));
        assert_eq!(tokens.pop().unwrap(), Token::Field("blubb"));
        assert_eq!(tokens.pop().unwrap(), Token::Field("bla"));
    }

    #[test]
    fn tokenization_with_literals() {
        let str = "bla,\"bla,bla\",2.0";
        let mut tokens = tokenize(str, ',').unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens.pop().unwrap(), Token::Field("2.0"));
        assert_eq!(tokens.pop().unwrap(), Token::Field("\"bla,bla\""));
        assert_eq!(tokens.pop().unwrap(), Token::Field("bla"));
    }

    #[test]
    fn tokenization_with_multi_line_literals() {
        let str = "bla,\"bla\nbla\",2.0";
        let mut tokens = tokenize(str, ',').unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens.pop().unwrap(), Token::Field("2.0"));
        assert_eq!(tokens.pop().unwrap(), Token::Field("\"bla\nbla\""));
        assert_eq!(tokens.pop().unwrap(), Token::Field("bla"));
    }

    #[test]
    fn tokenize_to_values_cuts_last_nl() {
        let str = "bla\n2.0\n\n";
        let mut parser = Parser::new_guess_format(Cursor::new(str)).unwrap();
        assert_eq!(parser.parse_to_rows().unwrap().len(), 2);
    }

    #[test]
    fn tokenization_with_multi_line_with_escape_break_literals() {
        let str = "\\\"bla,\"'bla\\\"\nbla'\",2.0";
        let mut tokens = tokenize(str, ',').unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens.pop().unwrap(), Token::Field("2.0"));
        assert_eq!(tokens.pop().unwrap(), Token::Field("\"'bla\\\"\nbla'\""));
        assert_eq!(tokens.pop().unwrap(), Token::Field("\\\"bla"));
    }

    #[test]
    fn tokenization_new_lines() {
        let str = "bla,bla\nbla,bla";
        let mut tokens = tokenize(str, ',').unwrap();
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
        let mut parser = Parser::new_guess_format(actual).unwrap();
        parser.parse_to_rows().unwrap();
        let nominal = File::open(
            "tests/integ/data/display_of_status_message_in_cm_tables/expected/Volume1.csv",
        )
        .unwrap();
        let mut parser = Parser::new_guess_format(nominal).unwrap();
        parser.parse_to_rows().unwrap();
    }

    #[test]
    fn tokenizer_semicolon_test() {
        let nominal =
            File::open("tests/csv/data/easy_pore_export_annoration_table_result.csv").unwrap();
        let mut parser = Parser::new_guess_format(nominal).unwrap();
        for line in parser.parse_to_rows().unwrap() {
            assert_eq!(line.len(), 5);
        }
    }
}
