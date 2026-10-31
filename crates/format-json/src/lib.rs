//! JSON Schema import and JSON instance read/write.
//!
//! Shaping rules: a [`SchemaKind::Group`] is a JSON object; a child marked
//! `repeating` holds a JSON array of that child's shape (a missing repeating
//! field reads as empty, matching the XML reader's zero-match behavior);
//! scalars map per [`ScalarType`], with explicit JSON `null` accepted only
//! when the scalar or its object/array container is nullable. Absent nullable
//! containers, explicit nulls, and empty objects/arrays remain distinct.
//! Unconstrained dynamic properties retain arbitrary values as canonical JSON
//! text in the graph's string domain and restore them at the output boundary.
//! A root array of objects can reuse a non-repeating object schema as flat
//! rows; nested arrays still require a repeating child schema.

mod borrowed_writer;
pub mod json5_boundary;
mod json5_finite;
mod json5_unique;
pub mod json_schema;
#[doc(hidden)]
pub mod json_tuple;
mod null_layers;
mod prepared;
mod reader;
#[doc(hidden)]
pub mod runtime_constraints;
pub mod source_document;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{
    GroupAlternativeConstraintValue, GroupAlternativeMode, Instance, ScalarType, ScalarTypeSet,
    SchemaKind, SchemaNode, Value, parse_exact_decimal_i64,
};
use thiserror::Error;

pub use prepared::PreparedSchema;
#[cfg(test)]
use reader::read_node;
pub use reader::{from_json5_str, from_lines, from_str, read, read_json5, read_lines};
use reader::{
    from_value_with, read_node_with_patterns, read_repeated, read_scalar, read_scalar_union,
    strip_utf8_bom,
};
use runtime_constraints::{PatternPrograms, PatternRuntime};

pub(crate) const MAX_JSON5_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
const MAX_JSON_RECURSION_DEPTH: usize = 64;

#[derive(Debug, Error)]
pub enum JsonFormatError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("JSON5 error: {0}")]
    Json5(#[from] json5::Error),
    #[error("JSON5 nesting exceeds {limit} levels")]
    Json5NestingLimit { limit: usize },
    #[error("JSON5 document exceeds the {limit}-byte input limit")]
    Json5DocumentLimit { limit: usize },
    #[error("JSON5 exact validation exceeds the {limit}-byte document limit")]
    Json5ExactLimit { limit: usize },
    #[error("JSON5 exact validation cannot normalize this document: {reason}")]
    Json5ExactUnsupported { reason: &'static str },
    #[error("`{name}`: expected {expected}, got {got}")]
    Shape {
        name: String,
        expected: &'static str,
        got: &'static str,
    },
    #[error("JSON Schema union `{name}` is not representable: {reason}")]
    UnsupportedSchemaUnion { name: String, reason: String },
    #[error("JSON Schema object `{name}` is not representable: {reason}")]
    UnsupportedSchemaObject { name: String, reason: String },
    #[error("JSON Schema resource `{reference}` from `{base}` cannot be loaded: {reason}")]
    SchemaResource {
        reference: String,
        base: std::path::PathBuf,
        reason: String,
    },
    #[error("JSON Schema resource graph exceeds the {limit} {kind} limit")]
    SchemaResourceLimit { kind: &'static str, limit: usize },
    #[error("JSON Schema resource `{}` is not valid UTF-8: {error}", resource.display())]
    SchemaEncoding {
        resource: std::path::PathBuf,
        #[source]
        error: std::str::Utf8Error,
    },
    #[error("JSON Schema `{name}` describes no JSON value: {reason}")]
    ContentlessSchema {
        name: String,
        reason: ContentlessSchemaReason,
    },
    #[error("object `{name}` matches no declared schema alternative")]
    NoMatchingAlternative { name: String },
    #[error("object `{name}` matches more than one declared schema alternative")]
    AmbiguousAlternative { name: String },
    #[error("object `{object}` contains duplicate property `{property}`")]
    DuplicateProperty { object: String, property: String },
    #[error("object `{object}` requires property `{property}`")]
    MissingRequiredProperty { object: String, property: String },
    #[error(
        "object `{object}` contains trigger property `{trigger}` but requires dependent property `{property}`"
    )]
    MissingDependentProperty {
        object: String,
        trigger: String,
        property: String,
    },
    #[error(
        "object `{object}` contains trigger property `{trigger}` but does not satisfy its dependent schema"
    )]
    DependentSchemaMismatch { object: String, trigger: String },
    #[error("object `{object}` contains property name `{property}` rejected by its schema")]
    InvalidPropertyName { object: String, property: String },
    #[error("closed object `{object}` does not declare property `{property}`")]
    UndeclaredProperty { object: String, property: String },
    #[error(
        "object `{object}` contains dynamic property `{property}` that matches no patternProperties selector"
    )]
    UnmatchedPatternProperty { object: String, property: String },
    #[error("`{name}` requires constant {expected}, got {got}")]
    ConstantMismatch {
        name: String,
        expected: String,
        got: String,
    },
    #[error("`{name}` is not one of its allowed JSON values, got {got}")]
    AllowedValueMismatch { name: String, got: String },
    #[error("`{name}` requires numeric range {range}, got {got}")]
    RangeMismatch {
        name: String,
        range: String,
        got: String,
    },
    #[error("`{name}` requires a value divisible by {divisors}, got {got}")]
    MultipleOfMismatch {
        name: String,
        divisors: String,
        got: String,
    },
    #[error("`{name}` requires {range} array items, got {got}")]
    ItemCountMismatch {
        name: String,
        range: String,
        got: usize,
    },
    #[error("`{name}` requires {range} items matching its contains predicate, got {got}")]
    ContainsCountMismatch {
        name: String,
        range: String,
        got: usize,
    },
    #[error("`{name}` requires {range} object properties, got {got}")]
    PropertyCountMismatch {
        name: String,
        range: String,
        got: usize,
    },
    #[error(
        "`{name}` requires unique array items, but indexes {first_index} and {duplicate_index} are equal"
    )]
    UniqueItemsMismatch {
        name: String,
        first_index: usize,
        duplicate_index: usize,
    },
    #[error("JSON uniqueItems validation for `{name}` exceeds the {max} {resource} limit")]
    UniqueItemsLimit {
        name: String,
        resource: &'static str,
        max: usize,
    },
    #[error("`{name}` requires string length {range}, got {got} Unicode scalar values")]
    StringLengthMismatch {
        name: String,
        range: String,
        got: usize,
    },
    #[error("`{name}` does not match its JSON Schema pattern constraints")]
    PatternMismatch { name: String },
    #[error("JSON pattern metadata is invalid: {reason}")]
    InvalidPatternMetadata { reason: String },
    #[error("JSON allowed-value metadata is invalid: {reason}")]
    InvalidAllowedValuesMetadata { reason: String },
    #[error("JSON multipleOf metadata is invalid: {reason}")]
    InvalidMultipleOfMetadata { reason: String },
    #[error("JSON numeric-range metadata is invalid: {reason}")]
    InvalidNumericRangeMetadata { reason: String },
    #[error("JSON uniqueItems metadata is invalid: {reason}")]
    InvalidUniqueItemsMetadata { reason: String },
    #[error("JSON property-count metadata is invalid: {reason}")]
    InvalidPropertyCountMetadata { reason: String },
    #[error("JSON property-dependency metadata is invalid: {reason}")]
    InvalidPropertyDependenciesMetadata { reason: String },
    #[error("JSON dependent-schema metadata is invalid: {reason}")]
    InvalidDependentSchemasMetadata { reason: String },
    #[error("JSON property-name metadata is invalid: {reason}")]
    InvalidPropertyNameMetadata { reason: String },
    #[error("JSON contains metadata is invalid: {reason}")]
    InvalidContainsMetadata { reason: String },
    #[error("JSON pattern matching for `{name}` exceeds the bounded work limit")]
    PatternWorkLimit { name: String },
    #[error("JSON Lines cannot encode nullable array container `{name}`")]
    NullableJsonLinesContainer { name: String },
    #[error("recursive JSON schema reference `{node}` is malformed")]
    InvalidRecursiveReference { node: String },
    #[error("XML attribute-use metadata on `{node}` requires one named singular scalar attribute")]
    InvalidXmlAttributeRequired { node: String },
    #[error("JSON array-wrapper metadata on `{name}` is invalid")]
    InvalidArrayWrapperMetadata { name: String },
    #[error("JSON tuple frame metadata on `{name}` is invalid: {reason}")]
    InvalidJsonTupleFrameMetadata {
        name: String,
        reason: ir::JsonTupleFrameError,
    },
    #[error(
        "recursive JSON schema reference `{node}` expands tuple frame `{frame}` through one of its own positions; recursive tuple frames are not supported"
    )]
    RecursiveJsonTupleFrame { node: String, frame: String },
    #[error("`{name}` requires a tuple of {range} items, got {got}")]
    TupleLengthMismatch {
        name: String,
        range: String,
        got: usize,
    },
    #[error(
        "`{name}` cannot write tuple position {position} as absent before a later present item"
    )]
    TupleGap { name: String, position: usize },
    #[error("`{name}` tuple position {position}: {error}")]
    TuplePosition {
        name: String,
        position: usize,
        error: Box<JsonFormatError>,
    },
    #[error("tuple `{name}` does not declare field `{field}`")]
    UndeclaredTupleField { name: String, field: String },
    #[error("non-repeating tuple frame root `{name}` cannot hold root rows or JSON Lines records")]
    TupleRootRows { name: String },
    /// `.mfd` export refuses frames until design entries can address tuple
    /// positions; JSON Schema export itself writes them.
    #[error(
        "schema `{schema}` contains a JSON tuple frame; `.mfd` export cannot address tuple positions yet"
    )]
    UnsupportedJsonTupleFrameExport { schema: String },
    #[error("JSON object-item nullability metadata on `{name}` is invalid")]
    InvalidObjectItemNullabilityMetadata { name: String },
    #[error("JSON nullability metadata on `{name}` is invalid: {reason}")]
    InvalidNullabilityMetadata {
        name: String,
        reason: ir::EffectiveViewError,
    },
    #[error(
        "recursive JSON schema reference `{node}` has no unique concrete group anchor `{anchor}`"
    )]
    MissingRecursiveAnchor { node: String, anchor: String },
    #[error("JSON recursion exceeds the {limit}-reference depth limit")]
    RecursionLimit { limit: usize },
}

/// Why a JSON Schema position cannot describe any JSON value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ContentlessSchemaReason {
    /// The position holds a JSON value that is neither an object nor a boolean.
    #[error("a schema must be an object or a boolean, but this one is {found}")]
    NotASchema { found: &'static str },
    /// The root `$ref` chain returns to a reference it is already following.
    #[error("its `$ref` chain is a cycle that never reaches a schema with content")]
    ReferenceCycle,
    /// The root `$ref` chain reaches a JSON Pointer with no target.
    #[error("its `$ref` chain reaches a reference that does not resolve")]
    UnresolvedReference,
}

struct RecursiveSchemas<'a> {
    anchors: BTreeMap<String, &'a SchemaNode>,
}

impl<'a> RecursiveSchemas<'a> {
    /// The schema preflight shared by every native reader and writer,
    /// including prepared schemas, JSON Lines, JSON5 and the borrowed writer.
    fn new(root: &'a SchemaNode) -> Result<Self, JsonFormatError> {
        let mut anchors = BTreeMap::new();
        let mut duplicates = BTreeSet::new();
        let mut references = Vec::new();
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if !node.json_object_item_nullable_is_valid() {
                return Err(JsonFormatError::InvalidObjectItemNullabilityMetadata {
                    name: node.name.clone(),
                });
            }
            if !node.json_array_wrapper_is_valid() {
                return Err(JsonFormatError::InvalidArrayWrapperMetadata {
                    name: node.name.clone(),
                });
            }
            if let Err(reason) = node.check_json_tuple_frame() {
                return Err(JsonFormatError::InvalidJsonTupleFrameMetadata {
                    name: node.name.clone(),
                    reason,
                });
            }
            // Every value read or written below derives its null admission
            // from the effective JSON view; refuse contradictions up front.
            null_layers::layers(node)?;
            if !node.xml_attribute_required_is_valid() {
                return Err(JsonFormatError::InvalidXmlAttributeRequired {
                    node: node.name.clone(),
                });
            }
            if let Some(anchor) = &node.recursive_ref {
                if !node.recursive_ref_is_valid() || !Self::supported_occurrence_metadata(node) {
                    return Err(JsonFormatError::InvalidRecursiveReference {
                        node: node.name.clone(),
                    });
                }
                references.push((node.name.clone(), anchor.clone()));
            } else if matches!(node.kind, SchemaKind::Group { .. })
                && anchors.insert(node.name.clone(), node).is_some()
            {
                duplicates.insert(node.name.clone());
            }
            if let SchemaKind::Group {
                children, dynamic, ..
            } = &node.kind
            {
                pending.extend(children.iter());
                if let Some(dynamic) = dynamic {
                    pending.push(dynamic);
                }
            }
        }
        for (node, anchor) in &references {
            if !anchors.contains_key(anchor) || duplicates.contains(anchor) {
                return Err(JsonFormatError::MissingRecursiveAnchor {
                    node: node.clone(),
                    anchor: anchor.clone(),
                });
            }
        }
        if !references.is_empty() {
            json_tuple::refuse_recursive_frames(root, &anchors)?;
        }
        Ok(Self { anchors })
    }

    fn supported_occurrence_metadata(node: &SchemaNode) -> bool {
        // Only the JSON object/array occurrence metadata copied in `resolve`
        // may be set here. A marker with additional assertions cannot silently
        // inherit the anchor's different assertions instead.
        !node.json_any
            && node.fixed.is_none()
            && !node.nullable
            && node.json_allowed_values.is_none()
            && node.numeric_range.is_none()
            && node.json_multiple_of.is_none()
            && node.json_dependent_schemas.is_none()
            && node.property_count_range.is_none()
            && node.json_property_dependencies.is_none()
            && node.json_pattern_property_names.is_none()
            && node.json_property_names.is_none()
            && node.string_length_range.is_none()
            && node.json_patterns.is_none()
            && node.json_formats.is_empty()
            && (node.repeating
                || (node.item_count_range.is_none()
                    && node.json_contains.is_none()
                    && !node.json_unique_items))
    }

    fn resolve(
        &self,
        occurrence: &SchemaNode,
        recursion_depth: usize,
    ) -> Result<(SchemaNode, usize), JsonFormatError> {
        let Some(anchor) = &occurrence.recursive_ref else {
            unreachable!("resolve is only called for a recursive occurrence");
        };
        if recursion_depth >= MAX_JSON_RECURSION_DEPTH {
            return Err(JsonFormatError::RecursionLimit {
                limit: MAX_JSON_RECURSION_DEPTH,
            });
        }
        let mut resolved =
            (*self
                .anchors
                .get(anchor)
                .ok_or_else(|| JsonFormatError::MissingRecursiveAnchor {
                    node: occurrence.name.clone(),
                    anchor: anchor.clone(),
                })?)
            .clone();
        resolved.name.clone_from(&occurrence.name);
        resolved.repeating = occurrence.repeating;
        resolved.container_nullable = occurrence.container_nullable;
        resolved.json_object_item_nullable = occurrence.json_object_item_nullable;
        resolved
            .item_count_range
            .clone_from(&occurrence.item_count_range);
        resolved.json_contains.clone_from(&occurrence.json_contains);
        resolved.json_unique_items = occurrence.json_unique_items;
        Ok((resolved, recursion_depth + 1))
    }
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn exact_f64_from_i64(value: i64) -> Option<f64> {
    let converted = value as f64;
    ((converted as i128) == i128::from(value)).then_some(converted)
}

fn exact_f64_from_u64(value: u64) -> Option<f64> {
    let converted = value as f64;
    ((converted as u128) == u128::from(value)).then_some(converted)
}

pub(crate) fn exact_f64_from_json_number(number: &serde_json::Number) -> Option<f64> {
    if let Some(value) = number.as_i64() {
        return exact_f64_from_i64(value);
    }
    if let Some(value) = number.as_u64() {
        return exact_f64_from_u64(value);
    }
    number.as_f64().filter(|value| value.is_finite())
}

fn json_number_matches_f64(number: &serde_json::Number, expected: f64) -> bool {
    if let Some(value) = number.as_i64() {
        return exact_f64_from_i64(value) == Some(expected);
    }
    if let Some(value) = number.as_u64() {
        return exact_f64_from_u64(value) == Some(expected);
    }
    number.as_f64() == Some(expected)
}

/// Writes an [`Instance`] tree shaped by `schema` to a pretty-printed JSON
/// file.
pub fn write(path: &Path, schema: &SchemaNode, instance: &Instance) -> Result<(), JsonFormatError> {
    std::fs::write(path, to_string(schema, instance)?)?;
    Ok(())
}

/// Writes a JSON5 document, using ordinary JSON schema validation first.
pub fn write_json5(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<(), JsonFormatError> {
    std::fs::write(path, to_json5_string(schema, instance)?)?;
    Ok(())
}

/// Writes a repeated instance as JSON Lines using one compact value per line.
pub fn write_lines(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<(), JsonFormatError> {
    std::fs::write(path, to_lines(schema, instance)?)?;
    Ok(())
}

/// Writes an [`Instance`] tree shaped by `schema` as pretty-printed JSON.
///
/// The returned document ends with a newline, matching [`write`]. This is
/// the in-memory counterpart used by hosts without filesystem access.
pub fn to_string(schema: &SchemaNode, instance: &Instance) -> Result<String, JsonFormatError> {
    borrowed_writer::to_string(schema, instance)
}

fn owned_to_string(schema: &SchemaNode, instance: &Instance) -> Result<String, JsonFormatError> {
    let value = to_value(schema, instance)?;
    let mut text = serde_json::to_string_pretty(&value)?;
    text.push('\n');
    Ok(text)
}

/// Normalizes an [`Instance`] into schema-shaped JSON without a text round trip.
///
/// This applies the same output coercions and validation as [`to_string`].
/// Hosts can inspect or validate the normalized scalar values without parsing
/// their serialized floating-point representation a second time.
pub fn to_value(
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<serde_json::Value, JsonFormatError> {
    let recursive = RecursiveSchemas::new(schema)?;
    let programs = PatternPrograms::new(schema)?;
    to_value_with(schema, instance, &recursive, &mut programs.runtime())
}

fn to_value_with(
    schema: &SchemaNode,
    instance: &Instance,
    recursive: &RecursiveSchemas<'_>,
    patterns: &mut PatternRuntime,
) -> Result<serde_json::Value, JsonFormatError> {
    // A root scope can produce flat rows even though the row schema itself
    // is not repeating (the same convention used by CSV). Preserve that
    // established JSON-output shape while keeping nested nodes
    // schema-directed.
    let value = match instance {
        Instance::Repeated(items) if !schema.repeating => {
            json_tuple::refuse_root_rows(schema)?;
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(write_single_node_with_patterns(
                    schema, item, recursive, 0, patterns,
                )?);
            }
            serde_json::Value::Array(values)
        }
        _ => write_node_with_patterns(schema, instance, recursive, 0, patterns)?,
    };
    Ok(value)
}

/// Serializes a typed instance as JSON5. Safe property names are unquoted;
/// comments are intentionally not generated.
pub fn to_json5_string(
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<String, JsonFormatError> {
    let strict = owned_to_string(schema, instance)?;
    let value: serde_json::Value = serde_json::from_str(&strict)?;
    let mut text = json5::to_string(&value)?;
    text.push('\n');
    Ok(text)
}

/// Serializes an instance as JSON Lines using one compact root value per
/// line. A non-repeated instance becomes a single line.
pub fn to_lines(schema: &SchemaNode, instance: &Instance) -> Result<String, JsonFormatError> {
    reject_nullable_json_lines_container(schema)?;
    let recursive = RecursiveSchemas::new(schema)?;
    let programs = PatternPrograms::new(schema)?;
    let mut patterns = programs.runtime();
    let values = match instance {
        Instance::Repeated(items) => {
            if schema.repeating {
                runtime_constraints::validate_item_count(schema, items.len())?;
            }
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(write_single_node_with_patterns(
                    schema,
                    item,
                    &recursive,
                    0,
                    &mut patterns,
                )?);
            }
            values
        }
        item => {
            if schema.repeating {
                runtime_constraints::validate_item_count(schema, 1)?;
            }
            vec![write_single_node_with_patterns(
                schema,
                item,
                &recursive,
                0,
                &mut patterns,
            )?]
        }
    };
    if schema.repeating {
        runtime_constraints::validate_contains(schema, &values, &mut patterns)?;
        runtime_constraints::unique_items::validate(schema, &values)?;
    }
    let mut text = String::new();
    for value in values {
        text.push_str(&serde_json::to_string(&value)?);
        text.push('\n');
    }
    Ok(text)
}

fn reject_nullable_json_lines_container(schema: &SchemaNode) -> Result<(), JsonFormatError> {
    json_tuple::refuse_root_rows(schema)?;
    if schema.repeating && null_layers::instance_null(schema)?.whole {
        return Err(JsonFormatError::NullableJsonLinesContainer {
            name: schema.name.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
fn write_node(
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<serde_json::Value, JsonFormatError> {
    let recursive = RecursiveSchemas::new(schema)?;
    let programs = PatternPrograms::new(schema)?;
    let mut patterns = programs.runtime();
    write_node_with_patterns(schema, instance, &recursive, 0, &mut patterns)
}

fn write_node_with_patterns(
    schema: &SchemaNode,
    instance: &Instance,
    recursive: &RecursiveSchemas<'_>,
    recursion_depth: usize,
    patterns: &mut PatternRuntime,
) -> Result<serde_json::Value, JsonFormatError> {
    let resolved;
    let (schema, recursion_depth) = if schema.recursive_ref.is_some() {
        let (value, next_depth) = recursive.resolve(schema, recursion_depth)?;
        resolved = value;
        (&resolved, next_depth)
    } else {
        (schema, recursion_depth)
    };
    if matches!(instance, Instance::Scalar(Value::JsonNull(_)))
        && null_layers::instance_null(schema)?.whole
    {
        return Ok(serde_json::Value::Null);
    }
    if schema.repeating {
        let items = match instance {
            Instance::Repeated(items) | Instance::MappedSequence(items) => items,
            _ => {
                return Err(write_shape_error(
                    schema,
                    "array",
                    instance_type_name(instance),
                ));
            }
        };
        runtime_constraints::validate_item_count(schema, items.len())?;
        let mut values = Vec::with_capacity(items.len());
        for item in items {
            values.push(write_single_node_with_patterns(
                schema,
                item,
                recursive,
                recursion_depth,
                patterns,
            )?);
        }
        runtime_constraints::validate_contains(schema, &values, patterns)?;
        runtime_constraints::unique_items::validate(schema, &values)?;
        return Ok(serde_json::Value::Array(values));
    }
    write_single_node_with_patterns(schema, instance, recursive, recursion_depth, patterns)
}

fn write_single_node_with_patterns(
    schema: &SchemaNode,
    instance: &Instance,
    recursive: &RecursiveSchemas<'_>,
    recursion_depth: usize,
    patterns: &mut PatternRuntime,
) -> Result<serde_json::Value, JsonFormatError> {
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
                return Err(write_shape_error(
                    schema,
                    "one mapped item",
                    "mapped sequence",
                ));
            }
        },
        item => item,
    };
    if let Some(item) = schema.json_array_wrapper_item() {
        let Instance::Group(fields) = instance else {
            return Err(write_shape_error(
                schema,
                "array item group with one `item` field",
                instance_type_name(instance),
            ));
        };
        if fields.len() != 1 {
            return Err(write_shape_error(
                schema,
                "array item group with one `item` field",
                instance_type_name(instance),
            ));
        }
        let Some((_, value)) = fields.iter().find(|(name, _)| name == &item.name) else {
            return Err(write_shape_error(
                schema,
                "array item group with one `item` field",
                instance_type_name(instance),
            ));
        };
        return write_node_with_patterns(item, value, recursive, recursion_depth, patterns);
    }
    if schema.json_any {
        return write_json_any(schema, instance);
    }
    let null = null_layers::instance_null(schema)?;
    if null.single && matches!(instance, Instance::Scalar(Value::JsonNull(_))) {
        return Ok(serde_json::Value::Null);
    }
    if let Some(roles) = json_tuple::roles(schema) {
        let items = json_tuple::plan(schema, roles, instance)?;
        let mut values = Vec::with_capacity(items.len());
        for (position, item_schema, item) in items {
            values.push(json_tuple::at_position(
                schema,
                position,
                write_single_node_with_patterns(
                    item_schema,
                    item,
                    recursive,
                    recursion_depth,
                    patterns,
                ),
            )?);
        }
        return Ok(serde_json::Value::Array(values));
    }
    match (&schema.kind, instance) {
        (SchemaKind::Scalar { ty }, Instance::Scalar(value)) => {
            let value = write_scalar(value, *ty, null.value, &schema.name)?;
            runtime_constraints::validate_constant(schema, &value)?;
            let normalized = read_scalar(&value, *ty, null.value, &schema.name)?;
            runtime_constraints::validate_allowed_value(schema, &normalized)?;
            runtime_constraints::validate_numeric_range(schema, &value)?;
            runtime_constraints::validate_multiple_of(schema, &value)?;
            runtime_constraints::validate_string_length(schema, &value)?;
            patterns.validate_json(schema, &value)?;
            Ok(value)
        }
        (SchemaKind::ScalarUnion { types }, Instance::Scalar(value)) => {
            let value = write_scalar_union(value, *types, null.value, &schema.name)?;
            let normalized = read_scalar_union(&value, *types, null.value, &schema.name)?;
            runtime_constraints::validate_allowed_value(schema, &normalized)?;
            runtime_constraints::validate_numeric_range(schema, &value)?;
            runtime_constraints::validate_multiple_of(schema, &value)?;
            runtime_constraints::validate_string_length(schema, &value)?;
            patterns.validate_json(schema, &value)?;
            Ok(value)
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
                    reason: "open objects cannot use closed object alternatives".to_string(),
                });
            }
            let mut out = serde_json::Map::with_capacity(fields.len());
            if let Some(dynamic) = dynamic {
                for (name, child_instance) in fields {
                    if out.contains_key(name) {
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
                    if is_boundary_absence(child_schema, child_instance)? {
                        continue;
                    }
                    if is_dynamic {
                        patterns.validate_dynamic_property_name(schema, name)?;
                    }
                    out.insert(
                        name.clone(),
                        write_node_with_patterns(
                            child_schema,
                            child_instance,
                            recursive,
                            recursion_depth,
                            patterns,
                        )?,
                    );
                }
                for property in out.keys() {
                    patterns.validate_property_name(schema, property)?;
                }
                validate_required_fields(schema, required, |name| out.contains_key(name))?;
                runtime_constraints::validate_property_dependencies(
                    schema,
                    out.keys().map(String::as_str),
                )?;
                runtime_constraints::validate_property_count(schema, out.len())?;
                let value = serde_json::Value::Object(out);
                runtime_constraints::validate_dependent_schemas(
                    schema,
                    &value,
                    value
                        .as_object()
                        .into_iter()
                        .flat_map(serde_json::Map::keys),
                    patterns,
                )?;
                return Ok(value);
            }
            for child_schema in children {
                if let Some((_, child_instance)) =
                    fields.iter().find(|(n, _)| n == &child_schema.name)
                {
                    // A non-nullable Null scalar is boundary-level absence.
                    // Nullable scalars retain an explicit JSON null.
                    if is_boundary_absence(child_schema, child_instance)? {
                        continue;
                    }
                    out.insert(
                        child_schema.name.clone(),
                        write_node_with_patterns(
                            child_schema,
                            child_instance,
                            recursive,
                            recursion_depth,
                            patterns,
                        )?,
                    );
                }
            }
            for property in out.keys() {
                patterns.validate_property_name(schema, property)?;
            }
            validate_required_fields(schema, required, |name| out.contains_key(name))?;
            runtime_constraints::validate_property_dependencies(
                schema,
                out.keys().map(String::as_str),
            )?;
            validate_alternative_fields(schema, alternatives, &out)?;
            runtime_constraints::validate_property_count(schema, out.len())?;
            let value = serde_json::Value::Object(out);
            runtime_constraints::validate_dependent_schemas(
                schema,
                &value,
                value
                    .as_object()
                    .into_iter()
                    .flat_map(serde_json::Map::keys),
                patterns,
            )?;
            Ok(value)
        }
        (SchemaKind::Scalar { ty }, other) => Err(write_shape_error(
            schema,
            scalar_type_name(*ty),
            instance_type_name(other),
        )),
        (SchemaKind::ScalarUnion { .. }, other) => Err(write_shape_error(
            schema,
            "declared scalar union",
            instance_type_name(other),
        )),
        (SchemaKind::Group { .. }, other) => Err(write_shape_error(
            schema,
            "object",
            instance_type_name(other),
        )),
    }
}

fn validate_required_fields(
    schema: &SchemaNode,
    required: &[String],
    contains: impl Fn(&str) -> bool,
) -> Result<(), JsonFormatError> {
    if let Some(property) = required.iter().find(|property| !contains(property)) {
        return Err(JsonFormatError::MissingRequiredProperty {
            object: schema.name.clone(),
            property: property.clone(),
        });
    }
    Ok(())
}

fn write_json_any(
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<serde_json::Value, JsonFormatError> {
    let Instance::Scalar(value) = instance else {
        return Err(write_shape_error(
            schema,
            "arbitrary JSON scalar encoding",
            instance_type_name(instance),
        ));
    };
    match value {
        Value::String(value) => Ok(serde_json::from_str(value)
            .unwrap_or_else(|_| serde_json::Value::String(value.clone()))),
        Value::Bool(value) => Ok((*value).into()),
        Value::Int(value) => Ok((*value).into()),
        Value::Float(value) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| JsonFormatError::Shape {
                name: schema.name.clone(),
                expected: "finite JSON number",
                got: "non-finite float",
            }),
        Value::JsonNull(_) => Ok(serde_json::Value::Null),
        Value::Null | Value::XmlNil(_) | Value::Decimal(_) => Err(JsonFormatError::Shape {
            name: schema.name.clone(),
            expected: "arbitrary JSON value",
            got: value.type_name(),
        }),
    }
}

fn is_boundary_absence(schema: &SchemaNode, instance: &Instance) -> Result<bool, JsonFormatError> {
    if let Instance::MappedSequence(items) = instance
        && items.is_empty()
        && !schema.repeating
    {
        return Ok(true);
    }
    // A Null scalar, or a Null nullable object or array or single tuple, is
    // boundary-level absence; an explicit JSON null is a value.
    Ok(matches!(instance, Instance::Scalar(Value::Null))
        && (null_layers::instance_null(schema)?.whole
            || (!schema.repeating && schema.is_scalar())
            || json_tuple::is_single_frame(schema))
        || matches!(instance, Instance::Repeated(items) | Instance::MappedSequence(items) if items.is_empty())
            && schema
                .item_count_range
                .is_some_and(|range| range.minimum() > 0))
}

fn write_scalar_union(
    value: &Value,
    types: ScalarTypeSet,
    nullable: bool,
    name: &str,
) -> Result<serde_json::Value, JsonFormatError> {
    let allowed = match value {
        Value::String(_) => types.contains(ScalarType::String),
        Value::Int(_) => types.contains(ScalarType::Int),
        Value::Float(_) => types.contains(ScalarType::Float),
        Value::Bool(_) => types.contains(ScalarType::Bool),
        Value::JsonNull(_) => nullable,
        Value::Null | Value::XmlNil(_) | Value::Decimal(_) => false,
    };
    if !allowed {
        if matches!(value, Value::Int(_)) && types.contains(ScalarType::Float) {
            return write_scalar(value, ScalarType::Float, false, name);
        }
        if matches!(value, Value::String(_)) && !types.contains(ScalarType::String) {
            let mut converted = None;
            for ty in [ScalarType::Int, ScalarType::Float, ScalarType::Bool] {
                if !types.contains(ty) {
                    continue;
                }
                // A scalar union keeps its existing candidate selection: an
                // integral decimal string such as "1.000" still selects its
                // floating member when both numeric tags are available.
                let candidate = match (ty, value) {
                    (ScalarType::Int, Value::String(text)) => text
                        .trim()
                        .parse::<i64>()
                        .ok()
                        .map(|integer| serde_json::Value::Number(integer.into())),
                    _ => write_scalar(value, ty, false, name).ok(),
                };
                if let Some(candidate) = candidate {
                    if converted.is_some() {
                        return Err(JsonFormatError::Shape {
                            name: name.to_string(),
                            expected: "unambiguous declared scalar union",
                            got: "string",
                        });
                    }
                    converted = Some(candidate);
                }
            }
            if let Some(converted) = converted {
                return Ok(converted);
            }
        }
        return Err(JsonFormatError::Shape {
            name: name.to_string(),
            expected: "declared scalar union",
            got: value.type_name(),
        });
    }
    match value {
        Value::String(value) => Ok(value.clone().into()),
        Value::Int(value) => Ok((*value).into()),
        Value::Float(value) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| JsonFormatError::Shape {
                name: name.to_string(),
                expected: "finite number",
                got: "non-finite float",
            }),
        Value::Bool(value) => Ok((*value).into()),
        Value::JsonNull(_) => Ok(serde_json::Value::Null),
        Value::Null | Value::XmlNil(_) | Value::Decimal(_) => Err(JsonFormatError::Shape {
            name: name.to_string(),
            expected: "declared scalar union",
            got: value.type_name(),
        }),
    }
}

fn validate_alternative_fields(
    schema: &SchemaNode,
    alternatives: &[ir::GroupAlternative],
    fields: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFormatError> {
    if alternatives.is_empty() {
        return Ok(());
    }
    let mut matches = Vec::new();
    for alternative in alternatives {
        let mut required_present = true;
        for required in &alternative.required {
            if !required_member_present(schema, fields, required)? {
                required_present = false;
                break;
            }
        }
        if required_present
            && fields
                .keys()
                .all(|field| alternative.members.iter().any(|member| member == field))
            && alternative.constraints.iter().all(|constraint| {
                fields
                    .get(&constraint.member)
                    .is_none_or(|value| constraint_matches(&constraint.value, value))
            })
        {
            matches.push(alternative);
        }
    }
    match matches.len() {
        0 => Err(JsonFormatError::NoMatchingAlternative {
            name: schema.name.clone(),
        }),
        1 => Ok(()),
        _ if schema.alternative_mode() == GroupAlternativeMode::Exclusive => {
            if schema.xml_type_alternatives && has_unique_narrowest_xml_type(&matches) {
                Ok(())
            } else {
                Err(JsonFormatError::AmbiguousAlternative {
                    name: schema.name.clone(),
                })
            }
        }
        _ => Ok(()),
    }
}

/// A required alternative member is present: a non-null value, or a null
/// admitted by the member's scalar value layer.
fn required_member_present(
    schema: &SchemaNode,
    fields: &serde_json::Map<String, serde_json::Value>,
    required: &str,
) -> Result<bool, JsonFormatError> {
    let Some(value) = fields.get(required) else {
        return Ok(false);
    };
    if !value.is_null() {
        return Ok(true);
    }
    match schema.child(required) {
        Some(child) => Ok(null_layers::instance_null(child)?.value),
        None => Ok(false),
    }
}

fn has_unique_narrowest_xml_type(matches: &[&ir::GroupAlternative]) -> bool {
    matches
        .iter()
        .enumerate()
        .any(|(candidate_index, candidate)| {
            matches.iter().enumerate().all(|(other_index, other)| {
                candidate_index == other_index
                    || candidate.members.len() < other.members.len()
                        && candidate
                            .members
                            .iter()
                            .all(|member| other.members.contains(member))
            })
        })
}

fn constraint_matches(
    expected: &GroupAlternativeConstraintValue,
    actual: &serde_json::Value,
) -> bool {
    match (expected, actual) {
        (GroupAlternativeConstraintValue::String(expected), serde_json::Value::String(actual)) => {
            expected == actual
        }
        (GroupAlternativeConstraintValue::Int(expected), serde_json::Value::Number(actual)) => {
            actual.as_i64() == Some(*expected)
        }
        (GroupAlternativeConstraintValue::Float(expected), serde_json::Value::Number(actual)) => {
            json_number_matches_f64(actual, expected.get())
        }
        (GroupAlternativeConstraintValue::Bool(expected), serde_json::Value::Bool(actual)) => {
            expected == actual
        }
        (GroupAlternativeConstraintValue::JsonNull, serde_json::Value::Null) => true,
        _ => false,
    }
}

fn write_scalar(
    value: &Value,
    ty: ScalarType,
    nullable: bool,
    name: &str,
) -> Result<serde_json::Value, JsonFormatError> {
    if let Value::Float(value) = value
        && !value.is_finite()
    {
        return Err(JsonFormatError::Shape {
            name: name.to_string(),
            expected: "finite number",
            got: "non-finite float",
        });
    }

    let bad = || JsonFormatError::Shape {
        name: name.to_string(),
        expected: scalar_type_name(ty),
        got: value.type_name(),
    };
    match (ty, value) {
        (_, Value::JsonNull(_)) if nullable => Ok(serde_json::Value::Null),
        (ScalarType::String, Value::Bool(value)) => {
            Ok(serde_json::Value::String(value.to_string()))
        }
        (ScalarType::String, Value::Int(value)) => Ok(serde_json::Value::String(value.to_string())),
        (ScalarType::String, Value::Float(value)) => {
            Ok(serde_json::Value::String(value.to_string()))
        }
        (ScalarType::String, Value::String(value)) => Ok(serde_json::Value::String(value.clone())),
        (ScalarType::Int, Value::Int(value)) => Ok(serde_json::Value::Number((*value).into())),
        (ScalarType::Int, Value::String(value)) => parse_exact_decimal_i64(value)
            .map(|value| serde_json::Value::Number(value.into()))
            .ok_or_else(bad),
        (ScalarType::Float, Value::Int(value)) if exact_f64_from_i64(*value).is_some() => {
            Ok(serde_json::Value::Number((*value).into()))
        }
        (ScalarType::Float, Value::Int(_)) => Err(JsonFormatError::Shape {
            name: name.to_string(),
            expected: "number",
            got: "int outside the exact f64 range",
        }),
        (ScalarType::Float, Value::Float(value)) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .ok_or_else(bad),
        (ScalarType::Float, Value::String(value)) => {
            let parsed = value.trim().parse::<f64>().map_err(|_| bad())?;
            serde_json::Number::from_f64(parsed)
                .map(serde_json::Value::Number)
                .ok_or_else(|| JsonFormatError::Shape {
                    name: name.to_string(),
                    expected: "finite number",
                    got: "string",
                })
        }
        (ScalarType::Bool, Value::Bool(value)) => Ok(serde_json::Value::Bool(*value)),
        (ScalarType::Bool, Value::String(value)) => value
            .trim()
            .parse::<bool>()
            .map(serde_json::Value::Bool)
            .map_err(|_| bad()),
        _ => Err(bad()),
    }
}

fn scalar_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "number",
        ScalarType::Bool => "bool",
    }
}

fn instance_type_name(instance: &Instance) -> &'static str {
    match instance {
        Instance::Scalar(value) => value.type_name(),
        Instance::Group(_) => "object",
        Instance::Repeated(_) => "array",
        Instance::MappedSequence(_) => "mapped sequence",
        Instance::DocumentSet(_) => "document set",
    }
}

fn write_shape_error(
    schema: &SchemaNode,
    expected: &'static str,
    got: &'static str,
) -> JsonFormatError {
    JsonFormatError::Shape {
        name: schema.name.clone(),
        expected,
        got,
    }
}

#[cfg(test)]
mod tests;
