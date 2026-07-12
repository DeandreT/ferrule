use std::collections::BTreeMap;
use std::path::Path;

use ir::{GroupAlternative, SchemaKind, SchemaNode};

pub(super) fn merge_conditioned_xml_types(
    entry: &roxmltree::Node,
    schema: &mut SchemaNode,
    xsd_path: &Path,
    warnings: &mut Vec<String>,
) {
    merge_entry_children(entry, schema, xsd_path, warnings, &mut Vec::new());
}

fn merge_entry_children(
    entry: &roxmltree::Node,
    schema: &mut SchemaNode,
    xsd_path: &Path,
    warnings: &mut Vec<String>,
    path: &mut Vec<String>,
) {
    let children: Vec<_> = entry
        .children()
        .filter(|child| child.has_tag_name("entry"))
        .collect();
    let mut conditioned: BTreeMap<String, Vec<roxmltree::Node<'_, '_>>> = BTreeMap::new();
    for child in &children {
        if child.children().any(|node| node.has_tag_name("condition")) {
            conditioned
                .entry(normalized_entry_name(
                    child.attribute("name").unwrap_or_default(),
                ))
                .or_default()
                .push(*child);
        }
    }
    for (name, entries) in conditioned {
        if entries.len() < 2 {
            continue;
        }
        path.push(name);
        if let Err(reason) = merge_alternatives_at(schema, path, &entries, xsd_path) {
            warnings.push(format!(
                "conditional XML type alternatives at `{}` could not be represented: {reason}",
                path.join("/")
            ));
        }
        path.pop();
    }

    for child in children {
        let name = normalized_entry_name(child.attribute("name").unwrap_or_default());
        path.push(name);
        merge_entry_children(&child, schema, xsd_path, warnings, path);
        path.pop();
    }
}

fn merge_alternatives_at(
    schema: &mut SchemaNode,
    path: &[String],
    entries: &[roxmltree::Node<'_, '_>],
    xsd_path: &Path,
) -> Result<(), String> {
    let node = schema_node_at_mut(schema, path)
        .ok_or_else(|| "the base schema path does not exist".to_string())?;
    let mut metadata = Vec::with_capacity(entries.len());
    {
        let SchemaKind::Group { children, .. } = &mut node.kind else {
            return Err("the base schema node is not a group".to_string());
        };
        for entry in entries {
            let type_name = conditioned_type_name(entry).ok_or_else(|| {
                "a condition is not an exact equality between xsi:type and a constant QName"
                    .to_string()
            })?;
            let derived = format_xml::xsd::import_type(xsd_path, &type_name)
                .map_err(|error| error.to_string())?;
            let SchemaKind::Group {
                children: derived_children,
                ..
            } = derived.kind
            else {
                return Err(format!("type `{type_name}` is not a complex type"));
            };
            let mut members = Vec::with_capacity(derived_children.len());
            for child in derived_children {
                members.push(child.name.clone());
                if let Some(existing) = children.iter().find(|existing| existing.name == child.name)
                {
                    if existing != &child {
                        return Err(format!(
                            "field `{}` has incompatible schemas across derived types",
                            child.name
                        ));
                    }
                } else {
                    children.push(child);
                }
            }
            metadata.push(GroupAlternative {
                name: type_name,
                members,
                required: Vec::new(),
            });
        }
    }
    node.set_alternatives(metadata)
        .then_some(())
        .ok_or_else(|| "the derived type alternatives have inconsistent metadata".to_string())
}

fn conditioned_type_name(entry: &roxmltree::Node) -> Option<String> {
    let condition = entry
        .children()
        .find(|node| node.has_tag_name("condition"))?;
    let function = condition
        .children()
        .find(|node| node.has_tag_name("expression"))?
        .children()
        .find(|node| node.has_tag_name("function"))?;
    if function.attribute("name") != Some("equal") || function.attribute("library") != Some("core")
    {
        return None;
    }
    let operands: Vec<_> = function
        .children()
        .filter(|node| node.has_tag_name("expression"))
        .collect();
    let [first, second] = operands.as_slice() else {
        return None;
    };
    qname_equality_operands(first, second).or_else(|| qname_equality_operands(second, first))
}

fn qname_equality_operands(
    attribute_expression: &roxmltree::Node,
    constant_expression: &roxmltree::Node,
) -> Option<String> {
    let attribute = attribute_expression
        .children()
        .find(|node| node.has_tag_name("attribute"))?;
    if attribute.attribute("name") != Some("type")
        || attribute.attribute("ns") != Some("http://www.w3.org/2001/XMLSchema-instance")
    {
        return None;
    }
    let constant = constant_expression
        .children()
        .find(|node| node.has_tag_name("constant"))?;
    if constant.attribute("datatype") != Some("QName") {
        return None;
    }
    constant.attribute("value").map(str::to_string)
}

fn schema_node_at_mut<'a>(
    schema: &'a mut SchemaNode,
    path: &[String],
) -> Option<&'a mut SchemaNode> {
    let mut node = schema;
    for segment in path {
        let SchemaKind::Group { children, .. } = &mut node.kind else {
            return None;
        };
        node = children.iter_mut().find(|child| child.name == *segment)?;
    }
    Some(node)
}

fn normalized_entry_name(name: &str) -> String {
    let name = match name.split_once(':') {
        Some((prefix, local))
            if !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            local
        }
        _ => name,
    };
    name.strip_prefix('@').unwrap_or(name).to_string()
}
