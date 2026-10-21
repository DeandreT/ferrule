//! #467 owned dynamic source-field reads: IR, profile admission and refusals.
use crate::*;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Node, NodeId, OwnedDynamicSourceField, OwnedSourceRef, SourceOwner};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).into()).collect()
}
fn named() -> SourceOwner {
    SourceOwner::Named { name: "Ref".into() }
}
fn read(owner: SourceOwner, object: &[&str], frame: Option<&[&str]>) -> OwnedDynamicSourceField {
    OwnedDynamicSourceField {
        object: OwnedSourceRef {
            owner,
            path: path(object),
        },
        frame: frame.map(path),
        key: 1,
    }
}
fn open(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![])
        .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::String))
        .unwrap()
}
fn endpoint(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Key", ScalarType::String),
            open("Props"),
            SchemaNode::group("Rows", vec![open("Props")]).repeating(),
        ],
    )
}
/// `Value` is the named endpoint's owned dynamic read (node 10, key 1).
fn project_with(field: OwnedDynamicSourceField) -> mapping::Project {
    let mut graph = mapping::Graph::default();
    graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("color".into()),
        },
    );
    graph.nodes.insert(10, Node::OwnedDynamicSourceField(field));
    mapping::Project {
        source: endpoint("Input"),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        user_functions: Default::default(),
        extra_sources: vec![mapping::NamedSource {
            name: "Ref".into(),
            path: "ref.json".into(),
            schema: endpoint("Ref"),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: mapping::Scope {
            bindings: vec![mapping::Binding {
                target_field: "Value".into(),
                node: 10,
            }],
            ..mapping::Scope::default()
        },
    }
}
fn project() -> mapping::Project {
    project_with(read(named(), &["Props"], None))
}
fn request(field: &OwnedDynamicSourceField) -> OwnedSourceDiagnosticRequest {
    OwnedSourceDiagnosticRequest::DynamicField {
        owner: (&field.object.owner).into(),
        object: field.object.path.clone(),
        frame: field.frame.clone(),
    }
}
fn closed(node: NodeId, scope: bool) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
            location: OwnedSourceDiagnosticLocation::Expression {
                namespace: OwnedSourceNamespace::Main,
                node,
                scope: scope.then(|| {
                    mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
                }),
            },
            request: Some(request(&read(named(), &["Props"], None))),
            reason: OwnedSourceCapabilityReason::UnsupportedBasicContext,
            origin: OwnedSourceCapabilityOrigin::Expression {
                namespace: OwnedSourceNamespace::Main,
                node,
            },
        }),
    }
}

#[test]
fn owned_read_lowers_with_its_exact_owner_and_lazy_named_input() {
    let program = lower_with_basic_owned_dynamic_field(&project()).unwrap();
    let Some(Expression::OwnedDynamicSourceField(field)) = program
        .expressions
        .iter()
        .find(|node| node.id == 10)
        .map(|node| &node.expression)
    else {
        panic!("lowered owned dynamic read");
    };
    assert_eq!(field, &read(named(), &["Props"], None));
    validate_program_with_basic_owned_dynamic_field(&program).unwrap();
    validate_basic_owned_dynamic_field_emission(&program).unwrap();
    // The selected owner never becomes an eager input.
    let plan = legacy_named_input_admission_plan_with_basic_owned_dynamic_field(&program).unwrap();
    assert_eq!(
        plan.primary.entries[0].availability,
        NamedInputAvailability::Optional
    );
}

#[test]
fn every_earlier_profile_and_direct_validation_refuses_the_owned_read() {
    let program = lower_with_basic_owned_dynamic_field(&project()).unwrap();
    for result in [
        validate_program(&program),
        validate_owned_emission(&program),
        validate_basic_owned_emission(&program),
        validate_program_with_basic_owned_sequence_item_at(&program),
        validate_program_with_basic_owned_group_ending_with(&program),
        validate_program_with_basic_owned_failure_sources(&program),
        validate_basic_owned_failure_source_emission(&program),
        validate_program_with_basic_owned_path_hierarchy(&program),
        validate_program_with_basic_owned_adjacency_tree(&program),
        validate_basic_owned_adjacency_tree_emission(&program),
        validate_program_with_basic_owned_group_into_blocks(&program),
        validate_basic_owned_group_into_blocks_emission(&program),
        validate_program_with_basic_owned_recursive_collect(&program),
        validate_basic_owned_recursive_collect_emission(&program),
        legacy_named_input_admission_plan_with_basic_owned_recursive_collect(&program).map(drop),
        validate_program_with_basic_owned_sequence_exists(&program),
        validate_basic_owned_sequence_exists_emission(&program),
        validate_program_with_basic_owned_recursive_filter(&program),
        validate_basic_owned_recursive_filter_emission(&program),
        validate_program_with_basic_owned_lookup_find(&program),
        validate_basic_owned_lookup_find_emission(&program),
        // The sibling aggregate companion also composes owned search only.
        validate_program_with_basic_owned_collection_aggregates(&program),
        validate_basic_owned_collection_aggregates_emission(&program),
    ] {
        assert!(
            result == Err(closed(10, true)) || result == Err(closed(10, false)),
            "{result:?}"
        );
    }
    for lowered in [
        lower(&project()),
        lower_with_basic_owned_failure_sources(&project()),
        lower_with_basic_owned_adjacency_tree(&project()),
        lower_with_basic_owned_group_into_blocks(&project()),
        lower_with_basic_owned_recursive_collect(&project()),
        lower_with_basic_owned_sequence_exists(&project()),
        lower_with_basic_owned_recursive_filter(&project()),
        lower_with_basic_owned_lookup_find(&project()),
        lower_with_basic_owned_collection_aggregates(&project()),
    ] {
        assert!(lowered.is_err());
    }
}

#[test]
fn malformed_directly_constructed_reads_are_structural_refusals() {
    let program = lower_with_basic_owned_dynamic_field(&project()).unwrap();
    for (field, reason) in [
        (
            read(
                SourceOwner::Named {
                    name: String::new(),
                },
                &["Props"],
                None,
            ),
            OwnedSourceStructureReason::InvalidNamedOwner,
        ),
        (
            read(
                SourceOwner::Named {
                    name: "Missing".into(),
                },
                &["Props"],
                None,
            ),
            OwnedSourceStructureReason::UnknownOwner,
        ),
        (
            read(named(), &["Absent"], None),
            OwnedSourceStructureReason::MissingOwnedPath { segment: 0 },
        ),
        (
            read(named(), &["Key"], None),
            OwnedSourceStructureReason::InvalidOwnedDynamicObject {
                found: OwnedSourceSchemaShape::Scalar { repeating: false },
            },
        ),
        (
            read(named(), &["Rows"], None),
            OwnedSourceStructureReason::InvalidOwnedDynamicObject {
                found: OwnedSourceSchemaShape::Group { repeating: true },
            },
        ),
        (
            read(named(), &["Props"], Some(&["Rows"])),
            OwnedSourceStructureReason::IncompatibleFramePath,
        ),
        (
            read(named(), &["Rows", "Props"], Some(&["Rows", "Props"])),
            OwnedSourceStructureReason::InvalidOwnedCollection {
                found: OwnedSourceSchemaShape::Group { repeating: false },
            },
        ),
    ] {
        let mut malformed = program.clone();
        let node = malformed
            .expressions
            .iter_mut()
            .find(|node| node.id == 10)
            .unwrap();
        node.expression = Expression::OwnedDynamicSourceField(field.clone());
        let expected = ProgramValidationError::OwnedSource {
            diagnostic: Box::new(OwnedSourceDiagnostic::Structure {
                location: OwnedSourceDiagnosticLocation::Expression {
                    namespace: OwnedSourceNamespace::Main,
                    node: 10,
                    scope: None,
                },
                request: request(&field),
                reason,
            }),
        };
        assert_eq!(
            validate_program_with_basic_owned_dynamic_field(&malformed),
            Err(expected.clone())
        );
        assert_eq!(validate_program(&malformed), Err(expected));
    }
}

#[test]
fn closed_contexts_refuse_the_owned_read_in_the_new_profile() {
    // A dynamic named owner stays closed.
    let mut dynamic = project();
    dynamic.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
        node: 1,
        iteration: Vec::new(),
    });
    let error = lower_with_basic_owned_dynamic_field(&dynamic).unwrap_err();
    assert!(
        matches!(error.diagnostics(), [Diagnostic::OwnedSource { diagnostic }]
        if matches!(**diagnostic, OwnedSourceDiagnostic::Capability {
            reason: OwnedSourceCapabilityReason::DynamicOwner, ..
        })),
        "{error:?}"
    );
    // Failure-rule expressions keep their closed scalar subset.
    let mut failure = project();
    failure.graph.nodes.insert(
        20,
        Node::Call {
            function: "exists".into(),
            args: vec![10],
        },
    );
    failure.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: path(&["Rows"]),
        },
        selection: mapping::FailureSelection::WhenTrue { predicate: 20 },
        message: None,
    });
    let error = lower_with_basic_owned_dynamic_field(&failure).unwrap_err();
    assert!(
        matches!(error.diagnostics(), [Diagnostic::OwnedSource { diagnostic }]
        if matches!(**diagnostic, OwnedSourceDiagnostic::Capability {
            reason: OwnedSourceCapabilityReason::FailureRule, ..
        })),
        "{error:?}"
    );
    // A legacy dynamic read stays closed in the paired profile.
    let mut legacy = project();
    legacy.graph.nodes.insert(
        10,
        Node::DynamicSourceField {
            object: path(&["Props"]),
            frame: None,
            key: 1,
        },
    );
    assert!(lower_with_basic_owned_dynamic_field(&legacy).is_err());
    assert!(lower(&legacy).is_ok());
}

#[test]
fn exists_predicates_stay_closed_in_the_dynamic_field_profile() {
    let mut exists = project();
    exists.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Value", ScalarType::Bool)],
    );
    for (id, node) in [
        (
            20,
            Node::SequenceExists {
                sequence: mapping::SequenceExpr::Generate {
                    from: None,
                    to: 21,
                    item: 22,
                },
                predicate: 23,
            },
        ),
        (
            21,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (
            22,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
        (
            23,
            Node::Call {
                function: "exists".into(),
                args: vec![10],
            },
        ),
    ] {
        exists.graph.nodes.insert(id, node);
    }
    exists.root.bindings[0].node = 20;
    let error = lower_with_basic_owned_dynamic_field(&exists).unwrap_err();
    assert!(
        matches!(error.diagnostics(), [Diagnostic::OwnedSource { diagnostic }]
        if **diagnostic == OwnedSourceDiagnostic::Capability {
            location: OwnedSourceDiagnosticLocation::Expression {
                namespace: OwnedSourceNamespace::Main,
                node: 10,
                scope: Some(mapping::SourceScopeLocation::root(
                    mapping::SourceScopeTarget::Primary,
                )),
            },
            request: Some(request(&read(named(), &["Props"], None))),
            reason: OwnedSourceCapabilityReason::GeneratedItem,
            origin: OwnedSourceCapabilityOrigin::Expression {
                namespace: OwnedSourceNamespace::Main,
                node: 20,
            },
        }),
        "{error:?}"
    );
}
