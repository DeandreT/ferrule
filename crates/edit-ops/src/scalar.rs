use ir::Value;
use serde::{Deserialize, Serialize};

use crate::EditError;

/// Exact scalar text, parsed only by Rust. Null-like values have no text member.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scalar {
    Null {},
    JsonNull {},
    XmlNil {},
    Bool { text: String },
    Int { text: String },
    Float { text: String },
    String { text: String },
}

impl Scalar {
    pub fn value(&self) -> Result<Value, EditError> {
        let invalid = || EditError::InvalidScalar {
            scalar: self.clone(),
        };
        Ok(match self {
            Self::Null {} => Value::Null,
            Self::JsonNull {} => Value::JsonNull(ir::JsonNull),
            Self::XmlNil {} => Value::XmlNil(ir::XmlNil),
            Self::String { text } => Value::String(text.clone()),
            Self::Bool { text } => Value::Bool(match text.as_str() {
                "true" => true,
                "false" => false,
                _ => return Err(invalid()),
            }),
            Self::Int { text } => {
                let value = text.parse::<i64>().map_err(|_| invalid())?;
                if value.to_string() != *text {
                    return Err(invalid());
                }
                Value::Int(value)
            }
            Self::Float { text } => {
                // JSON number grammar excludes whitespace, plus signs, NaN and infinity.
                let parsed: serde_json::Number =
                    serde_json::from_str(text).map_err(|_| invalid())?;
                if text.trim() != text {
                    return Err(invalid());
                }
                let value = parsed.as_f64().ok_or_else(invalid)?;
                if !value.is_finite() {
                    return Err(invalid());
                }
                // Parsing directly retains -0 as a float even when JSON regards it as integer.
                Value::Float(text.parse::<f64>().map_err(|_| invalid())?)
            }
        })
    }
}
