use super::*;
use ir::{ScalarType, SchemaNode};
use mapping::{Graph, Node, Scope, UserFunction};

#[test]
fn graph_and_function_nodes_with_the_same_id_keep_distinct_owners() {
    let mut project = tests::valid_project();
    let invalid = Node::Call {
        function: "unknown-operation".into(),
        args: vec![],
    };
    project.graph.nodes.insert(17, invalid.clone());
    let function = mapping::FunctionId::new(4);
    project.user_functions.insert(
        function,
        UserFunction {
            library: "test".into(),
            name: "graph node 17".into(),
            description: None,
            parameters: vec![],
            output_name: "result".into(),
            output_type: ScalarType::String,
            body: Graph {
                nodes: [(17, invalid)].into(),
            },
            output: 17,
        },
    );
    let issues = validate(&project);
    let graph_issue = issues
        .iter()
        .find(|issue| issue.location == "graph node 17")
        .unwrap();
    assert_eq!(
        graph_issue.to_string(),
        "graph node 17: unknown function `unknown-operation`"
    );
    assert_eq!(
        graph_issue.owner,
        Some(ValidationOwner::GraphNode {
            function: None,
            node: 17
        })
    );
    assert!(issues.iter().any(|issue| issue.owner
        == Some(ValidationOwner::GraphNode {
            function: Some(function),
            node: 17,
        })));
}

#[test]
fn schema_owners_preserve_duplicate_child_and_endpoint_indexes() {
    let mut project = tests::valid_project();
    let mut invalid = SchemaNode::group("literal/*/field", vec![]);
    invalid.fixed = Some("invalid group default".into());
    for _ in 0..2 {
        project.extra_sources.push(mapping::NamedSource {
            name: "duplicate` schema".into(),
            path: "input.json".into(),
            schema: SchemaNode::group("root", vec![invalid.clone(), invalid.clone()]),
            options: Default::default(),
            dynamic_path: None,
        });
    }
    let issues = validate(&project);
    for index in 0..2 {
        for child in 0..2 {
            assert!(issues.iter().any(|issue| issue.owner
                == Some(ValidationOwner::SchemaNode(ValidationSchemaLocation {
                    endpoint: ValidationEndpoint::NamedSource {
                        index,
                        name: "duplicate` schema".into()
                    },
                    path: vec![ValidationSchemaStep::Child(child)],
                }))));
        }
    }
    assert!(
        issues
            .iter()
            .any(|issue| issue.message == "extra source name is duplicated"
                && issue.owner
                    == Some(ValidationOwner::Endpoint(ValidationEndpoint::NamedSource {
                        index: 1,
                        name: "duplicate` schema".into(),
                    })))
    );
}

#[test]
fn named_scope_owners_distinguish_children_segments_and_dynamic_children() {
    let mut project = tests::valid_project();
    let invalid = Scope {
        bindings: vec![mapping::Binding {
            target_field: "name".into(),
            node: 999,
        }],
        ..Scope::default()
    };
    project.extra_targets.push(mapping::NamedTarget {
        name: "audit".into(),
        path: None,
        options: Default::default(),
        schema: SchemaNode::group("root", vec![SchemaNode::group("same", vec![])]),
        root: Scope {
            children: vec![
                Scope {
                    target_field: "same".into(),
                    ..invalid.clone()
                },
                Scope {
                    target_field: "same".into(),
                    iteration: mapping::ScopeIteration::Concatenate(mapping::ScopeSequence::new(
                        invalid.clone(),
                        vec![],
                    )),
                    ..Scope::default()
                },
            ],
            dynamic_children: vec![mapping::DynamicChild {
                key: 0,
                scope: invalid,
            }],
            ..Scope::default()
        },
    });
    let issues = validate(&project);
    for path in [
        vec![ValidationScopeStep::Child(0)],
        vec![
            ValidationScopeStep::Child(1),
            ValidationScopeStep::Segment(0),
        ],
        vec![ValidationScopeStep::DynamicChild(0)],
    ] {
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("missing node 999")
                    && issue.owner
                        == Some(ValidationOwner::Scope(ValidationScopeLocation {
                            target: ValidationEndpoint::NamedTarget {
                                index: 0,
                                name: "audit".into()
                            },
                            path: path.clone(),
                        })))
        );
    }
}

#[test]
fn endpoint_options_and_failure_rules_have_owners_without_reading_display_text() {
    let mut project = tests::valid_project();
    project.source_options.xlsx_update_existing = true;
    project.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source { collection: vec![] },
        selection: mapping::FailureSelection::All,
        message: Some(999),
    });
    let issues = validate(&project);
    assert!(issues.iter().any(|issue| issue.owner == Some(ValidationOwner::Endpoint(ValidationEndpoint::Source))));
    assert!(issues.iter().any(
        |issue| issue.message == "message references missing node 999"
            && issue.owner == Some(ValidationOwner::FailureRule { index: 0 })
    ));
}
