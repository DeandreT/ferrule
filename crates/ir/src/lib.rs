//! Schema-agnostic in-memory IR shared by every format adapter: schema trees
//! (structure of a source/target format) and instance trees (actual data).
//!
//! This first cut only models flat records (a fixed, ordered list of scalar
//! fields) — enough for CSV. Hierarchical/repeating structure (needed for
//! XML and JSON) is a later milestone.

use serde::{Deserialize, Serialize};

/// The scalar types a field can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScalarType {
    String,
    Int,
    Float,
    Bool,
}

/// A single scalar value flowing through a mapping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::String(_) => "string",
        }
    }
}

/// The declared shape of one field in a flat record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldSchema {
    pub name: String,
    pub ty: ScalarType,
}

/// The declared shape of a flat record: an ordered list of fields.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RecordSchema {
    pub fields: Vec<FieldSchema>,
}

impl RecordSchema {
    pub fn field(&self, name: &str) -> Option<&FieldSchema> {
        self.fields.iter().find(|f| f.name == name)
    }
}

/// A single row of data: an ordered list of (field name, value) pairs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Record(pub Vec<(String, Value)>);

impl Record {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    pub fn set(&mut self, name: impl Into<String>, value: Value) {
        self.0.push((name.into(), value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_get_returns_last_set_value() {
        let mut record = Record::new();
        record.set("age", Value::Int(30));
        assert_eq!(record.get("age"), Some(&Value::Int(30)));
        assert_eq!(record.get("missing"), None);
    }

    #[test]
    fn value_json_roundtrip_picks_the_right_variant() {
        assert_eq!(serde_json::from_str::<Value>("42").unwrap(), Value::Int(42));
        assert_eq!(
            serde_json::from_str::<Value>("1.5").unwrap(),
            Value::Float(1.5)
        );
        assert_eq!(
            serde_json::from_str::<Value>("true").unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            serde_json::from_str::<Value>("\"hi\"").unwrap(),
            Value::String("hi".to_string())
        );
        assert_eq!(serde_json::from_str::<Value>("null").unwrap(), Value::Null);
    }
}
