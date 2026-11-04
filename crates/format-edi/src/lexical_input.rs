//! Validate source spellings before restoring their declared scalar types.

use ir::{Instance, ScalarType, SchemaKind, SchemaNode};
use mapping::EdiValueConstraint;

use crate::EdiFormatError;

pub(crate) fn read(
    schema: &SchemaNode,
    constraints: &[EdiValueConstraint],
    decode: impl FnOnce(&SchemaNode) -> Result<Instance, EdiFormatError>,
) -> Result<Instance, EdiFormatError> {
    if constraints.is_empty() {
        return decode(schema);
    }

    // Document admission bounds the schema before this private view is cloned.
    // The same decoder still owns hierarchy, separators, escapes and nulls; only
    // numeric conversion waits until lexical length/code checks have completed.
    let mut lexical_schema = schema.clone();
    let mut pending = vec![&mut lexical_schema];
    while let Some(node) = pending.pop() {
        match &mut node.kind {
            SchemaKind::Scalar { ty } => *ty = ScalarType::String,
            SchemaKind::Group { children, .. } => pending.extend(children),
            SchemaKind::ScalarUnion { .. } => {
                return Err(EdiFormatError::UnsupportedSchema(node.name.clone()));
            }
        }
    }
    let mut instance = decode(&lexical_schema)?;
    let report = crate::validate_values(&instance, constraints)?;
    if !report.is_empty() {
        return Err(EdiFormatError::Validation(report));
    }
    crate::segments::restore_lexical_types(schema, &mut instance)?;
    Ok(instance)
}
