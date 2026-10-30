//! Reads a core kind=32 join component's tuple branches, output ports and
//! equality key paths, before any input is resolved against the design.

use std::collections::{BTreeMap, BTreeSet};

use roxmltree::Node as XmlNode;

use super::super::schema::normalize_xml_entry_name;
use super::{JoinEquality, JoinInput, JoinKey, JoinOutput, ParsedJoin};

pub(super) fn declared_output_ports(component: XmlNode<'_, '_>) -> (BTreeSet<u32>, BTreeSet<u32>) {
    let mut rows = BTreeSet::new();
    let mut fields = BTreeSet::new();
    let Some(tuple) = component
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("tuple"))
    else {
        return (rows, fields);
    };
    if let Some(port) = tuple
        .attribute("outkey")
        .and_then(|value| value.parse().ok())
    {
        rows.insert(port);
    }
    for branch in tuple.children().filter(|node| node.has_tag_name("entry")) {
        let Some(root) = branch.children().find(|node| node.has_tag_name("entry")) else {
            continue;
        };
        for output in root.descendants().filter(|node| node.has_tag_name("entry")) {
            let Some(port) = output
                .attribute("outkey")
                .and_then(|value| value.parse().ok())
            else {
                continue;
            };
            if output == root {
                rows.insert(port);
            } else {
                fields.insert(port);
            }
        }
    }
    (rows, fields)
}

pub(super) fn parse(component: &XmlNode<'_, '_>) -> Result<ParsedJoin, String> {
    if component.attribute("library") != Some("core") || component.attribute("kind") != Some("32") {
        return Err("component is not a core kind=32 join".to_string());
    }
    let data = one_child(*component, "data", "join component")?;
    let root = one_child(data, "root", "join data")?;
    let document = one_named_entry(root, "document", "join root")?;
    let tuple = one_named_entry(document, "tuple", "join document")?;
    let tuple_output = optional_port(tuple, "outkey", "join tuple")?;
    let inputs = parse_inputs(tuple)?;

    let join = one_child(data, "join", "join data")?;
    ensure_empty_custom_conditions(join)?;
    let keypaths = one_child(join, "keypaths", "join metadata")?;
    let paths = parse_keypaths(keypaths)?;
    let joinkeys = one_child(join, "joinkeys", "join metadata")?;
    let equalities = parse_equalities(joinkeys, &paths, inputs.len())?;

    validate_ports(tuple_output, &inputs)?;
    Ok(ParsedJoin {
        tuple_output,
        inputs,
        equalities,
    })
}

fn parse_inputs(tuple: XmlNode<'_, '_>) -> Result<Vec<JoinInput>, String> {
    let mut branches = BTreeMap::new();
    for branch in tuple.children().filter(|node| node.has_tag_name("entry")) {
        let name = branch.attribute("name").unwrap_or_default();
        let index = name
            .strip_prefix("dynamic_tree_node")
            .filter(|suffix| !suffix.is_empty())
            .and_then(|suffix| suffix.parse::<usize>().ok())
            .ok_or_else(|| format!("join tuple has unsupported branch `{name}`"))?;
        if branches.insert(index, branch).is_some() {
            return Err(format!("join tuple repeats input index {index}"));
        }
    }
    if branches.len() < 2 {
        return Err("join must declare at least two inputs".to_string());
    }
    for (expected, actual) in branches.keys().copied().enumerate() {
        if actual != expected {
            return Err(format!(
                "join input indices must be contiguous from 0; expected {expected}, found {actual}"
            ));
        }
    }

    branches
        .into_iter()
        .map(|(index, branch)| parse_input(index, branch))
        .collect()
}

fn parse_input(index: usize, branch: XmlNode<'_, '_>) -> Result<JoinInput, String> {
    let roots = branch
        .children()
        .filter(|node| node.has_tag_name("entry"))
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(format!(
            "join input {index} must contain exactly one root entry"
        ));
    };
    let input_port = required_port(*root, "inpkey", &format!("join input {index}"))?;
    let (name, _) = normalize_xml_entry_name(root.attribute("name").unwrap_or_default());
    if name.is_empty() {
        return Err(format!("join input {index} has no root name"));
    }
    let mut outputs = Vec::new();
    if let Some(port) = optional_port(*root, "outkey", &format!("join input {index} root"))? {
        outputs.push(JoinOutput {
            port,
            path: Vec::new(),
        });
    }
    collect_outputs(*root, &mut Vec::new(), &mut outputs)?;
    Ok(JoinInput {
        index,
        name: name.to_string(),
        input_port,
        outputs,
    })
}

fn collect_outputs(
    entry: XmlNode<'_, '_>,
    path: &mut Vec<String>,
    outputs: &mut Vec<JoinOutput>,
) -> Result<(), String> {
    for child in entry.children().filter(|node| node.has_tag_name("entry")) {
        let (name, _) = normalize_xml_entry_name(child.attribute("name").unwrap_or_default());
        if name.is_empty() {
            return Err("join output entry has no name".to_string());
        }
        path.push(name.to_string());
        if let Some(port) = optional_port(child, "outkey", "join output entry")? {
            outputs.push(JoinOutput {
                port,
                path: path.clone(),
            });
        }
        collect_outputs(child, path, outputs)?;
        path.pop();
    }
    Ok(())
}

fn parse_keypaths(keypaths: XmlNode<'_, '_>) -> Result<BTreeMap<u32, Vec<String>>, String> {
    let mut paths = BTreeMap::new();
    for root in keypaths
        .children()
        .filter(|node| node.has_tag_name("entry"))
    {
        collect_keypaths(root, &mut Vec::new(), &mut paths)?;
    }
    Ok(paths)
}

fn collect_keypaths(
    entry: XmlNode<'_, '_>,
    path: &mut Vec<String>,
    paths: &mut BTreeMap<u32, Vec<String>>,
) -> Result<(), String> {
    let raw_name = entry.attribute("name").unwrap_or_default();
    let (name, _) = normalize_xml_entry_name(raw_name);
    if !name.is_empty() {
        path.push(name.to_string());
    }
    if let Some(id) = optional_port(entry, "outkey", "join key path")?
        && paths.insert(id, path.clone()).is_some()
    {
        return Err(format!("join repeats key path id {id}"));
    }
    for child in entry.children().filter(|node| node.has_tag_name("entry")) {
        collect_keypaths(child, path, paths)?;
    }
    if !name.is_empty() {
        path.pop();
    }
    Ok(())
}

fn parse_equalities(
    joinkeys: XmlNode<'_, '_>,
    paths: &BTreeMap<u32, Vec<String>>,
    input_count: usize,
) -> Result<Vec<JoinEquality>, String> {
    let pairs = joinkeys
        .children()
        .filter(|node| node.has_tag_name("keypair"))
        .collect::<Vec<_>>();
    if pairs.is_empty() {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        return Err("join must declare at least one equality condition".to_string());
    }
    let mut equalities = Vec::with_capacity(pairs.len());
    let mut used_inputs = BTreeSet::new();
    for pair in pairs {
        let first = parse_key(
            one_child(pair, "first-key", "join key pair")?,
            0,
            paths,
            input_count,
        )?;
        let second = parse_key(
            one_child(pair, "second-key", "join key pair")?,
            1,
            paths,
            input_count,
        )?;
        if first.input_index == second.input_index {
            return Err(format!(
                "join equality must compare distinct inputs; both use index {}",
                first.input_index
            ));
        }
        used_inputs.insert(first.input_index);
        used_inputs.insert(second.input_index);
        equalities.push(JoinEquality { first, second });
    }
    if used_inputs.len() != input_count {
        return Err("every join input must participate in an equality condition".to_string());
    }
    for stage in 1..input_count {
        if !equalities.iter().any(|equality| {
            equality.first.input_index == stage && equality.second.input_index < stage
                || equality.second.input_index == stage && equality.first.input_index < stage
        }) {
            return Err(format!(
                "join input {stage} must have an equality with an earlier input"
            ));
        }
    }
    Ok(equalities)
}

fn parse_key(
    key: XmlNode<'_, '_>,
    default_input: usize,
    paths: &BTreeMap<u32, Vec<String>>,
    input_count: usize,
) -> Result<JoinKey, String> {
    let path_id = required_port(key, "path-id", "join equality key")?;
    let input_index = match key.attribute("input-index") {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| format!("join equality has invalid input index `{value}`"))?,
        None => default_input,
    };
    if input_index >= input_count {
        return Err(format!(
            "join equality input index {input_index} is out of range for {input_count} inputs"
        ));
    }
    let path = paths
        .get(&path_id)
        .cloned()
        .ok_or_else(|| format!("join equality references unknown key path id {path_id}"))?;
    Ok(JoinKey {
        input_index,
        path_id,
        path,
    })
}

fn ensure_empty_custom_conditions(join: XmlNode<'_, '_>) -> Result<(), String> {
    for condition in join
        .descendants()
        .filter(|node| node.has_tag_name("condition"))
    {
        let has_content = condition.attributes().len() != 0
            || condition.children().any(|child| {
                child.is_element() || child.text().is_some_and(|text| !text.trim().is_empty())
            });
        if has_content {
            return Err("join custom key-path conditions are not supported".to_string());
        }
    }
    Ok(())
}

fn validate_ports(tuple_output: Option<u32>, inputs: &[JoinInput]) -> Result<(), String> {
    let mut ports = BTreeSet::new();
    if let Some(port) = tuple_output {
        ports.insert(port);
    }
    for input in inputs {
        if !ports.insert(input.input_port) {
            return Err(format!("join repeats port key {}", input.input_port));
        }
        for output in &input.outputs {
            if !ports.insert(output.port) {
                return Err(format!("join repeats port key {}", output.port));
            }
        }
    }
    Ok(())
}

fn one_child<'a, 'input>(
    parent: XmlNode<'a, 'input>,
    tag: &str,
    context: &str,
) -> Result<XmlNode<'a, 'input>, String> {
    let matches = parent
        .children()
        .filter(|node| node.has_tag_name(tag))
        .collect::<Vec<_>>();
    let [node] = matches.as_slice() else {
        return Err(format!("{context} must contain exactly one <{tag}>"));
    };
    Ok(*node)
}

fn one_named_entry<'a, 'input>(
    parent: XmlNode<'a, 'input>,
    name: &str,
    context: &str,
) -> Result<XmlNode<'a, 'input>, String> {
    let matches = parent
        .children()
        .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some(name))
        .collect::<Vec<_>>();
    let [node] = matches.as_slice() else {
        return Err(format!("{context} must contain exactly one `{name}` entry"));
    };
    Ok(*node)
}

fn required_port(node: XmlNode<'_, '_>, attribute: &str, context: &str) -> Result<u32, String> {
    let value = node
        .attribute(attribute)
        .ok_or_else(|| format!("{context} has no `{attribute}`"))?;
    value
        .parse::<u32>()
        .map_err(|_| format!("{context} has invalid `{attribute}` value `{value}`"))
}

fn optional_port(
    node: XmlNode<'_, '_>,
    attribute: &str,
    context: &str,
) -> Result<Option<u32>, String> {
    node.attribute(attribute)
        .map(|_| required_port(node, attribute, context))
        .transpose()
}
