use crate::report::{self, DiffDetail, Difference, FailureKind, XMLDiffKind};
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
        x: Option<AxisRule>,
        y: Option<AxisRule>,
        z: Option<AxisRule>,
    },
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
pub struct AxisRule {
    pub abs: Option<Range>,
    pub rel: Option<Range>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
pub struct Range {
    pub min: f64,
    pub max: f64,
}

impl Range {
    fn validate(&self) -> Result<(), XMLCompareError> {
        if self.min > self.max {
            return Err(XMLCompareError::InvalidRange {
                min: self.min,
                max: self.max,
            });
        }

        Ok(())
    }
}

impl NumericRule {
    fn validate(&self) -> Result<(), XMLCompareError> {
        if let Some(abs) = &self.abs {
            abs.validate()?;
        }

        if let Some(rel) = &self.rel {
            rel.validate()?;
        }

        Ok(())
    }
}

impl VectorRule {
    fn validate(&self) -> Result<(), XMLCompareError> {
        for (i, axis) in self.axes.iter().enumerate() {
            axis.validate().map_err(|e| {
                tracing::error!("Invalid vector axis [{}]: {}", ["x", "y", "z"][i], e);
                e
            })?;
        }

        Ok(())
    }
}

//
// ================= ERROR =================
//

#[derive(Debug, Error)]
pub enum XMLCompareError {
    #[error("XML parse error: {0}")]
    ParseFailure(#[from] roxmltree::Error),

    #[error("Regex error: {0}")]
    RegexFailure(#[from] regex::Error),

    #[error("IO error: {0}")]
    IoFailure(#[from] std::io::Error),

    #[error("Reporting error: {0}")]
    Reporting(#[from] report::Error),

    #[error("Invalid vector config: missing axes (x={x}, y={y}, z={z})")]
    InvalidVectorConfigDetailed { x: bool, y: bool, z: bool },

    #[error("Invalid range: min ({min}) > max ({max})")]
    InvalidRange { min: f64, max: f64 },

    #[error(
        "Tag '{tag}' was configured as an invalid tag but appears as a valid XML attribute. \
        Expected invalid syntax like '<Time expected>' but found '<Time expected=\"...\">'"
    )]
    AmbiguousInvalidTag { tag: String },

    #[error("Invalid ignore_tags declared: {tag} - only support on space in tag name e.g. 'Time expected'")]
    InvalidIgnoreTag { tag: String },
}

//
// ================= COMPILED CONFIG =================
//
struct InvalidTagPattern {
    invalid_open: Regex,
    invalid_close: Regex,
    attribute_usage: Regex,
    original: String,
    normalized: String,
}

struct CompiledXMLConfig {
    ignore_tags: Vec<Regex>,
    numeric: Option<NumericRule>,
    string: Option<StringRule>,
    vector: Option<VectorRule>,
    invalid_tag_patterns: Vec<InvalidTagPattern>,
}

struct NumericRule {
    abs: Option<Range>,
    rel: Option<Range>,
}

struct StringRule {
    threshold: f64,
}

struct VectorRule {
    axes: [NumericRule; 3],
}

impl InvalidTagPattern {
    fn from_invalid_tag(tag: &str) -> Result<Self, regex::Error> {
        let tag_escaped = regex::escape(tag);

        Ok(Self {
            invalid_open: Regex::new(&format!(r"<\s*{}(\s|>)", tag_escaped))?,

            invalid_close: Regex::new(&format!(r"</\s*{}\s*>", tag_escaped))?,

            attribute_usage: Regex::new("$^")?,

            original: tag.to_string(),

            normalized: if tag.starts_with('_') {
                tag.to_string()
            } else {
                format!("_{}", tag)
            },
        })
    }
}

impl XMLCompareConfig {
    fn compile(&self) -> Result<CompiledXMLConfig, XMLCompareError> {
        let mut numeric = None;
        let mut string = None;
        let mut vector = None;

        for rule in &self.tag {
            match rule {
                TagRule::Numeric { abs, rel } => {
                    let rule = NumericRule {
                        abs: abs.clone(),
                        rel: rel.clone(),
                    };

                    rule.validate()?;

                    numeric = Some(rule);
                }
                TagRule::String { threshold } => {
                    string = Some(StringRule {
                        threshold: *threshold,
                    });
                }
                TagRule::Vector { x, y, z } => {
                    let (x, y, z) = match (x, y, z) {
                        (Some(x), Some(y), Some(z)) => (x, y, z),
                        _ => {
                            return Err(XMLCompareError::InvalidVectorConfigDetailed {
                                x: x.is_some(),
                                y: y.is_some(),
                                z: z.is_some(),
                            });
                        }
                    };

                    let rule = VectorRule {
                        axes: [
                            NumericRule {
                                abs: x.abs.clone(),
                                rel: x.rel.clone(),
                            },
                            NumericRule {
                                abs: y.abs.clone(),
                                rel: y.rel.clone(),
                            },
                            NumericRule {
                                abs: z.abs.clone(),
                                rel: z.rel.clone(),
                            },
                        ],
                    };

                    rule.validate()?;

                    vector = Some(rule);
                }
            }
        }

        let mut normalized_ignore_tags = Vec::new();
        let mut invalid_tag_patterns = Vec::new();

        if let Some(tags) = &self.invalid_tags {
            for tag in tags {
                invalid_tag_patterns.push(InvalidTagPattern::from_invalid_tag(tag)?);
            }
        }

        if let Some(tags) = &self.ignore_tags {
            for tag in tags {
                if tag.contains(' ') {
                    let normalized = tag.replace(' ', "_");
                    let parts: Vec<_> = tag.split_whitespace().collect();

                    if parts.len() != 2 {
                        return Err(XMLCompareError::InvalidIgnoreTag { tag: tag.clone() });
                    }

                    if parts.len() == 2 {
                        let tag_name = parts[0];
                        let pseudo_tag = parts[1];

                        invalid_tag_patterns.push(InvalidTagPattern {
                            invalid_open: Regex::new(&format!(
                                r"<{}\s+{}\s*>",
                                regex::escape(tag_name),
                                regex::escape(pseudo_tag)
                            ))?,

                            invalid_close: Regex::new(&format!(
                                r"</{}\s+{}\s*>",
                                regex::escape(tag_name),
                                regex::escape(pseudo_tag)
                            ))?,

                            attribute_usage: Regex::new(&format!(
                                r"<{}\s+{}\s*=",
                                regex::escape(tag_name),
                                regex::escape(pseudo_tag)
                            ))?,

                            original: tag.clone(),
                            normalized: normalized.clone(),
                        });
                    }

                    normalized_ignore_tags.push(normalized);
                } else {
                    normalized_ignore_tags.push(tag.clone());
                }
            }
        }

        let ignore_tags: Vec<Regex> = normalized_ignore_tags
            .iter()
            .map(|t| glob_to_regex(t))
            .collect::<Result<_, _>>()?;

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
) -> Result<Difference, XMLCompareError> {
    let nominal_text = std::fs::read_to_string(&nominal_path)?;
    let actual_text = std::fs::read_to_string(&actual_path)?;
    let compiled = config.compile()?;

    let nominal_text = normalize_invalid_tags(&nominal_text, &compiled)?;
    let actual_text = normalize_invalid_tags(&actual_text, &compiled)?;

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
        diff.push_detail(DiffDetail::XML {
            path: current_path.clone(),
            nominal: tag.to_string(),
            actual: actual.tag_name().name().to_string(),
            kind: XMLDiffKind::TagMismatch {
                expected: tag.to_string(),
                found: actual.tag_name().name().to_string(),
            },
        });
        diff.error();
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
                diff.push_detail(DiffDetail::XML {
                    path: path.to_string(),
                    nominal: "".into(),
                    actual: "".into(),
                    kind: XMLDiffKind::AttributeMissing {
                        name: attr_name.to_string(),
                    },
                });
                diff.error()
            }
        }
    }

    for attr in actual.attributes() {
        if nominal.attribute(attr.name()).is_none() {
            error!("Unexpected attribute {} at {}", attr.name(), path);
            diff.push_detail(DiffDetail::XML {
                path: path.to_string(),
                nominal: "".into(),
                actual: "".into(),
                kind: XMLDiffKind::AttributeUnexpected {
                    name: attr.name().to_string(),
                },
            });
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
                    let axis_rule = &rule.axes[i];

                    match evaluate_tolerance(nv[i], av[i], axis_rule) {
                        ToleranceResult::Passed => {}

                        ToleranceResult::Failed {
                            diff_abs,
                            diff_rel,
                            failure,
                        } => {
                            diff.push_detail(DiffDetail::XML {
                                path: format!("{}[{}]", path, ["x", "y", "z"][i]),
                                nominal: nv[i].to_string(),
                                actual: av[i].to_string(),
                                kind: XMLDiffKind::Vector {
                                    axis: ["x", "y", "z"][i],
                                    diff_abs,
                                    diff_rel,
                                    abs_range: axis_rule.abs.clone(),
                                    rel_range: axis_rule.rel.clone(),
                                    failed_on: failure,
                                },
                            });

                            diff.error();
                        }
                    }
                }
            }
            return;
        }
    }

    // NUMERIC
    if let (Ok(nv), Ok(av)) = (n.parse::<f64>(), a.parse::<f64>()) {
        if let Some(rule) = &config.numeric {
            match evaluate_tolerance(nv, av, rule) {
                ToleranceResult::Passed => {
                    // ✅ Do nothing (this is your "ignore passed")
                }

                ToleranceResult::Failed {
                    diff_abs,
                    diff_rel,
                    failure,
                } => {
                    diff.push_detail(DiffDetail::XML {
                        path: path.to_string(),
                        nominal: n.to_string(),
                        actual: a.to_string(),
                        kind: XMLDiffKind::Numeric {
                            diff_abs,
                            diff_rel,
                            abs_range: rule.abs.clone(),
                            rel_range: rule.rel.clone(),
                            failed_on: failure,
                        },
                    });

                    diff.error();
                }
            }
        }
        return;
    }

    // STRING
    if let Some(rule) = &config.string {
        let distance = normalized_damerau_levenshtein(n, a);
        if distance < rule.threshold {
            diff.push_detail(DiffDetail::XML {
                path: path.to_string(),
                nominal: n.to_string(),
                actual: a.to_string(),
                kind: XMLDiffKind::String {
                    similarity: distance,
                    threshold: rule.threshold,
                },
            });

            diff.error();
        }
    }
}

//
// CENTRALIZED TOLERANCE LOGIC
//

enum ToleranceResult {
    Passed,
    Failed {
        diff_abs: f64,
        diff_rel: f64,
        failure: FailureKind,
    },
}

fn evaluate_tolerance(n: f64, a: f64, rule: &NumericRule) -> ToleranceResult {
    let diff_abs = (a - n).abs();
    let diff_rel = if n != 0.0 {
        diff_abs / n.abs()
    } else {
        diff_abs
    };

    const EPS: f64 = 1e-12;

    match (&rule.abs, &rule.rel) {
        (Some(abs), Some(rel)) => {
            let abs_ok = diff_abs >= abs.min && diff_abs <= abs.max;
            let rel_ok = diff_rel >= rel.min && diff_rel <= rel.max;

            if abs_ok || rel_ok {
                ToleranceResult::Passed
            } else {
                let failure = if !abs_ok && rel_ok {
                    FailureKind::Absolute
                } else if abs_ok && !rel_ok {
                    FailureKind::Relative
                } else {
                    FailureKind::Both
                };

                ToleranceResult::Failed {
                    diff_abs,
                    diff_rel,
                    failure,
                }
            }
        }

        (Some(abs), None) => {
            let abs_ok = diff_abs >= abs.min && diff_abs <= abs.max;

            if abs_ok {
                ToleranceResult::Passed
            } else {
                ToleranceResult::Failed {
                    diff_abs,
                    diff_rel,
                    failure: FailureKind::Absolute,
                }
            }
        }

        (None, Some(rel)) => {
            let rel_ok = diff_rel >= rel.min && diff_rel <= rel.max;

            if rel_ok {
                ToleranceResult::Passed
            } else {
                ToleranceResult::Failed {
                    diff_abs,
                    diff_rel,
                    failure: FailureKind::Relative,
                }
            }
        }

        (None, None) => {
            let passed = (n - a).abs() <= EPS;

            if passed {
                ToleranceResult::Passed
            } else {
                ToleranceResult::Failed {
                    diff_abs,
                    diff_rel,
                    failure: FailureKind::Exact,
                }
            }
        }
    }
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

fn normalize_invalid_tags(
    input: &str,
    config: &CompiledXMLConfig,
) -> Result<String, XMLCompareError> {
    if config.invalid_tag_patterns.is_empty() {
        return Ok(input.to_string());
    }

    let mut output = input.to_string();

    for pattern in &config.invalid_tag_patterns {
        if pattern.attribute_usage.is_match(&output) {
            return Err(XMLCompareError::AmbiguousInvalidTag {
                tag: pattern.original.clone(),
            });
        }

        output = pattern
            .invalid_open
            .replace_all(&output, format!("<{}>", pattern.normalized).as_str())
            .to_string();

        output = pattern
            .invalid_close
            .replace_all(&output, format!("</{}>", pattern.normalized).as_str())
            .to_string();
    }

    Ok(output)
}
