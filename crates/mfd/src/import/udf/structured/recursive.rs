use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{SchemaKind, Value};

use super::{ImportedDefinition, Recipe, RecipeSource};
use crate::import::function::read as read_function;
use crate::import::graph::read_edges;
use crate::import::schema::{SchemaComponent, parse_u32, read_schema_component, schema_node_at};
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
    let xml = children
        .children()
        .filter(|node| node.has_tag_name("component") && node.attribute("library") == Some("xml"))
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
        .ok_or("recursive XML input schema cannot be read")?;
    let output = read_schema_component(&output_node, mfd_path, &mut warnings)
        .ok_or("recursive XML output schema cannot be read")?;
    let recursive_input = matches!(&input.schema.kind, SchemaKind::Group { children, .. }
        if children.iter().any(|child| child.repeating && child.recursive_ref.is_some()));
    if !recursive_input {
        return Ok(None);
    }
    if input.schema == output.schema {
        return try_read_filter(component, mfd_path).map(Some);
    }
    let scalar_output = matches!(&output.schema.kind, SchemaKind::Group { children, .. }
        if children.iter().any(|child| child.repeating && matches!(child.kind, SchemaKind::Scalar { .. })));
    if scalar_output {
        return try_read_collect(component, mfd_path).map(Some);
    }
    Ok(None)
}

fn try_read_collect(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
) -> Result<ImportedDefinition, String> {
    let structure = component
        .children()
        .find(|node| node.has_tag_name("structure"))
        .ok_or("recursive definition has no structure")?;
    let components = structure
        .children()
        .find(|node| node.has_tag_name("children"))
        .ok_or("recursive definition has no component list")?
        .children()
        .filter(|node| node.has_tag_name("component"))
        .collect::<Vec<_>>();
    let xml = components
        .iter()
        .copied()
        .filter(|node| node.attribute("library") == Some("xml"))
        .collect::<Vec<_>>();
    let [first, second] = xml.as_slice() else {
        return Err("recursive scalar collection requires one XML input and output".to_string());
    };
    let (input_node, output_node) = match (is_output(*first), is_output(*second)) {
        (false, true) => (*first, *second),
        (true, false) => (*second, *first),
        _ => return Err("recursive XML parameter roles are ambiguous".to_string()),
    };
    let mut warnings = Vec::new();
    let input = read_schema_component(&input_node, mfd_path, &mut warnings)
        .ok_or("recursive XML input schema cannot be read")?;
    let output = read_schema_component(&output_node, mfd_path, &mut warnings)
        .ok_or("recursive XML output schema cannot be read")?;
    let input_id = component_id(input_node)?;
    let output_id = component_id(output_node)?;

    let SchemaKind::Group {
        children: input_children,
        ..
    } = &input.schema.kind
    else {
        return Err("recursive input parameter is not a group".to_string());
    };
    let recursive = input_children
        .iter()
        .find(|child| child.repeating && child.recursive_ref.is_some())
        .ok_or("recursive input has no repeating recursive child group")?;
    let SchemaKind::Group {
        children: output_children,
        ..
    } = &output.schema.kind
    else {
        return Err("recursive output parameter is not a group".to_string());
    };
    let output_value = output_children
        .iter()
        .find(|child| child.repeating && matches!(child.kind, SchemaKind::Scalar { .. }))
        .ok_or("recursive output has no repeating scalar field")?;

    let functions = components
        .iter()
        .copied()
        .map(|node| (node, read_function(&node)))
        .collect::<Vec<_>>();
    let scalar_parameters = functions
        .iter()
        .filter(|(_, function)| function.kind == 6)
        .collect::<Vec<_>>();
    let [(_, prefix_function)] = scalar_parameters.as_slice() else {
        return Err("recursive scalar collection requires one scalar prefix parameter".to_string());
    };
    let prefix_id = scalar_parameters
        .first()
        .and_then(|(node, _)| parse_u32(node.attribute("uid")))
        .ok_or("recursive prefix parameter id is invalid")?;
    let [prefix_output] = prefix_function.outputs.as_slice() else {
        return Err("recursive prefix parameter pins are invalid".to_string());
    };
    let concats = functions
        .iter()
        .filter(|(_, function)| function.kind == 5 && function.name == "concat")
        .collect::<Vec<_>>();
    let [(_, first_concat), (_, second_concat)] = concats.as_slice() else {
        return Err("recursive scalar collection requires two concat functions".to_string());
    };
    let constants = functions
        .iter()
        .filter_map(|(_, function)| {
            let (value, datatype) = function.constant.as_ref()?;
            let [output] = function.outputs.as_slice() else {
                return None;
            };
            match crate::import::function::parse_constant(value, datatype) {
                Value::String(value) => Some((*output, value)),
                _ => None,
            }
        })
        .collect::<Vec<_>>();
    let [(separator_output, separator)] = constants.as_slice() else {
        return Err("recursive scalar collection requires one string separator".to_string());
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
        .ok_or("recursive definition has no direct self call")?;
    let recursive_input = call_port(recursive_call, input_id, "inpkey")?;
    let recursive_prefix = call_port(recursive_call, prefix_id, "inpkey")?;
    let recursive_output = call_port(recursive_call, output_id, "outkey")?;
    let recursive_source = edge_from
        .get(&recursive_input)
        .copied()
        .ok_or("recursive child input is not connected")?;
    if input.ports.get(&recursive_source) != Some(&vec![recursive.name.clone()]) {
        return Err("self call is not driven by the recursive child group".to_string());
    }

    let output_inputs = output
        .ports
        .iter()
        .filter(|(key, path)| {
            **path == vec![output_value.name.clone()] && edge_from.contains_key(key)
        })
        .map(|(key, _)| *key)
        .collect::<Vec<_>>();
    if output_inputs.len() != 2
        || !output_inputs
            .iter()
            .any(|input| edge_from.get(input) == Some(&recursive_output))
    {
        return Err("recursive output must concatenate direct and recursive values".to_string());
    }

    let concat_candidates = [first_concat, second_concat];
    let direct_concat = concat_candidates
        .iter()
        .copied()
        .find(|function| {
            function.outputs.iter().any(|output| {
                output_inputs
                    .iter()
                    .any(|input| edge_from.get(input) == Some(output))
            })
        })
        .ok_or("direct scalar output is not driven by concat")?;
    let descent_concat = concat_candidates
        .iter()
        .copied()
        .find(|function| {
            function
                .outputs
                .iter()
                .any(|output| edge_from.get(&recursive_prefix) == Some(output))
        })
        .ok_or("recursive prefix is not driven by concat")?;
    if std::ptr::eq(direct_concat, descent_concat) {
        return Err("direct value and recursive prefix must use distinct concat functions".into());
    }
    let direct_feeds = input_feeds(direct_concat, &edge_from);
    let descent_feeds = input_feeds(descent_concat, &edge_from);
    let [descent_output] = descent_concat.outputs.as_slice() else {
        return Err("recursive prefix concat has invalid outputs".to_string());
    };
    if !descent_feeds.contains(prefix_output)
        || !descent_feeds.contains(separator_output)
        || !direct_feeds.contains(descent_output)
        || !direct_feeds.contains(separator_output)
    {
        return Err("recursive concat omits the accumulated prefix or separator".to_string());
    }
    let leaf_path = input
        .ports
        .iter()
        .find(|(key, path)| {
            path.len() >= 2
                && direct_feeds.contains(key)
                && matches!(
                    crate::import::schema::schema_node_at(&input.schema, path)
                        .map(|node| &node.kind),
                    Some(SchemaKind::Scalar { .. })
                )
        })
        .map(|(_, path)| path.clone())
        .ok_or("direct concat does not read a nested scalar leaf")?;
    let descent_path = input
        .ports
        .iter()
        .find(|(key, path)| {
            path.len() == 1
                && descent_feeds.contains(key)
                && matches!(
                    crate::import::schema::schema_node_at(&input.schema, path)
                        .map(|node| &node.kind),
                    Some(SchemaKind::Scalar { .. })
                )
        })
        .map(|(_, path)| path.clone())
        .ok_or("recursive prefix concat does not read a root scalar")?;
    let (values, value) = leaf_path.split_at(leaf_path.len() - 1);

    Ok((
        Definition {
            scalar_interface: None,
            parameters: BTreeSet::from([prefix_id]),
            structured_parameters: BTreeSet::from([input_id]),
            outputs: BTreeMap::from([(
                output_id,
                OutputExpr::Structured(Recipe {
                    source: RecipeSource::RecursiveCollect {
                        component_id: input_id,
                        prefix_parameter: prefix_id,
                        children: vec![recursive.name.clone()],
                        descent_value: descent_path,
                        values: values.to_vec(),
                        value: value.to_vec(),
                        output: vec![output_value.name.clone()],
                        separator: separator.clone(),
                    },
                    filter: None,
                    bindings: BTreeMap::new(),
                }),
            )]),
        },
        None,
        warnings,
    ))
}

fn try_read_filter(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
) -> Result<ImportedDefinition, String> {
    let structure = component
        .children()
        .find(|node| node.has_tag_name("structure"))
        .ok_or("recursive filter definition has no structure")?;
    let components = structure
        .children()
        .find(|node| node.has_tag_name("children"))
        .ok_or("recursive filter definition has no component list")?
        .children()
        .filter(|node| node.has_tag_name("component"))
        .collect::<Vec<_>>();
    let xml = components
        .iter()
        .copied()
        .filter(|node| node.attribute("library") == Some("xml"))
        .collect::<Vec<_>>();
    let [first, second] = xml.as_slice() else {
        return Err("recursive filter requires one XML input and output".to_string());
    };
    let (input_node, output_node) = match (is_output(*first), is_output(*second)) {
        (false, true) => (*first, *second),
        (true, false) => (*second, *first),
        _ => return Err("recursive filter XML parameter roles are ambiguous".to_string()),
    };
    let mut warnings = Vec::new();
    let input = read_schema_component(&input_node, mfd_path, &mut warnings)
        .ok_or("recursive filter input schema cannot be read")?;
    let output = read_schema_component(&output_node, mfd_path, &mut warnings)
        .ok_or("recursive filter output schema cannot be read")?;
    if input.schema != output.schema {
        return Err("recursive filter input and output schemas must match".to_string());
    }
    let input_id = component_id(input_node)?;
    let output_id = component_id(output_node)?;
    let SchemaKind::Group { children, .. } = &input.schema.kind else {
        return Err("recursive filter input is not a group".to_string());
    };
    let recursive = children
        .iter()
        .filter(|child| child.repeating && child.recursive_ref.is_some())
        .collect::<Vec<_>>();
    let [recursive] = recursive.as_slice() else {
        return Err("recursive filter requires exactly one recursive child collection".to_string());
    };
    let item_groups = children
        .iter()
        .filter(|child| {
            child.repeating
                && child.recursive_ref.is_none()
                && matches!(child.kind, SchemaKind::Group { .. })
        })
        .collect::<Vec<_>>();
    let [items] = item_groups.as_slice() else {
        return Err("recursive filter requires exactly one direct item collection".to_string());
    };

    let functions = components
        .iter()
        .copied()
        .map(|node| (node, read_function(&node)))
        .collect::<Vec<_>>();
    let filters = functions
        .iter()
        .filter(|(_, function)| function.kind == 3)
        .collect::<Vec<_>>();
    let [(_, filter)] = filters.as_slice() else {
        return Err("recursive filter requires exactly one filter component".to_string());
    };
    let predicates = functions
        .iter()
        .filter(|(_, function)| function.kind == 5 && function.name == "contains")
        .collect::<Vec<_>>();
    let [(_, predicate)] = predicates.as_slice() else {
        return Err("recursive filter requires exactly one contains predicate".to_string());
    };
    let parameters = functions
        .iter()
        .filter(|(_, function)| function.kind == 6)
        .collect::<Vec<_>>();
    let [(parameter_node, parameter)] = parameters.as_slice() else {
        return Err("recursive filter requires exactly one scalar parameter".to_string());
    };
    let recursive_calls = components
        .iter()
        .copied()
        .filter(|node| {
            node.attribute("kind") == Some("19")
                && node.attribute("library") == component.attribute("library")
                && node.attribute("name") == component.attribute("name")
        })
        .collect::<Vec<_>>();
    let [recursive_call] = recursive_calls.as_slice() else {
        return Err("recursive filter requires exactly one direct self call".to_string());
    };
    if components.len() != 6 {
        return Err("recursive filter contains an unsupported nested component".to_string());
    }

    let parameter_id = component_id(*parameter_node)?;
    let [parameter_output] = parameter.outputs.as_slice() else {
        return Err("recursive filter scalar parameter pins are invalid".to_string());
    };
    let [Some(filter_values), Some(filter_predicate)] = filter.inputs.as_slice() else {
        return Err("recursive filter component pins are invalid".to_string());
    };
    let [filter_output, ..] = filter.outputs.as_slice() else {
        return Err("recursive filter component has no output".to_string());
    };
    let [Some(predicate_left), Some(predicate_right)] = predicate.inputs.as_slice() else {
        return Err("recursive filter contains pins are invalid".to_string());
    };
    let [predicate_output] = predicate.outputs.as_slice() else {
        return Err("recursive filter contains has no output".to_string());
    };
    let edge_from = read_edges(&structure, Some(component));

    let input_root = port_at(&input, &[])?;
    let output_root = port_at(&output, &[])?;
    require_edge(
        &edge_from,
        output_root,
        input_root,
        "document root is not copied",
    )?;
    let item_path = vec![items.name.clone()];
    let input_items = port_at(&input, &item_path)?;
    let output_items = port_at(&output, &item_path)?;
    require_edge(
        &edge_from,
        *filter_values,
        input_items,
        "filter is not driven by the direct item collection",
    )?;
    require_edge(
        &edge_from,
        output_items,
        *filter_output,
        "filtered items do not drive the output item collection",
    )?;
    require_edge(
        &edge_from,
        *filter_predicate,
        *predicate_output,
        "filter predicate is not driven by contains",
    )?;

    let predicate_inputs = [*predicate_left, *predicate_right];
    let value_candidates = input
        .ports
        .iter()
        .filter(|(key, path)| {
            path.starts_with(&item_path)
                && path.len() > item_path.len()
                && predicate_inputs
                    .iter()
                    .any(|pin| edge_from.get(pin) == Some(*key))
                && schema_node_at(&input.schema, path)
                    .is_some_and(|node| matches!(node.kind, SchemaKind::Scalar { .. }))
        })
        .collect::<Vec<_>>();
    let [(value_port, value_path)] = value_candidates.as_slice() else {
        return Err("contains must read exactly one scalar item descendant".to_string());
    };
    let value_input = predicate_inputs
        .iter()
        .position(|pin| edge_from.get(pin) == Some(*value_port))
        .ok_or("contains item input is not connected")?;
    let parameter_input = 1usize.saturating_sub(value_input);
    require_edge(
        &edge_from,
        predicate_inputs[parameter_input],
        *parameter_output,
        "contains is not driven by the scalar parameter",
    )?;

    let child_path = vec![recursive.name.clone()];
    let input_children = port_at(&input, &child_path)?;
    let output_children = port_at(&output, &child_path)?;
    let self_input = call_port(*recursive_call, input_id, "inpkey")?;
    let self_parameter = call_port(*recursive_call, parameter_id, "inpkey")?;
    let self_output = call_port(*recursive_call, output_id, "outkey")?;
    require_edge(
        &edge_from,
        self_input,
        input_children,
        "self call is not driven by the recursive child collection",
    )?;
    require_edge(
        &edge_from,
        self_parameter,
        *parameter_output,
        "self call does not receive the scalar parameter",
    )?;
    require_edge(
        &edge_from,
        output_children,
        self_output,
        "self call output does not drive the recursive child collection",
    )?;

    for child in children
        .iter()
        .filter(|child| child.name != items.name && child.name != recursive.name)
    {
        let path = vec![child.name.clone()];
        let source = port_at(&input, &path)?;
        let target = port_at(&output, &path)?;
        require_edge(
            &edge_from,
            target,
            source,
            &format!("direct field `{}` is not copied", child.name),
        )?;
    }

    Ok((
        Definition {
            scalar_interface: None,
            parameters: BTreeSet::from([parameter_id]),
            structured_parameters: BTreeSet::from([input_id]),
            outputs: BTreeMap::from([(
                output_id,
                OutputExpr::Structured(Recipe {
                    source: RecipeSource::RecursiveFilter {
                        component_id: input_id,
                        predicate_parameter: parameter_id,
                        children: recursive.name.clone(),
                        items: items.name.clone(),
                        value: value_path[item_path.len()..].to_vec(),
                        value_first: value_input == 0,
                    },
                    filter: None,
                    bindings: BTreeMap::new(),
                }),
            )]),
        },
        None,
        warnings,
    ))
}

fn port_at(component: &SchemaComponent, path: &[String]) -> Result<u32, String> {
    let ports = component
        .ports
        .iter()
        .filter(|(_, candidate)| candidate.as_slice() == path)
        .map(|(key, _)| *key)
        .collect::<Vec<_>>();
    let [port] = ports.as_slice() else {
        return Err(format!(
            "recursive XML parameter has no unique `{}` port",
            path.join("/")
        ));
    };
    Ok(*port)
}

fn require_edge(
    edge_from: &BTreeMap<u32, u32>,
    input: u32,
    output: u32,
    reason: &str,
) -> Result<(), String> {
    (edge_from.get(&input) == Some(&output))
        .then_some(())
        .ok_or_else(|| reason.to_string())
}

fn is_output(component: roxmltree::Node<'_, '_>) -> bool {
    component
        .descendants()
        .any(|node| node.has_tag_name("parameter") && node.attribute("usageKind") == Some("output"))
}

fn component_id(component: roxmltree::Node<'_, '_>) -> Result<u32, String> {
    parse_u32(component.attribute("uid"))
        .ok_or_else(|| "recursive XML parameter id is invalid".to_string())
}

fn call_port(
    call: roxmltree::Node<'_, '_>,
    component_id: u32,
    attribute: &str,
) -> Result<u32, String> {
    inherited_call_port(call, None, component_id, attribute)
        .ok_or_else(|| format!("recursive call has no `{attribute}` for parameter {component_id}"))
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

fn input_feeds(
    function: &crate::import::function::FnComponent,
    edge_from: &BTreeMap<u32, u32>,
) -> BTreeSet<u32> {
    function
        .inputs
        .iter()
        .flatten()
        .filter_map(|input| edge_from.get(input).copied())
        .collect()
}
