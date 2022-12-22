use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct Quantity {
    pub(crate) value: f32,
    pub(crate) unit: Option<String>,
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

#[derive(Debug, PartialEq, Clone)]
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
    pub fn deleted() -> Value {
        Value::from_str("DELETED", &None)
    }

    fn get_numerical_value(field_split: &[&str]) -> Option<f32> {
        if field_split.len() == 1 || field_split.len() == 2 {
            return field_split.first().and_then(|s| s.parse::<f32>().ok());
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
