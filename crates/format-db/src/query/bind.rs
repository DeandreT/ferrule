//! Binding preflight; it never touches the filesystem.

use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::ValidatedSqliteRead;

use super::{BoundSqliteRead, QueryPhase, QueryReadError, QueryReadErrorKind};

pub(super) fn bind<'p>(
    path: &Path,
    schema: &SchemaNode,
    plan: &'p ValidatedSqliteRead,
    bindings: &[(String, Value)],
) -> Result<BoundSqliteRead<'p>, QueryReadError> {
    let error = |kind| QueryReadError::new(path, QueryPhase::Bind, kind);
    if schema.contains_json_tuple() || !schema_matches(schema, plan) {
        return Err(error(QueryReadErrorKind::PlanSchemaMismatch));
    }
    for (index, (name, _)) in bindings.iter().enumerate() {
        if bindings[..index]
            .iter()
            .any(|(previous, _)| previous == name)
        {
            return Err(error(QueryReadErrorKind::DuplicateParameter).parameter(name));
        }
    }
    for (name, _) in bindings {
        if !plan
            .parameters()
            .iter()
            .any(|parameter| parameter.name == *name)
        {
            return Err(error(QueryReadErrorKind::UnknownParameter).parameter(name));
        }
    }
    let mut values = Vec::with_capacity(plan.parameters().len());
    for parameter in plan.parameters() {
        let Some((_, value)) = bindings.iter().find(|(name, _)| *name == parameter.name) else {
            return Err(error(QueryReadErrorKind::MissingParameter).parameter(&parameter.name));
        };
        let kind = match (parameter.ty, value) {
            (_, value) if value.is_null_like() => Some(QueryReadErrorKind::NullParameter),
            (ScalarType::Float, Value::Float(float)) if !float.is_finite() => {
                Some(QueryReadErrorKind::NonFiniteParameter)
            }
            (ScalarType::String, Value::String(_))
            | (ScalarType::Int, Value::Int(_))
            | (ScalarType::Float, Value::Float(_))
            | (ScalarType::Bool, Value::Bool(_)) => None,
            (expected, value) => Some(QueryReadErrorKind::ParameterType {
                expected,
                found: value.type_name(),
            }),
        };
        if let Some(kind) = kind {
            return Err(error(kind).parameter(&parameter.name));
        }
        values.push(value.clone());
    }
    Ok(BoundSqliteRead {
        source: path.to_path_buf(),
        plan,
        values,
    })
}

/// The plan's projection must describe exactly this schema's fields.
fn schema_matches(schema: &SchemaNode, plan: &ValidatedSqliteRead) -> bool {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return false;
    };
    schema.repeating
        && children.len() == plan.projection().len()
        && children
            .iter()
            .zip(plan.projection())
            .all(|(child, projected)| {
                child.name == projected.field
                    && matches!(child.kind, SchemaKind::Scalar { ty } if ty == projected.ty)
            })
}
