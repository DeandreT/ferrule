use ir::{ScalarType, SchemaKind, Value};
use mapping::{Binding, Node};

use super::graph::GraphBuilder;
use super::schema::{SchemaComponent, normalize_xml_entry_name, parse_u32, schema_node_at};
use super::scope::ScopeBuilder;

struct TargetDefault {
    path: Vec<String>,
    value: Value,
}

pub(super) fn install(
    target: &SchemaComponent,
    structure: &roxmltree::Node<'_, '_>,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) {
    let Some(component) = structure
        .descendants()
        .filter(|node| node.has_tag_name("component"))
        .find(|component| owns_target_port(*component, target))
    else {
        return;
    };
    let Some(root) = component
        .children()
        .find(|node| node.has_tag_name("data"))
        .and_then(|data| data.children().find(|node| node.has_tag_name("root")))
    else {
        return;
    };

    let mut defaults = Vec::new();
    for entry in root.children().filter(|node| node.has_tag_name("entry")) {
        collect(entry, target, &[], &mut defaults);
    }
    for default in defaults {
        install_one(default, builder, scopes);
    }
}

fn owns_target_port(component: roxmltree::Node<'_, '_>, target: &SchemaComponent) -> bool {
    component
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| parse_u32(entry.attribute("inpkey")))
        .any(|key| target.input_keys.contains(&key))
}

fn collect(
    entry: roxmltree::Node<'_, '_>,
    target: &SchemaComponent,
    parent: &[String],
    output: &mut Vec<TargetDefault>,
) {
    let (name, _) = normalize_xml_entry_name(entry.attribute("name").unwrap_or_default());
    let wrapper = matches!(name, "FileInstance" | "document")
        || parent.is_empty() && name == target.schema.name;
    let mut path = parent.to_vec();
    if !wrapper && !name.is_empty() {
        path.push(name.to_string());
    }

    if let Some(default) = direct_default(entry)
        && let Some(SchemaKind::Scalar { ty }) =
            schema_node_at(&target.schema, &path).map(|n| &n.kind)
        && let Some(value) = parse_value(&default, *ty)
    {
        output.push(TargetDefault {
            path: path.clone(),
            value,
        });
    }

    for child in entry.children().filter(|node| node.has_tag_name("entry")) {
        collect(child, target, &path, output);
    }
}

fn direct_default(entry: roxmltree::Node<'_, '_>) -> Option<String> {
    entry
        .children()
        .find(|node| node.has_tag_name("inputnodefunctions"))?
        .children()
        .filter(|node| node.has_tag_name("rule"))
        .filter(|rule| {
            rule.attribute("applyto")
                .is_none_or(|value| value == "self")
        })
        .find_map(|rule| {
            rule.children()
                .find(|node| node.has_tag_name("default"))
                .and_then(|default| default.attribute("value"))
                .map(str::to_string)
        })
}

fn parse_value(value: &str, ty: ScalarType) -> Option<Value> {
    match ty {
        ScalarType::String => Some(Value::String(value.to_string())),
        ScalarType::Bool => match value {
            "true" | "1" => Some(Value::Bool(true)),
            "false" | "0" => Some(Value::Bool(false)),
            _ => None,
        },
        ScalarType::Int => value.parse().ok().map(Value::Int),
        ScalarType::Float => value
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(Value::Float),
    }
}

fn install_one(default: TargetDefault, builder: &mut GraphBuilder<'_>, scopes: &mut ScopeBuilder) {
    let Some((field, chain)) = default.path.split_last() else {
        return;
    };
    let Some(scope) = existing_scope(&mut scopes.root, chain) else {
        return;
    };
    let fallback = builder.alloc(Node::Const {
        value: default.value,
    });
    if let Some(binding) = scope
        .bindings
        .iter_mut()
        .find(|binding| binding.target_field == *field)
    {
        let input = binding.node;
        let condition = builder.alloc(Node::Call {
            function: "exists".to_string(),
            args: vec![input],
        });
        binding.node = builder.alloc(Node::If {
            condition,
            then: input,
            else_: fallback,
        });
    } else {
        scope.bindings.push(Binding {
            target_field: field.clone(),
            node: fallback,
        });
    }
}

fn existing_scope<'a>(
    mut scope: &'a mut mapping::Scope,
    chain: &[String],
) -> Option<&'a mut mapping::Scope> {
    for field in chain {
        let index = scope
            .children
            .iter()
            .position(|child| child.target_field == *field)?;
        scope = &mut scope.children[index];
    }
    Some(scope)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults_as_the_target_scalar_type() {
        assert_eq!(parse_value("1", ScalarType::Bool), Some(Value::Bool(true)));
        assert_eq!(parse_value("42", ScalarType::Int), Some(Value::Int(42)));
        assert_eq!(
            parse_value("2.5", ScalarType::Float),
            Some(Value::Float(2.5))
        );
        assert_eq!(parse_value("invalid", ScalarType::Bool), None);
    }
}
