use core::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::{FiniteF64, ScalarType, Value};

pub const MAX_JSON_ALLOWED_VALUES: usize = 4096;
pub const MAX_JSON_ALLOWED_VALUE_STRING_BYTES: usize = 256 * 1024;
pub const MAX_JSON_ALLOWED_VALUE_TOTAL_STRING_BYTES: usize = 1024 * 1024;

/// One exact JSON-compatible scalar value in a JSON Schema `enum`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum JsonAllowedValue {
    String(String),
    Int(i64),
    Float(FiniteF64),
    Bool(bool),
    JsonNull,
}

impl JsonAllowedValue {
    pub fn canonicalized(self) -> Self {
        match self {
            Self::Float(value) => exact_i64(value.get()).map_or(Self::Float(value), Self::Int),
            other => other,
        }
    }

    pub fn scalar_type(&self) -> Option<ScalarType> {
        match self {
            Self::String(_) => Some(ScalarType::String),
            Self::Int(_) => Some(ScalarType::Int),
            Self::Float(_) => Some(ScalarType::Float),
            Self::Bool(_) => Some(ScalarType::Bool),
            Self::JsonNull => None,
        }
    }

    pub fn semantically_equals(&self, other: &Self) -> bool {
        self.clone().canonicalized() == other.clone().canonicalized()
    }

    fn canonical_cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::JsonNull, Self::JsonNull) => Ordering::Equal,
            (Self::JsonNull, _) => Ordering::Less,
            (_, Self::JsonNull) => Ordering::Greater,
            (Self::Bool(left), Self::Bool(right)) => left.cmp(right),
            (Self::Bool(_), _) => Ordering::Less,
            (_, Self::Bool(_)) => Ordering::Greater,
            (Self::Int(left), Self::Int(right)) => left.cmp(right),
            (Self::Int(_), _) => Ordering::Less,
            (_, Self::Int(_)) => Ordering::Greater,
            (Self::Float(left), Self::Float(right)) => left.get().total_cmp(&right.get()),
            (Self::Float(_), Self::String(_)) => Ordering::Less,
            (Self::String(_), Self::Float(_)) => Ordering::Greater,
            (Self::String(left), Self::String(right)) => left.cmp(right),
        }
    }

    pub fn matches(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::String(expected), Value::String(actual)) => expected == actual,
            (Self::Int(expected), Value::Int(actual)) => expected == actual,
            (Self::Int(expected), Value::Float(actual)) => exact_i64(*actual) == Some(*expected),
            (Self::Float(expected), Value::Float(actual)) => expected.get() == *actual,
            (Self::Float(expected), Value::Int(actual)) => {
                exact_i64(expected.get()) == Some(*actual)
            }
            (Self::Bool(expected), Value::Bool(actual)) => expected == actual,
            (Self::JsonNull, Value::JsonNull(_)) => true,
            _ => false,
        }
    }
}

/// A bounded canonical set of at least two exact scalar values.
///
/// JSON Schema enum order is not semantic, so values use one stable order.
/// Integral floats inside the exact `i64` domain are represented as integers;
/// this also makes JSON Schema's mathematical integer/number equality
/// canonical without approximate comparisons.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct JsonAllowedValues(Vec<JsonAllowedValue>);

impl JsonAllowedValues {
    pub fn new(
        values: impl IntoIterator<Item = JsonAllowedValue>,
    ) -> Result<Self, JsonAllowedValuesError> {
        let mut canonical = Vec::new();
        let mut total_string_bytes = 0_usize;
        for value in values {
            let value = value.canonicalized();
            validate_value(&value, &mut total_string_bytes)?;
            if canonical.contains(&value) {
                continue;
            }
            if canonical.len() == MAX_JSON_ALLOWED_VALUES {
                return Err(JsonAllowedValuesError::TooMany);
            }
            canonical.push(value);
        }
        let mut values = canonical;
        values.sort_by(JsonAllowedValue::canonical_cmp);
        values.dedup();
        Self::from_canonical(values)
    }

    pub fn values(&self) -> &[JsonAllowedValue] {
        &self.0
    }

    pub fn into_values(self) -> Vec<JsonAllowedValue> {
        self.0
    }

    pub fn contains(&self, value: &JsonAllowedValue) -> bool {
        let value = value.clone().canonicalized();
        self.0
            .binary_search_by(|candidate| candidate.canonical_cmp(&value))
            .is_ok()
    }

    pub fn matches(&self, value: &Value) -> bool {
        self.0.iter().any(|candidate| candidate.matches(value))
    }

    pub fn contains_json_null(&self) -> bool {
        matches!(self.0.first(), Some(JsonAllowedValue::JsonNull))
    }

    fn from_canonical(values: Vec<JsonAllowedValue>) -> Result<Self, JsonAllowedValuesError> {
        validate_canonical(&values)?;
        Ok(Self(values))
    }
}

impl<'de> Deserialize<'de> for JsonAllowedValues {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct AllowedValuesVisitor;

        impl<'de> serde::de::Visitor<'de> for AllowedValuesVisitor {
            type Value = JsonAllowedValues;

            fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                formatter.write_str("a canonical bounded JSON allowed-value array")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut values: Vec<JsonAllowedValue> = Vec::new();
                let mut total_string_bytes = 0_usize;
                while let Some(value) = sequence.next_element::<JsonAllowedValue>()? {
                    if values.len() == MAX_JSON_ALLOWED_VALUES {
                        return Err(serde::de::Error::custom(JsonAllowedValuesError::TooMany));
                    }
                    validate_value(&value, &mut total_string_bytes)
                        .map_err(serde::de::Error::custom)?;
                    if let Some(previous) = values.last() {
                        match previous.canonical_cmp(&value) {
                            Ordering::Less => {}
                            Ordering::Equal => {
                                return Err(serde::de::Error::custom(
                                    JsonAllowedValuesError::Duplicate,
                                ));
                            }
                            Ordering::Greater => {
                                return Err(serde::de::Error::custom(
                                    JsonAllowedValuesError::NonCanonicalOrder,
                                ));
                            }
                        }
                    }
                    values.push(value);
                }
                JsonAllowedValues::from_canonical(values).map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_seq(AllowedValuesVisitor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonAllowedValuesError {
    TooFew,
    TooMany,
    StringTooLong,
    TooManyStringBytes,
    NonCanonicalValue,
    NonCanonicalOrder,
    Duplicate,
}

impl core::fmt::Display for JsonAllowedValuesError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooFew => write!(
                formatter,
                "JSON allowed values must contain at least two distinct scalar values"
            ),
            Self::TooMany => write!(
                formatter,
                "JSON allowed values exceed the {MAX_JSON_ALLOWED_VALUES}-value limit"
            ),
            Self::StringTooLong => write!(
                formatter,
                "a JSON allowed string exceeds the {MAX_JSON_ALLOWED_VALUE_STRING_BYTES}-byte limit"
            ),
            Self::TooManyStringBytes => write!(
                formatter,
                "JSON allowed strings exceed the {MAX_JSON_ALLOWED_VALUE_TOTAL_STRING_BYTES}-byte total limit"
            ),
            Self::NonCanonicalValue => write!(
                formatter,
                "an integral JSON allowed float must use its canonical integer representation"
            ),
            Self::NonCanonicalOrder => {
                write!(formatter, "JSON allowed values are not in canonical order")
            }
            Self::Duplicate => write!(formatter, "JSON allowed values contain a duplicate"),
        }
    }
}

impl std::error::Error for JsonAllowedValuesError {}

fn validate_canonical(values: &[JsonAllowedValue]) -> Result<(), JsonAllowedValuesError> {
    if values.len() < 2 {
        return Err(JsonAllowedValuesError::TooFew);
    }
    if values.len() > MAX_JSON_ALLOWED_VALUES {
        return Err(JsonAllowedValuesError::TooMany);
    }
    let mut total_string_bytes = 0_usize;
    for (index, value) in values.iter().enumerate() {
        validate_value(value, &mut total_string_bytes)?;
        if let Some(previous) = index.checked_sub(1).and_then(|index| values.get(index)) {
            match previous.canonical_cmp(value) {
                Ordering::Less => {}
                Ordering::Equal => return Err(JsonAllowedValuesError::Duplicate),
                Ordering::Greater => return Err(JsonAllowedValuesError::NonCanonicalOrder),
            }
        }
    }
    Ok(())
}

fn validate_value(
    value: &JsonAllowedValue,
    total_string_bytes: &mut usize,
) -> Result<(), JsonAllowedValuesError> {
    if let JsonAllowedValue::String(value) = value {
        if value.len() > MAX_JSON_ALLOWED_VALUE_STRING_BYTES {
            return Err(JsonAllowedValuesError::StringTooLong);
        }
        *total_string_bytes = total_string_bytes
            .checked_add(value.len())
            .ok_or(JsonAllowedValuesError::TooManyStringBytes)?;
        if *total_string_bytes > MAX_JSON_ALLOWED_VALUE_TOTAL_STRING_BYTES {
            return Err(JsonAllowedValuesError::TooManyStringBytes);
        }
    }
    if let JsonAllowedValue::Float(value) = value
        && exact_i64(value.get()).is_some()
    {
        return Err(JsonAllowedValuesError::NonCanonicalValue);
    }
    Ok(())
}

fn exact_i64(value: f64) -> Option<i64> {
    const I64_UPPER_EXCLUSIVE: f64 = 9_223_372_036_854_775_808.0;

    if !value.is_finite()
        || value.fract() != 0.0
        || value < i64::MIN as f64
        || value >= I64_UPPER_EXCLUSIVE
    {
        return None;
    }
    let integer = value as i64;
    ((integer as f64) == value).then_some(integer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finite(value: f64) -> Result<FiniteF64, &'static str> {
        FiniteF64::new(value).ok_or("test value must be finite")
    }

    #[test]
    fn construction_canonicalizes_order_duplicates_and_integral_floats()
    -> Result<(), Box<dyn std::error::Error>> {
        let values = JsonAllowedValues::new([
            JsonAllowedValue::String("z".to_string()),
            JsonAllowedValue::Float(finite(1.0)?),
            JsonAllowedValue::JsonNull,
            JsonAllowedValue::Int(1),
            JsonAllowedValue::Bool(true),
            JsonAllowedValue::Float(finite(-0.0)?),
        ])?;
        assert_eq!(
            values.values(),
            [
                JsonAllowedValue::JsonNull,
                JsonAllowedValue::Bool(true),
                JsonAllowedValue::Int(0),
                JsonAllowedValue::Int(1),
                JsonAllowedValue::String("z".to_string()),
            ]
        );
        assert!(values.contains(&JsonAllowedValue::Float(finite(1.0)?)));
        assert!(values.contains_json_null());
        Ok(())
    }

    #[test]
    fn exact_numeric_and_null_membership_preserves_runtime_semantics()
    -> Result<(), Box<dyn std::error::Error>> {
        let values = JsonAllowedValues::new([
            JsonAllowedValue::Int(i64::MIN),
            JsonAllowedValue::Float(finite(1.5)?),
            JsonAllowedValue::JsonNull,
        ])?;
        assert!(values.matches(&Value::Int(i64::MIN)));
        assert!(values.matches(&Value::Float(i64::MIN as f64)));
        assert!(values.matches(&Value::Float(1.5)));
        assert!(values.matches(&Value::json_null()));
        assert!(!values.matches(&Value::Null));
        assert!(!values.matches(&Value::Float(1.500_000_000_000_000_2)));
        assert!(!values.matches(&Value::Int(1)));
        Ok(())
    }

    #[test]
    fn construction_enforces_cardinality_and_string_budgets()
    -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            JsonAllowedValues::new([
                JsonAllowedValue::Int(1),
                JsonAllowedValue::Float(finite(1.0)?),
            ]),
            Err(JsonAllowedValuesError::TooFew)
        );
        assert_eq!(
            JsonAllowedValues::new([
                JsonAllowedValue::String("x".repeat(MAX_JSON_ALLOWED_VALUE_STRING_BYTES + 1)),
                JsonAllowedValue::JsonNull,
            ]),
            Err(JsonAllowedValuesError::StringTooLong)
        );
        let too_many =
            (0..=MAX_JSON_ALLOWED_VALUES).map(|index| JsonAllowedValue::String(index.to_string()));
        assert_eq!(
            JsonAllowedValues::new(too_many),
            Err(JsonAllowedValuesError::TooMany)
        );
        Ok(())
    }

    #[test]
    fn serde_requires_the_canonical_wire_representation() -> Result<(), Box<dyn std::error::Error>>
    {
        let values = JsonAllowedValues::new([
            JsonAllowedValue::JsonNull,
            JsonAllowedValue::Bool(false),
            JsonAllowedValue::Int(1),
            JsonAllowedValue::Float(finite(1.5)?),
            JsonAllowedValue::String("x".to_string()),
        ])?;
        let encoded = serde_json::to_string(&values)?;
        assert_eq!(serde_json::from_str::<JsonAllowedValues>(&encoded)?, values);

        for invalid in [
            r#"[{"type":"int","value":1}]"#,
            r#"[{"type":"string","value":"x"},{"type":"int","value":1}]"#,
            r#"[{"type":"int","value":1},{"type":"int","value":1}]"#,
            r#"[{"type":"float","value":1.0},{"type":"string","value":"x"}]"#,
        ] {
            assert!(serde_json::from_str::<JsonAllowedValues>(invalid).is_err());
        }
        Ok(())
    }
}
