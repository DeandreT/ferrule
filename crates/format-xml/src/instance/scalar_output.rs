//! Private writer projection; the declared/public schema is never changed.

use super::XmlFormatError;
use ir::{
    Instance, ScalarType, SchemaKind, SchemaNode, Value, XmlDoubleFields, XmlScalarPreservation,
};

pub(super) fn project(
    schema: &SchemaNode,
    instance: &Instance,
    preservation: XmlScalarPreservation<'_>,
) -> Result<(SchemaNode, Instance), XmlFormatError> {
    match preservation {
        XmlScalarPreservation::RootFields(fields) => project_root_fields(schema, instance, fields),
        // The validated document schema is a bounded plain element tree, so
        // this lockstep walk is bounded by the schema rather than the value.
        XmlScalarPreservation::Document => {
            Ok((lexical_schema(schema), project_document(schema, instance)?))
        }
    }
}

fn lexical_schema(schema: &SchemaNode) -> SchemaNode {
    let mut projected = schema.clone();
    fn visit(node: &mut SchemaNode) {
        match &mut node.kind {
            SchemaKind::Scalar { ty } => *ty = ScalarType::String,
            SchemaKind::Group { children, .. } => children.iter_mut().for_each(visit),
            _ => {}
        }
    }
    visit(&mut projected);
    projected
}

fn project_document(schema: &SchemaNode, instance: &Instance) -> Result<Instance, XmlFormatError> {
    if schema.repeating {
        let Instance::Repeated(items) = instance else {
            return Err(super::shape_error(schema, "repeating elements", instance));
        };
        return items
            .iter()
            .map(|item| project_document_item(schema, item))
            .collect::<Result<Vec<_>, _>>()
            .map(Instance::Repeated);
    }
    // As in the ordinary writer, a non-repeating group may receive a mapped
    // sequence of zero or more items.
    if let (Instance::MappedSequence(items), SchemaKind::Group { .. }) = (instance, &schema.kind) {
        return items
            .iter()
            .map(|item| project_document_item(schema, item))
            .collect::<Result<Vec<_>, _>>()
            .map(Instance::MappedSequence);
    }
    project_document_item(schema, instance)
}

fn project_document_item(
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<Instance, XmlFormatError> {
    match &schema.kind {
        SchemaKind::Scalar { .. } => {
            if !matches!(instance, Instance::Scalar(_)) {
                return Err(super::shape_error(schema, "a scalar", instance));
            }
            lexical_scalar(&schema.name, instance, true)
        }
        SchemaKind::Group {
            children,
            alternatives,
            ..
        } => {
            let Instance::Group(group) = instance else {
                return Err(super::shape_error(schema, "an element group", instance));
            };
            super::validate_group_fields(schema, children, alternatives, group)?;
            let mut values = Vec::with_capacity(group.len());
            for (name, value) in group.iter() {
                let Some(field) = children.iter().find(|child| child.name == *name) else {
                    return Err(XmlFormatError::UnexpectedField {
                        group: schema.name.clone(),
                        field: name.clone(),
                    });
                };
                values.push((name.clone(), project_document(field, value)?));
            }
            Ok(Instance::Group(ir::InstanceGroup::from(values)))
        }
        _ => Err(XmlFormatError::InvalidScalarOutput(
            ir::XmlScalarOutputError::UnsupportedSchema,
        )),
    }
}

fn project_root_fields(
    schema: &SchemaNode,
    instance: &Instance,
    fields: &XmlDoubleFields,
) -> Result<(SchemaNode, Instance), XmlFormatError> {
    let Instance::Group(group) = instance else {
        return Err(super::shape_error(schema, "an element group", instance));
    };
    let SchemaKind::Group {
        children,
        alternatives,
        ..
    } = &schema.kind
    else {
        return Err(super::shape_error(schema, "an element group", instance));
    };
    // Check shallow shape before cloning. In particular, a hostile nested value
    // in a scalar slot must never reach Instance's recursive Clone path.
    super::validate_group_fields(schema, children, alternatives, group)?;
    for (name, value) in group.iter() {
        let Some(field) = children.iter().find(|child| child.name == *name) else {
            return Err(XmlFormatError::UnexpectedField {
                group: schema.name.clone(),
                field: name.clone(),
            });
        };
        if field.repeating {
            let Instance::Repeated(items) = value else {
                return Err(super::shape_error(field, "repeating elements", value));
            };
            for item in items {
                if !matches!(item, Instance::Scalar(_)) {
                    return Err(super::shape_error(field, "a scalar", item));
                }
            }
        } else if !matches!(value, Instance::Scalar(_)) {
            return Err(super::shape_error(field, "a scalar", value));
        }
    }
    let mut projected_schema = schema.clone();
    if let SchemaKind::Group { children, .. } = &mut projected_schema.kind {
        for child in children {
            if fields.contains(&child.name) {
                child.kind = SchemaKind::Scalar {
                    ty: ScalarType::String,
                };
            }
        }
    }
    let mut values = group.clone();
    for (name, value) in &mut values {
        if fields.contains(name) {
            *value = lexical_instance(name, value)?;
        }
    }
    Ok((projected_schema, Instance::Group(values)))
}

fn lexical_instance(name: &str, instance: &Instance) -> Result<Instance, XmlFormatError> {
    match instance {
        Instance::Repeated(values) => values
            .iter()
            .map(|value| lexical_scalar(name, value, false))
            .collect::<Result<Vec<_>, _>>()
            .map(Instance::Repeated),
        _ => lexical_scalar(name, instance, false),
    }
}

/// The flat profile's double fields keep their original String/Int/Float
/// admission; the document profile also writes Bool values lexically.
fn lexical_scalar(
    name: &str,
    instance: &Instance,
    bool_text: bool,
) -> Result<Instance, XmlFormatError> {
    let value = match instance {
        Instance::Scalar(Value::String(value)) => Value::String(value.clone()),
        Instance::Scalar(Value::Int(value)) => Value::String(value.to_string()),
        Instance::Scalar(Value::Float(value)) if value.is_finite() => {
            Value::String(value.to_string())
        }
        Instance::Scalar(Value::Bool(value)) if bool_text => Value::String(value.to_string()),
        Instance::Scalar(Value::Null) => Value::Null,
        _ => {
            return Err(XmlFormatError::ValueType {
                name: name.into(),
                expected: ScalarType::Float,
                got: "unsupported suppressed-cast value",
            });
        }
    };
    Ok(Instance::Scalar(value))
}
