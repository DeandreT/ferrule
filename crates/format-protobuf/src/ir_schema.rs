use std::collections::HashSet;

use ir::{ScalarType as IrScalarType, SchemaNode};

use crate::{Cardinality, FieldType, Layout, MessageId, ProtobufError, ScalarType};

pub(crate) fn project(layout: &Layout, root: MessageId) -> Result<SchemaNode, ProtobufError> {
    project_message(layout, root, None, &mut HashSet::new())
}

fn project_message(
    layout: &Layout,
    id: MessageId,
    field_name: Option<&str>,
    active: &mut HashSet<MessageId>,
) -> Result<SchemaNode, ProtobufError> {
    let message = layout.message(id).ok_or_else(|| {
        ProtobufError::schema(format!("unknown resolved message id {}", id.index()))
    })?;
    if !active.insert(id) {
        return Err(ProtobufError::schema(format!(
            "recursive message `{}` cannot be represented by the tree-shaped IR",
            message.full_name()
        )));
    }

    let result = message
        .fields()
        .iter()
        .map(|field| {
            let mut child = match field.ty() {
                FieldType::Message(child) => {
                    project_message(layout, child, Some(field.name()), active)?
                }
                FieldType::Enum(_) => SchemaNode::scalar(field.name(), IrScalarType::Int),
                FieldType::Scalar(scalar) => {
                    SchemaNode::scalar(field.name(), ir_scalar_type(scalar))
                }
            };
            child.repeating = field.cardinality() == Cardinality::Repeated;
            Ok(child)
        })
        .collect::<Result<Vec<_>, ProtobufError>>()
        .map(|children| SchemaNode::group(field_name.unwrap_or(message.name()), children));
    active.remove(&id);
    result
}

fn ir_scalar_type(ty: ScalarType) -> IrScalarType {
    match ty {
        ScalarType::Double | ScalarType::Float => IrScalarType::Float,
        ScalarType::Int32
        | ScalarType::Int64
        | ScalarType::Uint32
        | ScalarType::Uint64
        | ScalarType::Sint32
        | ScalarType::Sint64
        | ScalarType::Fixed32
        | ScalarType::Fixed64
        | ScalarType::Sfixed32
        | ScalarType::Sfixed64 => IrScalarType::Int,
        ScalarType::Bool => IrScalarType::Bool,
        ScalarType::String | ScalarType::Bytes => IrScalarType::String,
    }
}
