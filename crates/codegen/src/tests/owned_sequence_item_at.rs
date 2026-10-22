use crate::*;
use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{Node, OwnedSourceField, OwnedSourceRef, SourceOwner};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
enum Producer {
    Range(bool),
    Tokenize,
    Length,
    Regex(bool),
}

const PRODUCERS: [Producer; 6] = [
    Producer::Range(false),
    Producer::Range(true),
    Producer::Tokenize,
    Producer::Length,
    Producer::Regex(false),
    Producer::Regex(true),
];

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn named() -> SourceOwner {
    SourceOwner::Named { name: "Cfg".into() }
}

fn field(owner: SourceOwner, parts: &[&str], pin: Option<&[&str]>) -> OwnedSourceField {
    OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: pin.map(path),
    }
}

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Cfg",
                vec![
                    SchemaNode::scalar("Text", ScalarType::String),
                    SchemaNode::scalar("Delimiter", ScalarType::String),
                    SchemaNode::scalar("Length", ScalarType::Int),
                    SchemaNode::scalar("Pattern", ScalarType::String),
                    SchemaNode::scalar("Flags", ScalarType::String),
                    SchemaNode::scalar("From", ScalarType::Int),
                    SchemaNode::scalar("To", ScalarType::Int),
                    SchemaNode::scalar("Index", ScalarType::Int),
                ],
            ),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Key", ScalarType::String),
                    SchemaNode::scalar("Keep", ScalarType::Bool),
                    SchemaNode::scalar("Label", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    )
}

impl Producer {
    fn sequence(self, item: u32) -> mapping::SequenceExpr {
        match self {
            Self::Range(from) => mapping::SequenceExpr::Generate {
                from: from.then_some(6),
                to: 7,
                item,
            },
            Self::Tokenize => mapping::SequenceExpr::Tokenize {
                input: 1,
                delimiter: 2,
                item,
            },
            Self::Length => mapping::SequenceExpr::TokenizeByLength {
                input: 1,
                length: 3,
                item,
            },
            Self::Regex(flags) => mapping::SequenceExpr::TokenizeRegex {
                input: 1,
                pattern: 4,
                flags: flags.then_some(5),
                item,
            },
        }
    }

    fn ty(self) -> ScalarType {
        if matches!(self, Self::Range(_)) {
            ScalarType::Int
        } else {
            ScalarType::String
        }
    }
}

fn project(producer: Producer) -> mapping::Project {
    let fields = [
        "Text",
        "Delimiter",
        "Length",
        "Pattern",
        "Flags",
        "From",
        "To",
        "Index",
    ];
    let mut nodes: BTreeMap<_, _> = fields
        .into_iter()
        .enumerate()
        .map(|(i, name)| {
            (
                (i + 1) as u32,
                Node::OwnedSourceField(field(named(), &["Cfg", name], Some(&[]))),
            )
        })
        .collect();
    nodes.insert(
        9,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    nodes.insert(
        90,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    nodes.insert(
        100,
        Node::SequenceItemAt {
            sequence: producer.sequence(90),
            index: 8,
        },
    );
    mapping::Project {
        source: schema(),
        target: SchemaNode::group("Output", vec![SchemaNode::scalar("Result", producer.ty())]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: ["Unused", "Cfg", "Legacy", "Dynamic", " Cfg💠 ", "Global"]
            .into_iter()
            .map(|name| mapping::NamedSource {
                name: name.into(),
                path: "unused.json".into(),
                schema: schema(),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect(),
        extra_targets: vec![],
        user_functions: Default::default(),
        failure_rules: vec![],
        graph: mapping::Graph { nodes },
        root: mapping::Scope {
            bindings: vec![mapping::Binding {
                target_field: "Result".into(),
                node: 100,
            }],
            ..Default::default()
        },
    }
}

fn fixture(producer: Producer) -> Program {
    let p = project(producer);
    lower_with_basic_owned_sequence_item_at(&p)
        .unwrap_or_else(|error| panic!("{producer:?} Project={p:#?}: {error:#?}"))
}

fn expression(program: &mut Program, id: u32) -> &mut Expression {
    &mut program
        .expressions
        .iter_mut()
        .find(|node| node.id == id)
        .unwrap()
        .expression
}

fn primary() -> mapping::SourceScopeLocation {
    mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
}

fn capability(
    error: &ProgramValidationError,
    reason: OwnedSourceCapabilityReason,
) -> &OwnedSourceDiagnostic {
    let ProgramValidationError::OwnedSource { diagnostic } = error else {
        panic!("expected {reason:?}: {error:#?}")
    };
    assert!(
        matches!(diagnostic.as_ref(), OwnedSourceDiagnostic::Capability { reason: actual, .. } if *actual == reason),
        "{error:#?}"
    );
    diagnostic
}

fn availability(catalogue: &NamedInputCatalogue<'_>) -> Vec<NamedInputAvailability> {
    catalogue
        .entries
        .iter()
        .map(|entry| entry.availability)
        .collect()
}

#[test]
fn owned_sequence_item_at_lowers_every_scalar_producer_with_exact_parent_owned_arguments() {
    for producer in PRODUCERS {
        let p = project(producer);
        let program = lower_with_basic_owned_sequence_item_at(&p).unwrap();
        assert_eq!(
            validate_program_with_basic_owned_sequence_item_at(&program),
            Ok(())
        );
        assert_eq!(
            validate_basic_owned_sequence_item_at_emission(&program),
            Ok(())
        );
        let consumer = program
            .expressions
            .iter()
            .find(|node| node.id == 100)
            .unwrap();
        let Expression::SequenceItemAt { sequence, index } = &consumer.expression else {
            panic!("consumer retained")
        };
        assert_eq!(*index, 8);
        assert_eq!(sequence.item(), 90);
        assert_eq!(
            sequence.inputs().collect::<Vec<_>>(),
            producer.sequence(90).inputs()
        );
        assert_eq!(
            program
                .expressions
                .iter()
                .find(|node| node.id == 90)
                .unwrap()
                .expression,
            Expression::SourceField {
                path: vec![],
                frame: None
            }
        );
        for input in sequence.inputs().chain([8]) {
            assert!(
                matches!(&program.expressions.iter().find(|node| node.id == input).unwrap().expression,
                Expression::OwnedSourceField(OwnedSourceField { source: OwnedSourceRef { owner: SourceOwner::Named { name }, .. }, frame: Some(pin) })
                if name == "Cfg" && pin.is_empty())
            );
        }
        assert_eq!(program.source, p.source);
        assert_eq!(program.extra_sources[1].name, "Cfg");
        let plan =
            legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(&program).unwrap();
        assert_eq!(
            availability(&plan.primary),
            [NamedInputAvailability::Optional; 6]
        );
    }
}

#[test]
fn owned_sequence_item_at_preserves_all_older_api_refusals() {
    for producer in PRODUCERS {
        let p = project(producer);
        let program = fixture(producer);
        let expected = validate_program(&program).unwrap_err();
        let diagnostic = capability(&expected, OwnedSourceCapabilityReason::GeneratedItem);
        assert!(matches!(diagnostic, OwnedSourceDiagnostic::Capability {
            location: OwnedSourceDiagnosticLocation::Expression { scope: Some(scope), .. },
            origin: OwnedSourceCapabilityOrigin::Expression { node: 100, .. }, ..
        } if *scope == primary()));
        for validate in [
            validate_basic_owned_emission,
            validate_basic_owned_filter_emission,
            validate_basic_owned_sort_emission,
            validate_basic_owned_window_emission,
            validate_basic_owned_row_failure_emission,
            validate_basic_owned_group_by_emission,
        ] {
            assert_eq!(validate(&program), Err(expected.clone()));
        }
        for lower in [
            lower,
            lower_with_basic_owned_filters,
            lower_with_basic_owned_sort,
            lower_with_basic_owned_windows,
            lower_with_basic_owned_row_failures,
            lower_with_basic_owned_group_by,
        ] {
            assert!(lower(&p).is_err());
        }
        for plan in [
            legacy_named_input_admission_plan,
            legacy_named_input_admission_plan_with_basic_owned_filters,
            legacy_named_input_admission_plan_with_basic_owned_sort,
            legacy_named_input_admission_plan_with_basic_owned_windows,
            legacy_named_input_admission_plan_with_basic_owned_row_failures,
            legacy_named_input_admission_plan_with_basic_owned_group_by,
        ] {
            assert_eq!(plan(&program).unwrap_err(), expected);
        }
    }
}

#[test]
fn owned_sequence_item_at_admits_owned_demand_in_producer_only_or_index_only() {
    for producer in PRODUCERS {
        for index_only in [false, true] {
            let mut p = project(producer);
            for id in 1..=8 {
                if (id == 8) != index_only {
                    let value = match id {
                        1 => Value::String("a|b|c".into()),
                        2 => Value::String("|".into()),
                        4 => Value::String("[|]".into()),
                        5 => Value::String(String::new()),
                        _ => Value::Int(2),
                    };
                    p.graph.nodes.insert(id, Node::Const { value });
                }
            }
            let program = lower_with_basic_owned_sequence_item_at(&p).unwrap();
            assert_eq!(
                validate_basic_owned_sequence_item_at_emission(&program),
                Ok(())
            );
            let plan =
                legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(&program)
                    .unwrap();
            assert_eq!(
                availability(&plan.all_outputs),
                [NamedInputAvailability::Optional; 6]
            );
        }
    }
}

#[test]
fn owned_sequence_item_at_nests_in_parent_producer_and_index_without_new_item_permissions() {
    for in_producer in [false, true] {
        let mut p = project(Producer::Range(true));
        p.graph.nodes.insert(
            91,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        );
        p.graph.nodes.insert(
            101,
            Node::SequenceItemAt {
                sequence: Producer::Range(true).sequence(91),
                index: 8,
            },
        );
        p.graph.nodes.insert(
            100,
            Node::SequenceItemAt {
                sequence: mapping::SequenceExpr::Generate {
                    from: Some(if in_producer { 101 } else { 6 }),
                    to: 7,
                    item: 90,
                },
                index: if in_producer { 8 } else { 101 },
            },
        );
        let program = lower_with_basic_owned_sequence_item_at(&p).unwrap();
        assert_eq!(
            validate_basic_owned_sequence_item_at_emission(&program),
            Ok(())
        );
        assert_eq!(
            program
                .expressions
                .iter()
                .filter(|node| matches!(node.expression, Expression::SequenceItemAt { .. }))
                .count(),
            2
        );
        assert_eq!(
            legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(&program)
                .unwrap()
                .primary
                .entries[1]
                .availability,
            NamedInputAvailability::Optional
        );
    }
}

#[test]
fn owned_sequence_item_at_composes_by_controls_bounds_and_scalar_scopes() {
    let mut p = project(Producer::Range(true));
    p.target.repeating = true;
    p.root.iteration = mapping::ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: named(),
            path: path(&["Rows"]),
        },
        output_path: None,
    };
    p.graph.nodes.insert(
        20,
        Node::OwnedSourceField(field(named(), &["Rows", "Key"], Some(&["Rows"]))),
    );
    p.graph.nodes.insert(
        21,
        Node::OwnedSourceField(field(named(), &["Rows", "Keep"], Some(&["Rows"]))),
    );
    p.root.group_by = Some(20);
    p.root.post_group_filter = Some(21);
    p.root.filter = Some(21);
    p.root.sort_by = Some(100);
    p.root.sort_filter_order = mapping::SortFilterOrder::FilterThenSort;
    p.root.windows = vec![mapping::SequenceWindow::First { count: 100 }];
    let program = lower_with_basic_owned_sequence_item_at(&p).unwrap();
    let iteration = program.root.iteration.as_ref().unwrap();
    assert_eq!(iteration.grouping(), Some(GroupingPlan::By { key: 20 }));
    assert_eq!(iteration.post_group_filter(), Some(21));
    assert_eq!(iteration.filter(), Some(21));
    assert_eq!(iteration.windows(), &[SequenceWindow::First { count: 100 }]);
    assert!(validate_basic_owned_group_by_emission(&program).is_err());
    assert_eq!(
        validate_basic_owned_sequence_item_at_emission(&program),
        Ok(())
    );
    let mut p = project(Producer::Range(true));
    p.target = SchemaNode::scalar("Result", ScalarType::Int);
    p.root.bindings.clear();
    p.root.construction = mapping::ScopeConstruction::Scalar { value: 100 };
    assert!(lower_with_basic_owned_sequence_item_at(&p).is_ok());
}

#[test]
fn owned_sequence_item_at_parent_owned_positions_keep_the_source_collection_identity() {
    for grouped in [false, true] {
        let mut p = project(Producer::Range(false));
        p.target.repeating = true;
        p.root.iteration = mapping::ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: named(),
                path: path(&["Rows"]),
            },
            output_path: None,
        };
        let position = mapping::OwnedPosition {
            owner: named(),
            collection: Some(path(&["Rows"])),
        };
        p.graph
            .nodes
            .insert(8, Node::OwnedPosition(position.clone()));
        if grouped {
            p.graph.nodes.insert(
                20,
                Node::OwnedSourceField(field(named(), &["Rows", "Key"], Some(&["Rows"]))),
            );
            p.root.group_by = Some(20);
        }
        let program = lower_with_basic_owned_sequence_item_at(&p).unwrap();
        assert_eq!(
            program
                .expressions
                .iter()
                .find(|node| node.id == 8)
                .unwrap()
                .expression,
            Expression::OwnedPosition(position)
        );
        assert_eq!(
            program
                .expressions
                .iter()
                .find(|node| node.id == 90)
                .unwrap()
                .expression,
            Expression::SourceField {
                path: vec![],
                frame: None
            }
        );
        assert_eq!(
            validate_basic_owned_sequence_item_at_emission(&program),
            Ok(())
        );
    }
}

#[test]
fn owned_sequence_item_at_reuses_by_profile_ordinary_legacy_islands_without_widening_owned_modes() {
    for mode in 0..5 {
        let mut p = project(Producer::Tokenize);
        let SchemaKind::Group { children, .. } = &mut p.target.kind else {
            panic!("target group")
        };
        children.push(SchemaNode::group("Legacy", vec![]).repeating());
        p.graph.nodes.insert(
            202,
            Node::Const {
                value: Value::Int(2),
            },
        );
        let mut scope = mapping::Scope {
            target_field: "Legacy".into(),
            iteration: mapping::ScopeIteration::Source(path(&["Rows"])),
            ..Default::default()
        };
        match mode {
            0 => scope.group_by = Some(9),
            1 => scope.group_adjacent_by = Some(9),
            2 => scope.group_starting_with = Some(9),
            3 => scope.group_ending_with = Some(9),
            _ => scope.group_into_blocks = Some(202),
        }
        p.root.children.push(scope);
        let program = lower_with_basic_owned_sequence_item_at(&p).unwrap();
        assert_eq!(
            validate_basic_owned_sequence_item_at_emission(&program),
            Ok(())
        );
        assert!(
            program.root.children[0]
                .iteration
                .as_ref()
                .unwrap()
                .source_iteration()
                .is_some()
        );
    }
}

#[test]
fn owned_sequence_item_at_full_catalogues_preserve_global_selection_alias_and_reserved_slots() {
    let mut p = project(Producer::Tokenize);
    p.graph.nodes.insert(
        200,
        Node::Const {
            value: Value::String("unused.json".into()),
        },
    );
    p.extra_sources[3].dynamic_path = Some(mapping::DynamicSourcePath::legacy(200, vec![]));
    p.graph.nodes.insert(
        201,
        Node::SourceField {
            path: path(&["Legacy", "Cfg", "Text"]),
            frame: None,
        },
    );
    p.graph.nodes.insert(
        202,
        Node::SourceField {
            path: path(&["Global", "Cfg", "Text"]),
            frame: None,
        },
    );
    p.extra_targets.push(mapping::NamedTarget {
        name: " exact 💠 ".into(),
        path: None,
        options: Default::default(),
        schema: p.target.clone(),
        root: mapping::Scope {
            bindings: vec![mapping::Binding {
                target_field: "Result".into(),
                node: 201,
            }],
            ..Default::default()
        },
    });
    p.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source { collection: vec![] },
        selection: mapping::FailureSelection::All,
        message: Some(202),
    });
    let program = lower_with_basic_owned_sequence_item_at(&p).unwrap();
    let plan =
        legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(&program).unwrap();
    use NamedInputAvailability::{Optional as O, RequiredLegacy as R, ReservedUnavailable as U};
    assert_eq!(availability(&plan.primary), [O, O, O, U, O, R]);
    assert_eq!(
        availability(&plan.named_targets[0].catalogue),
        [O, O, R, U, O, R]
    );
    assert_eq!(availability(&plan.all_outputs), [O, O, R, U, O, R]);
    assert_eq!(
        (
            plan.named_targets[0].target_index,
            plan.named_targets[0].name
        ),
        (0, " exact 💠 ")
    );
    assert_eq!(
        plan.primary
            .entries
            .iter()
            .map(|entry| (entry.declaration_index, entry.name))
            .collect::<Vec<_>>(),
        vec![
            (0, "Unused"),
            (1, "Cfg"),
            (2, "Legacy"),
            (3, "Dynamic"),
            (4, " Cfg💠 "),
            (5, "Global")
        ]
    );
    assert!(std::ptr::eq(
        plan.primary.entries[4].name.as_ptr(),
        program.extra_sources[4].name.as_ptr()
    ));
    for frame in [None, Some(vec![])] {
        let mut p = project(Producer::Tokenize);
        p.graph.nodes.insert(
            8,
            Node::SourceField {
                path: path(&["Cfg", "Index"]),
                frame: frame.clone(),
            },
        );
        let program = lower_with_basic_owned_sequence_item_at(&p).unwrap();
        let plan =
            legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(&program).unwrap();
        assert_eq!(
            plan.primary.entries[1].availability,
            if frame.is_none() { R } else { O }
        );
    }
}

#[test]
fn owned_sequence_item_at_potential_parent_branches_keep_legacy_reads_eager() {
    let mut p = project(Producer::Tokenize);
    p.graph.nodes.insert(
        200,
        Node::SourceField {
            path: path(&["Legacy", "Cfg", "Text"]),
            frame: None,
        },
    );
    p.graph.nodes.insert(
        201,
        Node::If {
            condition: 9,
            then: 1,
            else_: 200,
        },
    );
    p.graph.nodes.insert(
        100,
        Node::SequenceItemAt {
            sequence: mapping::SequenceExpr::Tokenize {
                input: 201,
                delimiter: 2,
                item: 90,
            },
            index: 8,
        },
    );
    let program = lower_with_basic_owned_sequence_item_at(&p).unwrap();
    let plan =
        legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(&program).unwrap();
    assert_eq!(
        plan.primary.entries[1].availability,
        NamedInputAvailability::Optional
    );
    assert_eq!(
        plan.primary.entries[2].availability,
        NamedInputAvailability::RequiredLegacy
    );
}

#[test]
fn owned_sequence_item_at_target_demand_follows_every_hidden_producer_and_index_edge() {
    for producer in PRODUCERS {
        let roots = producer
            .sequence(90)
            .inputs()
            .into_iter()
            .chain([8])
            .collect::<Vec<_>>();
        for root in roots {
            let mut p = project(producer);
            p.graph.nodes.insert(
                200,
                Node::Const {
                    value: Value::String("unused.json".into()),
                },
            );
            p.extra_sources[3].dynamic_path = Some(mapping::DynamicSourcePath::legacy(200, vec![]));
            let Node::OwnedSourceField(original) = p.graph.nodes[&root].clone() else {
                panic!("owned input")
            };
            let mut dynamic_path = path(&["Dynamic"]);
            dynamic_path.extend(original.source.path);
            p.graph.nodes.insert(
                201,
                Node::SourceField {
                    path: dynamic_path,
                    frame: None,
                },
            );
            p.graph.nodes.insert(
                202,
                Node::ValueMap {
                    input: 201,
                    input_type: None,
                    table: vec![],
                    default: None,
                },
            );
            p.graph.nodes.insert(
                root,
                Node::RuntimeParameterDefault {
                    name: "hidden".into(),
                    ty: ScalarType::String,
                    default: 202,
                    preview: None,
                },
            );
            let error = lower_with_basic_owned_sequence_item_at(&p).unwrap_err();
            assert!(
                matches!(error.diagnostics(), [Diagnostic::OwnedSource { diagnostic }]
                if matches!(diagnostic.as_ref(), OwnedSourceDiagnostic::Capability {
                    location: OwnedSourceDiagnosticLocation::Expression { node:201, .. },
                    reason: OwnedSourceCapabilityReason::UnsupportedBasicContext,
                    origin: OwnedSourceCapabilityOrigin::RetainedExpression { node:201, .. }, ..
                })),
                "{producer:?} root={root}: {error:#?}"
            );
        }
    }
}

#[test]
fn owned_sequence_item_at_keeps_primary_row_failure_eligibility_independent() {
    let mut p = project(Producer::Tokenize);
    p.graph.nodes.insert(
        20,
        Node::OwnedSourceField(field(
            SourceOwner::Primary,
            &["Rows", "Keep"],
            Some(&["Rows"]),
        )),
    );
    p.graph.nodes.insert(
        21,
        Node::OwnedSourceField(field(
            SourceOwner::Primary,
            &["Rows", "Label"],
            Some(&["Rows"]),
        )),
    );
    p.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: path(&["Rows"]),
        },
        selection: mapping::FailureSelection::WhenTrue { predicate: 20 },
        message: Some(21),
    });
    let base = lower_with_basic_owned_sequence_item_at(&p).unwrap();
    assert!(is_basic_owned_primary_row_failure(
        &base,
        &base.failure_rules[0]
    ));
    assert_eq!(
        validate_basic_owned_sequence_item_at_emission(&base),
        Ok(())
    );
    for root in [100, 203] {
        let mut p = base.clone();
        p.expressions.push(ExpressionNode {
            id: 202,
            expression: Expression::Const {
                value: Value::Bool(false),
            },
        });
        p.expressions.push(ExpressionNode {
            id: 203,
            expression: Expression::If {
                condition: 202,
                then: 100,
                else_: 21,
            },
        });
        p.failure_rules[0].message = Some(root);
        assert!(!is_basic_owned_primary_row_failure(&p, &p.failure_rules[0]));
        let error = validate_basic_owned_sequence_item_at_emission(&p).unwrap_err();
        assert!(matches!(
            capability(&error, OwnedSourceCapabilityReason::FailureRule),
            OwnedSourceDiagnostic::Capability {
                origin: OwnedSourceCapabilityOrigin::Failure { index: 0 },
                ..
            }
        ));
    }
    let mut p = base;
    p.failure_rules[0].selection = FailureSelection::WhenTrue(100);
    assert!(!is_basic_owned_primary_row_failure(&p, &p.failure_rules[0]));
    capability(
        &validate_basic_owned_sequence_item_at_emission(&p).unwrap_err(),
        OwnedSourceCapabilityReason::FailureRule,
    );
}

#[test]
fn owned_sequence_item_at_direct_programs_keep_structure_and_owner_diagnostics_first() {
    let base = fixture(Producer::Range(true));
    let mut p = base.clone();
    *expression(&mut p, 8) = Expression::OwnedSourceField(field(
        SourceOwner::Named {
            name: "missing".into(),
        },
        &["Cfg", "Index"],
        Some(&[]),
    ));
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Structure {
            location: OwnedSourceDiagnosticLocation::Expression {
                namespace: OwnedSourceNamespace::Main,
                node: 8,
                scope: None,
            },
            request: OwnedSourceDiagnosticRequest::Field {
                owner: OwnedSourceDiagnosticOwner::Named {
                    name: "missing".into(),
                },
                path: path(&["Cfg", "Index"]),
                frame: Some(vec![]),
            },
            reason: OwnedSourceStructureReason::UnknownOwner,
        }),
    };
    assert_eq!(
        validate_basic_owned_sequence_item_at_emission(&p),
        Err(expected.clone())
    );
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(&p).unwrap_err(),
        expected
    );
    p.expressions.push(
        p.expressions
            .iter()
            .find(|node| node.id == 8)
            .unwrap()
            .clone(),
    );
    assert_eq!(
        validate_basic_owned_sequence_item_at_emission(&p),
        Err(ProgramValidationError::DuplicateExpression { node: 8 })
    );
    let mut p = base.clone();
    p.extra_sources.push(p.extra_sources[1].clone());
    assert_eq!(
        validate_basic_owned_sequence_item_at_emission(&p),
        Err(ProgramValidationError::DuplicateExtraSourceName {
            name: "Cfg".into(),
            first: 1,
            duplicate: 6
        })
    );
    let mut p = base.clone();
    *expression(&mut p, 8) =
        Expression::OwnedSourceField(field(named(), &["Cfg", "Index"], Some(&["Rows"])));
    assert!(
        matches!(validate_basic_owned_sequence_item_at_emission(&p), Err(ProgramValidationError::OwnedSource { diagnostic })
        if matches!(diagnostic.as_ref(), OwnedSourceDiagnostic::Structure { reason:OwnedSourceStructureReason::IncompatibleFramePath, .. }))
    );
    let mut p = base;
    p.expressions.retain(|node| node.id != 8);
    assert_eq!(
        validate_basic_owned_sequence_item_at_emission(&p),
        Err(ProgramValidationError::MissingDependency {
            node: 100,
            dependency: 8
        })
    );
}

#[test]
fn owned_sequence_item_at_preserves_cycle_priority_and_dynamic_owner_refusals() {
    let mut p = fixture(Producer::Range(true));
    *expression(&mut p, 8) = Expression::RuntimeParameterDefault {
        name: "cycle".into(),
        ty: ScalarType::Int,
        default: 100,
    };
    let expected = validate_program(&p).unwrap_err();
    assert!(matches!(
        expected,
        ProgramValidationError::ExpressionCycle { .. }
    ));
    assert_eq!(
        validate_basic_owned_sequence_item_at_emission(&p),
        Err(expected.clone())
    );
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(&p).unwrap_err(),
        expected
    );
    let mut p = fixture(Producer::Range(true));
    p.expressions.push(ExpressionNode {
        id: 200,
        expression: Expression::Const {
            value: Value::String("dynamic.json".into()),
        },
    });
    p.extra_sources[3].dynamic = Some(DynamicSourceProgram {
        path: 200,
        driver: SourceIteration::new(vec![]).into(),
    });
    *expression(&mut p, 8) = Expression::OwnedSourceField(field(
        SourceOwner::Named {
            name: "Dynamic".into(),
        },
        &["Cfg", "Index"],
        Some(&[]),
    ));
    let error = validate_basic_owned_sequence_item_at_emission(&p).unwrap_err();
    assert!(
        matches!(capability(&error, OwnedSourceCapabilityReason::DynamicOwner), OwnedSourceDiagnostic::Capability {
        location:OwnedSourceDiagnosticLocation::Expression { node:8, .. },
        origin:OwnedSourceCapabilityOrigin::DynamicSource { index:3, name }, ..
    } if name == "Dynamic")
    );
}

#[test]
fn owned_sequence_item_at_retains_own_and_foreign_item_lexical_refusals() {
    let base = fixture(Producer::Range(true));
    let mut p = base.clone();
    let Expression::SequenceItemAt { index, .. } = expression(&mut p, 100) else {
        panic!("consumer")
    };
    *index = 90;
    assert!(matches!(
        validate_basic_owned_sequence_item_at_emission(&p),
        Err(ProgramValidationError::SequenceItemOutOfContext {
            owner: SequenceOwner::Expression(100),
            item: 90,
            ..
        })
    ));
    let mut p = base.clone();
    p.expressions.push(ExpressionNode {
        id: 91,
        expression: Expression::SourceField {
            path: vec![],
            frame: None,
        },
    });
    p.root.iteration = Some(IterationPlan::generated(GeneratedSequence::Range {
        from: None,
        to: 7,
        item: 91,
    }));
    let Expression::SequenceItemAt { index, .. } = expression(&mut p, 100) else {
        panic!("consumer")
    };
    *index = 91;
    assert!(matches!(
        validate_basic_owned_sequence_item_at_emission(&p),
        Err(ProgramValidationError::SequenceItemOutOfContext {
            owner: SequenceOwner::Expression(100),
            item: 91,
            ..
        })
    ));
    let mut p = base;
    *expression(&mut p, 90) =
        Expression::OwnedSourceField(field(named(), &["Cfg", "Index"], Some(&[])));
    assert_eq!(
        validate_basic_owned_sequence_item_at_emission(&p),
        Err(ProgramValidationError::InvalidSequenceItem {
            owner: SequenceOwner::Expression(100),
            expression: 90
        })
    );
}

#[test]
fn owned_sequence_item_at_does_not_erase_inherited_closed_expression_contexts() {
    for outer in [
        Expression::Aggregate {
            function: AggregateFunction::Count,
            collection: path(&["Rows"]),
            value: AggregateValue::Expression(100),
            arg: None,
        },
        Expression::Lookup {
            collection: path(&["Rows"]),
            key: path(&["Key"]),
            matches: 100,
            value: path(&["Label"]),
        },
        Expression::SequenceExists {
            sequence: GeneratedSequence::Range {
                from: None,
                to: 7,
                item: 91,
            },
            predicate: 100,
        },
        Expression::SequenceAggregate {
            function: AggregateFunction::ItemAt,
            sequence: GeneratedSequence::Range {
                from: None,
                to: 7,
                item: 91,
            },
            predicate: None,
            expression: None,
            arg: Some(100),
        },
    ] {
        let reason = if matches!(
            outer,
            Expression::Aggregate { .. } | Expression::Lookup { .. }
        ) {
            OwnedSourceCapabilityReason::AggregateLookup
        } else {
            OwnedSourceCapabilityReason::GeneratedItem
        };
        let mut p = fixture(Producer::Range(true));
        p.expressions.push(ExpressionNode {
            id: 91,
            expression: Expression::SourceField {
                path: vec![],
                frame: None,
            },
        });
        p.expressions.push(ExpressionNode {
            id: 101,
            expression: outer,
        });
        p.root.bindings[0].expression = 101;
        let error = validate_basic_owned_sequence_item_at_emission(&p).unwrap_err();
        assert!(matches!(
            capability(&error, reason),
            OwnedSourceDiagnostic::Capability {
                origin: OwnedSourceCapabilityOrigin::Expression { node: 101, .. },
                ..
            }
        ));
    }
}

#[test]
fn owned_sequence_item_at_does_not_open_generated_or_non_by_scope_ancestry() {
    let mut p = fixture(Producer::Range(true));
    p.expressions.push(ExpressionNode {
        id: 91,
        expression: Expression::SourceField {
            path: vec![],
            frame: None,
        },
    });
    p.root.iteration = Some(IterationPlan::generated(GeneratedSequence::Range {
        from: None,
        to: 7,
        item: 91,
    }));
    let error = validate_basic_owned_sequence_item_at_emission(&p).unwrap_err();
    assert!(
        matches!(capability(&error, OwnedSourceCapabilityReason::GeneratedItem), OwnedSourceDiagnostic::Capability { origin:OwnedSourceCapabilityOrigin::Scope { scope }, .. } if *scope == primary())
    );
    let mut p = fixture(Producer::Range(true));
    p.target.repeating = true;
    p.root.repeating = true;
    p.root.iteration = Some(
        IterationPlan::owned_source(OwnedSourceRef {
            owner: named(),
            path: path(&["Rows"]),
        })
        .with_grouping(GroupingPlan::AdjacentBy { key: 100 }),
    );
    capability(
        &validate_basic_owned_sequence_item_at_emission(&p).unwrap_err(),
        OwnedSourceCapabilityReason::Grouping,
    );
}

#[test]
fn owned_sequence_item_at_keeps_join_and_document_boundaries_closed() {
    let mut p = fixture(Producer::Range(true));
    let SchemaKind::Group { children, .. } = &mut p.source.kind else {
        panic!("source group")
    };
    for name in ["A", "B"] {
        children.push(
            SchemaNode::group(name, vec![SchemaNode::scalar("Key", ScalarType::Int)]).repeating(),
        );
    }
    let plan = JoinPlan::new(
        JoinSource::new(path(&["A"])),
        JoinSource::new(path(&["B"])),
        JoinConditions::new(JoinKey::new(path(&["A"]), path(&["Key"]), path(&["Key"]))),
    )
    .unwrap();
    p.expressions.push(ExpressionNode {
        id: 101,
        expression: Expression::JoinAggregate {
            function: AggregateFunction::Count,
            join: InnerJoin::new(JoinId::new(17), plan),
            expression: Some(100),
            arg: None,
        },
    });
    p.root.bindings[0].expression = 101;
    let error = validate_basic_owned_sequence_item_at_emission(&p).unwrap_err();
    assert!(matches!(
        capability(&error, OwnedSourceCapabilityReason::Join),
        OwnedSourceDiagnostic::Capability {
            origin: OwnedSourceCapabilityOrigin::Expression { node: 101, .. },
            ..
        }
    ));
    let mut p = fixture(Producer::Range(true));
    *expression(&mut p, 8) = Expression::SourceDocumentPath;
    capability(
        &validate_basic_owned_sequence_item_at_emission(&p).unwrap_err(),
        OwnedSourceCapabilityReason::Document,
    );
}

#[test]
fn owned_sequence_item_at_same_ids_in_isolated_function_namespace_are_not_target_permissions() {
    let mut p = fixture(Producer::Range(true));
    let function = mapping::FunctionId::new(19);
    p.user_functions.push(UserFunctionProgram {
        id: function,
        library: "tests".into(),
        name: "isolated".into(),
        parameters: vec![],
        output_type: ScalarType::Int,
        expressions: vec![
            ExpressionNode {
                id: 8,
                expression: Expression::OwnedSourceField(field(
                    named(),
                    &["Cfg", "Index"],
                    Some(&[]),
                )),
            },
            ExpressionNode {
                id: 7,
                expression: Expression::Const {
                    value: Value::Int(3),
                },
            },
            ExpressionNode {
                id: 100,
                // Keep this isolated body's existing scalar structure valid:
                // ItemAt itself is not an admitted UDF expression. Reused IDs
                // must still isolate its owned leaf from the target's index.
                expression: Expression::Call {
                    function: ScalarFunction::Add,
                    args: vec![8, 7],
                },
            },
        ],
        output: 100,
    });
    let error = validate_basic_owned_sequence_item_at_emission(&p).unwrap_err();
    assert!(
        matches!(capability(&error, OwnedSourceCapabilityReason::IsolatedFunction), OwnedSourceDiagnostic::Capability {
        location:OwnedSourceDiagnosticLocation::Expression { namespace:OwnedSourceNamespace::Function(id), node:8, scope:None },
        origin:OwnedSourceCapabilityOrigin::FunctionBody { function:origin }, ..
    } if *id == function && *origin == function)
    );
}

#[test]
fn owned_sequence_item_at_reused_target_consumer_retains_exact_named_scope_origin() {
    let mut p = fixture(Producer::Range(true));
    let route = mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Named {
        index: 0,
        name: " exact 💠 ".into(),
    });
    let mut root = p.root.clone();
    root.repeating = true;
    root.iteration = Some(
        IterationPlan::owned_source(OwnedSourceRef {
            owner: named(),
            path: path(&["Rows"]),
        })
        .with_grouping(GroupingPlan::EndingWith { predicate: 100 }),
    );
    let mut target = p.target.clone();
    target.repeating = true;
    p.extra_targets.push(NamedTargetProgram {
        name: " exact 💠 ".into(),
        target,
        root,
    });
    let error = validate_basic_owned_sequence_item_at_emission(&p).unwrap_err();
    assert!(
        matches!(capability(&error, OwnedSourceCapabilityReason::Grouping), OwnedSourceDiagnostic::Capability {
        location:OwnedSourceDiagnosticLocation::Iteration { scope },
        origin:OwnedSourceCapabilityOrigin::Scope { scope:origin }, ..
    } if *scope == route && *origin == route)
    );
}

#[test]
fn owned_sequence_item_at_recursive_producer_remains_closed_after_valid_legacy_lowering() {
    let mut p = project(Producer::Tokenize);
    let SchemaKind::Group { children, .. } = &mut p.source.kind else {
        panic!("group")
    };
    children.push(SchemaNode::group(
        "Tree",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::group(
                "Values",
                vec![SchemaNode::scalar("Name", ScalarType::String)],
            )
            .repeating(),
            SchemaNode::recursive_group("Children", "Tree").repeating(),
        ],
    ));
    p.graph.nodes.insert(
        10,
        Node::Const {
            value: Value::String(String::new()),
        },
    );
    p.graph.nodes.insert(
        11,
        Node::Const {
            value: Value::String("/".into()),
        },
    );
    p.graph.nodes.insert(
        8,
        Node::Const {
            value: Value::Int(2),
        },
    );
    p.graph.nodes.insert(
        100,
        Node::SequenceItemAt {
            sequence: mapping::SequenceExpr::RecursiveCollect {
                collection: path(&["Tree"]),
                children: path(&["Children"]),
                descent_value: path(&["Name"]),
                values: path(&["Values"]),
                value: path(&["Name"]),
                prefix: 10,
                separator: 11,
                item: 90,
            },
            index: 8,
        },
    );
    let mut program =
        lower(&p).expect("ordinary recursive producer is independently structurally valid");
    assert_eq!(validate_program(&program), Ok(()));
    *expression(&mut program, 8) =
        Expression::OwnedSourceField(field(named(), &["Cfg", "Index"], Some(&[])));
    let error = validate_basic_owned_sequence_item_at_emission(&program).unwrap_err();
    capability(&error, OwnedSourceCapabilityReason::GeneratedItem);
}

#[test]
fn owned_sequence_item_at_filter_map_and_isolated_functions_remain_closed() {
    use mapping::{FunctionId, FunctionParameterId};
    let mut p = fixture(Producer::Range(false));
    p.expressions.push(ExpressionNode {
        id: 91,
        expression: Expression::SourceField {
            path: vec![],
            frame: None,
        },
    });
    for (id, ty, value) in [
        (
            1,
            ScalarType::Bool,
            Expression::Const {
                value: Value::Bool(true),
            },
        ),
        (
            2,
            ScalarType::Int,
            Expression::FunctionParameter {
                parameter: FunctionParameterId::new(1),
            },
        ),
    ] {
        p.user_functions.push(UserFunctionProgram {
            id: FunctionId::new(id),
            library: "tests".into(),
            name: format!("stage{id}"),
            parameters: vec![
                UserFunctionParameter {
                    id: FunctionParameterId::new(1),
                    ty: ScalarType::Int,
                },
                UserFunctionParameter {
                    id: FunctionParameterId::new(2),
                    ty: ScalarType::Int,
                },
            ],
            output_type: ty,
            expressions: vec![ExpressionNode {
                id: 0,
                expression: value,
            }],
            output: 0,
        });
    }
    *expression(&mut p, 100) = Expression::SequenceItemAt {
        sequence: GeneratedSequence::FilterMapV1(GeneratedFilterMapV1 {
            source: GeneratedRange {
                from: None,
                to: 7,
                item: 90,
            },
            item: 91,
            predicate: FunctionId::new(1),
            mapper: FunctionId::new(2),
            output_type: ScalarType::Int,
            captures: vec![],
        }),
        index: 8,
    };
    let mut legacy = p.clone();
    for node in &mut legacy.expressions {
        if matches!(
            node.expression,
            Expression::OwnedSourceField(_) | Expression::OwnedPosition(_)
        ) {
            node.expression = Expression::Const {
                value: Value::Int(2),
            };
        }
    }
    assert_eq!(
        validate_program(&legacy),
        Ok(()),
        "ordinary FilterMap structure must be valid before testing ownership"
    );
    capability(
        &validate_basic_owned_sequence_item_at_emission(&p).unwrap_err(),
        OwnedSourceCapabilityReason::GeneratedItem,
    );
    // Removing the composition must not turn its functions into a newly admitted
    // Basic feature, even when their isolated bodies contain no owned leaves.
    *expression(&mut p, 100) = Expression::SequenceItemAt {
        sequence: GeneratedSequence::Range {
            from: None,
            to: 7,
            item: 90,
        },
        index: 8,
    };
    capability(
        &validate_basic_owned_sequence_item_at_emission(&p).unwrap_err(),
        OwnedSourceCapabilityReason::IsolatedFunction,
    );
}

#[test]
fn owned_sequence_item_at_legacy_wire_and_control_free_companion_results_stay_unchanged() {
    let mut p = project(Producer::Tokenize);
    for (id, value) in [
        (1, Value::String("a|b|c".into())),
        (2, Value::String("|".into())),
        (8, Value::Int(2)),
    ] {
        p.graph.nodes.insert(id, Node::Const { value });
    }
    p.graph
        .nodes
        .retain(|id, _| [1, 2, 8, 90, 100].contains(id));
    let wire = serde_json::to_string(&p).unwrap();
    assert!(!wire.contains("owned_source"));
    let restored: mapping::Project = serde_json::from_str(&wire).unwrap();
    assert_eq!(serde_json::to_string(&restored).unwrap(), wire);
    let legacy = lower(&restored).unwrap();
    assert_eq!(
        lower_with_basic_owned_sequence_item_at(&restored),
        Ok(legacy.clone())
    );
    assert_eq!(validate_program(&legacy), Ok(()));
    p.root.bindings[0].node = 8;
    p.graph.nodes.insert(
        8,
        Node::OwnedSourceField(field(named(), &["Cfg", "Index"], Some(&[]))),
    );
    let old = lower_with_basic_owned_group_by(&p).unwrap();
    assert_eq!(lower_with_basic_owned_sequence_item_at(&p), Ok(old.clone()));
    assert_eq!(
        legacy_named_input_admission_plan_with_basic_owned_group_by(&old).unwrap(),
        legacy_named_input_admission_plan_with_basic_owned_sequence_item_at(&old).unwrap()
    );
}
