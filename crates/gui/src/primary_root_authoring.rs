//! Eligibility for the primary-root authoring controls.

pub(crate) fn available(project: &mapping::Project) -> bool {
    ir::primary_root_schema_is_supported(&project.source)
        && !project.source_options.local_xml_file_set
        && !project.target.repeating
        && matches!(project.target.kind, ir::SchemaKind::Group { .. })
        && flat_binding_scope(&project.root)
        && project
            .extra_sources
            .iter()
            .all(|source| source.dynamic_path.is_none())
}

pub(crate) fn flat_binding_scope(scope: &mapping::Scope) -> bool {
    matches!(scope.iteration, mapping::ScopeIteration::None)
        && matches!(scope.construction, mapping::ScopeConstruction::Constructed)
        && scope.filter.is_none()
        && scope.post_group_filter.is_none()
        && !scope.has_grouping()
        && !scope.has_sort()
        && scope.windows.is_empty()
        && !scope.merge_dynamic_fields
        && scope.children.is_empty()
        && scope.dynamic_children.is_empty()
        && scope.concatenated().is_none()
}

/// Imported graphs can contain cycles or missing nodes; inspecting an edit
/// must preserve them without recursive traversal or automatic repair.
pub(crate) fn has_dependency(graph: &mapping::Graph, start: mapping::NodeId) -> bool {
    let mut pending = vec![start];
    let mut visited = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Some(node) = graph.nodes.get(&id) else {
            continue;
        };
        if matches!(
            node,
            mapping::Node::SourceRootField { .. } | mapping::Node::SourceRootXmlTypeEquals { .. }
        ) {
            return true;
        }
        pending.extend(node.dependencies());
    }
    false
}

/// Classify a whole choice list once. Reverse edges visit shared subgraphs and
/// imported cycles without repeating a dependency walk for each expression.
pub(crate) fn dependent_nodes(
    graph: &mapping::Graph,
) -> std::collections::BTreeSet<mapping::NodeId> {
    let mut pending: Vec<_> = graph
        .nodes
        .iter()
        .filter_map(|(&id, node)| {
            matches!(
                node,
                mapping::Node::SourceRootField { .. }
                    | mapping::Node::SourceRootXmlTypeEquals { .. }
            )
            .then_some(id)
        })
        .collect();
    if pending.is_empty() {
        return std::collections::BTreeSet::new();
    }
    let mut consumers = std::collections::BTreeMap::<_, Vec<_>>::new();
    for (&id, node) in &graph.nodes {
        for dependency in node.dependencies() {
            consumers.entry(dependency).or_default().push(id);
        }
    }
    let mut found = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop() {
        if found.insert(id)
            && let Some(next) = consumers.get(&id)
        {
            pending.extend(next.iter().copied());
        }
    }
    found
}

pub(crate) fn check_binding(
    graph: &mapping::Graph,
    node: mapping::NodeId,
    primary_root_binding: bool,
) -> Result<(), String> {
    if !primary_root_binding && has_dependency(graph, node) {
        return Err(
            "Primary root expressions can only feed static fields on the flat primary target root"
                .to_string(),
        );
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn test_schema() -> ir::SchemaNode {
    use ir::{GroupAlternative, ScalarType, SchemaNode};
    let mut source = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Code", ScalarType::String).attribute(),
            SchemaNode::scalar("Extra", ScalarType::String).attribute(),
        ],
    )
    .with_alternatives(vec![
        GroupAlternative {
            name: "Base".into(),
            members: vec!["Code".into()],
            required: vec![],
            constraints: vec![],
        },
        GroupAlternative {
            name: "{urn:root}Derived".into(),
            members: vec!["Code".into(), "Extra".into()],
            required: vec![],
            constraints: vec![],
        },
    ])
    .unwrap();
    source.xml_type_alternatives = true;
    source.xml_default_type = Some("Base".into());
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_membership_tracks_shared_dags_cycles_and_both_root_primitives() {
        use mapping::Node;
        let mut graph = mapping::Graph::default();
        graph.nodes.extend([
            (
                1,
                Node::SourceRootField {
                    path: vec!["Code".into()],
                    required: false,
                },
            ),
            (
                2,
                Node::SourceRootXmlTypeEquals {
                    canonical_expanded_type: "Base".into(),
                },
            ),
            (
                3,
                Node::Call {
                    function: "concat".into(),
                    args: vec![1, 13],
                },
            ),
            (
                4,
                Node::Call {
                    function: "concat".into(),
                    args: vec![3, 3],
                },
            ),
            (
                5,
                Node::If {
                    condition: 2,
                    then: 3,
                    else_: 13,
                },
            ),
            (
                6,
                Node::Call {
                    function: "concat".into(),
                    args: vec![7, 999],
                },
            ),
            (
                7,
                Node::Call {
                    function: "concat".into(),
                    args: vec![6, 4],
                },
            ),
            (
                8,
                Node::Call {
                    function: "concat".into(),
                    args: vec![9, 999],
                },
            ),
            (
                9,
                Node::Call {
                    function: "concat".into(),
                    args: vec![8],
                },
            ),
            (
                10,
                Node::Call {
                    function: "concat".into(),
                    args: vec![3, 4, 5],
                },
            ),
            (
                11,
                Node::Call {
                    function: "concat".into(),
                    args: vec![10, 4],
                },
            ),
            (
                12,
                Node::RuntimeParameterDefault {
                    name: "value".into(),
                    ty: ir::ScalarType::String,
                    default: 1,
                    preview: None,
                },
            ),
            (
                13,
                Node::Const {
                    value: ir::Value::String("ordinary".into()),
                },
            ),
        ]);
        let before = serde_json::to_value(&graph).unwrap();
        assert_eq!(
            dependent_nodes(&graph),
            std::collections::BTreeSet::from([1, 2, 3, 4, 5, 6, 7, 10, 11, 12]),
        );
        assert_eq!(serde_json::to_value(&graph).unwrap(), before);
        graph.nodes.remove(&1);
        graph.nodes.remove(&2);
        let before = serde_json::to_value(&graph).unwrap();
        assert!(dependent_nodes(&graph).is_empty());
        assert_eq!(serde_json::to_value(&graph).unwrap(), before);
    }

    #[test]
    fn imported_cycles_and_missing_dependencies_are_inspected_without_mutation() {
        let mut graph = mapping::Graph::default();
        graph.nodes.extend([
            (
                1,
                mapping::Node::Call {
                    function: "concat".into(),
                    args: vec![2, 999],
                },
            ),
            (
                2,
                mapping::Node::Call {
                    function: "concat".into(),
                    args: vec![1],
                },
            ),
        ]);
        let before = serde_json::to_value(&graph).unwrap();
        assert!(!has_dependency(&graph, 1));
        assert_eq!(serde_json::to_value(&graph).unwrap(), before);
        graph.nodes.insert(
            3,
            mapping::Node::SourceRootField {
                path: vec!["Code".into()],
                required: false,
            },
        );
        if let Some(mapping::Node::Call { args, .. }) = graph.nodes.get_mut(&2) {
            args.push(3);
        }
        let before = serde_json::to_value(&graph).unwrap();
        assert!(has_dependency(&graph, 1));
        assert_eq!(serde_json::to_value(&graph).unwrap(), before);
    }

    #[test]
    fn root_authoring_availability_matches_static_owner_context() {
        let mut project = crate::new_mapping::blank_project();
        project.source = test_schema();
        assert!(available(&project));
        project.target.repeating = true;
        assert!(!available(&project));
        project.target.repeating = false;
        project.root.children.push(mapping::Scope::default());
        assert!(!available(&project));
        project.root.children.clear();
        project.source_options.local_xml_file_set = true;
        assert!(!available(&project));
    }
}
