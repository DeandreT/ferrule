use crate::*;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Node, OwnedSourceField, OwnedSourceRef, SourceOwner};
use std::collections::BTreeMap;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn owned(owner: SourceOwner, parts: &[&str]) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: Some(vec![]),
    })
}
fn named() -> SourceOwner {
    SourceOwner::Named { name: "Cfg".into() }
}
fn schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Code", ScalarType::String),
            SchemaNode::scalar("Count", ScalarType::Int),
            SchemaNode::group("Rows", vec![SchemaNode::scalar("Code", ScalarType::String)])
                .repeating(),
        ],
    )
}
fn item() -> Node {
    Node::SourceField {
        path: vec![],
        frame: None,
    }
}
/// `Found = exists(sequence, equal(item, Cfg.Code))`.
fn project(sequence: mapping::SequenceExpr) -> mapping::Project {
    mapping::Project {
        source: schema(),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Found", ScalarType::Bool)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![mapping::NamedSource {
            name: "Cfg".into(),
            path: "cfg.json".into(),
            schema: schema(),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        graph: mapping::Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::Const {
                        value: Value::String("a|b".into()),
                    },
                ),
                (
                    2,
                    Node::Const {
                        value: Value::String("|".into()),
                    },
                ),
                (3, owned(named(), &["Count"])),
                (
                    4,
                    Node::Const {
                        value: Value::String("[|]".into()),
                    },
                ),
                (30, owned(named(), &["Code"])),
                (
                    31,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![90, 30],
                    },
                ),
                (
                    40,
                    Node::SequenceExists {
                        sequence,
                        predicate: 31,
                    },
                ),
                (90, item()),
            ]),
        },
        root: mapping::Scope {
            bindings: vec![mapping::Binding {
                target_field: "Found".into(),
                node: 40,
            }],
            ..Default::default()
        },
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}
fn producers() -> Vec<mapping::SequenceExpr> {
    vec![
        mapping::SequenceExpr::Tokenize {
            input: 1,
            delimiter: 2,
            item: 90,
        },
        mapping::SequenceExpr::TokenizeByLength {
            input: 1,
            length: 3,
            item: 90,
        },
        mapping::SequenceExpr::TokenizeRegex {
            input: 1,
            pattern: 4,
            flags: None,
            item: 90,
        },
        mapping::SequenceExpr::Generate {
            from: None,
            to: 3,
            item: 90,
        },
        mapping::SequenceExpr::Generate {
            from: Some(3),
            to: 3,
            item: 90,
        },
    ]
}
fn generated_item_refusal(error: &ProgramValidationError) -> bool {
    matches!(error, ProgramValidationError::OwnedSource { diagnostic }
    if matches!(diagnostic.as_ref(), OwnedSourceDiagnostic::Capability {
        location: OwnedSourceDiagnosticLocation::Expression { node: 3 | 30, .. },
        reason: OwnedSourceCapabilityReason::GeneratedItem,
        origin: OwnedSourceCapabilityOrigin::Expression { node: 40, .. },
        ..
    }))
}

#[test]
fn explicit_profile_admits_owned_predicate_reads_over_every_pure_producer() {
    for sequence in producers() {
        let project = project(sequence);
        let program = lower_with_basic_owned_sequence_exists(&project).unwrap();
        validate_program_with_basic_owned_sequence_exists(&program).unwrap();
        validate_basic_owned_sequence_exists_emission(&program).unwrap();
        let plan =
            legacy_named_input_admission_plan_with_basic_owned_sequence_exists(&program).unwrap();
        // The owned predicate read and generated item create no eager input.
        assert_eq!(
            plan.all_outputs.entries[0].availability,
            NamedInputAvailability::Optional
        );
        // Earlier profiles, including ItemAt and Ending, keep refusing.
        for result in [
            validate_program_with_basic_owned_sequence_item_at(&program),
            validate_program_with_basic_owned_group_starting_with(&program),
            validate_program_with_basic_owned_group_ending_with(&program),
            validate_program_with_basic_owned_group_by(&program),
            validate_program(&program),
        ] {
            assert!(
                result.as_ref().is_err_and(generated_item_refusal),
                "{result:?}"
            );
        }
    }
}

#[test]
fn legacy_alias_reads_inside_the_predicate_remain_eager() {
    let mut project = project(producers().remove(0));
    project.graph.nodes.insert(
        30,
        Node::SourceField {
            path: path(&["Cfg", "Code"]),
            frame: None,
        },
    );
    project.graph.nodes.insert(32, owned(named(), &["Code"]));
    project.graph.nodes.insert(
        33,
        Node::Call {
            function: "and".into(),
            args: vec![31, 34],
        },
    );
    project.graph.nodes.insert(
        34,
        Node::Call {
            function: "equal".into(),
            args: vec![90, 32],
        },
    );
    project.graph.nodes.insert(
        40,
        Node::SequenceExists {
            sequence: producers().remove(0),
            predicate: 33,
        },
    );
    let program = lower_with_basic_owned_sequence_exists(&project).unwrap();
    let plan =
        legacy_named_input_admission_plan_with_basic_owned_sequence_exists(&program).unwrap();
    assert_eq!(
        plan.all_outputs.entries[0].availability,
        NamedInputAvailability::RequiredLegacy
    );
    assert_eq!(plan.primary, plan.all_outputs);
}

#[test]
fn recursive_collect_and_failure_predicates_stay_closed() {
    let mut project = project(mapping::SequenceExpr::RecursiveCollect {
        collection: path(&["Rows"]),
        children: vec![],
        descent_value: path(&["Code"]),
        values: vec![],
        value: path(&["Code"]),
        prefix: 1,
        separator: 2,
        item: 90,
    });
    let error = lower_with_basic_owned_sequence_exists(&project).unwrap_err();
    assert!(
        matches!(error.diagnostics(), [Diagnostic::OwnedSource { diagnostic }]
        if generated_item_refusal(&ProgramValidationError::OwnedSource {
            diagnostic: diagnostic.clone(),
        })),
        "{error:?}"
    );

    project = self::project(producers().remove(0));
    project.root.bindings.clear();
    project.failure_rules = vec![mapping::FailureRule {
        iteration: mapping::FailureIteration::Source { collection: vec![] },
        selection: mapping::FailureSelection::WhenTrue { predicate: 40 },
        message: None,
    }];
    let error = lower_with_basic_owned_sequence_exists(&project).unwrap_err();
    assert!(
        matches!(error.diagnostics(), [Diagnostic::OwnedSource { diagnostic }]
        if matches!(diagnostic.as_ref(), OwnedSourceDiagnostic::Capability {
            reason: OwnedSourceCapabilityReason::FailureRule,
            origin: OwnedSourceCapabilityOrigin::Failure { index: 0 },
            ..
        })),
        "{error:?}"
    );
}

#[test]
fn directly_constructed_programs_cannot_widen_the_producer_set() {
    let program = lower_with_basic_owned_sequence_exists(&project(producers().remove(0))).unwrap();
    let mut widened = program.clone();
    for node in &mut widened.expressions {
        if let Expression::SequenceExists { sequence, .. } = &mut node.expression {
            *sequence = GeneratedSequence::RecursiveCollect {
                collection: path(&["Rows"]),
                children: vec![],
                descent_value: path(&["Code"]),
                values: vec![],
                value: path(&["Code"]),
                prefix: 1,
                separator: 2,
                item: 90,
            };
        }
    }
    for result in [
        validate_program_with_basic_owned_sequence_exists(&widened),
        validate_basic_owned_sequence_exists_emission(&widened),
        legacy_named_input_admission_plan_with_basic_owned_sequence_exists(&widened).map(|_| ()),
    ] {
        assert!(
            result.as_ref().is_err_and(generated_item_refusal),
            "{result:?}"
        );
    }
}
