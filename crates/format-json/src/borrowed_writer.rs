//! Validated JSON output whose ordinary text and member names borrow the target.
//!
//! Containers retain only output structure. Existing validators still receive
//! temporary owned values when they need a complete normalized predicate input.
//! Those snapshots are released before the final pretty String is rendered.

use std::collections::BTreeMap;

use ir::{Instance, SchemaKind, SchemaNode, Value};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};

use crate::{JsonFormatError, PatternRuntime, RecursiveSchemas, json_schema};

enum Output<'a> {
    String(&'a str),
    Value(serde_json::Value),
    Array(Vec<Output<'a>>),
    Object(Vec<(&'a str, Output<'a>)>),
}

impl Serialize for Output<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::String(value) => serializer.serialize_str(value),
            Self::Value(value) => value.serialize(serializer),
            Self::Array(items) => {
                let mut sequence = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    sequence.serialize_element(item)?;
                }
                sequence.end()
            }
            Self::Object(fields) => {
                let mut object = serializer.serialize_map(Some(fields.len()))?;
                for (name, value) in fields {
                    object.serialize_entry(name, value)?;
                }
                object.end()
            }
        }
    }
}

impl Output<'_> {
    fn validation_value(&self) -> serde_json::Value {
        match self {
            Self::String(value) => serde_json::Value::String((*value).to_owned()),
            Self::Value(value) => value.clone(),
            Self::Array(items) => {
                serde_json::Value::Array(items.iter().map(Self::validation_value).collect())
            }
            Self::Object(fields) => serde_json::Value::Object(
                fields
                    .iter()
                    .map(|(name, value)| ((*name).to_owned(), value.validation_value()))
                    .collect(),
            ),
        }
    }
}

pub(super) fn to_string(
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<String, JsonFormatError> {
    let output = prepare(schema, instance)?;
    let mut text = serde_json::to_string_pretty(&output)?;
    text.push('\n');
    Ok(text)
}

fn prepare<'a>(schema: &SchemaNode, instance: &'a Instance) -> Result<Output<'a>, JsonFormatError> {
    let recursive = RecursiveSchemas::new(schema)?;
    let mut patterns = PatternRuntime::new(schema)?;
    match instance {
        Instance::Repeated(items) if !schema.repeating => {
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(single(schema, item, &recursive, 0, &mut patterns)?);
            }
            Ok(Output::Array(values))
        }
        _ => node(schema, instance, &recursive, 0, &mut patterns),
    }
}

fn node<'a>(
    schema: &SchemaNode,
    instance: &'a Instance,
    recursive: &RecursiveSchemas<'_>,
    recursion_depth: usize,
    patterns: &mut PatternRuntime,
) -> Result<Output<'a>, JsonFormatError> {
    let resolved;
    let (schema, recursion_depth) = if schema.recursive_ref.is_some() {
        let (value, next_depth) = recursive.resolve(schema, recursion_depth)?;
        resolved = value;
        (&resolved, next_depth)
    } else {
        (schema, recursion_depth)
    };
    if schema.container_nullable && matches!(instance, Instance::Scalar(Value::JsonNull(_))) {
        return Ok(Output::Value(serde_json::Value::Null));
    }
    if schema.repeating {
        let items = match instance {
            Instance::Repeated(items) | Instance::MappedSequence(items) => items,
            _ => {
                return Err(crate::write_shape_error(
                    schema,
                    "array",
                    crate::instance_type_name(instance),
                ));
            }
        };
        json_schema::item_counts::validate_len(schema, items.len())?;
        let mut values = Vec::with_capacity(items.len());
        for item in items {
            values.push(single(schema, item, recursive, recursion_depth, patterns)?);
        }
        if schema.json_contains.is_some() || schema.json_unique_items {
            let validation = values
                .iter()
                .map(Output::validation_value)
                .collect::<Vec<_>>();
            json_schema::contains::validate_values(schema, &validation, patterns)?;
            json_schema::unique_items::validate(schema, &validation)?;
        }
        return Ok(Output::Array(values));
    }
    single(schema, instance, recursive, recursion_depth, patterns)
}

fn single<'a>(
    schema: &SchemaNode,
    instance: &'a Instance,
    recursive: &RecursiveSchemas<'_>,
    recursion_depth: usize,
    patterns: &mut PatternRuntime,
) -> Result<Output<'a>, JsonFormatError> {
    let resolved;
    let (schema, recursion_depth) = if schema.recursive_ref.is_some() {
        let (value, next_depth) = recursive.resolve(schema, recursion_depth)?;
        resolved = value;
        (&resolved, next_depth)
    } else {
        (schema, recursion_depth)
    };
    let instance = match instance {
        Instance::MappedSequence(items) => match items.as_slice() {
            [item] => item,
            _ => {
                return Err(crate::write_shape_error(
                    schema,
                    "one mapped item",
                    "mapped sequence",
                ));
            }
        },
        item => item,
    };
    if schema.json_any {
        return match instance {
            Instance::Scalar(Value::String(text)) => match serde_json::from_str(text) {
                Ok(value) => Ok(Output::Value(value)),
                Err(_) => Ok(Output::String(text)),
            },
            _ => crate::write_json_any(schema, instance).map(Output::Value),
        };
    }
    if schema.container_nullable && matches!(instance, Instance::Scalar(Value::JsonNull(_))) {
        return Ok(Output::Value(serde_json::Value::Null));
    }
    match (&schema.kind, instance) {
        (SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. }, Instance::Scalar(value)) => {
            scalar(schema, value, patterns)
        }
        (
            SchemaKind::Group {
                children,
                alternatives,
                required,
                dynamic,
                ..
            },
            Instance::Group(fields),
        ) => {
            if dynamic.is_some() && !alternatives.is_empty() {
                return Err(JsonFormatError::UnsupportedSchemaUnion {
                    name: schema.name.clone(),
                    reason: "open objects cannot use closed object alternatives".to_owned(),
                });
            }
            let mut out: Vec<(&str, Output<'a>)> = Vec::with_capacity(fields.len());
            let mut positions: BTreeMap<&str, usize> = BTreeMap::new();
            if let Some(dynamic) = dynamic {
                for (name, child_instance) in fields {
                    if positions.contains_key(name.as_str()) {
                        return Err(JsonFormatError::DuplicateProperty {
                            object: schema.name.clone(),
                            property: name.clone(),
                        });
                    }
                    let (child_schema, is_dynamic) =
                        match children.iter().find(|child| child.name == *name) {
                            Some(child) => (child, false),
                            None => (dynamic.as_ref(), true),
                        };
                    if crate::is_boundary_absence(child_schema, child_instance) {
                        continue;
                    }
                    if is_dynamic {
                        patterns.validate_dynamic_property_name(schema, name)?;
                    }
                    let value = node(
                        child_schema,
                        child_instance,
                        recursive,
                        recursion_depth,
                        patterns,
                    )?;
                    positions.insert(name.as_str(), out.len());
                    out.push((name.as_str(), value));
                }
            } else {
                for child_schema in children {
                    if let Some((name, child_instance)) =
                        fields.iter().find(|(name, _)| name == &child_schema.name)
                    {
                        if crate::is_boundary_absence(child_schema, child_instance) {
                            continue;
                        }
                        let value = node(
                            child_schema,
                            child_instance,
                            recursive,
                            recursion_depth,
                            patterns,
                        )?;
                        // Map::insert keeps an existing insertion slot. Closed
                        // objects also keep their first matching target field.
                        if let Some(position) = positions.get(name.as_str()) {
                            out[*position].1 = value;
                        } else {
                            positions.insert(name.as_str(), out.len());
                            out.push((name.as_str(), value));
                        }
                    }
                }
            }
            for (property, _) in &out {
                patterns.validate_property_name(schema, property)?;
            }
            crate::validate_required_fields(schema, required, |name| positions.contains_key(name))?;
            json_schema::property_dependencies::validate_properties(
                schema,
                out.iter().map(|(name, _)| *name),
            )?;
            let output = Output::Object(out);
            let validation = (!alternatives.is_empty() || schema.json_dependent_schemas.is_some())
                .then(|| output.validation_value());
            if dynamic.is_none()
                && let Some(value) = &validation
            {
                crate::validate_alternative_fields(
                    schema,
                    alternatives,
                    value.as_object().expect("object validation snapshot"),
                )?;
            }
            json_schema::property_counts::validate_len(schema, positions.len())?;
            if let Some(value) = &validation {
                json_schema::dependent_schemas::validate_object(
                    schema,
                    value,
                    value
                        .as_object()
                        .into_iter()
                        .flat_map(serde_json::Map::keys),
                    patterns,
                )?;
            }
            Ok(output)
        }
        (SchemaKind::Scalar { ty }, other) => Err(crate::write_shape_error(
            schema,
            crate::scalar_type_name(*ty),
            crate::instance_type_name(other),
        )),
        (SchemaKind::ScalarUnion { .. }, other) => Err(crate::write_shape_error(
            schema,
            "declared scalar union",
            crate::instance_type_name(other),
        )),
        (SchemaKind::Group { .. }, other) => Err(crate::write_shape_error(
            schema,
            "object",
            crate::instance_type_name(other),
        )),
    }
}

fn scalar<'a>(
    schema: &SchemaNode,
    value: &'a Value,
    patterns: &mut PatternRuntime,
) -> Result<Output<'a>, JsonFormatError> {
    // This domain is already normalized: read_scalar/read_scalar_union return
    // the same text. Without scalar assertions every following validator is a
    // no-op, so no owned text is needed to call their Value-based APIs.
    if let Value::String(text) = value
        && schema.accepts_scalar_type(ir::ScalarType::String)
    {
        if schema.fixed.is_some()
            || schema.json_allowed_values.is_some()
            || schema.numeric_range.is_some()
            || schema.json_multiple_of.is_some()
            || schema.string_length_range.is_some()
            || schema.json_patterns.is_some()
        {
            validate_scalar(schema, &serde_json::Value::String(text.clone()), patterns)?;
        }
        return Ok(Output::String(text));
    }
    let output = match schema.kind {
        SchemaKind::Scalar { ty } => crate::write_scalar(value, ty, schema.nullable, &schema.name)?,
        SchemaKind::ScalarUnion { types } => {
            crate::write_scalar_union(value, types, schema.nullable, &schema.name)?
        }
        SchemaKind::Group { .. } => unreachable!("scalar called only for a scalar schema"),
    };
    validate_scalar(schema, &output, patterns)?;
    Ok(Output::Value(output))
}

fn validate_scalar(
    schema: &SchemaNode,
    output: &serde_json::Value,
    patterns: &mut PatternRuntime,
) -> Result<(), JsonFormatError> {
    let normalized = match schema.kind {
        SchemaKind::Scalar { ty } => {
            json_schema::constraints::validate_json(schema, output)?;
            crate::read_scalar(output, ty, schema.nullable, &schema.name)?
        }
        SchemaKind::ScalarUnion { types } => {
            crate::read_scalar_union(output, types, schema.nullable, &schema.name)?
        }
        SchemaKind::Group { .. } => unreachable!("validated only scalar output"),
    };
    json_schema::allowed_values::validate_value(schema, &normalized)?;
    json_schema::ranges::validate_json(schema, output)?;
    json_schema::multiples::validate_json(schema, output)?;
    json_schema::string_lengths::validate_json(schema, output)?;
    patterns.validate_json(schema, output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::ScalarType;

    #[test]
    fn ordinary_text_and_member_names_borrow_the_target() {
        let schema =
            SchemaNode::group("Root", vec![SchemaNode::scalar("Text", ScalarType::String)]);
        let text = "x".repeat(65_536);
        let instance = Instance::Group(
            vec![("Text".to_owned(), Instance::Scalar(Value::String(text)))].into(),
        );
        let output = prepare(&schema, &instance).unwrap();
        let Output::Object(fields) = output else {
            panic!("expected object")
        };
        let Instance::Group(original) = &instance else {
            unreachable!()
        };
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0.as_ptr(), original[0].0.as_ptr());
        let Output::String(borrowed) = &fields[0].1 else {
            panic!("ordinary text was owned")
        };
        let Instance::Scalar(Value::String(original)) = &original[0].1 else {
            unreachable!()
        };
        assert_eq!(borrowed.as_ptr(), original.as_ptr());
        assert_eq!(borrowed.len(), original.len());
    }

    #[test]
    fn constrained_text_is_borrowed_after_unchanged_validation() {
        let mut schema = SchemaNode::scalar("Root", ScalarType::String);
        schema.fixed = Some("ok".to_owned());
        let instance = Instance::Scalar(Value::String("ok".to_owned()));
        let Output::String(borrowed) = prepare(&schema, &instance).unwrap() else {
            panic!("text was owned")
        };
        let Instance::Scalar(Value::String(original)) = &instance else {
            unreachable!()
        };
        assert_eq!(borrowed.as_ptr(), original.as_ptr());
    }
}
