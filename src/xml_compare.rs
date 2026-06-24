use crate::report::{self, DiffDetail, Difference};
use regex::Regex;
use roxmltree::{Document, Node};
use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;
use strsim::normalized_damerau_levenshtein;
use thiserror::Error;
use tracing::error;

//
// ================= CONFIG =================
//

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
/// XML comparison config
pub struct XMLCompareConfig {
    /// Tags that can be ignored in the comparison
    pub ignore_tags: Option<Vec<String>>,
    /// Rules for different tag types
    pub tag: Vec<TagRule>,
    /// Tags are not XML conform (i.e start in number)
    pub invalid_tags: Option<Vec<String>>,
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
    #[serde(rename = "vector")]
    Vector {
        abs: Option<Range>,
        rel: Option<Range>,
    },
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
pub struct Range {
    pub min: f64,
    pub max: f64,
}

//
// ================= ERROR =================
//

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

//
// ================= COMPILED CONFIG =================
//

struct CompiledXMLConfig {
    ignore_tags: Vec<Regex>,
    numeric: Option<NumericRule>,
    string: Option<StringRule>,
    vector: Option<NumericRule>,
    invalid_tag_patterns: Vec<(Regex, Regex, String)>,
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
        let mut vector = None;

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
                TagRule::Vector { abs, rel } => {
                    vector = Some(NumericRule {
                        abs: abs.clone(),
                        rel: rel.clone(),
                    });
                }
            }
        }

        // ✅ NEW: compile invalid tags
        let mut invalid_tag_patterns = Vec::new();

        if let Some(tags) = &self.invalid_tags {
            for tag in tags {
                let tag_escaped = regex::escape(tag);

                let open = Regex::new(&format!(r"<\s*{}(\s|>)", tag_escaped))?;
                let close = Regex::new(&format!(r"</\s*{}\s*>", tag_escaped))?;

                invalid_tag_patterns.push((open, close, tag.clone()));
            }
        }

        Ok(CompiledXMLConfig {
            ignore_tags,
            numeric,
            string,
            vector,
            invalid_tag_patterns,
        })
    }
}

fn glob_to_regex(pattern: &str) -> Result<Regex, regex::Error> {
    let escaped = regex::escape(pattern).replace("\\*", ".*");
    Regex::new(&format!("^{}$", escaped))
}

//
// ================= ENTRY =================
//

pub fn compare_files<P: AsRef<Path>>(
    nominal_path: P,
    actual_path: P,
    config: &XMLCompareConfig,
) -> Result<Difference, Error> {
    let nominal_text = std::fs::read_to_string(&nominal_path)?;
    let actual_text = std::fs::read_to_string(&actual_path)?;
    let compiled = config.compile()?;

    let nominal_text = normalize_invalid_tags(&nominal_text, &compiled);
    let actual_text = normalize_invalid_tags(&actual_text, &compiled);

    let nominal_doc = Document::parse(&nominal_text)?;
    let actual_doc = Document::parse(&actual_text)?;

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

//
// ================= CORE =================
//

fn compare_nodes<'a, 'i>(
    nominal: Node<'a, 'i>,
    actual: Node<'a, 'i>,
    config: &CompiledXMLConfig,
    diff: &mut Difference,
    path: String,
) {
    let tag = nominal.tag_name().name();
    let current_path = format!("{}/{}", path, tag);

    if config.ignore_tags.iter().any(|r| r.is_match(tag)) {
        return;
    }

    if tag != actual.tag_name().name() {
        report_error(diff, &current_path, tag, actual.tag_name().name());
        return;
    }

    compare_values(nominal, actual, config, diff, &current_path);
    compare_attributes(nominal, actual, config, diff, &current_path);

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

fn compare_values<'a, 'i>(
    nominal_node: Node<'a, 'i>,
    actual_node: Node<'a, 'i>,
    config: &CompiledXMLConfig,
    diff: &mut Difference,
    path: &str,
) {
    let n = nominal_node.text().unwrap_or("");
    let a = actual_node.text().unwrap_or("");

    let is_vector = nominal_node.attribute("type") == Some("xyz");

    compare_text_values(n, a, config, diff, path, is_vector);
}

fn compare_attributes<'a, 'i>(
    nominal: Node<'a, 'i>,
    actual: Node<'a, 'i>,
    config: &CompiledXMLConfig,
    diff: &mut Difference,
    path: &str,
) {
    for attr in nominal.attributes() {
        let attr_name = attr.name();

        match actual.attribute(attr_name) {
            Some(actual_value) => {
                let attr_path = format!("{}[@{}]", path, attr_name);

                compare_text_values(attr.value(), actual_value, config, diff, &attr_path, false);
            }
            None => {
                error!("Missing attribute {} at {}", attr_name, path);
                diff.error();
            }
        }
    }

    for attr in actual.attributes() {
        if nominal.attribute(attr.name()).is_none() {
            error!("Unexpected attribute {} at {}", attr.name(), path);
            diff.error();
        }
    }
}

//
// ================= VALUE LOGIC =================
//

fn compare_text_values(
    nominal: &str,
    actual: &str,
    config: &CompiledXMLConfig,
    diff: &mut Difference,
    path: &str,
    vector_hint: bool,
) {
    let n = nominal.trim();
    let a = actual.trim();

    // VECTOR
    if vector_hint {
        if let (Some(nv), Some(av)) = (parse_vector(n), parse_vector(a)) {
            if let Some(rule) = &config.vector {
                for i in 0..3 {
                    if !within_tolerance(nv[i], av[i], rule) {
                        report_value_mismatch(
                            diff,
                            &format!("{}[{}]", path, ["x", "y", "z"][i]),
                            &nv[i].to_string(),
                            &av[i].to_string(),
                            (av[i] - nv[i]).abs(),
                        );
                    }
                }
            }
            return;
        }
    }

    // NUMERIC
    if let (Ok(nv), Ok(av)) = (n.parse::<f64>(), a.parse::<f64>()) {
        if let Some(rule) = &config.numeric {
            if !within_tolerance(nv, av, rule) {
                report_value_mismatch(diff, path, n, a, (av - nv).abs());
            }
        }
        return;
    }

    // STRING
    if let Some(rule) = &config.string {
        let distance = normalized_damerau_levenshtein(n, a);
        if distance < rule.threshold {
            report_value_mismatch(diff, path, n, a, distance);
        }
    }
}

//
// CENTRALIZED TOLERANCE LOGIC
//

fn within_tolerance(n: f64, a: f64, rule: &NumericRule) -> bool {
    let diff_abs = (a - n).abs();
    let diff_rel = if n != 0.0 { diff_abs / n.abs() } else { 0.0 };

    let abs_ok = rule
        .abs
        .as_ref()
        .is_none_or(|r| diff_abs >= r.min && diff_abs <= r.max);

    let rel_ok = rule
        .rel
        .as_ref()
        .is_none_or(|r| diff_rel >= r.min && diff_rel <= r.max);

    abs_ok || rel_ok
}

//
// ================= HELPERS =================
//

fn parse_vector(value: &str) -> Option<[f64; 3]> {
    let trimmed = value.trim().trim_start_matches('(').trim_end_matches(')');
    let parts: Vec<_> = trimmed.split(',').collect();

    if parts.len() != 3 {
        return None;
    }

    let coords: Vec<f64> = parts
        .iter()
        .map(|p| p.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;

    Some([coords[0], coords[1], coords[2]])
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

fn normalize_invalid_tags(input: &str, config: &CompiledXMLConfig) -> String {
    if config.invalid_tag_patterns.is_empty() {
        return input.to_string();
    }

    let mut output = input.to_string();

    for (open_re, close_re, tag) in &config.invalid_tag_patterns {
        let prefixed = if tag.starts_with('_') {
            tag.clone()
        } else {
            format!("_{}", tag)
        };

        output = open_re
            .replace_all(&output, format!("<{}$1", prefixed).as_str())
            .to_string();

        output = close_re
            .replace_all(&output, format!("</{}>", prefixed).as_str())
            .to_string();
    }

    output
}
