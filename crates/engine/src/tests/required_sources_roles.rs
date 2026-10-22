//! Required-source discovery reads a sequence node's inputs and item
//! identities through the shared node-input visitor. These checks compare it
//! with the walk that also pushed them by hand, over the in-tree Project files
//! and over sequence nodes whose every input names a source.

use std::collections::BTreeMap;

use ir::{ScalarType, SchemaNode};
use mapping::{
    AggregateOp, Binding, DynamicSourcePath, FilterMapCapture, FilterMapV1, FunctionId, Graph,
    NamedSource, Node, Project, Scope, SequenceExpr,
};

use super::*;

/// The graph walk as it pushed a sequence node's inputs and item by hand.
fn legacy_inspect_graph(analysis: &mut Analysis<'_>) {
    while let Some(id) = analysis.pending.pop() {
        if !analysis.visited.insert(id) {
            continue;
        }
        let Some(node) = analysis.project.graph.nodes.get(&id) else {
            continue;
        };
        analysis.pending.extend(node.dependencies());
        if let Node::SequenceExists { sequence, .. }
        | Node::SequenceItemAt { sequence, .. }
        | Node::SequenceAggregate { sequence, .. } = node
        {
            analysis.pending.extend(sequence.inputs());
            analysis.pending.push(sequence.item());
        }
        analysis.inspect_node(node);
    }
}

type Discovery = (BTreeSet<usize>, BTreeSet<usize>, BTreeSet<NodeId>);

fn discover(project: &Project, root: &Scope, walk: impl FnOnce(&mut Analysis<'_>)) -> Discovery {
    let mut analysis = Analysis::new(project);
    analysis.inspect_scope(root);
    analysis.inspect_failure_rules();
    walk(&mut analysis);
    (
        analysis.required,
        analysis.inspected_dynamic,
        analysis.visited,
    )
}

fn check(project: &Project) -> usize {
    let mut roots = vec![&project.root];
    roots.extend(project.extra_targets.iter().map(|target| &target.root));
    for root in &roots {
        assert_eq!(
            discover(project, root, |analysis| analysis.inspect_graph()),
            discover(project, root, legacy_inspect_graph),
        );
    }
    roots.len()
}

#[test]
fn every_in_tree_project_requires_the_same_sources() {
    let projects = crate::scope_slots::project_corpus::projects();
    let checked: usize = projects.iter().map(check).sum();
    assert!(checked > 50, "checked {checked} target selections");
}

/// Each sequence consumer's inputs, captures and item identities read a
/// distinct static or dynamic source, so a missed input misses a source.
#[test]
fn every_sequence_input_still_names_its_source() {
    let read = |source: &str| Node::SourceField {
        path: vec![source.into(), "Value".into()],
        frame: None,
    };
    let source = |name: &str, dynamic_path| NamedSource {
        name: name.into(),
        path: format!("{name}.json"),
        schema: SchemaNode::group(name, vec![SchemaNode::scalar("Value", ScalarType::String)]),
        options: Default::default(),
        dynamic_path,
    };
    let names = [
        "Input",
        "Delimiter",
        "SourceItem",
        "Item",
        "Capture",
        "Predicate",
    ];
    let sequence = SequenceExpr::FilterMapV1(FilterMapV1 {
        source: Box::new(SequenceExpr::Tokenize {
            input: 10,
            delimiter: 11,
            item: 12,
        }),
        item: 13,
        predicate: FunctionId::new(1),
        mapper: FunctionId::new(2),
        output_type: ScalarType::String,
        captures: vec![FilterMapCapture {
            node: 14,
            ty: ScalarType::Int,
        }],
    });
    let consumers = [
        Node::SequenceExists {
            sequence: sequence.clone(),
            predicate: 15,
        },
        Node::SequenceItemAt {
            sequence: sequence.clone(),
            index: 15,
        },
        Node::SequenceAggregate {
            function: AggregateOp::Join,
            sequence,
            predicate: Some(15),
            expression: None,
            arg: None,
        },
    ];
    for consumer in consumers {
        let mut nodes: BTreeMap<NodeId, Node> = names
            .iter()
            .zip(10..)
            .map(|(name, id)| (id, read(name)))
            .collect();
        nodes.insert(1, consumer);
        let mut sources: Vec<_> = names.iter().map(|name| source(name, None)).collect();
        sources.push(source(
            "Loaded",
            Some(DynamicSourcePath::legacy(12, Vec::new())),
        ));
        nodes.insert(12, read("Loaded"));
        let project = Project {
            source: SchemaNode::group("Primary", Vec::new()),
            target: SchemaNode::group(
                "Output",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            ),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: sources,
            extra_targets: Vec::new(),
            failure_rules: Vec::new(),
            user_functions: Default::default(),
            graph: Graph { nodes },
            root: Scope {
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 1,
                }],
                ..Scope::default()
            },
        };
        check(&project);
        let (required, dynamic, _) =
            discover(&project, &project.root, |analysis| analysis.inspect_graph());
        assert_eq!(required, [0, 1, 3, 4, 5].into());
        assert_eq!(dynamic, [6].into());
    }
}
