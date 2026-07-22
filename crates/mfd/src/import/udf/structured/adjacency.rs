use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{ScalarType, SchemaKind};

use super::{ImportedDefinition, Recipe, RecipeSource};
use crate::import::function::{FnComponent, is_filter, read as read_function};
use crate::import::graph::read_edges;
use crate::import::schema::{SchemaComponent, parse_u32, read_schema_component};
use crate::import::udf::{Definition, OutputExpr};

pub(in crate::import) fn try_read(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
) -> Result<Option<ImportedDefinition>, String> {
    let Some(structure) = component
        .children()
        .find(|node| node.has_tag_name("structure"))
    else {
        return Ok(None);
    };
    let Some(children) = structure
        .children()
        .find(|node| node.has_tag_name("children"))
    else {
        return Ok(None);
    };
    let components = children
        .children()
        .filter(|node| node.has_tag_name("component"))
        .collect::<Vec<_>>();
    let xml = components
        .iter()
        .copied()
        .filter(|node| node.attribute("library") == Some("xml"))
        .collect::<Vec<_>>();
    let [first, second] = xml.as_slice() else {
        return Ok(None);
    };
    let (input_node, output_node) = match (is_output(*first), is_output(*second)) {
        (false, true) => (*first, *second),
        (true, false) => (*second, *first),
        _ => return Ok(None),
    };
    let mut warnings = Vec::new();
    let input = read_schema_component(&input_node, mfd_path, &mut warnings)
        .ok_or("adjacency hierarchy input schema cannot be read")?;
    let output = read_schema_component(&output_node, mfd_path, &mut warnings)
        .ok_or("adjacency hierarchy output schema cannot be read")?;
    let Some((collection, key, parent)) = adjacency_source_shape(&input.schema) else {
        return Ok(None);
    };
    let Some((target_key, target_children)) = recursive_target_shape(&output.schema) else {
        return Ok(None);
    };

    let input_id = component_id(input_node)?;
    let output_id = component_id(output_node)?;
    let functions = components
        .iter()
        .copied()
        .map(|node| (node, read_function(&node)))
        .collect::<Vec<_>>();
    let scalar_parameters = functions
        .iter()
        .filter(|(_, function)| function.kind == 6)
        .collect::<Vec<_>>();
    let [(base_node, base_function)] = scalar_parameters.as_slice() else {
        return Err("adjacency hierarchy requires one optional scalar base parameter".into());
    };
    let base_id = component_id(*base_node)?;
    let [base_output] = base_function.outputs.as_slice() else {
        return Err("adjacency hierarchy base parameter has invalid pins".into());
    };
    let edge_from = read_edges(&structure, Some(component));
    let recursive_call = components
        .iter()
        .copied()
        .find(|node| {
            node.attribute("kind") == Some("19")
                && node.attribute("library") == component.attribute("library")
                && node.attribute("name") == component.attribute("name")
        })
        .ok_or("adjacency hierarchy has no direct self call")?;
    let recursive_catalog = call_port(recursive_call, input_id, "inpkey")?;
    let recursive_base = call_port(recursive_call, base_id, "inpkey")?;
    let recursive_output = call_port(recursive_call, output_id, "outkey")?;

    let input_root_port = port_at(&input, &[])?;
    let collection_port = port_at(&input, std::slice::from_ref(&collection))?;
    let key_path = vec![collection.clone(), key.clone()];
    let parent_path = vec![collection.clone(), parent.clone()];
    let key_port = port_at(&input, &key_path)?;
    let parent_port = port_at(&input, &parent_path)?;
    if edge_from.get(&recursive_catalog) != Some(&input_root_port)
        || edge_from.get(&recursive_base) != Some(&key_port)
    {
        return Err(
            "adjacency hierarchy self call is not driven by the full row set and current key"
                .into(),
        );
    }

    let target_root_port = port_at(&output, &[])?;
    let target_key_port = port_at(&output, std::slice::from_ref(&target_key))?;
    let target_children_port = port_at(&output, std::slice::from_ref(&target_children))?;
    if edge_from.get(&target_key_port) != Some(&key_port)
        || edge_from.get(&target_children_port) != Some(&recursive_output)
    {
        return Err(
            "adjacency hierarchy output does not map the current key and recursive children".into(),
        );
    }

    validate_root_filter(
        &functions,
        &edge_from,
        collection_port,
        parent_port,
        *base_output,
        target_root_port,
    )?;

    Ok(Some((
        Definition {
            scalar_interface: None,
            parameters: BTreeSet::from([base_id]),
            structured_parameters: BTreeSet::from([input_id]),
            outputs: BTreeMap::from([(
                output_id,
                OutputExpr::Structured(Recipe {
                    source: RecipeSource::AdjacencyTree {
                        component_id: input_id,
                        base_parameter: base_id,
                        collection: vec![collection],
                        key: vec![key],
                        parent: vec![parent],
                        target_key,
                        target_children,
                    },
                    filter: None,
                    bindings: BTreeMap::new(),
                }),
            )]),
        },
        None,
        warnings,
    )))
}

fn adjacency_source_shape(schema: &ir::SchemaNode) -> Option<(String, String, String)> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return None;
    };
    let rows = children
        .iter()
        .filter(|child| child.repeating && matches!(child.kind, SchemaKind::Group { .. }))
        .collect::<Vec<_>>();
    let [rows] = rows.as_slice() else {
        return None;
    };
    let SchemaKind::Group {
        children: fields, ..
    } = &rows.kind
    else {
        return None;
    };
    let strings = fields
        .iter()
        .filter(|field| {
            !field.repeating
                && matches!(
                    field.kind,
                    SchemaKind::Scalar {
                        ty: ScalarType::String
                    }
                )
        })
        .collect::<Vec<_>>();
    let [key, parent] = strings.as_slice() else {
        return None;
    };
    Some((rows.name.clone(), key.name.clone(), parent.name.clone()))
}

fn recursive_target_shape(schema: &ir::SchemaNode) -> Option<(String, String)> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return None;
    };
    let keys = children
        .iter()
        .filter(|child| {
            !child.repeating
                && matches!(
                    child.kind,
                    SchemaKind::Scalar {
                        ty: ScalarType::String
                    }
                )
        })
        .collect::<Vec<_>>();
    let recursive = children
        .iter()
        .filter(|child| child.repeating && child.recursive_ref.as_deref() == Some(&schema.name))
        .collect::<Vec<_>>();
    let ([key], [recursive]) = (keys.as_slice(), recursive.as_slice()) else {
        return None;
    };
    Some((key.name.clone(), recursive.name.clone()))
}

fn validate_root_filter(
    functions: &[(roxmltree::Node<'_, '_>, FnComponent)],
    edge_from: &BTreeMap<u32, u32>,
    collection: u32,
    parent: u32,
    base: u32,
    target: u32,
) -> Result<(), String> {
    let filter = functions
        .iter()
        .map(|(_, function)| function)
        .find(|function| {
            is_filter(function)
                && function
                    .outputs
                    .iter()
                    .any(|output| edge_from.get(&target) == Some(output))
        })
        .ok_or("adjacency hierarchy root is not driven by a filter")?;
    if input_feed(filter, 0, edge_from) != Some(collection) {
        return Err("adjacency hierarchy filter does not iterate the flat rows".into());
    }
    let predicate = input_feed(filter, 1, edge_from)
        .ok_or("adjacency hierarchy filter predicate is not connected")?;
    let choose = function_by_output(functions, predicate)
        .filter(|function| function.kind == 4 && function.name == "if-else")
        .ok_or("adjacency hierarchy predicate is not an if-else")?;
    let exists = input_feed(choose, 0, edge_from)
        .and_then(|feed| function_by_output(functions, feed))
        .filter(|function| function.name == "exists")
        .ok_or("adjacency hierarchy predicate does not test the optional base")?;
    let equal = input_feed(choose, 1, edge_from)
        .and_then(|feed| function_by_output(functions, feed))
        .filter(|function| function.name == "equal")
        .ok_or("adjacency hierarchy predicate does not compare parent and base")?;
    let not_exists = input_feed(choose, 2, edge_from)
        .and_then(|feed| function_by_output(functions, feed))
        .filter(|function| function.name == "not-exists")
        .ok_or("adjacency hierarchy root predicate does not test a missing parent")?;
    if input_feeds(exists, edge_from) != BTreeSet::from([base])
        || input_feeds(equal, edge_from) != BTreeSet::from([base, parent])
        || input_feeds(not_exists, edge_from) != BTreeSet::from([parent])
    {
        return Err("adjacency hierarchy root predicate uses unexpected fields".into());
    }
    Ok(())
}

fn function_by_output<'a>(
    functions: &'a [(roxmltree::Node<'_, '_>, FnComponent)],
    output: u32,
) -> Option<&'a FnComponent> {
    functions
        .iter()
        .map(|(_, function)| function)
        .find(|function| function.outputs.contains(&output))
}

fn input_feed(
    function: &FnComponent,
    position: usize,
    edge_from: &BTreeMap<u32, u32>,
) -> Option<u32> {
    function
        .inputs
        .get(position)
        .copied()
        .flatten()
        .and_then(|input| edge_from.get(&input).copied())
}

fn input_feeds(function: &FnComponent, edge_from: &BTreeMap<u32, u32>) -> BTreeSet<u32> {
    function
        .inputs
        .iter()
        .flatten()
        .filter_map(|input| edge_from.get(input).copied())
        .collect()
}

fn port_at(component: &SchemaComponent, path: &[String]) -> Result<u32, String> {
    component
        .ports
        .iter()
        .find(|(_, candidate)| candidate.as_slice() == path)
        .map(|(key, _)| *key)
        .ok_or_else(|| format!("adjacency hierarchy port `{}` is missing", path.join("/")))
}

fn is_output(component: roxmltree::Node<'_, '_>) -> bool {
    component
        .descendants()
        .any(|node| node.has_tag_name("parameter") && node.attribute("usageKind") == Some("output"))
}

fn component_id(component: roxmltree::Node<'_, '_>) -> Result<u32, String> {
    parse_u32(component.attribute("uid"))
        .ok_or_else(|| "adjacency hierarchy component id is invalid".to_string())
}

fn call_port(
    call: roxmltree::Node<'_, '_>,
    component_id: u32,
    attribute: &str,
) -> Result<u32, String> {
    inherited_call_port(call, None, component_id, attribute).ok_or_else(|| {
        format!("adjacency hierarchy self call has no `{attribute}` for parameter {component_id}")
    })
}

fn inherited_call_port(
    node: roxmltree::Node<'_, '_>,
    inherited: Option<u32>,
    wanted: u32,
    attribute: &str,
) -> Option<u32> {
    for child in node.children().filter(|child| child.is_element()) {
        let component = if child.has_tag_name("entry") {
            parse_u32(child.attribute("componentid")).or(inherited)
        } else {
            inherited
        };
        if child.has_tag_name("entry")
            && component == Some(wanted)
            && let Some(key) = parse_u32(child.attribute(attribute))
        {
            return Some(key);
        }
        if let Some(key) = inherited_call_port(child, component, wanted, attribute) {
            return Some(key);
        }
    }
    None
}
