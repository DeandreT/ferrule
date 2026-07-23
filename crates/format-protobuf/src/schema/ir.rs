use std::collections::HashSet;

use ir::{GroupAlternative, ScalarType as IrScalarType, SchemaNode};

use crate::{Cardinality, Field, FieldType, Layout, Message, MessageId, ProtobufError, ScalarType};

const MAX_ONEOF_ALTERNATIVES: usize = 4_096;

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

    let result = (|| {
        let children = message
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
            .collect::<Result<Vec<_>, ProtobufError>>()?;
        let mut group = SchemaNode::group(field_name.unwrap_or(message.name()), children);
        if !message.oneofs().is_empty() {
            let alternatives = project_oneofs(message)?;
            group = group.with_alternatives(alternatives).ok_or_else(|| {
                ProtobufError::schema(format!(
                    "message `{}` produced invalid oneof alternatives",
                    message.full_name()
                ))
            })?;
        }
        Ok(group)
    })();
    active.remove(&id);
    result
}

fn project_oneofs(message: &Message) -> Result<Vec<GroupAlternative>, ProtobufError> {
    let ordinary = message
        .fields()
        .iter()
        .filter(|field| field.oneof().is_none())
        .collect::<Vec<_>>();
    let mut alternatives = vec![GroupAlternative {
        name: String::new(),
        members: ordinary
            .iter()
            .map(|field| field.name().to_string())
            .collect(),
        required: ordinary
            .iter()
            .filter(|field| field.cardinality() == Cardinality::Required)
            .map(|field| field.name().to_string())
            .collect(),
        constraints: Vec::new(),
    }];
    for (index, oneof) in message.oneofs().iter().enumerate() {
        let fields = message
            .fields()
            .iter()
            .filter(|field| field.oneof().is_some_and(|id| id.index() == index))
            .collect::<Vec<_>>();
        let choice_count = fields.len() + 1;
        let next_count = alternatives
            .len()
            .checked_mul(choice_count)
            .filter(|count| *count <= MAX_ONEOF_ALTERNATIVES)
            .ok_or_else(|| {
                ProtobufError::schema(format!(
                    "message `{}` oneof combinations exceed the limit of {MAX_ONEOF_ALTERNATIVES}",
                    message.full_name()
                ))
            })?;
        let mut next = Vec::with_capacity(next_count);
        for alternative in alternatives {
            next.push(with_oneof_choice(alternative.clone(), oneof.name(), None));
            for field in &fields {
                next.push(with_oneof_choice(
                    alternative.clone(),
                    oneof.name(),
                    Some(field),
                ));
            }
        }
        alternatives = next;
    }
    Ok(alternatives)
}

fn with_oneof_choice(
    mut alternative: GroupAlternative,
    oneof: &str,
    field: Option<&&Field>,
) -> GroupAlternative {
    if !alternative.name.is_empty() {
        alternative.name.push_str("; ");
    }
    alternative.name.push_str(oneof);
    alternative.name.push('=');
    match field {
        Some(field) => {
            alternative.name.push_str(field.name());
            alternative.members.push(field.name().to_string());
            alternative.required.push(field.name().to_string());
        }
        None => alternative.name.push_str("<unset>"),
    }
    alternative
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
