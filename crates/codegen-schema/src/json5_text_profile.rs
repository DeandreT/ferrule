//! Shape-only admission for the opt-in mapped JSON5 writer. Constraints stay in
//! the descriptor; this does not validate mapped values against those constraints.

use ir::{ScalarType, SchemaKind, SchemaNode};

pub fn validate(schema: &SchemaNode) -> Result<(), &'static str> {
    let mut pending = vec![(schema, 0)];
    while let Some((node, depth)) = pending.pop() {
        if depth > 64 {
            return Err("schema depth exceeds the mapped JSON5 profile");
        }
        if node.recursive_ref.is_some() || node.json_array_wrapper {
            return Err("recursive and nested-array schemas require separate admission");
        }
        match &node.kind {
            SchemaKind::Scalar { ty } => {
                if node.json_any && *ty != ScalarType::String {
                    return Err("json_any requires its encoded String representation");
                }
            }
            SchemaKind::Group {
                children,
                dynamic,
                alternatives,
                ..
            } => {
                if node.json_any || !alternatives.is_empty() {
                    return Err("object alternatives require separate admission");
                }
                let mut names = std::collections::BTreeSet::new();
                for child in children {
                    if !child.name.is_ascii() || !names.insert(&child.name) {
                        return Err("declared member names must be unique ASCII strings");
                    }
                    pending.push((child, depth + 1));
                }
                if let Some(dynamic) = dynamic {
                    pending.push((dynamic, depth + 1));
                }
            }
            SchemaKind::ScalarUnion { .. } => {
                return Err("scalar unions require separate admission");
            }
        }
    }
    Ok(())
}
