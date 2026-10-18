use crate::*;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Node, OwnedPosition, OwnedSourceField, OwnedSourceRef, SourceOwner};
use std::collections::BTreeMap;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).into()).collect()
}
fn owned(owner: SourceOwner, parts: &[&str], frame: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: frame.map(path),
    })
}
fn project() -> mapping::Project {
    let source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Message", ScalarType::String),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Flag", ScalarType::Bool),
                    SchemaNode::scalar("Message", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    let root = mapping::Scope {
        bindings: vec![mapping::Binding {
            target_field: "Value".into(),
            node: 10,
        }],
        ..Default::default()
    };
    mapping::Project {
        source: source.clone(),
        target: target.clone(),
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
        extra_targets: vec![mapping::NamedTarget {
            name: "Other".into(),
            path: None,
            schema: target,
            options: Default::default(),
            root: root.clone(),
        }],
        root,
        graph: mapping::Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    owned(SourceOwner::Primary, &["Rows", "Flag"], Some(&["Rows"])),
                ),
                (
                    2,
                    Node::OwnedPosition(OwnedPosition {
                        owner: SourceOwner::Primary,
                        collection: Some(path(&["Rows"])),
                    }),
                ),
                (
                    3,
                    owned(
                        SourceOwner::Named { name: "Ref".into() },
                        &["Message"],
                        Some(&[]),
                    ),
                ),
                (
                    4,
                    Node::Const {
                        value: Value::Bool(false),
                    },
                ),
                (
                    5,
                    Node::If {
                        condition: 4,
                        then: 3,
                        else_: 2,
                    },
                ),
                (
                    10,
                    Node::Const {
                        value: Value::String("ok".into()),
                    },
                ),
            ]),
        },
        failure_rules: vec![mapping::FailureRule {
            iteration: mapping::FailureIteration::Source {
                collection: path(&["Rows"]),
            },
            selection: mapping::FailureSelection::WhenTrue { predicate: 1 },
            message: Some(5),
        }],
    }
}

#[test]
fn owned_row_failures_lower_genuine_projects_without_changing_old_gates_or_ir() {
    let p = project();
    assert!(lower(&p).is_err());
    assert!(lower_with_basic_owned_windows(&p).is_err());
    let program = lower_with_basic_owned_row_failures(&p).unwrap();
    assert_eq!(
        program.failure_rules,
        vec![FailureRule {
            iteration: FailureIteration::Source(SourceIteration::new(path(&["Rows"]))),
            selection: FailureSelection::WhenTrue(1),
            message: Some(5)
        }]
    );
    assert!(is_basic_owned_primary_row_failure(
        &program,
        &program.failure_rules[0]
    ));
    assert_eq!(validate_basic_owned_row_failure_emission(&program), Ok(()));
    assert_eq!(
        validate_program_with_basic_owned_row_failures(&program),
        Ok(())
    );
    let expected = validate_program(&program).unwrap_err();
    for validate in [
        validate_basic_owned_emission,
        validate_basic_owned_filter_emission,
        validate_basic_owned_sort_emission,
        validate_basic_owned_window_emission,
    ] {
        assert_eq!(validate(&program), Err(expected.clone()));
    }
}

#[test]
fn owned_row_failures_keep_structure_priority_and_independent_window_target_permission() {
    let mut p = lower_with_basic_owned_row_failures(&project()).unwrap();
    p.expressions.push(ExpressionNode {
        id: 1,
        expression: Expression::Const {
            value: Value::Int(0),
        },
    });
    assert_eq!(
        validate_basic_owned_row_failure_emission(&p),
        Err(ProgramValidationError::DuplicateExpression { node: 1 })
    );
    let mut p = lower_with_basic_owned_row_failures(&project()).unwrap();
    p.root.repeating = true;
    p.target.repeating = true;
    p.root.iteration = Some(IterationPlan::new(
        OwnedSourceIteration::new(OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows"]),
        }),
        None,
        None,
        vec![SequenceWindow::First { count: 2 }],
        IterationOutput::Repeated,
    ));
    assert_eq!(validate_basic_owned_row_failure_emission(&p), Ok(()));
    assert!(validate_basic_owned_window_emission(&p).is_err());
}

#[test]
fn owned_row_failures_keep_named_roots_lazy_in_every_target_catalogue() {
    let p = lower_with_basic_owned_row_failures(&project()).unwrap();
    let plan = legacy_named_input_admission_plan_with_basic_owned_row_failures(&p).unwrap();
    for catalogue in [
        &plan.all_outputs,
        &plan.primary,
        &plan.named_targets[0].catalogue,
    ] {
        assert_eq!(catalogue.entries[0].name, "Ref");
        assert_eq!(
            catalogue.entries[0].availability,
            NamedInputAvailability::Optional
        );
    }
    assert!(legacy_named_input_admission_plan_with_basic_owned_windows(&p).is_err());
}

#[test]
fn owned_row_failures_refuse_other_failure_ownership_profiles_even_on_inactive_branches() {
    let base = lower_with_basic_owned_row_failures(&project()).unwrap();
    for replacement in [
        Expression::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Named { name: "Ref".into() },
            collection: None,
        }),
        Expression::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: SourceOwner::Named { name: "Ref".into() },
                path: path(&["Rows", "Message"]),
            },
            frame: Some(path(&["Rows"])),
        }),
    ] {
        let mut p = base.clone();
        p.expressions
            .iter_mut()
            .find(|n| n.id == 3)
            .unwrap()
            .expression = replacement;
        assert!(!is_basic_owned_primary_row_failure(&p, &p.failure_rules[0]));
        assert_eq!(
            validate_basic_owned_row_failure_emission(&p),
            validate_program_with_basic_owned_windows(&p)
        );
    }
    for source in [vec![], path(&["Ref"]), path(&["Rows", "Rows"])] {
        let mut p = base.clone();
        p.failure_rules[0].iteration = FailureIteration::Source(SourceIteration::new(source));
        assert!(!is_basic_owned_primary_row_failure(&p, &p.failure_rules[0]));
        assert!(validate_basic_owned_row_failure_emission(&p).is_err());
    }
}

#[test]
fn owned_row_failures_classify_each_rule_and_preserve_ordinary_global_rules() {
    let mut p = lower_with_basic_owned_row_failures(&project()).unwrap();
    p.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source(SourceIteration::new(vec![])),
        selection: FailureSelection::All,
        message: Some(10),
    });
    assert!(is_basic_owned_primary_row_failure(&p, &p.failure_rules[0]));
    assert!(!is_basic_owned_primary_row_failure(&p, &p.failure_rules[1]));
    assert_eq!(validate_basic_owned_row_failure_emission(&p), Ok(()));
    p.failure_rules.remove(0);
    assert_eq!(validate_basic_owned_window_emission(&p), Ok(()));
    assert_eq!(validate_basic_owned_row_failure_emission(&p), Ok(()));
}

fn expression(p: &mut Program, id: u32, expression: Expression) {
    p.expressions.push(ExpressionNode { id, expression });
}

fn closed_rule_leaf(node: u32, request: OwnedSourceDiagnosticRequest) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
            location: OwnedSourceDiagnosticLocation::Expression {
                namespace: OwnedSourceNamespace::Main,
                node,
                scope: None,
            },
            request: Some(request),
            reason: OwnedSourceCapabilityReason::FailureRule,
            origin: OwnedSourceCapabilityOrigin::Failure { index: 0 },
        }),
    }
}

#[test]
fn owned_row_failures_follow_potential_owned_defaults_and_raise_messages() {
    let base = lower_with_basic_owned_row_failures(&project()).unwrap();
    for (leaf, ty, request) in [
        (
            2,
            ScalarType::Int,
            OwnedSourceDiagnosticRequest::Position {
                owner: OwnedSourceDiagnosticOwner::Primary,
                collection: Some(path(&["Rows"])),
            },
        ),
        (
            3,
            ScalarType::String,
            OwnedSourceDiagnosticRequest::Field {
                owner: OwnedSourceDiagnosticOwner::Named { name: "Ref".into() },
                path: path(&["Message"]),
                frame: Some(vec![]),
            },
        ),
    ] {
        for message in [30, 31, 32] {
            let mut p = base.clone();
            // No independently owned predicate: classification must find the
            // sole owned demand through each default/message dependency edge.
            p.failure_rules[0].selection = FailureSelection::WhenTrue(4);
            p.failure_rules[0].message = Some(message);
            expression(
                &mut p,
                30,
                Expression::RuntimeParameterDefault {
                    name: "row_message".into(),
                    ty,
                    default: leaf,
                },
            );
            expression(&mut p, 31, Expression::Raise { message: Some(30) });
            expression(
                &mut p,
                32,
                Expression::If {
                    condition: 4,
                    then: 31,
                    else_: 10,
                },
            );
            assert!(is_basic_owned_primary_row_failure(&p, &p.failure_rules[0]));
            assert_eq!(validate_basic_owned_row_failure_emission(&p), Ok(()));
            assert_eq!(validate_program_with_basic_owned_row_failures(&p), Ok(()));
            assert_eq!(
                validate_program_with_basic_owned_windows(&p),
                Err(closed_rule_leaf(leaf, request.clone()))
            );
            let plan = legacy_named_input_admission_plan_with_basic_owned_row_failures(&p).unwrap();
            assert_eq!(
                plan.all_outputs.entries[0].availability,
                NamedInputAvailability::Optional
            );
        }
    }
}

fn value_map(input: u32) -> Expression {
    Expression::ValueMap {
        input,
        input_type: Some(ScalarType::Int),
        table: vec![(Value::Int(1), Value::String("mapped".into()))],
        default: Some(Value::String("default".into())),
    }
}

#[test]
fn owned_row_failures_reject_maps_hiding_disallowed_owned_branch_and_default_edges() {
    let base = lower_with_basic_owned_row_failures(&project()).unwrap();
    for input in [3, 80, 81, 82] {
        let mut p = base.clone();
        // Keep the valid owned predicate. Ignoring a hidden message edge could
        // otherwise misclassify the complete rule as eligible and bypass the
        // ordinary failure gate that still rejects named row positions.
        p.expressions
            .iter_mut()
            .find(|node| node.id == 3)
            .unwrap()
            .expression = Expression::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Named { name: "Ref".into() },
            collection: None,
        });
        expression(
            &mut p,
            79,
            Expression::Const {
                value: Value::Int(1),
            },
        );
        expression(
            &mut p,
            80,
            Expression::If {
                condition: 4,
                then: 3,
                else_: 79,
            },
        );
        expression(
            &mut p,
            81,
            Expression::RuntimeParameterDefault {
                name: "map_input".into(),
                ty: ScalarType::Int,
                default: 80,
            },
        );
        expression(&mut p, 82, Expression::Raise { message: Some(81) });
        expression(&mut p, 90, value_map(input));
        p.failure_rules[0].message = Some(90);
        assert!(!is_basic_owned_primary_row_failure(&p, &p.failure_rules[0]));
        let expected = closed_rule_leaf(
            1,
            OwnedSourceDiagnosticRequest::Field {
                owner: OwnedSourceDiagnosticOwner::Primary,
                path: path(&["Rows", "Flag"]),
                frame: Some(path(&["Rows"])),
            },
        );
        assert_eq!(
            validate_basic_owned_row_failure_emission(&p),
            Err(expected.clone())
        );
        assert_eq!(validate_program_with_basic_owned_windows(&p), Err(expected));
    }
}

#[test]
fn owned_row_failures_keep_ordinary_value_maps_closed_in_inactive_branches_and_defaults() {
    let base = lower_with_basic_owned_row_failures(&project()).unwrap();
    for message in [90, 91, 92, 93] {
        let mut p = base.clone();
        p.failure_rules[0].selection = FailureSelection::WhenTrue(4);
        p.failure_rules[0].message = Some(message);
        expression(
            &mut p,
            89,
            Expression::Const {
                value: Value::Int(1),
            },
        );
        expression(&mut p, 90, value_map(89));
        expression(
            &mut p,
            91,
            Expression::If {
                condition: 4,
                then: 90,
                else_: 10,
            },
        );
        expression(
            &mut p,
            92,
            Expression::RuntimeParameterDefault {
                name: "message".into(),
                ty: ScalarType::String,
                default: 91,
            },
        );
        expression(&mut p, 93, Expression::Raise { message: Some(92) });
        assert!(!is_basic_owned_primary_row_failure(&p, &p.failure_rules[0]));
        let expected = ProgramValidationError::OwnedSource {
            diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
                location: OwnedSourceDiagnosticLocation::Iteration {
                    scope: mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary),
                },
                request: None,
                reason: OwnedSourceCapabilityReason::FailureRule,
                origin: OwnedSourceCapabilityOrigin::Failure { index: 0 },
            }),
        };
        assert_eq!(
            validate_basic_owned_row_failure_emission(&p),
            Err(expected.clone())
        );
        assert_eq!(validate_basic_owned_window_emission(&p), Err(expected));
    }
}

fn collection_alias_program() -> Program {
    let mut p = project();
    p.extra_sources.push(mapping::NamedSource {
        name: "Rows".into(),
        path: "rows.json".into(),
        schema: p.source.clone(),
        options: Default::default(),
        dynamic_path: None,
    });
    lower_with_basic_owned_row_failures(&p).unwrap()
}

fn alias_catalogue(
    reference: NamedInputAvailability,
    rows: NamedInputAvailability,
) -> NamedInputCatalogue<'static> {
    NamedInputCatalogue {
        entries: vec![
            NamedInputAdmission {
                declaration_index: 0,
                name: "Ref",
                availability: reference,
            },
            NamedInputAdmission {
                declaration_index: 1,
                name: "Rows",
                availability: rows,
            },
        ],
    }
}

fn assert_alias_plan(
    actual: NamedInputAdmissionPlan<'_>,
    reference: NamedInputAvailability,
    rows: [NamedInputAvailability; 3],
) {
    // Independent complete All / Primary / Other catalogue contracts, including
    // the original declaration ordinals; no expected set is derived by analysis.
    assert_eq!(
        actual,
        NamedInputAdmissionPlan {
            all_outputs: alias_catalogue(reference, rows[0]),
            primary: alias_catalogue(reference, rows[1]),
            named_targets: vec![NamedTargetInputCatalogue {
                target_index: 0,
                name: "Other",
                catalogue: alias_catalogue(reference, rows[2]),
            }],
        }
    );
}

#[test]
fn owned_row_failure_admission_keeps_primary_collection_and_owned_ref_optional() {
    use NamedInputAvailability::Optional;
    let base = collection_alias_program();
    // Cover ownership in both predicate and message, including the position-
    // only All rule. A named owned field is optional even when read directly,
    // not just when it happens to occur in an inactive message branch.
    for (selection, message) in [
        (FailureSelection::WhenTrue(1), 3),
        (FailureSelection::WhenTrue(1), 5),
        (FailureSelection::All, 2),
    ] {
        let mut p = base.clone();
        p.failure_rules[0].selection = selection;
        p.failure_rules[0].message = Some(message);
        assert_alias_plan(
            legacy_named_input_admission_plan_with_basic_owned_row_failures(&p).unwrap(),
            Optional,
            [Optional; 3],
        );
    }
}

#[test]
fn owned_row_failure_admission_preserves_ordinary_rules_and_old_companion_catalogues() {
    use NamedInputAvailability::{Optional, RequiredLegacy};
    let base = collection_alias_program();
    for ordinary in [
        FailureRule {
            iteration: FailureIteration::Source(SourceIteration::new(path(&["Rows"]))),
            selection: FailureSelection::All,
            message: None,
        },
        FailureRule {
            iteration: FailureIteration::Source(SourceIteration::new(path(&["Rows"]))),
            selection: FailureSelection::WhenTrue(4),
            message: Some(10),
        },
    ] {
        let mut p = base.clone();
        p.failure_rules = vec![ordinary.clone()];
        // Freeze complete pre-existing companion metadata, not just the new
        // companion's behavior. Both ordinary rules keep the alias requirement.
        for actual in [
            legacy_named_input_admission_plan(&p).unwrap(),
            legacy_named_input_admission_plan_with_basic_owned_filters(&p).unwrap(),
            legacy_named_input_admission_plan_with_basic_owned_sort(&p).unwrap(),
            legacy_named_input_admission_plan_with_basic_owned_windows(&p).unwrap(),
            legacy_named_input_admission_plan_with_basic_owned_row_failures(&p).unwrap(),
        ] {
            assert_alias_plan(actual, Optional, [RequiredLegacy; 3]);
        }
        // A later, even statically false, ordinary rule remains a global eager
        // requirement; an eligible owned rule must not erase it in either order.
        for rules in [
            vec![base.failure_rules[0].clone(), ordinary.clone()],
            vec![ordinary.clone(), base.failure_rules[0].clone()],
        ] {
            p.failure_rules = rules;
            assert_alias_plan(
                legacy_named_input_admission_plan_with_basic_owned_row_failures(&p).unwrap(),
                Optional,
                [RequiredLegacy; 3],
            );
        }
    }
}

#[test]
fn owned_row_failure_admission_keeps_legacy_predicate_and_message_alias_dependencies() {
    use NamedInputAvailability::{Optional, RequiredLegacy};
    let base = collection_alias_program();
    for predicate in [false, true] {
        for kind in 0..3 {
            let mut p = base.clone();
            let field = if predicate { "Flag" } else { "Message" };
            let leaf = match kind {
                0 => Expression::SourceField {
                    path: path(&["Rows", field]),
                    frame: None,
                },
                1 => Expression::SourceField {
                    path: path(&[field]),
                    frame: Some(path(&["Rows"])),
                },
                _ => Expression::Position {
                    collection: path(&["Rows"]),
                },
            };
            expression(
                &mut p,
                49,
                Expression::Const {
                    value: Value::Int(0),
                },
            );
            expression(&mut p, 50, leaf);
            let direct = if predicate && kind == 2 {
                expression(
                    &mut p,
                    51,
                    Expression::Call {
                        function: ScalarFunction::GreaterThan,
                        args: vec![50, 49],
                    },
                );
                51
            } else {
                50
            };
            let (ty, fallback) = if predicate {
                (ScalarType::Bool, Value::Bool(false))
            } else if kind == 2 {
                (ScalarType::Int, Value::Int(0))
            } else {
                (ScalarType::String, Value::String("unused".into()))
            };
            expression(&mut p, 52, Expression::Const { value: fallback });
            expression(
                &mut p,
                53,
                Expression::If {
                    condition: 4,
                    then: direct,
                    else_: 52,
                },
            );
            expression(
                &mut p,
                54,
                Expression::RuntimeParameterDefault {
                    name: "legacy_input".into(),
                    ty,
                    default: 53,
                },
            );
            expression(&mut p, 55, Expression::Raise { message: Some(54) });
            for root in [direct, 55] {
                if predicate {
                    p.failure_rules[0].selection = FailureSelection::WhenTrue(root);
                    p.failure_rules[0].message = Some(2);
                } else {
                    p.failure_rules[0].selection = FailureSelection::WhenTrue(1);
                    p.failure_rules[0].message = Some(root);
                }
                // The ordinary leaf is part of an otherwise eligible owned
                // rule. Its direct or inactive/default-wrapped dependency must
                // independently preserve the existing legacy alias obligation.
                assert!(is_basic_owned_primary_row_failure(&p, &p.failure_rules[0]));
                assert_alias_plan(
                    legacy_named_input_admission_plan_with_basic_owned_row_failures(&p).unwrap(),
                    Optional,
                    [RequiredLegacy; 3],
                );
            }
        }
    }
}

#[test]
fn owned_row_failure_admission_keeps_target_aliases_selection_specific() {
    use NamedInputAvailability::{Optional, RequiredLegacy};
    let mut p = collection_alias_program();
    p.root.repeating = true;
    p.target.repeating = true;
    p.root.iteration = Some(IterationPlan::source(path(&["Rows"])));
    // Only the primary target has an ordinary Rows iteration. All includes it;
    // the independently selected Other target has only the eligible row rules.
    assert_alias_plan(
        legacy_named_input_admission_plan_with_basic_owned_row_failures(&p).unwrap(),
        Optional,
        [RequiredLegacy, RequiredLegacy, Optional],
    );
}
