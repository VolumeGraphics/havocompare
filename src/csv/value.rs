use schemars_derive::JsonSchema;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::{Display, Formatter};

pub(crate) type FloatType = f64;

#[derive(Debug, Clone, JsonSchema, Deserialize, Serialize, PartialEq)]
pub struct Quantity {
    pub(crate) value: FloatType,
    pub(crate) unit: Option<String>,
}

fn next_up(val: FloatType) -> FloatType {
    const TINY_BITS: u64 = 0x1; // Smallest positive FloatType.
    const CLEAR_SIGN_MASK: u64 = 0x7fff_ffff_ffff_ffff;

    let bits = val.to_bits();
    if val.is_nan() || bits == FloatType::INFINITY.to_bits() {
        return val;
    }

    let abs = bits & CLEAR_SIGN_MASK;
    let next_bits = if abs == 0 {
        TINY_BITS
    } else if bits == abs {
        bits + 1
    } else {
        bits - 1
    };
    FloatType::from_bits(next_bits)
}

fn next_down(val: FloatType) -> FloatType {
    const NEG_TINY_BITS: u64 = 0x8000_0000_0000_0001; // Smallest (in magnitude) negative FloatType.
    const CLEAR_SIGN_MASK: u64 = 0x7fff_ffff_ffff_ffff;

    let bits = val.to_bits();
    if val.is_nan() || bits == FloatType::NEG_INFINITY.to_bits() {
        return val;
    }

    let abs: u64 = bits & CLEAR_SIGN_MASK;
    let next_bits = if abs == 0 {
        NEG_TINY_BITS
    } else if bits == abs {
        bits - 1
    } else {
        bits + 1
    };
    FloatType::from_bits(next_bits)
}

impl Quantity {
    #[cfg(test)]
    pub(crate) fn new(value: FloatType, unit: Option<&str>) -> Self {
        Self {
            unit: unit.map(|s| s.to_owned()),
            value,
        }
    }

    pub(crate) fn secure_diff(&self, rhs: &Quantity) -> FloatType {
        let min = self.value.min(rhs.value);
        let max = self.value.max(rhs.value);
        let min_up = next_up(min);
        let max_down = next_down(max);
        next_down(max_down - min_up)
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
                write!(f, "{val}").unwrap();
            }
            Value::String(val) => {
                write!(f, "'{val}'").unwrap();
            }
        }
        Ok(())
    }
}

impl Value {
    pub fn deleted() -> Value {
        Value::from_str("DELETED", &None)
    }

    fn get_numerical_value(field_split: &[&str]) -> Option<FloatType> {
        if field_split.len() == 1 || field_split.len() == 2 {
            return field_split
                .first()
                .and_then(|s| s.parse::<FloatType>().ok());
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
            Value::String(s.trim().to_owned())
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

    pub fn as_str(&self) -> Cow<str> {
        match self {
            Value::String(str) => str.as_str().into(),
            Value::Quantity(quant) => quant.to_string().into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csv::Mode;
    #[test]
    fn trimming() {
        let val_spaced = Value::from_str(" value ", &None);
        let reference = Value::from_str("value", &None);
        assert_eq!(val_spaced, reference);
    }

    #[test]
    fn test_secure_diff() {
        for base in -30..=30 {
            for modulation in -30..=base {
                let magic_factor = 1.3;
                let num_one = magic_factor * 10.0f64.powi(base);
                let delta = magic_factor * 10.0f64.powi(modulation);
                let compare_mode = Mode::Absolute(delta.abs());
                let num_modulated = num_one + delta;
                let q1 = Quantity::new(num_one, None);
                let q2 = Quantity::new(num_modulated, None);
                assert!(compare_mode.in_tolerance(&q1, &q2));
            }
        }
    }
}
