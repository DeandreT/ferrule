use ir::{FiniteF64, ScalarType, ScalarTypeSet, SchemaKind, SchemaNode};

use crate::JsonFormatError;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ScalarConstant {
    String(String),
    Int(i64),
    Float(FiniteF64),
    Bool(bool),
}

impl ScalarConstant {
    pub(crate) fn lexical(&self) -> String {
        match self {
            Self::String(value) => value.clone(),
            Self::Int(value) => value.to_string(),
            Self::Float(value) => value.get().to_string(),
            Self::Bool(value) => value.to_string(),
        }
    }

    pub(crate) fn to_json(&self) -> serde_json::Value {
        match self {
            Self::String(value) => value.clone().into(),
            Self::Int(value) => (*value).into(),
            Self::Float(value) => value.get().into(),
            Self::Bool(value) => (*value).into(),
        }
    }

    pub(crate) fn semantically_equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::String(left), Self::String(right)) => left == right,
            (Self::Int(left), Self::Int(right)) => left == right,
            (Self::Float(left), Self::Float(right)) => left == right,
            (Self::Bool(left), Self::Bool(right)) => left == right,
            (Self::Int(integer), Self::Float(number))
            | (Self::Float(number), Self::Int(integer)) => {
                let encoded = serde_json::Number::from(*integer);
                crate::exact_f64_from_json_number(&encoded) == Some(number.get())
            }
            _ => false,
        }
    }
}

pub(super) fn selected_constraint<'a>(
    name: &str,
    schema: &'a serde_json::Value,
) -> Result<Option<&'a serde_json::Value>, JsonFormatError> {
    let constant = schema.get("const");
    let values = match schema.get("enum") {
        None => None,
        Some(serde_json::Value::Array(values)) if values.is_empty() => {
            return Err(unsupported(name, "enum has no possible values"));
        }
        Some(serde_json::Value::Array(values)) => Some(values),
        Some(_) => {
            return Err(unsupported(name, "enum must be an array"));
        }
    };
    if let Some(constant) = constant {
        if values.is_some_and(|values| {
            !values
                .iter()
                .any(|candidate| json_values_equal(candidate, constant))
        }) {
            return Err(unsupported(
                name,
                "const and enum constraints have no value in common",
            ));
        }
        return Ok(Some(constant));
    }
    match values.map(|values| values.as_slice()) {
        None => Ok(None),
        Some([value]) => Ok(Some(value)),
        Some(_) => Err(unsupported(
            name,
            "multi-value enum requires typed allowed-value metadata",
        )),
    }
}

pub(super) fn infer(
    name: &str,
    value: &serde_json::Value,
) -> Result<ScalarConstant, JsonFormatError> {
    match value {
        serde_json::Value::String(value) => Ok(ScalarConstant::String(value.clone())),
        serde_json::Value::Bool(value) => Ok(ScalarConstant::Bool(*value)),
        serde_json::Value::Number(number) if number.as_i64().is_some() => number
            .as_i64()
            .map(ScalarConstant::Int)
            .ok_or_else(|| unsupported(name, "constant must be a signed 64-bit integer")),
        serde_json::Value::Number(number) if number.as_u64().is_some() => Err(unsupported(
            name,
            "integer constant is outside ferrule's signed 64-bit range",
        )),
        serde_json::Value::Number(number) => crate::exact_f64_from_json_number(number)
            .and_then(FiniteF64::new)
            .map(ScalarConstant::Float)
            .ok_or_else(|| {
                unsupported(
                    name,
                    "numeric constant is not a finite exactly supported number",
                )
            }),
        serde_json::Value::Null => Err(unsupported(
            name,
            "a null-only constant has no distinct ferrule scalar value type",
        )),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => Err(unsupported(
            name,
            "object and array constants are not representable as scalar fixed values",
        )),
    }
}

pub(super) fn for_type(
    name: &str,
    value: &serde_json::Value,
    ty: ScalarType,
) -> Result<ScalarConstant, JsonFormatError> {
    let constant = match (ty, value) {
        (ScalarType::String, serde_json::Value::String(value)) => {
            ScalarConstant::String(value.clone())
        }
        (ScalarType::Int, serde_json::Value::Number(number)) => number
            .as_i64()
            .map(ScalarConstant::Int)
            .ok_or_else(|| unsupported(name, "constant must be a signed 64-bit integer"))?,
        (ScalarType::Float, serde_json::Value::Number(number)) => match number.as_i64() {
            Some(value) => ScalarConstant::Int(value),
            None => crate::exact_f64_from_json_number(number)
                .and_then(FiniteF64::new)
                .map(ScalarConstant::Float)
                .ok_or_else(|| {
                    unsupported(name, "constant must be a finite exactly supported number")
                })?,
        },
        (ScalarType::Bool, serde_json::Value::Bool(value)) => ScalarConstant::Bool(*value),
        (_, serde_json::Value::Null) => {
            return Err(unsupported(
                name,
                "const null narrows the schema to an unsupported null-only domain",
            ));
        }
        _ => {
            return Err(unsupported(
                name,
                "constant does not match its declared scalar type",
            ));
        }
    };
    Ok(constant)
}

pub(super) fn for_types(
    name: &str,
    value: &serde_json::Value,
    types: ScalarTypeSet,
) -> Result<ScalarConstant, JsonFormatError> {
    let ty = match value {
        serde_json::Value::String(_) if types.contains(ScalarType::String) => ScalarType::String,
        serde_json::Value::Bool(_) if types.contains(ScalarType::Bool) => ScalarType::Bool,
        serde_json::Value::Number(number)
            if number.as_i64().is_some() && types.contains(ScalarType::Int) =>
        {
            ScalarType::Int
        }
        serde_json::Value::Number(_) if types.contains(ScalarType::Float) => ScalarType::Float,
        serde_json::Value::Null => {
            return Err(unsupported(
                name,
                "const null narrows the schema to an unsupported null-only domain",
            ));
        }
        _ => {
            return Err(unsupported(
                name,
                "constant does not match any declared scalar union type",
            ));
        }
    };
    for_type(name, value, ty)
}

pub(super) fn schema(name: &str, constant: &ScalarConstant) -> SchemaNode {
    let ty = match constant {
        ScalarConstant::String(_) => ScalarType::String,
        ScalarConstant::Int(_) => ScalarType::Int,
        ScalarConstant::Float(_) => ScalarType::Float,
        ScalarConstant::Bool(_) => ScalarType::Bool,
    };
    let mut node = SchemaNode::scalar(name, ty);
    node.fixed = Some(constant.lexical());
    node
}

pub(crate) fn from_schema(schema: &SchemaNode) -> Result<Option<ScalarConstant>, JsonFormatError> {
    let Some(fixed) = schema.fixed.as_deref() else {
        return Ok(None);
    };
    let SchemaKind::Scalar { ty } = schema.kind else {
        return Err(JsonFormatError::UnsupportedSchemaUnion {
            name: schema.name.clone(),
            reason: "fixed JSON values require one concrete scalar type".to_string(),
        });
    };
    let constant = match ty {
        ScalarType::String => ScalarConstant::String(fixed.to_string()),
        ScalarType::Int => fixed
            .parse::<i64>()
            .map(ScalarConstant::Int)
            .map_err(|_| invalid_fixed(schema, "signed 64-bit integer"))?,
        ScalarType::Float => fixed
            .parse::<f64>()
            .ok()
            .and_then(FiniteF64::new)
            .map(ScalarConstant::Float)
            .ok_or_else(|| invalid_fixed(schema, "finite number"))?,
        ScalarType::Bool => fixed
            .parse::<bool>()
            .map(ScalarConstant::Bool)
            .map_err(|_| invalid_fixed(schema, "boolean"))?,
    };
    Ok(Some(constant))
}

pub(crate) fn validate_json(
    schema: &SchemaNode,
    value: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    let Some(expected) = from_schema(schema)? else {
        return Ok(());
    };
    if value.is_null() {
        return Err(constant_mismatch(schema, &expected, value));
    }
    let actual = match schema.kind {
        SchemaKind::Scalar { ty } => for_type(&schema.name, value, ty)?,
        SchemaKind::ScalarUnion { .. } | SchemaKind::Group { .. } => {
            return Err(JsonFormatError::UnsupportedSchemaUnion {
                name: schema.name.clone(),
                reason: "fixed JSON values require one concrete scalar type".to_string(),
            });
        }
    };
    if expected.semantically_equals(&actual) {
        return Ok(());
    }
    Err(constant_mismatch(schema, &expected, value))
}

pub(crate) fn rendered_fixed(schema: &SchemaNode) -> Option<serde_json::Value> {
    match from_schema(schema) {
        Ok(value) => value.map(|value| value.to_json()),
        // Preserve invalid in-memory constraints as an unsatisfiable
        // type/const combination instead of silently widening the schema.
        Err(_) => schema
            .fixed
            .as_ref()
            .map(|value| serde_json::Value::String(value.clone())),
    }
}

fn json_values_equal(left: &serde_json::Value, right: &serde_json::Value) -> bool {
    match (left, right) {
        (serde_json::Value::Number(left), serde_json::Value::Number(right)) => {
            if let (Some(left), Some(right)) = (left.as_i64(), right.as_i64()) {
                return left == right;
            }
            match (
                crate::exact_f64_from_json_number(left),
                crate::exact_f64_from_json_number(right),
            ) {
                (Some(left), Some(right)) => left == right,
                _ => false,
            }
        }
        _ => left == right,
    }
}

fn invalid_fixed(schema: &SchemaNode, expected: &'static str) -> JsonFormatError {
    JsonFormatError::UnsupportedSchemaUnion {
        name: schema.name.clone(),
        reason: format!("fixed value is not a valid {expected}"),
    }
}

fn constant_mismatch(
    schema: &SchemaNode,
    expected: &ScalarConstant,
    actual: &serde_json::Value,
) -> JsonFormatError {
    JsonFormatError::ConstantMismatch {
        name: schema.name.clone(),
        expected: expected.to_json().to_string(),
        got: actual.to_string(),
    }
}

fn unsupported(name: &str, reason: &str) -> JsonFormatError {
    JsonFormatError::UnsupportedSchemaUnion {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}
