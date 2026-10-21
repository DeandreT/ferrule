use crate::*;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{AggregateOp, Node, OwnedSourceField, OwnedSourceRef, SourceOwner};
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
fn call(function: &str, args: &[u32]) -> Node {
    Node::Call {
        function: function.into(),
        args: args.to_vec(),
    }
}
fn aggregate(function: AggregateOp, sequence: mapping::SequenceExpr) -> Node {
    Node::SequenceAggregate {
        function,
        sequence,
        predicate: Some(31),
        expression: Some(32),
        arg: Some(33),
    }
}
/// `Value = function(sequence, equal(item, Cfg.Code), concat(item, Cfg.Code), Cfg.Count)`.
fn project(function: AggregateOp, sequence: mapping::SequenceExpr) -> mapping::Project {
    mapping::Project {
        source: schema(),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
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
                (31, call("equal", &[90, 30])),
                (32, call("concat", &[90, 30])),
                (33, owned(named(), &["Count"])),
                (40, aggregate(function, sequence)),
                (90, item()),
            ]),
        },
        root: mapping::Scope {
            bindings: vec![mapping::Binding {
                target_field: "Value".into(),
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
const REDUCERS: [AggregateOp; 7] = [
    AggregateOp::Count,
    AggregateOp::Sum,
    AggregateOp::Avg,
    AggregateOp::Min,
    AggregateOp::Max,
    AggregateOp::Join,
    AggregateOp::ItemAt,
];
fn generated_item_refusal(error: &ProgramValidationError) -> bool {
    matches!(error, ProgramValidationError::OwnedSource { diagnostic }
    if matches!(diagnostic.as_ref(), OwnedSourceDiagnostic::Capability {
        location: OwnedSourceDiagnosticLocation::Expression { node: 3 | 30 | 33, .. },
        reason: OwnedSourceCapabilityReason::GeneratedItem,
        origin: OwnedSourceCapabilityOrigin::Expression { node: 40, .. },
        ..
    }))
}

#[test]
fn explicit_profile_admits_every_reducer_over_every_pure_producer() {
    for function in REDUCERS {
        for sequence in producers() {
            let project = project(function, sequence);
            let program = lower_with_basic_owned_sequence_aggregate(&project).unwrap();
            validate_program_with_basic_owned_sequence_aggregate(&program).unwrap();
            validate_basic_owned_sequence_aggregate_emission(&program).unwrap();
            let plan =
                legacy_named_input_admission_plan_with_basic_owned_sequence_aggregate(&program)
                    .unwrap();
            // Owned item-phase and argument reads create no eager input.
            assert_eq!(
                plan.all_outputs.entries[0].availability,
                NamedInputAvailability::Optional
            );
            // Earlier profiles, including Exists and ItemAt, keep refusing.
            for result in [
                validate_program_with_basic_owned_sequence_exists(&program),
                validate_basic_owned_sequence_exists_emission(&program),
                validate_program_with_basic_owned_sequence_item_at(&program),
                validate_program_with_basic_owned_group_ending_with(&program),
                validate_program_with_basic_owned_recursive_collect(&program),
                validate_program_with_basic_owned_recursive_filter(&program),
                validate_program_with_basic_owned_lookup_find(&program),
                validate_program_with_basic_owned_dynamic_field(&program),
                validate_program_with_basic_owned_group_by(&program),
                validate_program(&program),
            ] {
                assert!(
                    result.as_ref().is_err_and(generated_item_refusal),
                    "{function:?}: {result:?}"
                );
            }
        }
    }
}

#[test]
fn the_profile_also_admits_owned_exists_and_item_at() {
    let mut project = project(AggregateOp::Count, producers().remove(0));
    project.graph.nodes.insert(91, item());
    project.graph.nodes.insert(34, call("equal", &[91, 30]));
    project.graph.nodes.insert(
        41,
        Node::SequenceExists {
            sequence: mapping::SequenceExpr::Tokenize {
                input: 1,
                delimiter: 2,
                item: 91,
            },
            predicate: 34,
        },
    );
    project.graph.nodes.insert(31, call("and", &[41, 35]));
    project.graph.nodes.insert(35, call("equal", &[90, 30]));
    project.target =
        SchemaNode::group("Output", vec![SchemaNode::scalar("Value", ScalarType::Int)]);
    let program = lower_with_basic_owned_sequence_aggregate(&project).unwrap();
    validate_basic_owned_sequence_aggregate_emission(&program).unwrap();
}

#[test]
fn legacy_alias_reads_inside_item_phases_and_argument_remain_eager() {
    let legacy = || Node::SourceField {
        path: path(&["Cfg", "Code"]),
        frame: None,
    };
    for phase in ["predicate", "expression", "argument"] {
        let mut project = project(AggregateOp::Join, producers().remove(0));
        match phase {
            "predicate" => {
                project.graph.nodes.insert(36, legacy());
                project.graph.nodes.insert(37, call("equal", &[90, 36]));
                project.graph.nodes.insert(38, call("equal", &[90, 30]));
                project.graph.nodes.insert(31, call("and", &[37, 38]));
            }
            "expression" => {
                project.graph.nodes.insert(36, legacy());
                project
                    .graph
                    .nodes
                    .insert(32, call("concat", &[90, 30, 36]));
            }
            _ => {
                project.graph.nodes.insert(33, legacy());
            }
        }
        let program = lower_with_basic_owned_sequence_aggregate(&project).unwrap();
        let plan = legacy_named_input_admission_plan_with_basic_owned_sequence_aggregate(&program)
            .unwrap();
        assert_eq!(
            plan.all_outputs.entries[0].availability,
            NamedInputAvailability::RequiredLegacy,
            "{phase}"
        );
        assert_eq!(plan.primary, plan.all_outputs);
    }
}

#[test]
fn closed_producers_and_failure_predicates_stay_closed() {
    let recursive = mapping::SequenceExpr::RecursiveCollect {
        collection: path(&["Rows"]),
        children: vec![],
        descent_value: path(&["Code"]),
        values: vec![],
        value: path(&["Code"]),
        prefix: 1,
        separator: 2,
        item: 90,
    };
    let error = lower_with_basic_owned_sequence_aggregate(&project(AggregateOp::Count, recursive))
        .unwrap_err();
    assert!(
        matches!(error.diagnostics(), [Diagnostic::OwnedSource { diagnostic }]
        if generated_item_refusal(&ProgramValidationError::OwnedSource {
            diagnostic: diagnostic.clone(),
        })),
        "{error:?}"
    );

    // The owned walk is qualified only for parent-context ItemAt.
    let walk = mapping::SequenceExpr::OwnedRecursiveCollect(mapping::OwnedRecursiveCollect {
        collection: OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows"]),
        },
        children: vec![],
        descent_value: path(&["Code"]),
        values: vec![],
        value: path(&["Code"]),
        prefix: 1,
        separator: 2,
        item: 90,
    });
    let error =
        lower_with_basic_owned_sequence_aggregate(&project(AggregateOp::Count, walk)).unwrap_err();
    assert!(
        matches!(error.diagnostics(), [Diagnostic::OwnedSource { diagnostic }]
        if matches!(diagnostic.as_ref(), OwnedSourceDiagnostic::Capability {
            location: OwnedSourceDiagnosticLocation::Expression { node: 40, .. },
            request: Some(OwnedSourceDiagnosticRequest::Iteration { .. }),
            reason: OwnedSourceCapabilityReason::GeneratedItem,
            ..
        })),
        "{error:?}"
    );

    let mut project = self::project(AggregateOp::Count, producers().remove(0));
    project.root.bindings.clear();
    project.graph.nodes.insert(
        41,
        Node::Const {
            value: Value::Int(1),
        },
    );
    project.graph.nodes.insert(42, call("equal", &[40, 41]));
    project.failure_rules = vec![mapping::FailureRule {
        iteration: mapping::FailureIteration::Source { collection: vec![] },
        selection: mapping::FailureSelection::WhenTrue { predicate: 42 },
        message: None,
    }];
    let error = lower_with_basic_owned_sequence_aggregate(&project).unwrap_err();
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
    let program = lower_with_basic_owned_sequence_aggregate(&project(
        AggregateOp::Sum,
        producers().remove(0),
    ))
    .unwrap();
    let mut widened = program.clone();
    for node in &mut widened.expressions {
        if let Expression::SequenceAggregate { sequence, .. } = &mut node.expression {
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
        validate_program_with_basic_owned_sequence_aggregate(&widened),
        validate_basic_owned_sequence_aggregate_emission(&widened),
        legacy_named_input_admission_plan_with_basic_owned_sequence_aggregate(&widened).map(|_| ()),
    ] {
        assert!(
            result.as_ref().is_err_and(generated_item_refusal),
            "{result:?}"
        );
    }
}
