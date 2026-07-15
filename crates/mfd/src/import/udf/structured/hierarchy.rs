use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{SchemaKind, Value};

use super::{ImportedDefinition, Recipe, RecipeSource};
use crate::import::function::{FnComponent, parse_constant, read as read_function};
use crate::import::graph::read_edges;
use crate::import::schema::{SchemaComponent, parse_u32, read_schema_component};
use crate::import::udf::{Definition, OutputExpr};

pub(in crate::import::udf) fn try_read(
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
    let Some(input) = read_schema_component(&input_node, mfd_path, &mut warnings) else {
        return Ok(None);
    };
    let Some(output) = read_schema_component(&output_node, mfd_path, &mut warnings) else {
        return Ok(None);
    };
    let Some(value) = flat_path_value(&input) else {
        return Ok(None);
    };
    let Some(shape) = output_shape(&output) else {
        return Ok(None);
    };

    let input_id = component_id(input_node)?;
    let output_id = component_id(output_node)?;
    let functions = components
        .iter()
        .copied()
        .map(|node| (node, read_function(&node)))
        .collect::<Vec<_>>();
    let contains = one_function(&functions, "contains")?;
    let before = one_function(&functions, "substring-before")?;
    let after = one_function(&functions, "substring-after")?;
    let group = one_function(&functions, "group-by")?;
    let filter = functions
        .iter()
        .map(|(_, function)| function)
        .filter(|function| function.kind == 3)
        .collect::<Vec<_>>();
    let [filter] = filter.as_slice() else {
        return Err("path hierarchy requires one scalar path filter".to_string());
    };
    let constants = functions
        .iter()
        .filter_map(|(_, function)| {
            let (literal, datatype) = function.constant.as_ref()?;
            let [output] = function.outputs.as_slice() else {
                return None;
            };
            match parse_constant(literal, datatype) {
                Value::String(value) if !value.is_empty() => Some((*output, value)),
                _ => None,
            }
        })
        .collect::<Vec<_>>();
    let [(separator_output, separator)] = constants.as_slice() else {
        return Err("path hierarchy requires one non-empty string separator".to_string());
    };

    let edge_from = read_edges(&structure, Some(component));
    let source_port = port_at(&input.ports, &[value.as_str()])
        .ok_or("path hierarchy input scalar has no port")?;
    require_inputs(contains, &edge_from, &[source_port, *separator_output])?;
    require_inputs(before, &edge_from, &[source_port, *separator_output])?;
    require_inputs(after, &edge_from, &[source_port, *separator_output])?;
    let contains_output = only_output(contains)?;
    let before_output = only_output(before)?;
    let after_output = only_output(after)?;
    let filter_outputs = pair_outputs(filter, "path hierarchy filter")?;
    require_input_at(filter, &edge_from, 0, source_port)?;
    require_input_at(filter, &edge_from, 1, contains_output)?;
    let group_outputs = pair_outputs(group, "path hierarchy group-by")?;
    require_input_at(group, &edge_from, 0, filter_outputs[0])?;
    require_input_at(group, &edge_from, 1, before_output)?;

    require_target_feed(
        &output,
        &edge_from,
        &[shape.directories.as_str()],
        group_outputs[0],
    )?;
    require_target_feed(
        &output,
        &edge_from,
        &[shape.directories.as_str(), shape.name.as_str()],
        group_outputs[1],
    )?;
    require_target_feed(
        &output,
        &edge_from,
        &[shape.files.as_str()],
        filter_outputs[1],
    )?;
    require_target_feed(
        &output,
        &edge_from,
        &[shape.files.as_str(), shape.name.as_str()],
        source_port,
    )?;

    let recursive_call = components
        .iter()
        .copied()
        .find(|node| {
            node.attribute("kind") == Some("19")
                && node.attribute("library") == component.attribute("library")
                && node.attribute("name") == component.attribute("name")
        })
        .ok_or("path hierarchy definition has no direct self call")?;
    let recursive_input = inherited_call_port(recursive_call, None, input_id, "inpkey", None)
        .ok_or("path hierarchy self call has no input port")?;
    if edge_from.get(&recursive_input) != Some(&after_output) {
        return Err("path hierarchy self call is not driven by substring-after".to_string());
    }
    let recursive_files = inherited_call_port(
        recursive_call,
        None,
        output_id,
        "outkey",
        Some(&shape.files),
    )
    .ok_or("path hierarchy self call has no recursive file output")?;
    let recursive_directories = inherited_call_port(
        recursive_call,
        None,
        output_id,
        "outkey",
        Some(&shape.directories),
    )
    .ok_or("path hierarchy self call has no recursive directory output")?;
    require_target_feed(
        &output,
        &edge_from,
        &[shape.directories.as_str(), shape.files.as_str()],
        recursive_files,
    )?;
    require_target_feed(
        &output,
        &edge_from,
        &[shape.directories.as_str(), shape.directories.as_str()],
        recursive_directories,
    )?;

    Ok(Some((
        Definition {
            parameters: BTreeSet::new(),
            structured_parameters: BTreeSet::from([input_id]),
            outputs: BTreeMap::from([(
                output_id,
                OutputExpr::Structured(Recipe {
                    source: RecipeSource::PathHierarchy {
                        component_id: input_id,
                        values: vec![value],
                        separator: separator.clone(),
                        directories: shape.directories,
                        files: shape.files,
                        name: shape.name,
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

struct OutputShape {
    directories: String,
    files: String,
    name: String,
}

fn flat_path_value(input: &SchemaComponent) -> Option<String> {
    let SchemaKind::Group { children, .. } = &input.schema.kind else {
        return None;
    };
    let [value] = children.as_slice() else {
        return None;
    };
    (value.repeating && matches!(value.kind, SchemaKind::Scalar { .. })).then(|| value.name.clone())
}

fn output_shape(output: &SchemaComponent) -> Option<OutputShape> {
    let SchemaKind::Group { children, .. } = &output.schema.kind else {
        return None;
    };
    let directories = children
        .iter()
        .find(|child| child.repeating && child.recursive_ref.is_some())?;
    let files = children.iter().find(|child| {
        child.repeating
            && child.recursive_ref.is_none()
            && matches!(child.kind, SchemaKind::Group { .. })
    })?;
    let SchemaKind::Group {
        children: file_children,
        ..
    } = &files.kind
    else {
        return None;
    };
    let name = children
        .iter()
        .find(|child| !child.repeating && matches!(child.kind, SchemaKind::Scalar { .. }))?;
    if !file_children.iter().any(|child| {
        child.name == name.name
            && !child.repeating
            && matches!(child.kind, SchemaKind::Scalar { .. })
    }) {
        return None;
    }
    Some(OutputShape {
        directories: directories.name.clone(),
        files: files.name.clone(),
        name: name.name.clone(),
    })
}

fn is_output(component: roxmltree::Node<'_, '_>) -> bool {
    component
        .descendants()
        .any(|node| node.has_tag_name("parameter") && node.attribute("usageKind") == Some("output"))
}

fn component_id(component: roxmltree::Node<'_, '_>) -> Result<u32, String> {
    parse_u32(component.attribute("uid"))
        .ok_or_else(|| "path hierarchy parameter id is invalid".to_string())
}

fn one_function<'a>(
    functions: &'a [(roxmltree::Node<'_, '_>, FnComponent)],
    name: &str,
) -> Result<&'a FnComponent, String> {
    let matches = functions
        .iter()
        .map(|(_, function)| function)
        .filter(|function| function.kind == 5 && function.name == name)
        .collect::<Vec<_>>();
    let [function] = matches.as_slice() else {
        return Err(format!("path hierarchy requires one `{name}` function"));
    };
    Ok(function)
}

fn only_output(function: &FnComponent) -> Result<u32, String> {
    let [output] = function.outputs.as_slice() else {
        return Err(format!(
            "path hierarchy function `{}` has invalid outputs",
            function.name
        ));
    };
    Ok(*output)
}

fn pair_outputs(function: &FnComponent, label: &str) -> Result<[u32; 2], String> {
    let [first, second] = function.outputs.as_slice() else {
        return Err(format!("{label} has invalid outputs"));
    };
    Ok([*first, *second])
}

fn input_feeds(function: &FnComponent, edge_from: &BTreeMap<u32, u32>) -> BTreeSet<u32> {
    function
        .inputs
        .iter()
        .flatten()
        .filter_map(|input| edge_from.get(input).copied())
        .collect()
}

fn require_inputs(
    function: &FnComponent,
    edge_from: &BTreeMap<u32, u32>,
    expected: &[u32],
) -> Result<(), String> {
    if input_feeds(function, edge_from) == expected.iter().copied().collect() {
        Ok(())
    } else {
        Err(format!(
            "path hierarchy function `{}` has unsupported inputs",
            function.name
        ))
    }
}

fn require_input_at(
    function: &FnComponent,
    edge_from: &BTreeMap<u32, u32>,
    position: usize,
    expected: u32,
) -> Result<(), String> {
    let actual = function
        .inputs
        .get(position)
        .and_then(|input| *input)
        .and_then(|input| edge_from.get(&input).copied());
    (actual == Some(expected)).then_some(()).ok_or_else(|| {
        format!(
            "path hierarchy function `{}` has unsupported pin {position}",
            function.name
        )
    })
}

fn port_at(ports: &BTreeMap<u32, Vec<String>>, path: &[&str]) -> Option<u32> {
    ports
        .iter()
        .find(|(_, candidate)| {
            candidate
                .iter()
                .map(String::as_str)
                .eq(path.iter().copied())
        })
        .map(|(key, _)| *key)
}

fn require_target_feed(
    target: &SchemaComponent,
    edge_from: &BTreeMap<u32, u32>,
    path: &[&str],
    expected: u32,
) -> Result<(), String> {
    let port = port_at(&target.ports, path)
        .ok_or_else(|| format!("path hierarchy output `{}` has no port", path.join("/")))?;
    (edge_from.get(&port) == Some(&expected))
        .then_some(())
        .ok_or_else(|| {
            format!(
                "path hierarchy output `{}` has an unsupported feed",
                path.join("/")
            )
        })
}

fn inherited_call_port(
    node: roxmltree::Node<'_, '_>,
    inherited: Option<u32>,
    wanted: u32,
    attribute: &str,
    wanted_name: Option<&str>,
) -> Option<u32> {
    for child in node.children().filter(|child| child.is_element()) {
        let component = if child.has_tag_name("entry") {
            parse_u32(child.attribute("componentid")).or(inherited)
        } else {
            inherited
        };
        let name_matches = wanted_name.is_none_or(|wanted_name| {
            child.has_tag_name("entry")
                && crate::import::schema::normalize_xml_entry_name(
                    child.attribute("name").unwrap_or_default(),
                )
                .0 == wanted_name
        });
        if component == Some(wanted)
            && name_matches
            && let Some(key) = parse_u32(child.attribute(attribute))
        {
            return Some(key);
        }
        if let Some(key) = inherited_call_port(child, component, wanted, attribute, wanted_name) {
            return Some(key);
        }
    }
    None
}
