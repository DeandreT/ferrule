use ir::{ScalarType, SchemaNode};

pub(super) use crate::runtime_constraints::constants::{ScalarConstant, from_schema};

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

pub(super) fn rendered_fixed(schema: &SchemaNode) -> Option<serde_json::Value> {
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
