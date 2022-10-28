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
    pattern_include: "**/export_*.csv"
    pattern_exclude: "**/export_1337.csv"
    CSV:
      comparison_modes:
        - Relative: 0.1
        - Absolute: 1.0
```
It creates a new rule named rule including all files matching "export_*.csv" in all subfolders but exclude "export_1337.csv".
String cells will be checked for perfect identity, numbers (including numbers with units) will be checked for a relative deviation smaller than `0.1`
AND absolute deviation smaller than `1.0`.

__Comparison rules__
- Relative means validity is checked like: `|nominal - actual| / |nominal| < tolerance`
- Absolute means validity is checked like: `|nominal - actual| < tolerance`
- "nan" and "nan" is valid
- `0` difference with `0` nominal value is valid

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
See the following example with all optional parameters set:
```yaml
rules:
  - name: "All options"
    pattern_include: "**/*.csv"
    pattern_exclude: "**/ignored.csv"
    CSV:
      decimal_separator: '.'
      field_delimiter:  ';'
      comparison_modes:
        - Absolute: 1.0
        - Relative: 0.1
      exclude_field_regex: "Excluded"
```

#### Image comparison
Image comparison is done using the `image compare` crate's hybrid comparison which does MSSIM on the luma and RMS on the color information.
Only a threshold can be specified:
```yaml
rules:
  - name: "JPG comparison"
    pattern_include: "**/*.jpg"
    # exclude can of course also be specified!
    Image:
      threshold: 0.9
```

#### Plain text comparison
For plain text comparison the file is read and compared line by line. For each line the normalized Damerau-Levenshtein distance from the `strsim` 
crate is used. You can ignore single lines which you know are different by specifying an arbitrary number of ignored lines:

```yaml
  - name: "HTML-Compare strict"
    pattern_exclude: "**/*_changed.html"
    pattern_include: "**/*.html"
    PlainText:
      threshold: 1.0
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
    pattern_exclude: "**/*.bin"
    Hash:
      function: Sha256
```


## Changelog

### 0.1.2:
- Add SHA-256 comparison mode
- Fix BOM on windows for CSV comparison

### 0.1.1:
 - Better error message on folder not found
 - Better test coverage
 - Fix colors on windows terminal
 - Extend CI to windows and mac
