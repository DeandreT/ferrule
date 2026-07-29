use std::collections::BTreeMap;

use ir::{ScalarType, SchemaNode};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration};

use crate::{Expression, IterationSource, lower};

fn project() -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: mapping::FormatOptions {
            xml_document: true,
            local_xml_file_set: true,
            ..mapping::FormatOptions::default()
        },
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (0, Node::SourceDocumentPath),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: Vec::new(),
                output_path: 0,
            },
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn lowers_dynamic_document_iteration_and_its_item_scoped_path() {
    let program = lower(&project()).expect("dynamic document output is portable");
    let iteration = program
        .root
        .iteration
        .as_ref()
        .expect("root iteration is retained");
    let IterationSource::DynamicDocuments(dynamic) = iteration.input() else {
        panic!("expected dynamic document iteration")
    };

    assert!(dynamic.source().path().is_empty());
    assert_eq!(dynamic.output_path(), 0);
    assert_eq!(
        iteration.roots().collect::<Vec<_>>(),
        vec![0],
        "the output path participates in graph reachability"
    );
    assert!(
        program.expressions.iter().any(|node| {
            node.id == 0 && matches!(node.expression, Expression::SourceDocumentPath)
        })
    );
}
