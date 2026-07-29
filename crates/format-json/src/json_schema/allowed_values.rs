use ir::{
    FiniteF64, JsonAllowedValue, JsonAllowedValues, MAX_JSON_ALLOWED_VALUES, ScalarType,
    ScalarTypeSet, SchemaNode, Value,
};

use super::constraints::ScalarConstant;
use crate::JsonFormatError;

#[derive(Debug, Clone)]
pub(super) enum Selection {
    Fixed(ScalarConstant),
    Allowed(JsonAllowedValues),
}

impl Selection {
    fn values(&self) -> Vec<JsonAllowedValue> {
        match self {
            Self::Fixed(value) => vec![constant_to_allowed(value)],
            Self::Allowed(values) => values.values().to_vec(),
        }
    }
}

pub(super) fn has_keyword(schema: &serde_json::Value) -> bool {
    schema.get("const").is_some() || schema.get("enum").is_some()
}

pub(super) fn selected(
    name: &str,
    schema: &serde_json::Value,
) -> Result<Option<Selection>, JsonFormatError> {
    let constant = schema
        .get("const")
        .map(|value| parse_value(name, value))
        .transpose()?;
    let enumerated = match schema.get("enum") {
        None => None,
        Some(serde_json::Value::Array(values)) if values.is_empty() => {
            return Err(unsupported(name, "enum has no possible values"));
        }
        Some(serde_json::Value::Array(values)) => {
            if values.len() > MAX_JSON_ALLOWED_VALUES {
                return Err(unsupported(
                    name,
                    "enum exceeds the bounded scalar allowed-value limit",
                ));
            }
            let mut parsed = Vec::with_capacity(values.len());
            for value in values {
                let candidate = parse_value(name, value)?;
                if !parsed
                    .iter()
                    .any(|existing| values_equal(existing, &candidate))
                {
                    parsed.push(candidate);
                }
            }
            Some(parsed)
        }
        Some(_) => return Err(unsupported(name, "enum must be an array")),
    };

    let values = match (constant, enumerated) {
        (None, None) => return Ok(None),
        (Some(constant), None) => vec![constant],
        (None, Some(values)) => values,
        (Some(constant), Some(values))
            if values
                .iter()
                .any(|candidate| values_equal(candidate, &constant)) =>
        {
            vec![constant]
        }
        (Some(_), Some(_)) => {
            return Err(unsupported(
                name,
                "const and enum constraints have no value in common",
            ));
        }
    };
    selection_from_values(name, values).map(Some)
}

pub(super) fn inferred_schema(
    name: &str,
    selection: &Selection,
) -> Result<SchemaNode, JsonFormatError> {
    let values = selection.values();
    let mut types = Vec::new();
    let mut nullable = false;
    for value in &values {
        let Some(ty) = allowed_type(value) else {
            nullable = true;
            continue;
        };
        if !types.contains(&ty) {
            types.push(ty);
        }
    }
    if types.contains(&ScalarType::Float) {
        types.retain(|ty| *ty != ScalarType::Int);
    }
    let mut node = match types.as_slice() {
        [] => {
            return Err(unsupported(
                name,
                "a null-only const or enum has no distinct ferrule scalar value type",
            ));
        }
        [ty] => SchemaNode::scalar(name, *ty),
        _ => {
            let Some(types) = ScalarTypeSet::new(types) else {
                return Err(unsupported(
                    name,
                    "const or enum produced an invalid scalar type set",
                ));
            };
            SchemaNode::scalar_union(name, types)
        }
    };
    node.nullable = nullable;
    apply_selection(name, &mut node, selection.clone())?;
    Ok(node)
}

pub(super) fn schema_from_values(
    name: &str,
    values: Vec<JsonAllowedValue>,
) -> Result<SchemaNode, JsonFormatError> {
    let selection = selection_from_values(name, values)?;
    inferred_schema(name, &selection)
}

pub(super) fn apply(
    name: &str,
    schema: &serde_json::Value,
    node: &mut SchemaNode,
) -> Result<(), JsonFormatError> {
    let Some(selection) = selected(name, schema)? else {
        return Ok(());
    };
    apply_selection(name, node, selection)
}

pub(super) fn apply_selection(
    name: &str,
    node: &mut SchemaNode,
    selection: Selection,
) -> Result<(), JsonFormatError> {
    let candidate = selection.values();
    let existing = from_schema(node)?;
    let mut retained = candidate
        .into_iter()
        .filter(|value| admitted_by_node(node, value))
        .collect::<Vec<_>>();
    if retained.is_empty() {
        return Err(unsupported(
            name,
            "const or enum does not match the declared scalar type",
        ));
    }
    if let Some(existing) = existing {
        retained.retain(|value| {
            existing
                .iter()
                .any(|candidate| values_equal(candidate, value))
        });
    }
    set_node_values(name, node, retained)
}

pub(super) fn intersect_nodes(
    name: &str,
    node: &mut SchemaNode,
    left: Option<Vec<JsonAllowedValue>>,
    right: Option<Vec<JsonAllowedValue>>,
) -> Result<(), JsonFormatError> {
    let values = match (left, right) {
        (None, None) => {
            node.fixed = None;
            node.json_allowed_values = None;
            return Ok(());
        }
        (Some(values), None) | (None, Some(values)) => values,
        (Some(left), Some(right)) => left
            .into_iter()
            .filter(|value| right.iter().any(|candidate| values_equal(candidate, value)))
            .collect(),
    };
    let values = values
        .into_iter()
        .filter(|value| admitted_by_node(node, value))
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Err(unsupported(
            name,
            "allOf scalar allowed-value constraints have no value in common",
        ));
    }
    set_node_values(name, node, values)
}

pub(super) fn from_schema(
    schema: &SchemaNode,
) -> Result<Option<Vec<JsonAllowedValue>>, JsonFormatError> {
    if let Some(values) = &schema.json_allowed_values {
        if !schema.json_allowed_values_are_valid() {
            return Err(JsonFormatError::InvalidAllowedValuesMetadata {
                reason: format!("`{}` has incompatible allowed values", schema.name),
            });
        }
        return Ok(Some(values.values().to_vec()));
    }
    super::constraints::from_schema(schema)
        .map(|constant| constant.map(|constant| constant_to_allowed(&constant)))
        .map(|value| value.map(|value| vec![value]))
}

pub(super) fn retained_by_constraints(
    node: &SchemaNode,
    values: Vec<JsonAllowedValue>,
) -> Vec<JsonAllowedValue> {
    values
        .into_iter()
        .filter(|value| value_satisfies_constraints(node, value))
        .collect()
}

pub(crate) fn validate_value(schema: &SchemaNode, value: &Value) -> Result<(), JsonFormatError> {
    let Some(values) = &schema.json_allowed_values else {
        return Ok(());
    };
    if !schema.json_allowed_values_are_valid() {
        return Err(JsonFormatError::InvalidAllowedValuesMetadata {
            reason: format!("`{}` has incompatible allowed values", schema.name),
        });
    }
    if values.matches(value) {
        return Ok(());
    }
    Err(JsonFormatError::AllowedValueMismatch {
        name: schema.name.clone(),
        got: format!("{value:?}"),
    })
}

pub(super) fn render(
    node: &SchemaNode,
    out: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFormatError> {
    let Some(values) = &node.json_allowed_values else {
        return Ok(());
    };
    if !node.json_allowed_values_are_valid() {
        return Err(JsonFormatError::InvalidAllowedValuesMetadata {
            reason: format!("`{}` has incompatible allowed values", node.name),
        });
    }
    let values = values
        .values()
        .iter()
        .map(to_json)
        .collect::<Result<Vec<_>, _>>()?;
    out.insert("enum".into(), serde_json::Value::Array(values));
    Ok(())
}

fn selection_from_values(
    name: &str,
    values: Vec<JsonAllowedValue>,
) -> Result<Selection, JsonFormatError> {
    match values.as_slice() {
        [] => Err(unsupported(name, "enum has no possible scalar values")),
        [JsonAllowedValue::JsonNull] => Err(unsupported(
            name,
            "a null-only const or enum has no distinct ferrule scalar value type",
        )),
        [value] => allowed_to_constant(name, value).map(Selection::Fixed),
        _ => JsonAllowedValues::new(values)
            .map(Selection::Allowed)
            .map_err(|error| unsupported(name, &error.to_string())),
    }
}

fn set_node_values(
    name: &str,
    node: &mut SchemaNode,
    values: Vec<JsonAllowedValue>,
) -> Result<(), JsonFormatError> {
    let selection = selection_from_values(name, values)?;
    match selection {
        Selection::Fixed(constant) => {
            let constrained = super::constraints::schema(&node.name, &constant);
            node.kind = constrained.kind;
            node.fixed = constrained.fixed;
            node.json_allowed_values = None;
            node.nullable = false;
        }
        Selection::Allowed(values) => {
            node.fixed = None;
            node.nullable = values.contains_json_null();
            node.json_allowed_values = Some(values);
        }
    }
    if node.json_allowed_values_are_valid() {
        Ok(())
    } else {
        Err(unsupported(
            name,
            "const or enum values are incompatible with the scalar type",
        ))
    }
}

fn admitted_by_node(node: &SchemaNode, value: &JsonAllowedValue) -> bool {
    match value {
        JsonAllowedValue::String(_) => node.accepts_scalar_type(ScalarType::String),
        JsonAllowedValue::Int(_) => {
            node.accepts_scalar_type(ScalarType::Int) || node.accepts_scalar_type(ScalarType::Float)
        }
        JsonAllowedValue::Float(_) => node.accepts_scalar_type(ScalarType::Float),
        JsonAllowedValue::Bool(_) => node.accepts_scalar_type(ScalarType::Bool),
        JsonAllowedValue::JsonNull => node.nullable,
    }
}

fn value_satisfies_constraints(node: &SchemaNode, value: &JsonAllowedValue) -> bool {
    if !admitted_by_node(node, value) {
        return false;
    }
    let range_matches = match (node.numeric_range, value) {
        (Some(ir::NumericRange::Integer(range)), JsonAllowedValue::Int(value)) => {
            range.contains(*value)
        }
        (Some(ir::NumericRange::Number(range)), JsonAllowedValue::Int(value)) => {
            super::super::exact_f64_from_i64(*value).is_some_and(|value| range.contains(value))
        }
        (Some(ir::NumericRange::Number(range)), JsonAllowedValue::Float(value)) => {
            range.contains(value.get())
        }
        (Some(_), _) => false,
        (None, _) => true,
    };
    if !range_matches {
        return false;
    }
    let multiple_matches = node
        .json_multiple_of
        .as_ref()
        .is_none_or(|constraints| match value {
            JsonAllowedValue::Int(value) => constraints.matches_i64(*value),
            JsonAllowedValue::Float(value) => constraints.matches_f64(value.get()),
            _ => true,
        });
    if !multiple_matches {
        return false;
    }
    let length_matches = node.string_length_range.is_none_or(|range| match value {
        JsonAllowedValue::String(value) => range.contains_str(value),
        _ => true,
    });
    if !length_matches {
        return false;
    }
    node.json_patterns
        .as_ref()
        .is_none_or(|patterns| match value {
            JsonAllowedValue::String(value) => patterns.matches(value),
            _ => true,
        })
}

fn parse_value(name: &str, value: &serde_json::Value) -> Result<JsonAllowedValue, JsonFormatError> {
    match value {
        serde_json::Value::String(value)
            if value.len() <= ir::MAX_JSON_ALLOWED_VALUE_STRING_BYTES =>
        {
            Ok(JsonAllowedValue::String(value.clone()))
        }
        serde_json::Value::String(_) => Err(unsupported(
            name,
            "const or enum string exceeds the bounded allowed-value limit",
        )),
        serde_json::Value::Bool(value) => Ok(JsonAllowedValue::Bool(*value)),
        serde_json::Value::Number(number) if number.as_i64().is_some() => number
            .as_i64()
            .map(JsonAllowedValue::Int)
            .ok_or_else(|| unsupported(name, "enum integer must fit signed 64 bits")),
        serde_json::Value::Number(number) if number.as_u64().is_some() => Err(unsupported(
            name,
            "enum integer is outside ferrule's signed 64-bit range",
        )),
        serde_json::Value::Number(number) => crate::exact_f64_from_json_number(number)
            .and_then(FiniteF64::new)
            .map(JsonAllowedValue::Float)
            .ok_or_else(|| {
                unsupported(name, "enum number must be finite and exactly representable")
            }),
        serde_json::Value::Null => Ok(JsonAllowedValue::JsonNull),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => Err(unsupported(
            name,
            "object and array const or enum values are not representable as scalar allowed values",
        )),
    }
}

fn constant_to_allowed(value: &ScalarConstant) -> JsonAllowedValue {
    match value {
        ScalarConstant::String(value) => JsonAllowedValue::String(value.clone()),
        ScalarConstant::Int(value) => JsonAllowedValue::Int(*value),
        ScalarConstant::Float(value) => JsonAllowedValue::Float(*value),
        ScalarConstant::Bool(value) => JsonAllowedValue::Bool(*value),
    }
}

fn allowed_to_constant(
    name: &str,
    value: &JsonAllowedValue,
) -> Result<ScalarConstant, JsonFormatError> {
    match value {
        JsonAllowedValue::String(value) => Ok(ScalarConstant::String(value.clone())),
        JsonAllowedValue::Int(value) => Ok(ScalarConstant::Int(*value)),
        JsonAllowedValue::Float(value) => Ok(ScalarConstant::Float(*value)),
        JsonAllowedValue::Bool(value) => Ok(ScalarConstant::Bool(*value)),
        JsonAllowedValue::JsonNull => Err(unsupported(
            name,
            "a null-only const or enum has no distinct ferrule scalar value type",
        )),
    }
}

fn allowed_type(value: &JsonAllowedValue) -> Option<ScalarType> {
    match value {
        JsonAllowedValue::String(_) => Some(ScalarType::String),
        JsonAllowedValue::Int(_) => Some(ScalarType::Int),
        JsonAllowedValue::Float(_) => Some(ScalarType::Float),
        JsonAllowedValue::Bool(_) => Some(ScalarType::Bool),
        JsonAllowedValue::JsonNull => None,
    }
}

fn values_equal(left: &JsonAllowedValue, right: &JsonAllowedValue) -> bool {
    left.semantically_equals(right)
}

fn to_json(value: &JsonAllowedValue) -> Result<serde_json::Value, JsonFormatError> {
    match value {
        JsonAllowedValue::String(value) => Ok(value.clone().into()),
        JsonAllowedValue::Int(value) => Ok((*value).into()),
        JsonAllowedValue::Float(value) => serde_json::Number::from_f64(value.get())
            .map(serde_json::Value::Number)
            .ok_or_else(|| JsonFormatError::InvalidAllowedValuesMetadata {
                reason: "allowed float cannot be rendered as a finite JSON number".to_string(),
            }),
        JsonAllowedValue::Bool(value) => Ok((*value).into()),
        JsonAllowedValue::JsonNull => Ok(serde_json::Value::Null),
    }
}

fn unsupported(name: &str, reason: &str) -> JsonFormatError {
    JsonFormatError::UnsupportedSchemaUnion {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}
