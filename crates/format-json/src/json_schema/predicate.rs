use ir::{JsonSchemaPredicate, ScalarType, SchemaKind, SchemaNode};

use crate::{JsonFormatError, PatternRuntime};

pub(crate) fn validate_private_unique_items(schema: &SchemaNode) -> Result<(), JsonFormatError> {
    validate_schema(schema, false)
}

fn validate_schema(schema: &SchemaNode, private: bool) -> Result<(), JsonFormatError> {
    if private && schema.json_unique_items && item_shape_can_lose_number_precision(schema) {
        return Err(super::unsupported_union(
            &schema.name,
            "`uniqueItems` inside `contains` or `dependentSchemas` cannot preserve exact arbitrary-precision JSON numbers when its item schema admits `number` or arbitrary JSON",
        ));
    }
    if let Some(constraints) = &schema.json_contains {
        for constraint in constraints.as_slice() {
            if let Some(predicate) = constraint.predicate().as_schema() {
                validate_schema(predicate, true)?;
            }
        }
    }
    if let Some(constraints) = &schema.json_dependent_schemas {
        for constraint in constraints.as_slice() {
            if let Some(predicate) = constraint.predicate().as_schema() {
                validate_schema(predicate, true)?;
            }
        }
    }
    match &schema.kind {
        SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => Ok(()),
        SchemaKind::Group {
            children, dynamic, ..
        } => {
            for child in children {
                validate_schema(child, private)?;
            }
            if let Some(dynamic) = dynamic {
                validate_schema(dynamic, private)?;
            }
            Ok(())
        }
    }
}

fn item_shape_can_lose_number_precision(schema: &SchemaNode) -> bool {
    if schema.json_any {
        return true;
    }
    match &schema.kind {
        SchemaKind::Scalar { ty } => *ty == ScalarType::Float,
        SchemaKind::ScalarUnion { types } => types.contains(ScalarType::Float),
        SchemaKind::Group {
            children, dynamic, ..
        } => {
            children.iter().any(item_shape_can_lose_number_precision)
                || dynamic
                    .as_deref()
                    .is_some_and(item_shape_can_lose_number_precision)
        }
    }
}

pub(crate) fn matches(
    predicate: &JsonSchemaPredicate,
    value: &serde_json::Value,
    patterns: &mut PatternRuntime,
) -> Result<bool, JsonFormatError> {
    let JsonSchemaPredicate::Schema { schema } = predicate else {
        return Ok(false);
    };
    let matched = super::unique_items::validate_json_tree(schema, value).and_then(|()| {
        if schema.repeating {
            crate::read_repeated(value, schema, patterns)
        } else {
            crate::read_node_with_patterns(value, schema, patterns)
        }
    });
    match matched {
        Ok(_) => Ok(true),
        Err(error) if is_assertion_failure(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

fn is_assertion_failure(error: &JsonFormatError) -> bool {
    !matches!(
        error,
        JsonFormatError::Io(_)
            | JsonFormatError::Json(_)
            | JsonFormatError::UnsupportedSchemaUnion { .. }
            | JsonFormatError::UnsupportedSchemaObject { .. }
            | JsonFormatError::SchemaResource { .. }
            | JsonFormatError::SchemaResourceLimit { .. }
            | JsonFormatError::UniqueItemsLimit { .. }
            | JsonFormatError::InvalidPatternMetadata { .. }
            | JsonFormatError::InvalidAllowedValuesMetadata { .. }
            | JsonFormatError::InvalidMultipleOfMetadata { .. }
            | JsonFormatError::InvalidNumericRangeMetadata { .. }
            | JsonFormatError::InvalidUniqueItemsMetadata { .. }
            | JsonFormatError::InvalidPropertyCountMetadata { .. }
            | JsonFormatError::InvalidPropertyDependenciesMetadata { .. }
            | JsonFormatError::InvalidDependentSchemasMetadata { .. }
            | JsonFormatError::InvalidPropertyNameMetadata { .. }
            | JsonFormatError::InvalidContainsMetadata { .. }
            | JsonFormatError::PatternWorkLimit { .. }
    )
}
