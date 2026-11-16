use std::collections::{BTreeMap, BTreeSet};

use ir::XML_TEXT_FIELD;

use super::{PortDeclarationCensus, normalize_xml_entry_name, parse_u32, port_declarations};

pub(super) fn collect_entry_ports(
    entry: &roxmltree::Node,
    path: &mut Vec<String>,
    ports: &mut BTreeMap<u32, Vec<String>>,
    out_count: &mut usize,
    in_count: &mut usize,
) {
    collect_entry_ports_with_declarations(entry, path, ports, out_count, in_count, None);
}

pub(in super::super) fn collect_entry_ports_with_declarations<'doc, 'input>(
    entry: &roxmltree::Node<'doc, 'input>,
    path: &mut Vec<String>,
    ports: &mut BTreeMap<u32, Vec<String>>,
    out_count: &mut usize,
    in_count: &mut usize,
    mut declarations: Option<&mut PortDeclarationCensus<'doc, 'input>>,
) {
    for child in entry
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
    {
        if is_document_decoration_entry(&child) {
            continue;
        }
        let raw_name = child.attribute("name").unwrap_or_default();
        let (name, _) = normalize_xml_entry_name(raw_name);
        if child.attribute("type") == Some("xml-type") && name != XML_TEXT_FIELD {
            // xml-type entries are transparent type wrappers: descend
            // without extending the path.
            collect_entry_ports_with_declarations(
                &child,
                path,
                ports,
                out_count,
                in_count,
                declarations.as_deref_mut(),
            );
            continue;
        }
        path.push(name.to_string());
        if let Some(declarations) = declarations.as_deref_mut() {
            declarations.record(child, path);
        }
        if let Some(key) = parse_u32(child.attribute("outkey")) {
            *out_count += 1;
            ports.insert(key, path.clone());
        }
        if let Some(key) = parse_u32(child.attribute("inpkey")) {
            *in_count += 1;
            ports.insert(key, path.clone());
        }
        collect_entry_ports_with_declarations(
            &child,
            path,
            ports,
            out_count,
            in_count,
            declarations.as_deref_mut(),
        );
        path.pop();
    }
}

pub(super) fn input_port_ancestors(
    entry: &roxmltree::Node<'_, '_>,
    input_keys: &BTreeSet<u32>,
) -> BTreeMap<u32, Vec<u32>> {
    input_port_ancestry(entry, input_keys).ancestors
}

#[cfg(test)]
pub(super) fn input_branch_paths(
    entry: &roxmltree::Node<'_, '_>,
    input_keys: &BTreeSet<u32>,
) -> Vec<(u32, Vec<String>)> {
    input_port_ancestry(entry, input_keys).branches
}

pub(super) struct InputPortAncestry {
    pub(super) ancestors: BTreeMap<u32, Vec<u32>>,
    pub(super) branches: Vec<(u32, Vec<String>)>,
    pub(super) ambiguous_inputs: BTreeSet<u32>,
}

pub(super) fn input_port_ancestry(
    entry: &roxmltree::Node<'_, '_>,
    input_keys: &BTreeSet<u32>,
) -> InputPortAncestry {
    input_port_ancestry_projected(entry, input_keys, false)
}

pub(super) fn input_port_ancestry_projected(
    entry: &roxmltree::Node<'_, '_>,
    input_keys: &BTreeSet<u32>,
    json_paths: bool,
) -> InputPortAncestry {
    let mut result = BTreeMap::new();
    let mut branches = Vec::new();
    let mut ambiguous_inputs = BTreeSet::new();
    let mut ancestors = Vec::new();
    let mut next_branch = input_keys
        .last()
        .copied()
        .and_then(|key| key.checked_add(1))
        .unwrap_or_default();
    if let Some(key) = parse_u32(entry.attribute("inpkey")) {
        result.insert(key, Vec::new());
        ancestors.push(key);
    }
    collect_input_port_ancestors(
        entry,
        input_keys,
        &mut next_branch,
        &mut ancestors,
        &mut result,
        &mut Vec::new(),
        &mut branches,
        &mut ambiguous_inputs,
        json_paths,
    );
    InputPortAncestry {
        ancestors: result,
        branches,
        ambiguous_inputs,
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_input_port_ancestors(
    entry: &roxmltree::Node<'_, '_>,
    input_keys: &BTreeSet<u32>,
    next_branch: &mut u32,
    ancestors: &mut Vec<u32>,
    result: &mut BTreeMap<u32, Vec<u32>>,
    path: &mut Vec<String>,
    branches: &mut Vec<(u32, Vec<String>)>,
    ambiguous_inputs: &mut BTreeSet<u32>,
    json_paths: bool,
) {
    let children = entry
        .children()
        .filter(|node| {
            node.is_element() && node.has_tag_name("entry") && !is_document_decoration_entry(node)
        })
        .collect::<Vec<_>>();
    let mut name_counts = BTreeMap::new();
    for child in &children {
        let normalized = normalize_xml_entry_name(child.attribute("name").unwrap_or_default());
        *name_counts.entry(normalized).or_insert(0usize) += 1;
    }
    for child in children {
        let normalized = normalize_xml_entry_name(child.attribute("name").unwrap_or_default());
        let duplicate_branch = name_counts.get(&normalized).copied().unwrap_or_default() > 1;
        let branch = duplicate_branch.then(|| take_branch_marker(input_keys, next_branch));
        let advances_path = !json_paths || child.attribute("type") == Some("json-property");
        if advances_path {
            path.push(if json_paths {
                child.attribute("name").unwrap_or_default().to_string()
            } else {
                normalized.0.to_owned()
            });
        }
        if let Some(branch) = branch {
            // Allocate every legacy marker in the same raw declaration order.
            // JSON metadata projects only ordinary duplicate object item views;
            // scalar/null alternatives and protocol wrappers are not occurrences.
            ancestors.push(branch);
            let json_object_item = port_declarations::ordinary_json_object_item(child);
            if !json_paths || json_object_item {
                branches.push((branch, path.clone()));
            }
        }
        let key = parse_u32(child.attribute("inpkey"));
        if let Some(key) = key {
            if result
                .get(&key)
                .is_some_and(|previous| previous != ancestors)
            {
                ambiguous_inputs.insert(key);
            }
            result.insert(key, ancestors.clone());
            ancestors.push(key);
        }
        collect_input_port_ancestors(
            &child,
            input_keys,
            next_branch,
            ancestors,
            result,
            path,
            branches,
            ambiguous_inputs,
            json_paths,
        );
        if key.is_some() {
            ancestors.pop();
        }
        if branch.is_some() {
            ancestors.pop();
        }
        if advances_path {
            path.pop();
        }
    }
}

pub(super) fn is_document_decoration_entry(entry: &roxmltree::Node<'_, '_>) -> bool {
    matches!(
        entry.attribute("type"),
        Some(
            "comment-before"
                | "comment-after"
                | "processing-instruction-before"
                | "processing-instruction-after"
        )
    )
}

fn take_branch_marker(input_keys: &BTreeSet<u32>, next_branch: &mut u32) -> u32 {
    while input_keys.contains(next_branch) {
        *next_branch = next_branch.wrapping_add(1);
    }
    let marker = *next_branch;
    *next_branch = next_branch.wrapping_add(1);
    marker
}
