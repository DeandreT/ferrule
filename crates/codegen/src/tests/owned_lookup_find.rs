//! #446 owned Lookup and CollectionFind: IR, profile admission and refusals.
use crate::*;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Node, OwnedCollectionFind, OwnedLookup, OwnedSourceField, OwnedSourceRef, SourceOwner,
};
use std::collections::BTreeMap;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).into()).collect()
}
fn named() -> SourceOwner {
    SourceOwner::Named { name: "Ref".into() }
}
fn rows(owner: SourceOwner) -> OwnedSourceRef {
    OwnedSourceRef {
        owner,
        path: path(&["Rows"]),
    }
}
fn owned(owner: SourceOwner, parts: &[&str]) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: None,
    })
}
fn lookup(owner: SourceOwner) -> OwnedLookup {
    OwnedLookup {
        collection: rows(owner),
        key: path(&["Code"]),
        matches: 1,
        value: path(&["Name"]),
    }
}
fn find(owner: SourceOwner) -> OwnedCollectionFind {
    OwnedCollectionFind {
        collection: rows(owner),
        predicate: 4,
        value: 5,
    }
}
/// `Value` is a Primary lookup and `Other` a named find, with the same paths.
fn project() -> mapping::Project {
    let source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Note", ScalarType::String),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Code", ScalarType::String),
                    SchemaNode::scalar("Name", ScalarType::String),
                    SchemaNode::group("Lines", vec![SchemaNode::scalar("Sku", ScalarType::String)])
                        .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::scalar("Other", ScalarType::String),
        ],
    );
    let bind = |target_field: &str, node| mapping::Binding {
        target_field: target_field.into(),
        node,
    };
    mapping::Project {
        source: source.clone(),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        user_functions: Default::default(),
        extra_sources: vec![mapping::NamedSource {
            name: "Ref".into(),
            path: "ref.json".into(),
            schema: source,
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        root: mapping::Scope {
            bindings: vec![bind("Value", 2), bind("Other", 3)],
            ..Default::default()
        },
        graph: mapping::Graph {
            nodes: BTreeMap::from([
                (1, owned(SourceOwner::Primary, &["Note"])),
                (2, Node::OwnedLookup(lookup(SourceOwner::Primary))),
                (3, Node::OwnedCollectionFind(find(named()))),
                (
                    4,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![6, 1],
                    },
                ),
                (5, owned(named(), &["Rows", "Name"])),
                (6, owned(named(), &["Rows", "Code"])),
            ]),
        },
        failure_rules: Vec::new(),
    }
}
fn capability(
    node: u32,
    owner: SourceOwner,
    reason: OwnedSourceCapabilityReason,
    origin: OwnedSourceCapabilityOrigin,
) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
            location: OwnedSourceDiagnosticLocation::Expression {
                namespace: OwnedSourceNamespace::Main,
                node,
                scope: None,
            },
            request: Some(OwnedSourceDiagnosticRequest::Iteration {
                owner: (&owner).into(),
                path: path(&["Rows"]),
            }),
            reason,
            origin,
        }),
    }
}
fn retained(node: u32, owner: SourceOwner) -> ProgramValidationError {
    capability(
        node,
        owner,
        OwnedSourceCapabilityReason::AggregateLookup,
        OwnedSourceCapabilityOrigin::RetainedExpression {
            namespace: OwnedSourceNamespace::Main,
            node,
        },
    )
}
fn reason(error: &LowerError) -> Option<OwnedSourceCapabilityReason> {
    match error.diagnostics() {
        [Diagnostic::OwnedSource { diagnostic }] => match diagnostic.as_ref() {
            OwnedSourceDiagnostic::Capability { reason, .. } => Some(*reason),
            OwnedSourceDiagnostic::Structure { .. } => None,
        },
        _ => None,
    }
}

#[test]
fn owned_search_lowers_exactly_and_stays_demand_driven() {
    let program = lower_with_basic_owned_lookup_find(&project()).unwrap();
    let expression = |id| {
        program
            .expressions
            .iter()
            .find(|node| node.id == id)
            .map(|node| node.expression.clone())
    };
    assert_eq!(
        expression(2),
        Some(Expression::OwnedLookup(lookup(SourceOwner::Primary)))
    );
    assert_eq!(
        expression(3),
        Some(Expression::OwnedCollectionFind(find(named())))
    );
    assert_eq!(
        validate_program_with_basic_owned_lookup_find(&program),
        Ok(())
    );
    assert_eq!(validate_basic_owned_lookup_find_emission(&program), Ok(()));
    let plan = legacy_named_input_admission_plan_with_basic_owned_lookup_find(&program).unwrap();
    assert_eq!(
        plan.all_outputs.entries[0].availability,
        NamedInputAvailability::Optional
    );
    // The ordinary emission gate never opens owned search.
    assert!(validate_owned_emission(&program).is_err());
}

#[test]
fn every_earlier_profile_refuses_owned_search() {
    let p = project();
    let program = lower_with_basic_owned_lookup_find(&p).unwrap();
    let expected = retained(2, SourceOwner::Primary);
    for result in [
        validate_basic_owned_emission(&program),
        validate_program_with_basic_owned_sort(&program),
        validate_program_with_basic_owned_group_by(&program),
        validate_program_with_basic_owned_group_ending_with(&program),
        validate_program_with_basic_owned_failure_sources(&program),
        validate_basic_owned_failure_source_emission(&program),
        validate_program_with_basic_owned_path_hierarchy(&program),
        validate_basic_owned_path_hierarchy_emission(&program),
        validate_program_with_basic_owned_adjacency_tree(&program),
        validate_basic_owned_adjacency_tree_emission(&program),
        validate_program_with_basic_owned_group_into_blocks(&program),
        validate_basic_owned_group_into_blocks_emission(&program),
        validate_program_with_basic_owned_recursive_collect(&program),
        validate_basic_owned_recursive_collect_emission(&program),
        validate_program_with_basic_owned_sequence_exists(&program),
        validate_basic_owned_sequence_exists_emission(&program),
        validate_program_with_basic_owned_recursive_filter(&program),
        validate_basic_owned_recursive_filter_emission(&program),
    ] {
        assert_eq!(result, Err(expected.clone()));
    }
    for result in [
        lower_with_basic_owned_windows(&p),
        lower_with_basic_owned_group_ending_with(&p),
        lower_with_basic_owned_failure_sources(&p),
        lower_with_basic_owned_path_hierarchy(&p),
        lower_with_basic_owned_adjacency_tree(&p),
        lower_with_basic_owned_group_into_blocks(&p),
        lower_with_basic_owned_recursive_collect(&p),
        lower_with_basic_owned_sequence_exists(&p),
        lower_with_basic_owned_recursive_filter(&p),
    ] {
        let Err(error) = result else {
            panic!("older profile admitted owned search");
        };
        assert_eq!(
            reason(&error),
            Some(OwnedSourceCapabilityReason::AggregateLookup)
        );
    }
}

#[test]
fn owned_search_stays_closed_in_specialized_and_isolated_contexts() {
    // A legacy CollectionFind item expression is an AggregateLookup context.
    let mut p = project();
    p.graph.nodes.insert(
        7,
        Node::CollectionFind {
            collection: path(&["Rows"]),
            predicate: 4,
            value: 2,
        },
    );
    p.root.bindings[0].node = 7;
    let error = lower_with_basic_owned_lookup_find(&p).unwrap_err();
    assert_eq!(
        reason(&error),
        Some(OwnedSourceCapabilityReason::AggregateLookup)
    );
    // A generated-item predicate keeps its closed context.
    let mut p = project();
    p.graph.nodes.insert(
        7,
        Node::SequenceExists {
            sequence: mapping::SequenceExpr::Generate {
                from: None,
                to: 8,
                item: 9,
            },
            predicate: 10,
        },
    );
    p.graph.nodes.insert(
        8,
        Node::Const {
            value: Value::Int(2),
        },
    );
    p.graph.nodes.insert(
        9,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    p.graph.nodes.insert(
        10,
        Node::Call {
            function: "exists".into(),
            args: vec![2],
        },
    );
    p.root.bindings[0].node = 7;
    let error = lower_with_basic_owned_lookup_find(&p).unwrap_err();
    assert_eq!(
        reason(&error),
        Some(OwnedSourceCapabilityReason::GeneratedItem)
    );
    // Owned failure predicates keep their scalar subset.
    let mut p = project();
    p.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::OwnedSource {
            source: rows(named()),
        },
        selection: mapping::FailureSelection::WhenTrue { predicate: 4 },
        message: Some(2),
    });
    let error = lower_with_basic_owned_lookup_find(&p).unwrap_err();
    assert_eq!(
        reason(&error),
        Some(OwnedSourceCapabilityReason::FailureRule)
    );
}

#[test]
fn direct_programs_check_owned_search_structure() {
    let program = lower_with_basic_owned_lookup_find(&project()).unwrap();
    let mut direct = program.clone();
    for node in &mut direct.expressions {
        if node.id == 2 {
            node.expression = Expression::OwnedLookup(OwnedLookup {
                key: path(&["Lines", "Sku"]),
                ..lookup(SourceOwner::Primary)
            });
        }
    }
    let expected = Err(ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Structure {
            location: OwnedSourceDiagnosticLocation::Expression {
                namespace: OwnedSourceNamespace::Main,
                node: 2,
                scope: None,
            },
            request: OwnedSourceDiagnosticRequest::Field {
                owner: OwnedSourceDiagnosticOwner::Primary,
                path: path(&["Rows", "Lines", "Sku"]),
                frame: Some(path(&["Rows"])),
            },
            reason: OwnedSourceStructureReason::InvalidOwnedField {
                found: OwnedSourceSchemaShape::Group { repeating: true },
            },
        }),
    });
    assert_eq!(
        validate_program_with_basic_owned_lookup_find(&direct),
        expected
    );
    assert_eq!(validate_owned_emission(&direct), expected);
    let mut direct = program;
    for node in &mut direct.expressions {
        if node.id == 3 {
            node.expression = Expression::OwnedCollectionFind(OwnedCollectionFind {
                collection: OwnedSourceRef {
                    owner: named(),
                    path: path(&["Note"]),
                },
                ..find(named())
            });
        }
    }
    assert_eq!(
        validate_program_with_basic_owned_lookup_find(&direct),
        Err(ProgramValidationError::OwnedSource {
            diagnostic: Box::new(OwnedSourceDiagnostic::Structure {
                location: OwnedSourceDiagnosticLocation::Expression {
                    namespace: OwnedSourceNamespace::Main,
                    node: 3,
                    scope: None,
                },
                request: OwnedSourceDiagnosticRequest::Iteration {
                    owner: OwnedSourceDiagnosticOwner::Named { name: "Ref".into() },
                    path: path(&["Note"]),
                },
                reason: OwnedSourceStructureReason::InvalidOwnedCollection {
                    found: OwnedSourceSchemaShape::Scalar { repeating: false },
                },
            }),
        })
    );
}
