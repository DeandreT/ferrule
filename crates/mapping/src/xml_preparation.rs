//! Explicit XML materialization boundaries. This does not infer native phase
//! behavior from an XML schema or reorder the original mapping graph.
use crate::{Graph, Node, NodeId, Scope, ScopeConstruction, ScopeIteration};
use std::collections::BTreeSet;

pub fn split_xml_preparation(
    graph: &Graph,
    root: &Scope,
    early_fields: &[Vec<String>],
) -> Result<(Scope, Scope), &'static str> {
    let selected = early_fields.iter().cloned().collect::<BTreeSet<_>>();
    if selected.len() != early_fields.len()
        || selected.len() > 1024
        || selected
            .iter()
            .any(|p| p.is_empty() || p.len() > 64 || p.iter().any(String::is_empty))
    {
        return Err("invalid XML preparation field paths");
    }
    let mut found = BTreeSet::new();
    let mut count = 0;
    let (early, late) = split(root, &[], &selected, &mut found, 0, &mut count)?;
    if found != selected {
        return Err("unknown XML preparation field path");
    }
    let early = early.unwrap_or_default();
    let late = late.unwrap_or_default();
    let left = reachable(graph, &early)?;
    let right = reachable(graph, &late)?;
    for id in left.intersection(&right) {
        if matches!(
            graph.nodes.get(id),
            Some(
                Node::RuntimeValue { .. }
                    | Node::InvocationCounter { .. }
                    | Node::UserFunctionCall { .. }
            )
        ) {
            return Err("stateful XML expression crosses a materialization boundary");
        }
    }
    Ok((early, late))
}

fn split(
    scope: &Scope,
    path: &[String],
    selected: &BTreeSet<Vec<String>>,
    found: &mut BTreeSet<Vec<String>>,
    depth: usize,
    count: &mut usize,
) -> Result<(Option<Scope>, Option<Scope>), &'static str> {
    *count += 1;
    if depth > 64 || *count > 10000 {
        return Err("XML preparation scope limit");
    }
    if !scope.dynamic_bindings.is_empty()
        || !scope.dynamic_children.is_empty()
        || scope.merge_dynamic_fields
        || scope.group_by.is_some()
        || scope.group_adjacent_by.is_some()
        || scope.group_starting_with.is_some()
        || scope.group_ending_with.is_some()
        || scope.group_into_blocks.is_some()
        || scope.sort_by.is_some()
        || !scope.sort_then_by.is_empty()
        || !scope.windows.is_empty()
        || scope.post_group_filter.is_some()
    {
        return Err("unsupported XML preparation scope controls");
    }
    match &scope.iteration {
        ScopeIteration::None | ScopeIteration::Source(_) => {}
        ScopeIteration::Concatenate(parts) => {
            if selected.iter().any(|field| field.starts_with(path)) {
                return Err("early XML concatenations require a separate profile");
            }
            // The concatenation is retained intact, but every nested segment
            // must still satisfy this profile's controls and depth limits.
            let no_fields = BTreeSet::new();
            for part in parts.iter() {
                split(part, path, &no_fields, found, depth + 1, count)?;
            }
            return Ok((None, Some(scope.clone())));
        }
        _ => return Err("unsupported XML preparation iteration"),
    }
    match scope.construction {
        ScopeConstruction::Scalar { .. } => {
            if selected.contains(path) {
                found.insert(path.to_vec());
                return Ok((Some(scope.clone()), None));
            }
            return Ok((None, Some(scope.clone())));
        }
        ScopeConstruction::Constructed => {}
        _ => return Err("unsupported XML preparation construction"),
    }
    let mut early = scope.clone();
    early.bindings.clear();
    early.children.clear();
    let mut late = early.clone();
    // A constructed empty element still has observable presence. With no
    // selected fields it belongs to materialization, not to neither phase.
    if scope.bindings.is_empty() && scope.children.is_empty() {
        return Ok((None, Some(scope.clone())));
    }
    for binding in &scope.bindings {
        let mut field = path.to_vec();
        field.push(binding.target_field.clone());
        if selected.contains(&field) {
            found.insert(field);
            early.bindings.push(binding.clone());
        } else {
            late.bindings.push(binding.clone());
        }
    }
    for child in &scope.children {
        let mut child_path = path.to_vec();
        child_path.push(child.target_field.clone());
        let (a, b) = split(child, &child_path, selected, found, depth + 1, count)?;
        if let Some(child) = a {
            early.children.push(child);
        }
        if let Some(child) = b {
            late.children.push(child);
        }
    }
    Ok((
        (!early.bindings.is_empty() || !early.children.is_empty()).then_some(early),
        (!late.bindings.is_empty() || !late.children.is_empty()).then_some(late),
    ))
}

fn reachable(graph: &Graph, scope: &Scope) -> Result<BTreeSet<NodeId>, &'static str> {
    fn roots(scope: &Scope, into: &mut Vec<NodeId>) {
        into.extend(scope.filter);
        into.extend(scope.bindings.iter().map(|b| b.node));
        if let ScopeConstruction::Scalar { value } = scope.construction {
            into.push(value);
        }
        if let ScopeIteration::Concatenate(parts) = &scope.iteration {
            for part in parts.iter() {
                roots(part, into);
            }
        }
        for child in &scope.children {
            roots(child, into);
        }
    }
    let mut pending = Vec::new();
    roots(scope, &mut pending);
    let mut result = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !result.insert(id) {
            continue;
        }
        if result.len() > 100000 {
            return Err("XML preparation expression limit");
        }
        let node = graph
            .nodes
            .get(&id)
            .ok_or("missing XML preparation expression")?;
        pending.extend(node.dependencies());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Binding;
    #[test]
    fn split_preserves_graph_and_rejects_unknown_or_stateful_overlap() {
        let mut graph = Graph::default();
        graph.nodes.insert(
            0,
            Node::Const {
                value: ir::Value::Int(1),
            },
        );
        graph.nodes.insert(
            1,
            Node::Const {
                value: ir::Value::Int(2),
            },
        );
        let root = Scope {
            bindings: vec![
                Binding {
                    target_field: "Early".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Late".into(),
                    node: 1,
                },
            ],
            ..Default::default()
        };
        let original = serde_json::to_vec(&root).unwrap();
        let (early, late) = split_xml_preparation(&graph, &root, &[vec!["Early".into()]]).unwrap();
        assert_eq!(early.bindings[0].target_field, "Early");
        assert_eq!(late.bindings[0].target_field, "Late");
        assert_eq!(serde_json::to_vec(&root).unwrap(), original);
        assert!(split_xml_preparation(&graph, &root, &[vec!["Unknown".into()]]).is_err());
        assert!(
            split_xml_preparation(&graph, &root, &[vec!["Early".into()], vec!["Early".into()]])
                .is_err()
        );
        graph.nodes.insert(
            2,
            Node::InvocationCounter {
                key: "counter".into(),
                start: 0,
                step: 0,
                restart: None,
            },
        );
        let root = Scope {
            bindings: vec![
                Binding {
                    target_field: "Early".into(),
                    node: 2,
                },
                Binding {
                    target_field: "Late".into(),
                    node: 2,
                },
            ],
            ..Default::default()
        };
        assert!(split_xml_preparation(&graph, &root, &[vec!["Early".into()]]).is_err());
    }

    #[test]
    fn split_preserves_empty_constructed_elements_in_the_late_phase() {
        let graph = Graph::default();
        let root = Scope {
            children: vec![Scope {
                target_field: "Empty".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let (early, late) = split_xml_preparation(&graph, &root, &[]).unwrap();
        assert!(early.children.is_empty());
        assert_eq!(
            serde_json::to_vec(&late).unwrap(),
            serde_json::to_vec(&root).unwrap()
        );
    }

    #[test]
    fn retained_concatenations_cannot_hide_unsupported_controls() {
        let root = Scope {
            iteration: ScopeIteration::Concatenate(crate::ScopeSequence::new(
                Scope {
                    merge_dynamic_fields: true,
                    ..Default::default()
                },
                vec![],
            )),
            ..Default::default()
        };
        assert!(split_xml_preparation(&Graph::default(), &root, &[]).is_err());
    }
}
