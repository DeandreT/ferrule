use ir::{
    GroupAlternativeConstraintValue, GroupAlternativeMode, ScalarType, SchemaKind, SchemaNode,
};

/// Writes `node`'s shape (sans repetition) into `out`; repetition wraps it
/// in an array schema.
pub(super) fn render(node: &SchemaNode, out: &mut serde_json::Map<String, serde_json::Value>) {
    if node.container_nullable {
        let mut content = serde_json::Map::new();
        render_non_nullable(node, &mut content);
        out.insert(
            "anyOf".into(),
            serde_json::Value::Array(vec![
                serde_json::Value::Object(content),
                serde_json::json!({ "type": "null" }),
            ]),
        );
        return;
    }
    render_non_nullable(node, out);
}

fn render_non_nullable(node: &SchemaNode, out: &mut serde_json::Map<String, serde_json::Value>) {
    if node.repeating {
        out.insert("type".into(), "array".into());
        let mut items = serde_json::Map::new();
        render_shape(node, &mut items);
        out.insert("items".into(), serde_json::Value::Object(items));
    } else {
        render_shape(node, out);
    }
}

fn render_shape(node: &SchemaNode, out: &mut serde_json::Map<String, serde_json::Value>) {
    match &node.kind {
        SchemaKind::Scalar { ty } => {
            if node.json_any {
                return;
            }
            let name = match ty {
                ScalarType::String => "string",
                ScalarType::Int => "integer",
                ScalarType::Float => "number",
                ScalarType::Bool => "boolean",
            };
            let ty = if node.nullable {
                serde_json::Value::Array(vec![name.into(), "null".into()])
            } else {
                name.into()
            };
            out.insert("type".into(), ty);
        }
        SchemaKind::Group {
            children,
            alternatives,
            dynamic,
        } => {
            out.insert("type".into(), "object".into());
            if !alternatives.is_empty() {
                let variants = alternatives
                    .iter()
                    .map(|alternative| {
                        let mut variant = serde_json::Map::new();
                        variant.insert("title".into(), alternative.name.clone().into());
                        variant.insert("type".into(), "object".into());
                        variant.insert("additionalProperties".into(), false.into());
                        let mut properties = serde_json::Map::new();
                        for member in &alternative.members {
                            if let Some(child) = children.iter().find(|child| child.name == *member)
                            {
                                let mut property = serde_json::Map::new();
                                render(child, &mut property);
                                if let Some(constraint) = alternative
                                    .constraints
                                    .iter()
                                    .find(|constraint| constraint.member == *member)
                                {
                                    property.insert(
                                        "const".into(),
                                        constraint_value_to_json(&constraint.value),
                                    );
                                }
                                properties.insert(
                                    child.name.clone(),
                                    serde_json::Value::Object(property),
                                );
                            }
                        }
                        variant.insert("properties".into(), properties.into());
                        if !alternative.required.is_empty() {
                            variant.insert("required".into(), alternative.required.clone().into());
                        }
                        serde_json::Value::Object(variant)
                    })
                    .collect();
                let keyword = match node.alternative_mode() {
                    GroupAlternativeMode::Exclusive => "oneOf",
                    GroupAlternativeMode::Inclusive => "anyOf",
                };
                out.insert(keyword.into(), serde_json::Value::Array(variants));
                return;
            }
            let mut props = serde_json::Map::new();
            for child in children {
                let mut prop = serde_json::Map::new();
                render(child, &mut prop);
                props.insert(child.name.clone(), serde_json::Value::Object(prop));
            }
            out.insert("properties".into(), serde_json::Value::Object(props));
            if let Some(dynamic) = dynamic {
                let mut additional = serde_json::Map::new();
                render(dynamic, &mut additional);
                out.insert(
                    "additionalProperties".into(),
                    serde_json::Value::Object(additional),
                );
            } else {
                out.insert("additionalProperties".into(), false.into());
            }
        }
    }
}

fn constraint_value_to_json(value: &GroupAlternativeConstraintValue) -> serde_json::Value {
    match value {
        GroupAlternativeConstraintValue::String(value) => value.clone().into(),
        GroupAlternativeConstraintValue::Int(value) => (*value).into(),
        GroupAlternativeConstraintValue::Float(value) => value.get().into(),
        GroupAlternativeConstraintValue::Bool(value) => (*value).into(),
        GroupAlternativeConstraintValue::JsonNull => serde_json::Value::Null,
    }
}
