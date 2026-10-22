use ir::{
    GroupAlternativeConstraintValue, GroupAlternativeMode, ScalarType, SchemaKind, SchemaNode,
};

use crate::JsonFormatError;
use crate::null_layers::{JsonLayers, layers};

/// Writes `node`'s shape (sans repetition) into `out`; repetition wraps it
/// in an array schema.
pub(super) fn render(
    node: &SchemaNode,
    out: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFormatError> {
    let layers = layers(node)?;
    if layers.container_null {
        let mut content = serde_json::Map::new();
        render_non_nullable(node, layers, &mut content)?;
        out.insert(
            "anyOf".into(),
            serde_json::Value::Array(vec![
                serde_json::Value::Object(content),
                serde_json::json!({ "type": "null" }),
            ]),
        );
        return Ok(());
    }
    render_non_nullable(node, layers, out)
}

fn render_non_nullable(
    node: &SchemaNode,
    layers: JsonLayers,
    out: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFormatError> {
    if layers.array {
        out.insert("type".into(), "array".into());
        let mut items = serde_json::Map::new();
        render_shape(node, layers, &mut items)?;
        out.insert("items".into(), serde_json::Value::Object(items));
        super::item_counts::render(node, out);
        super::contains::render(node, out)?;
        super::unique_items::render(node, out);
    } else {
        render_shape(node, layers, out)?;
    }
    Ok(())
}

fn render_shape(
    node: &SchemaNode,
    layers: JsonLayers,
    out: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFormatError> {
    if layers.object_item_null {
        let mut content = serde_json::Map::new();
        render_non_nullable_shape(node, layers, &mut content)?;
        out.insert(
            "anyOf".into(),
            serde_json::json!([serde_json::Value::Object(content), {"type": "null"}]),
        );
        return Ok(());
    }
    render_non_nullable_shape(node, layers, out)
}

fn render_non_nullable_shape(
    node: &SchemaNode,
    layers: JsonLayers,
    out: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFormatError> {
    if node.json_array_wrapper {
        let item = node.json_array_wrapper_item().ok_or_else(|| {
            JsonFormatError::InvalidArrayWrapperMetadata {
                name: node.name.clone(),
            }
        })?;
        return render(item, out);
    }
    match &node.kind {
        SchemaKind::Scalar { ty } => {
            if node.json_any {
                return Ok(());
            }
            let name = match ty {
                ScalarType::String => "string",
                ScalarType::Int => "integer",
                ScalarType::Float => "number",
                ScalarType::Bool => "boolean",
            };
            let ty = if layers.value_null {
                serde_json::Value::Array(vec![name.into(), "null".into()])
            } else {
                name.into()
            };
            out.insert("type".into(), ty);
            if let Some(value) = super::constraints::rendered_fixed(node) {
                out.insert("const".into(), value);
            }
            super::allowed_values::render(node, out)?;
            super::ranges::render(node, out)?;
            super::string_lengths::render(node, out);
            super::formats::render(node, out);
            super::patterns::render(node, layers.value_null, out);
            super::multiples::render(node, layers.value_null, out)?;
        }
        SchemaKind::ScalarUnion { types } => {
            let mut types = types
                .iter()
                .map(scalar_type_name)
                .map(serde_json::Value::from)
                .collect::<Vec<_>>();
            if layers.value_null {
                types.push("null".into());
            }
            out.insert("type".into(), types.into());
            super::allowed_values::render(node, out)?;
            super::ranges::render(node, out)?;
            super::string_lengths::render(node, out);
            super::formats::render(node, out);
            super::patterns::render(node, layers.value_null, out);
            super::multiples::render(node, layers.value_null, out)?;
        }
        SchemaKind::Group {
            children,
            alternatives,
            required,
            dynamic,
            ..
        } => {
            out.insert("type".into(), "object".into());
            super::property_counts::render(node, out);
            super::property_dependencies::render(node, out);
            let ordered_dependent_schemas_in_alternatives =
                !alternatives.is_empty() && super::dependent_schemas::requires_ordered_all_of(node);
            if !ordered_dependent_schemas_in_alternatives {
                super::dependent_schemas::render_constraints(node, out)?;
            }
            super::property_names::render(node, out);
            if !required.is_empty() {
                out.insert("required".into(), required.clone().into());
            }
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
                                render(child, &mut property)?;
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
                        if ordered_dependent_schemas_in_alternatives {
                            super::dependent_schemas::render_constraints(node, &mut variant)?;
                        }
                        Ok(serde_json::Value::Object(variant))
                    })
                    .collect::<Result<Vec<_>, JsonFormatError>>()?;
                let keyword = match node.alternative_mode() {
                    GroupAlternativeMode::Exclusive => "oneOf",
                    GroupAlternativeMode::Inclusive => "anyOf",
                };
                out.insert(keyword.into(), serde_json::Value::Array(variants));
                return Ok(());
            }
            let mut props = serde_json::Map::new();
            for child in children {
                let mut prop = serde_json::Map::new();
                render(child, &mut prop)?;
                props.insert(child.name.clone(), serde_json::Value::Object(prop));
            }
            out.insert("properties".into(), serde_json::Value::Object(props));
            if let Some(dynamic) = dynamic {
                if super::pattern_properties::render(node, dynamic, out)? {
                    return Ok(());
                }
                let mut additional = serde_json::Map::new();
                render(dynamic, &mut additional)?;
                out.insert(
                    "additionalProperties".into(),
                    serde_json::Value::Object(additional),
                );
            } else {
                out.insert("additionalProperties".into(), false.into());
            }
        }
    }
    Ok(())
}

fn scalar_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "number",
        ScalarType::Bool => "boolean",
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
