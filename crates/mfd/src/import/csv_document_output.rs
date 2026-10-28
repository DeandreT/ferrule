//! A whole-row copy and a filename derived from the same fixed-block driver.
//! No existing row/group copy or filename-input contract is broadened.

use std::collections::BTreeSet;

use mapping::{Node, Scope, ScopeConstruction, ScopeIteration};
use roxmltree::Node as XmlNode;

use super::{
    graph::GraphBuilder,
    schema::{ComponentFormat, SchemaComponent, parse_u32},
};

fn single_key(structure: XmlNode<'_, '_>, key: u32) -> bool {
    structure
        .descendants()
        .filter(|node| node.has_tag_name("entry") || node.has_tag_name("datapoint"))
        .flat_map(|node| {
            [
                node.attribute("key"),
                node.attribute("inpkey"),
                node.attribute("outkey"),
            ]
        })
        .filter(|value| parse_u32(*value) == Some(key))
        .count()
        == 1
}

fn output_entry<'a, 'input>(
    structure: XmlNode<'a, 'input>,
    key: u32,
) -> Option<XmlNode<'a, 'input>> {
    let entries = structure
        .descendants()
        .filter(|node| {
            node.has_tag_name("entry") && parse_u32(node.attribute("outkey")) == Some(key)
        })
        .collect::<Vec<_>>();
    let [entry] = entries.as_slice() else {
        return None;
    };
    single_key(structure, key).then_some(*entry)
}

fn plain_entry(node: XmlNode<'_, '_>, attributes: &[&str]) -> bool {
    node.has_tag_name("entry")
        && node
            .attributes()
            .all(|attribute| attributes.contains(&attribute.name()))
        && !node
            .children()
            .any(|child| child.is_text() && !child.text().unwrap_or_default().trim().is_empty())
}

fn function<'a, 'input>(
    structure: XmlNode<'a, 'input>,
    output: u32,
    name: &str,
    builder: &GraphBuilder<'_>,
) -> Option<(usize, XmlNode<'a, 'input>)> {
    let index = *builder.fn_by_output.get(&output)?;
    let function = &builder.fn_components[index];
    if function.library != "core"
        || function.name != name
        || !function.raw_single_output_valid
        || !single_key(structure, output)
    {
        return None;
    }
    let nodes = structure
        .descendants()
        .filter(|node| {
            node.has_tag_name("datapoint") && parse_u32(node.attribute("key")) == Some(output)
        })
        .collect::<Vec<_>>();
    let [point] = nodes.as_slice() else {
        return None;
    };
    let raw = point.parent()?.parent()?;
    if !raw.has_tag_name("component")
        || raw.attribute("library") != Some("core")
        || raw.attribute("name") != Some(name)
        || parse_u32(raw.attribute("kind")) != Some(function.kind)
    {
        return None;
    }
    if raw.attributes().any(|attribute| {
        !matches!(
            attribute.name(),
            "name" | "library" | "kind" | "uid" | "growable" | "growablebasename"
        )
    }) || raw.children().any(|node| {
        node.is_element()
            && !matches!(
                node.tag_name().name(),
                "sources" | "targets" | "properties" | "view" | "data"
            )
    }) || raw
        .children()
        .filter(|node| node.has_tag_name("properties"))
        .any(|node| node.attributes().len() != 0 || node.children().any(|child| child.is_element()))
        || function.kind == 5
            && raw
                .children()
                .filter(|node| node.has_tag_name("data"))
                .any(|node| {
                    node.attributes().len() != 0
                        || node.children().any(|child| {
                            child.is_element()
                                || child.is_text()
                                    && !child.text().unwrap_or_default().trim().is_empty()
                        })
                })
    {
        return None;
    }
    let sources = raw
        .children()
        .filter(|node| node.has_tag_name("sources"))
        .collect::<Vec<_>>();
    let points = match sources.as_slice() {
        [] => vec![],
        [sources] => sources
            .children()
            .filter(|node| node.is_element())
            .collect(),
        _ => return None,
    };
    if points.len() != function.inputs.len() {
        return None;
    }
    let mut positions = BTreeSet::new();
    for point in points {
        let position = parse_u32(point.attribute("pos"))? as usize;
        let key = parse_u32(point.attribute("key"))?;
        if !point.has_tag_name("datapoint")
            || !positions.insert(position)
            || function.inputs.get(position) != Some(&Some(key))
            || !single_key(structure, key)
            || point
                .attributes()
                .any(|attribute| !matches!(attribute.name(), "key" | "pos"))
            || point.children().any(|child| child.is_element())
        {
            return None;
        }
    }
    Some((index, raw))
}

fn literal(structure: XmlNode<'_, '_>, key: u32, builder: &GraphBuilder<'_>) -> Option<ir::Value> {
    let (index, raw) = function(structure, key, "constant", builder)?;
    let function = &builder.fn_components[index];
    if function.kind != 2 || !function.inputs.is_empty() {
        return None;
    }
    let data = raw
        .children()
        .filter(|node| node.has_tag_name("data"))
        .collect::<Vec<_>>();
    let [data] = data.as_slice() else {
        return None;
    };
    if data.attributes().len() != 0
        || data
            .children()
            .filter(|node| node.has_tag_name("wsdl"))
            .count()
            > 1
        || data
            .children()
            .any(|node| node.is_text() && !node.text().unwrap_or_default().trim().is_empty())
    {
        return None;
    }
    let mut constants = data.children().filter(|node| node.has_tag_name("constant"));
    let constant = constants.next()?;
    if constants.next().is_some()
        || constant.attribute("datatype") != Some("string")
        || constant
            .attributes()
            .any(|attribute| !matches!(attribute.name(), "datatype" | "value"))
        || constant.children().any(|child| child.is_element())
        || data.children().any(|node| {
            node.is_element()
                && node != constant
                && !(node.has_tag_name("wsdl")
                    && node.attributes().len() == 0
                    && !node.children().any(|child| {
                        child.is_element()
                            || child.is_text()
                                && !child.text().unwrap_or_default().trim().is_empty()
                    }))
        })
    {
        return None;
    }
    Some(ir::Value::String(constant.attribute("value")?.into()))
}

fn context_free_size(node: mapping::NodeId, builder: &GraphBuilder<'_>, depth: usize) -> bool {
    if depth > 16 {
        return false;
    }
    match builder.graph.nodes.get(&node) {
        Some(Node::Const { .. } | Node::RuntimeParameter { .. }) => true,
        Some(Node::RuntimeParameterDefault { default, .. }) => {
            context_free_size(*default, builder, depth + 1)
        }
        _ => false,
    }
}

fn size_declaration(
    structure: XmlNode<'_, '_>,
    feed: u32,
    builder: &GraphBuilder<'_>,
    depth: usize,
) -> bool {
    if depth > 16 || !single_key(structure, feed) {
        return false;
    }
    let Some(&index) = builder.fn_by_output.get(&feed) else {
        return false;
    };
    let declaration = &builder.fn_components[index];
    if declaration.kind == 2 {
        return declaration.raw_single_output_valid
            && (declaration.bounded_scalar_pins_valid && declaration.raw_literal_valid
                || literal(structure, feed, builder).is_some());
    }
    let Some((_, raw)) = function(structure, feed, &declaration.name, builder) else {
        return false;
    };
    if declaration.kind != 6
        || declaration.input_type.is_none()
        || declaration.input_parameter_name.is_none()
        || declaration.inputs.len() > 1
    {
        return false;
    }
    let data = raw
        .children()
        .filter(|node| node.has_tag_name("data"))
        .collect::<Vec<_>>();
    let [data] = data.as_slice() else {
        return false;
    };
    let children = data
        .children()
        .filter(|node| node.is_element())
        .collect::<Vec<_>>();
    if children
        .iter()
        .filter(|node| node.has_tag_name("input"))
        .count()
        != 1
        || children
            .iter()
            .filter(|node| node.has_tag_name("parameter"))
            .count()
            != 1
        || children
            .iter()
            .filter(|node| node.has_tag_name("wsdl"))
            .count()
            > 1
        || children.iter().any(|node| match node.tag_name().name() {
            "input" => {
                node.attributes()
                    .any(|attribute| attribute.name() != "datatype")
                    || node.children().any(|child| child.is_element())
            }
            "parameter" => {
                node.attributes()
                    .any(|attribute| !matches!(attribute.name(), "name" | "usageKind" | "optional"))
                    || node.children().any(|child| child.is_element())
            }
            "wsdl" => {
                node.attributes().len() != 0
                    || node.children().any(|child| {
                        child.is_element()
                            || child.is_text()
                                && !child.text().unwrap_or_default().trim().is_empty()
                    })
            }
            _ => true,
        })
    {
        return false;
    }
    match declaration
        .inputs
        .first()
        .copied()
        .flatten()
        .and_then(|input| builder.edge_from.get(&input))
    {
        Some(default) => size_declaration(structure, *default, builder, depth + 1),
        None => true,
    }
}

pub(super) fn build(
    target: &SchemaComponent,
    structure: XmlNode<'_, '_>,
    copy_all: &BTreeSet<u32>,
    builder: &mut GraphBuilder<'_>,
) -> Option<(Scope, u32)> {
    let [source] = builder.sources else {
        return None;
    };
    if target.format != ComponentFormat::Csv
        || source.format != ComponentFormat::Csv
        || !ir::flat_row_schema_is_supported(&source.schema)
        || source.schema.kind != target.schema.kind
        || !builder.intermediates.is_empty()
        || target.options.csv_text_repair_dependency.is_some()
        || target.has_target_node_controls
    {
        return None;
    }
    if target.ports.len() != 1 {
        return None;
    }
    let (&body_input, body_path) = target.ports.first_key_value()?;
    if !body_path.is_empty()
        || !copy_all.contains(&body_input)
        || !single_key(structure, body_input)
    {
        return None;
    }
    let source_key = *builder.edge_from.get(&body_input)?;
    if !source.ports.get(&source_key)?.is_empty() {
        return None;
    }
    let source_entry = output_entry(structure, source_key)?;
    if source_entry
        .ancestors()
        .take_while(|node| !node.has_tag_name("component"))
        .filter(|node| node.has_tag_name("entry"))
        .any(|node| {
            !plain_entry(node, &["name", "outkey", "expanded"])
                || node.children().any(|child| child.has_tag_name("condition"))
        })
    {
        return None;
    }
    let body_entries = structure
        .descendants()
        .filter(|node| {
            node.has_tag_name("entry") && parse_u32(node.attribute("inpkey")) == Some(body_input)
        })
        .collect::<Vec<_>>();
    let [body] = body_entries.as_slice() else {
        return None;
    };
    let document = body.parent()?;
    let wrapper = document.parent()?;
    let root = wrapper.parent()?;
    if !root.has_tag_name("root")
        || root
            .children()
            .filter(|node| node.has_tag_name("entry"))
            .count()
            != 1
    {
        return None;
    }
    if !plain_entry(*body, &["name", "inpkey", "expanded"])
        || body.children().any(|node| node.is_element())
        || !plain_entry(document, &["name", "expanded"])
        || document.attribute("name") != Some("document")
        || document.children().filter(|node| node.is_element()).count() != 1
        || !plain_entry(wrapper, &["name", "inpkey", "expanded"])
        || wrapper.attribute("name") != Some("FileInstance")
        || wrapper.children().filter(|node| node.is_element()).count() != 1
    {
        return None;
    }
    let filename_input = parse_u32(wrapper.attribute("inpkey"))?;
    if !single_key(structure, filename_input) {
        return None;
    }
    let filename_feed = *builder.edge_from.get(&filename_input)?;
    let (concat_index, _) = function(structure, filename_feed, "concat", builder)?;
    let concat = &builder.fn_components[concat_index];
    if concat.kind != 5 || concat.inputs.len() < 2 || concat.inputs.len() > 32 {
        return None;
    }
    let mut parts = Vec::new();
    let mut group_output = None;
    for pin in &concat.inputs {
        let feed = *builder.edge_from.get(&(*pin)?)?;
        if let Some(value) = literal(structure, feed, builder) {
            parts.push(Some(value));
            continue;
        }
        let (position, _) = function(structure, feed, "position", builder)?;
        if builder.fn_components[position].kind != 5
            || builder.fn_components[position].inputs.len() != 1
            || group_output.is_some()
        {
            return None;
        }
        group_output = builder.input_feed(position, 0);
        parts.push(None);
    }
    let (group, _) = function(structure, group_output?, "group-into-blocks", builder)?;
    if builder.fn_components[group].kind != 5
        || builder.fn_components[group].inputs.len() != 2
        || builder.input_feed(group, 0) != Some(source_key)
    {
        return None;
    }
    let size_feed = builder.input_feed(group, 1)?;
    if !size_declaration(structure, size_feed, builder, 0) {
        return None;
    }
    // Materialize in a private candidate so every unsuccessful proof is inert.
    let mut candidate = builder.clone();
    let size = candidate.scalar_node_at_anchor(size_feed, &[])?;
    if candidate.warnings.len() != builder.warnings.len() || !context_free_size(size, &candidate, 0)
    {
        return None;
    }
    let position = candidate.alloc(Node::Position { collection: vec![] });
    let args = parts
        .into_iter()
        .map(|value| match value {
            Some(value) => candidate.alloc(Node::Const { value }),
            None => position,
        })
        .collect();
    let filename = candidate.alloc(Node::Call {
        function: "concat".into(),
        args,
    });
    let root = Scope {
        construction: ScopeConstruction::CopyCurrentRows,
        iteration: ScopeIteration::DynamicDocuments {
            source: vec![],
            output_path: filename,
        },
        group_into_blocks: Some(size),
        ..Scope::default()
    };
    *builder = candidate;
    Some((root, filename_input))
}
