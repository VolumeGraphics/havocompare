# Havocompare - a folder comparison utility
[![Crates.io](https://img.shields.io/crates/d/havocompare?style=flat)](https://crates.io/crates/havocompare)
[![Documentation](https://docs.rs/havocompare/badge.svg)](https://docs.rs/havocompare)
![CI](https://github.com/VolumeGraphics/havocompare/actions/workflows/rust.yml/badge.svg?branch=main "CI")
[![Coverage Status](https://coveralls.io/repos/github/VolumeGraphics/havocompare/badge.svg?branch=main)](https://coveralls.io/github/VolumeGraphics/havocompare?branch=main)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat)](LICENSE-MIT)

## Quickstart

### 0. Install havocompare
You have rust? cool! try:
`cargo install havocompare`

You just want a binary:
Check our binary downloads on github-pages

### 1. Create a config file
Havocompare was developed with a few design goals in mind. We wanted a human readable and easily composable configuration file format.
After a few tries we ended up with the current format, which is a list of rules inside a yaml file. 
See the following example `config.yaml`:
```yaml
rules:
  - name: "Numerical results csv"
    # you can have multiple includes and excludes
    pattern_include: 
      - "**/export_*.csv"
    # excludes are optional
    pattern_exclude: 
      - "**/export_1337.csv"
    CSV:
      comparison_modes:
        - Relative: 0.1
        - Absolute: 1.0
```
It creates a new rule named rule including all files matching "export_*.csv" in all sub-folders but exclude "export_1337.csv".
String cells will be checked for perfect identity, numbers (including numbers with units) will be checked for a relative deviation smaller than `0.1`
AND absolute deviation smaller than `1.0`.

__Comparison rules__
- Relative means validity is checked like: `|nominal - actual| / |nominal| < tolerance`
- Absolute means validity is checked like: `|nominal - actual| < tolerance`
- "nan" and "nan" is equal
- `0` difference with `0` nominal value is valid for any relative difference

### 2. Run the compare

Running the comparison is super easy, just supply nominal, actual and the config:
`./havocompare compare nominal_dir actual_dir config.yaml`
The report of the comparison will be written inside the `./report` folder.  Differences will also be printed to the terminal.
Furthermore, if differences are found, the return code will be `1`, if no differences are found, it will be `0` making integration of
havocompare into a CI system rather easy.

## Details on the config
### Validation Scheme
Writing a valid configuration file can be error prone without auto completion. We suggest using json schema to validate your yaml
and even enable auto completion in IDEs like pycharm. To generate the schema you can call:
`./havocompare schema > config_scheme.json` and import the resulting scheme into your IDE.

### Comparison options
#### CSV
The `comparison_modes` option is required and of type 'list'. It can comprise either a relative numerical ('Relative') maximum deviation or a maximum 
deviation ('Absolute'). 
You can specify the decimal separator and the field separator. If you don't specify, havocompare will try to guess it from each csv file.
Note: If delimiters are not specified, even different delimiters between nominal and actual are accepted as long as all deviations are in bounds.
To ignore specific cells, you can specify an exclusion regex.

The preprocessing steps are done after the file is parsed using the given delimiters (or guessing) but before anything else. Processing order is as written in the list.
In the below example, headers will be extracted from the csv-input file, then a column with the title "Columnn to delete" will be deleted.
If any of the preprocessing steps fail, havocompare will exit with an error immediately so use them carefully.

See the following example with all optional parameters set:
```yaml
rules:
  - name: "CSV - Demo all options"
    # what files to include - use as many as make sense to reduce duplication in your rules
    pattern_include: 
      - "**/*.csv"
    # optional: of all included files, remove the ones matching any exclude pattern
    pattern_exclude: 
      - "**/ignored.csv"
    CSV:
      # delimiters are optional, if not given, they will be auto-detected.
      # auto-detection allows different delimiters for nominal and actual
      decimal_separator: '.'
      field_delimiter:  ';'
      # can have Absolute or Relative or both
      comparison_modes:
        - Absolute: 1.0
        - Relative: 0.1
      # optional: exclude fields matching the regex from comparison
      exclude_field_regex: "Excluded"
      # optional: preprocessing of the csv files
      preprocessing:
        # extracts the headers to the header-fields, makes reports more legible and allows for further processing "ByName".
        # While it may fail, there's no penalty for it, as long as you don't rely on it.
        - ExtractHeaders
        # Sort the table by column 0, beware that the column must only contain numbers / quantities
        - SortByColumnNumber: 0
        # Delete a column by name, needs `ExtractHeaders` first - delete sets all values to 'DELETED'
        - DeleteColumnByName: "Column to delete"
        - DeleteColumnByNumber: 1
        # Sorts are stable, so a second sort will keep the first sort as sub-order.
        - SortByColumnName: "Sort by column name blabla"
        # Deletes the first row by setting all values to 'DELETED' - meaning that numbering stays constant 
        - DeleteRowByNumber: 0
        # Deletes rows having any element matching the given regex (may delete different lines in nom / act!
        - DeleteRowByRegex: "Vertex_Count"
```

#### Image comparison
Image comparison is done using the `image compare` crate's hybrid comparison which does MSSIM on the luma and RMS on the color information.
Only a threshold can be specified:
```yaml
rules:
  - name: "JPG comparison"
    pattern_include: 
      - "**/*.jpg"
    # exclude can of course also be specified!
    Image:
      # threshold is between 0.0 for total difference, 0.5 for very dissimilar and 1.0 for perfect mach
      # Usually you want to test with values between 0.90 and 0.97
      threshold: 0.9
```

#### Plain text comparison
For plain text comparison the file is read and compared line by line. For each line the normalized Damerau-Levenshtein distance from the `strsim` 
crate is used. You can ignore single lines which you know are different by specifying an arbitrary number of ignored lines:

```yaml
  - name: "HTML-Compare strict"
    pattern_exclude: 
      - "**/*_changed.html"
    pattern_include: 
      - "**/*.html"
    PlainText:
      # Normalized Damerau-Levenshtein distance
      threshold: 1.0
      # All lines matching any regex below will be ignored
      ignore_lines:
        - "stylesheet"
        - "next_ignore"
        - "[A-Z]*[0-9]"
```

#### PDF text comparison
For PDF text comparison the text will be extracted and written to temporary files. The files will then be compared using the Plain text comparison:

```yaml
  - name: "PDF-Text-Compare"
    pattern_exclude: 
      - "**/*_changed.pdf"
    pattern_include: 
      - "**/*.pdf"
    PDFText:
      # Normalized Damerau-Levenshtein distance
      threshold: 1.0
      # All lines matching any regex below will be ignored
      ignore_lines:
        - "stylesheet"
        - "next_ignore"
        - "[A-Z]*[0-9]"
```


#### Hash comparison
For binary files which cannot otherwise be checked we can also do a simple hash comparison.
Currently we only support SHA-256 but more checks can be added easily.

```yaml
  - name: "Hash comparison strict"
    pattern_exclude: 
      - "**/*.bin"
    Hash:
      # Currently we only have Sha256
      function: Sha256
```


## Changelog

### 0.2.4
- add check for row lines of both compared csv files, and throw error if they are unequal
- fix floating point value comparison of non-displayable diff values

### 0.2.3
- bump pdf-extract crate to 0.6.4 to fix "'attempted to leave type `linked_hash_map::Node<alloc::vec::Vec<u8>, object::Object>` uninitialized"

### 0.2.2
- Include files which has error and can't be compared to the report
- Fixed a bug which caused the program exited early out of rules-loop, and not processing all

### 0.2.0
- Deletion of columns will no longer really delete them but replace every value with "DELETED"
- Expose config struct to library API
- Fixed a bug regarding wrong handling of multiple empty lines
- Reworked CSV reporting to have an interleaved and more compact view 
  - Display the relative path of compared files instead of file name in the report index.html
  - Made header-extraction fallible but uncritical - can now always be enabled
- Wrote a completely new csv parser:
  - Respects escaping with '\'
  - Allows string-literals containing unescaped field separators (field1, "field2, but as literal", field3)
  - Allows multi-line string literals with quotes
- CSVs with non-rectangular format will now fail

### 0.1.4
- Add multiple includes and excludes - warning, this will break yamls from 0.1.3 and earlier
- Remove all `unwrap` and `expect` in the library code in favor of correct error propagation
- Add preprocessing options for CSV files
- Refined readme.md
- fix unique key creation in the report generation
- Add PDF-Text compare

### 0.1.3:
- Add optional cli argument to configure the folder to store the report

### 0.1.2:
- Add SHA-256 comparison mode
- Fix BOM on windows for CSV comparison

### 0.1.1:
 - Better error message on folder not found
 - Better test coverage
 - Fix colors on windows terminal
 - Extend CI to windows and mac
