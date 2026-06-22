use crate::report::{self, DiffDetail, Difference};
use regex::Regex;
use roxmltree::{Document, Node};
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;
use strsim::normalized_damerau_levenshtein;
use thiserror::Error;
use tracing::error;

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
/// XML comparison config
pub struct XMLCompareConfig {
    /// Tags that can be ignored in the comparison
    pub ignore_tags: Option<Vec<String>>,
    /// Rules for different tag types
    pub tag: Vec<TagRule>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
#[serde(tag = "type")]
pub enum TagRule {
    #[serde(rename = "numeric")]
    Numeric {
        abs: Option<Range>,
        rel: Option<Range>,
    },
    #[serde(rename = "string")]
    String { threshold: f64 },
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
pub struct Range {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("XML parse error: {0}")]
    ParseFailure(#[from] roxmltree::Error),

    #[error("Regex error: {0}")]
    RegexFailure(#[from] regex::Error),

    #[error("IO error: {0}")]
    IoFailure(#[from] std::io::Error),

    #[error("Reporting error: {0}")]
    Reporting(#[from] report::Error),
}

struct CompiledXMLConfig {
    ignore_tags: Vec<Regex>,
    numeric: Option<NumericRule>,
    string: Option<StringRule>,
}

struct NumericRule {
    abs: Option<Range>,
    rel: Option<Range>,
}

struct StringRule {
    threshold: f64,
}

impl XMLCompareConfig {
    fn compile(&self) -> Result<CompiledXMLConfig, regex::Error> {
        let ignore_tags = self
            .ignore_tags
            .as_ref()
            .map(|tags| tags.iter().map(|t| glob_to_regex(t)).collect())
            .transpose()?
            .unwrap_or_default();

        let mut numeric = None;
        let mut string = None;

        for rule in &self.tag {
            match rule {
                TagRule::Numeric { abs, rel } => {
                    numeric = Some(NumericRule {
                        abs: abs.clone(),
                        rel: rel.clone(),
                    });
                }
                TagRule::String { threshold } => {
                    string = Some(StringRule {
                        threshold: *threshold,
                    });
                }
            }
        }

        Ok(CompiledXMLConfig {
            ignore_tags,
            numeric,
            string,
        })
    }
}

fn glob_to_regex(pattern: &str) -> Result<Regex, regex::Error> {
    let escaped = regex::escape(pattern).replace("\\*", ".*");
    Regex::new(&format!("^{}$", escaped))
}

pub fn compare_files<P: AsRef<Path>>(
    nominal_path: P,
    actual_path: P,
    config: &XMLCompareConfig,
) -> Result<Difference, Error> {
    let nominal_text = std::fs::read_to_string(&nominal_path)?;
    let actual_text = std::fs::read_to_string(&actual_path)?;

    let nominal_doc = Document::parse(&nominal_text)?;
    let actual_doc = Document::parse(&actual_text)?;

    let compiled = config.compile()?;

    let mut diff = Difference::new_for_file(nominal_path, actual_path);

    compare_nodes(
        nominal_doc.root_element(),
        actual_doc.root_element(),
        &compiled,
        &mut diff,
        "/".to_string(),
    );

    Ok(diff)
}

fn compare_nodes<'a, 'i>(
    nominal: Node<'a, 'i>,
    actual: Node<'a, 'i>,
    config: &CompiledXMLConfig,
    diff: &mut Difference,
    path: String,
) {
    let tag = nominal.tag_name().name();
    let current_path = format!("{}/{}", path, tag);

    // ignore
    if config.ignore_tags.iter().any(|r| r.is_match(tag)) {
        return;
    }

    // tag mismatch
    if tag != actual.tag_name().name() {
        report_error(diff, &current_path, tag, actual.tag_name().name());
        return;
    }

    // compare text
    compare_values(nominal.text(), actual.text(), config, diff, &current_path);

    // compare attributes
    compare_attributes(nominal, actual, config, diff, &current_path);

    // collect children
    let nominal_children: Vec<_> = nominal.children().filter(|n| n.is_element()).collect();
    let actual_children: Vec<_> = actual.children().filter(|n| n.is_element()).collect();

    if nominal_children.len() != actual_children.len() {
        error!("Different number of children at {}", current_path);
        diff.error();
    }

    for (i, (n, a)) in nominal_children
        .iter()
        .zip(actual_children.iter())
        .enumerate()
    {
        compare_nodes(*n, *a, config, diff, format!("{}[{}]", current_path, i));
    }
}

fn compare_values(
    nominal: Option<&str>,
    actual: Option<&str>,
    config: &CompiledXMLConfig,
    diff: &mut Difference,
    path: &str,
) {
    let n = nominal.unwrap_or("").trim();
    let a = actual.unwrap_or("").trim();

    // numeric attempt
    if let (Ok(nv), Ok(av)) = (n.parse::<f64>(), a.parse::<f64>()) {
        if let Some(rule) = &config.numeric {
            let diff_abs = (av - nv).abs();
            let diff_rel = if nv != 0.0 { diff_abs / nv.abs() } else { 0.0 };

            let abs_ok = rule
                .abs
                .as_ref()
                .is_none_or(|r| diff_abs >= r.min && diff_abs <= r.max);
            let rel_ok = rule
                .rel
                .as_ref()
                .is_none_or(|r| diff_rel >= r.min && diff_rel <= r.max);

            if !(abs_ok || rel_ok) {
                report_value_mismatch(diff, path, n, a, diff_abs);
            }
        }
        return;
    }

    // string fallback
    if let Some(rule) = &config.string {
        let distance = normalized_damerau_levenshtein(n, a);

        if distance < rule.threshold {
            report_value_mismatch(diff, path, n, a, distance);
        }
    }
}

fn compare_attributes<'a, 'i>(
    nominal: Node<'a, 'i>,
    actual: Node<'a, 'i>,
    config: &CompiledXMLConfig,
    diff: &mut Difference,
    path: &str,
) {
    for attr in nominal.attributes() {
        match actual.attribute(attr.name()) {
            Some(av) => {
                let attr_path = format!("{}[@{}]", path, attr.name());
                compare_values(Some(attr.value()), Some(av), config, diff, &attr_path);
            }
            None => {
                error!("Missing attribute {} at {}", attr.name(), path);
                diff.error();
            }
        }
    }
}

fn report_error(diff: &mut Difference, path: &str, expected: &str, found: &str) {
    let msg = format!(
        "Tag mismatch at {}: expected '{}' found '{}'",
        path, expected, found
    );
    error!("{}", msg);
    diff.error();
}

fn report_value_mismatch(
    diff: &mut Difference,
    path: &str,
    nominal: &str,
    actual: &str,
    score: f64,
) {
    let msg = format!(
        "Mismatch at {}: expected '{}' found '{}' (score: {})",
        path, nominal, actual, score
    );

    error!("{}", msg);

    diff.push_detail(DiffDetail::Text {
        actual: actual.to_string(),
        nominal: nominal.to_string(),
        score,
        line: 0,
    });

    diff.error();
}

#[cfg(test)]
mod test {
    use super::*;

    fn write_temp_xml(name: &str, content: &str) -> String {
        let path = format!("tests/xml_{}.xml", name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn basic_config() -> XMLCompareConfig {
        XMLCompareConfig {
            ignore_tags: None,
            tag: vec![
                TagRule::Numeric {
                    abs: None,
                    rel: Some(Range {
                        min: 0.0,
                        max: 0.001,
                    }),
                },
                TagRule::String { threshold: 1.0 },
            ],
        }
    }

    // ✅ identical files
    #[test]
    fn test_identity() {
        let xml = r#"<root><value>123</value></root>"#;

        let f1 = write_temp_xml("identity_1", xml);
        let f2 = write_temp_xml("identity_2", xml);

        let result = compare_files(f1, f2, &basic_config()).unwrap();

        assert!(!result.is_error);
    }

    // ✅ numeric within tolerance
    #[test]
    fn test_numeric_within_tolerance() {
        let nominal = r#"<root><value>100.0</value></root>"#;
        let actual = r#"<root><value>100.05</value></root>"#;

        let f1 = write_temp_xml("num_ok_1", nominal);
        let f2 = write_temp_xml("num_ok_2", actual);

        let result = compare_files(f1, f2, &basic_config()).unwrap();

        assert!(!result.is_error);
    }

    // ✅ numeric outside tolerance
    #[test]
    fn test_numeric_outside_tolerance() {
        let nominal = r#"<root><value>100.0</value></root>"#;
        let actual = r#"<root><value>200.0</value></root>"#;

        let f1 = write_temp_xml("num_fail_1", nominal);
        let f2 = write_temp_xml("num_fail_2", actual);

        let result = compare_files(f1, f2, &basic_config()).unwrap();

        assert!(result.is_error);
    }

    // ✅ string difference
    #[test]
    fn test_string_difference() {
        let nominal = r#"<root><name>Hello</name></root>"#;
        let actual = r#"<root><name>World</name></root>"#;

        let f1 = write_temp_xml("str_fail_1", nominal);
        let f2 = write_temp_xml("str_fail_2", actual);

        let result = compare_files(f1, f2, &basic_config()).unwrap();

        assert!(result.is_error);
    }

    // ✅ ignore tag
    #[test]
    fn test_ignore_tag() {
        let config = XMLCompareConfig {
            ignore_tags: Some(vec!["time".to_string()]),
            ..basic_config()
        };

        let nominal = r#"<root><time>123</time></root>"#;
        let actual = r#"<root><time>999</time></root>"#;

        let f1 = write_temp_xml("ignore_1", nominal);
        let f2 = write_temp_xml("ignore_2", actual);

        let result = compare_files(f1, f2, &config).unwrap();

        assert!(!result.is_error);
    }

    // ✅ attribute numeric comparison
    #[test]
    fn test_attribute_numeric() {
        let nominal = r#"<root><point x="100.0"/></root>"#;
        let actual = r#"<root><point x="100.05"/></root>"#;

        let f1 = write_temp_xml("attr_ok_1", nominal);
        let f2 = write_temp_xml("attr_ok_2", actual);

        let result = compare_files(f1, f2, &basic_config()).unwrap();

        assert!(!result.is_error);
    }

    // ✅ attribute mismatch
    #[test]
    fn test_attribute_missing() {
        let nominal = r#"<root><point x="1.0"/></root>"#;
        let actual = r#"<root><point/></root>"#;

        let f1 = write_temp_xml("attr_missing_1", nominal);
        let f2 = write_temp_xml("attr_missing_2", actual);

        let result = compare_files(f1, f2, &basic_config()).unwrap();

        assert!(result.is_error);
    }

    // ✅ structural difference (extra node)
    #[test]
    fn test_structure_difference() {
        let nominal = r#"<root><a>1</a></root>"#;
        let actual = r#"<root><a>1</a><b>2</b></root>"#;

        let f1 = write_temp_xml("struct_1", nominal);
        let f2 = write_temp_xml("struct_2", actual);

        let result = compare_files(f1, f2, &basic_config()).unwrap();

        assert!(result.is_error);
    }

    // ✅ nested path reporting sanity
    #[test]
    fn test_nested_difference() {
        let nominal = r#"
            <root>
                <level1>
                    <value>10</value>
                </level1>
            </root>
        "#;

        let actual = r#"
            <root>
                <level1>
                    <value>20</value>
                </level1>
            </root>
        "#;

        let f1 = write_temp_xml("nested_1", nominal);
        let f2 = write_temp_xml("nested_2", actual);

        let result = compare_files(f1, f2, &basic_config()).unwrap();

        assert!(result.is_error);
    }
}
