use super::*;

pub(super) fn sort668_program(
    owner: SourceOwner,
    order: SortFilterOrder,
    predicate: Option<u32>,
) -> Program {
    let mut p = filter644_program(owner);
    sort668_iteration(&mut p, order, predicate, 1, Vec::new());
    p
}
pub(super) fn sort668_iteration(
    p: &mut Program,
    order: SortFilterOrder,
    predicate: Option<u32>,
    primary: u32,
    secondary: Vec<codegen::SortKey>,
) {
    let input = p.root.iteration.as_ref().unwrap().input().clone();
    p.root.iteration = Some(IterationPlan::new(
        input,
        predicate,
        Some(codegen::SortPlan::new(
            codegen::SortKey {
                expression: primary,
                descending: false,
            },
            secondary,
            order,
        )),
        Vec::new(),
        IterationOutput::Repeated,
    ));
}
pub(super) fn sort668_result(
    label: &str,
    p: &Program,
    options: &Options,
) -> Result<ArtifactSet, EmitError> {
    eprintln!("INPUT S668 {label} complete_program={p:#?} complete_options={options:#?}");
    let result = emit_with_basic_owned_sort(p, options);
    eprintln!("RAW S668 {label} complete_artifacts_or_error={result:#?}");
    if let Err(error) = &result {
        eprintln!(
            "RAW S668 {label} cause={:?}",
            error.source().map(ToString::to_string)
        );
    }
    result
}
pub(super) fn sort668_scope_error(
    reason: codegen::OwnedSourceCapabilityReason,
) -> ProgramValidationError {
    let route = mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary);
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Iteration {
                scope: route.clone(),
            },
            request: Some(codegen::OwnedSourceDiagnosticRequest::Iteration {
                owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                path: path(&["Rows"]),
            }),
            reason,
            origin: codegen::OwnedSourceCapabilityOrigin::Scope { scope: route },
        }),
    }
}

#[test]
fn public_basic_sort_preserves_complete_control_free_and_filter_only_artifact_bytes() {
    for p in [
        public519_outputs_program(),
        root_pin632_rows_program(),
        public582_lazy_program(),
    ] {
        let options = public519_options("public_sort_control_free_668");
        let baseline = public519_result("S668 control-free baseline", &p, &options).unwrap();
        let sorted = sort668_result("control-free sort opt-in", &p, &options).unwrap();
        assert_control_free_typed_artifacts_match(&p, &baseline, &sorted);
    }
    for owner in [SourceOwner::Primary, named("Ref")] {
        let p = filter644_program(owner);
        let options = public519_options("public_sort_filter_only_668");
        let baseline = filter644_result("S668 filter-only baseline", &p, &options).unwrap();
        let sorted = sort668_result("filter-only sort opt-in", &p, &options).unwrap();
        assert_eq!(baseline.files(), sorted.files());
    }
}

#[test]
fn public_basic_sort_source_retains_whole_operation_ordered_keys_and_unsigned_leaf_annotations() {
    for order in [
        SortFilterOrder::FilterThenSort,
        SortFilterOrder::SortThenFilter,
    ] {
        let p = sort668_program(SourceOwner::Primary, order, Some(4));
        let files = sort668_result(
            "exact complete sorting scope",
            &p,
            &public519_options("public_sort_source_668"),
        )
        .unwrap();
        let source = public519_source(&files);
        let start = source.find("fn scope_root(").unwrap();
        let end = source[start..].find("\n}\n").unwrap() + start + "\n}\n".len();
        let expected = format!(
            "fn scope_root(context: &BasicScopeContext<'_>) -> Result<Instance, RuntimeError> {{\n    let cohort = context.walk_owned_source_cohort(SourceOwnerRef::Primary, &[\"Rows\"])?;\n    let mut outputs = Vec::new();\n    context.visit_sorted_source_cohort(\n        &cohort,\n        [codegen_runtime::SortDirection::Ascending,],\n        {},\n        |predicate_context| -> Result<bool, RuntimeError> {{\n            let value = expression_4(predicate_context)?;\n            require_bool(4, value)\n        }},\n        |key_context| -> Result<[Value; 1], RuntimeError> {{\n            let key_0 = expression_1(key_context)?;\n            Ok([key_0,])\n        }},\n        |item_context| -> Result<(), RuntimeError> {{\n            let value_0 = adapt_target_value(expression_1(item_context)?, ScalarType::Int);\n            let value_1 = adapt_target_value(expression_2(item_context)?, ScalarType::Int);\n            let value_2 = adapt_target_value(expression_5(item_context)?, ScalarType::String);\n            let output = group([\n                field(\"Value\", scalar(value_0)),\n                field(\"Position\", scalar(value_1)),\n                field(\"Root\", scalar(value_2)),\n            ]);\n            if outputs.capacity() == 0 {{ outputs = Vec::with_capacity(cohort.len()); }}\n            outputs.push(output);\n            Ok(())\n        }},\n    )?;\n    Ok(repeated(outputs))\n}}\n",
            order == SortFilterOrder::FilterThenSort
        );
        assert_eq!(&source[start..end], expected);
        assert_eq!(
            source
                .matches("context.visit_sorted_source_cohort(")
                .count(),
            1
        );
        assert!(!source.contains("context.reindex_phase("));
        assert!(source.contains("execute_selected_checked_private_owned_basic(source, supplied, execution, limits, selection)"));
    }
    for id in [0, u32::MAX] {
        let mut p = sort668_program(SourceOwner::Primary, SortFilterOrder::SortThenFilter, None);
        p.expressions.push(node(
            id,
            owned(named("Ref"), &["Rows", "Value"], Some(&["Rows"])),
        ));
        sort668_iteration(
            &mut p,
            SortFilterOrder::SortThenFilter,
            None,
            1,
            vec![codegen::SortKey {
                expression: id,
                descending: true,
            }],
        );
        let files = sort668_result(
            "unsigned ordered secondary key leaf",
            &p,
            &public519_options("public_sort_unsigned_668"),
        )
        .unwrap();
        let source = public519_source(&files);
        assert!(source.contains("[codegen_runtime::SortDirection::Ascending,codegen_runtime::SortDirection::Descending,]"));
        assert!(source.contains(&format!("let key_0 = expression_1(key_context)?;\n            let key_1 = expression_{id}(key_context)?;\n            Ok([key_0,key_1,])")));
        assert!(public582_map_body(source, id).contains(&format!(".with_owned_source_node({id})")));
    }
    let project = sort668_scalar_project();
    let p = sort668_scalar_program();
    assert_eq!(
        p.target,
        SchemaNode::scalar("Output", ScalarType::Int).repeating()
    );
    assert_eq!(
        p.root.construction,
        TargetConstruction::Scalar {
            expression: 1,
            target_domain: ScalarTargetDomain::Single(ScalarType::Int),
        }
    );
    let files = sort668_result(
        "genuine sorted Scalar target",
        &p,
        &public519_options("public_sort_scalar_668"),
    )
    .unwrap();
    assert!(public519_source(&files).contains(
        "let output = scalar(adapt_target_value(expression_1(item_context)?, ScalarType::Int));"
    ));
    let input = ir::Instance::Group(
        vec![
            (
                "Value".into(),
                ir::Instance::Scalar(Value::String("P.root".into())),
            ),
            (
                "Ref".into(),
                ir::Instance::Group(
                    vec![(
                        "Value".into(),
                        ir::Instance::Scalar(Value::String("primary-collision".into())),
                    )]
                    .into(),
                ),
            ),
            (
                "Rows".into(),
                ir::Instance::Repeated(
                    [30, 10, 20]
                        .into_iter()
                        .map(|value| {
                            ir::Instance::Group(
                                vec![
                                    ("Value".into(), ir::Instance::Scalar(Value::Int(value))),
                                    ("Flag".into(), ir::Instance::Scalar(Value::Bool(true))),
                                    ("Rows".into(), ir::Instance::Repeated(Vec::new())),
                                ]
                                .into(),
                            )
                        })
                        .collect(),
                ),
            ),
        ]
        .into(),
    );
    let expected = ir::Instance::Repeated(
        [10, 20, 30]
            .into_iter()
            .map(|value| ir::Instance::Scalar(Value::Int(value)))
            .collect(),
    );
    let all = engine::run_outputs(&project, &input);
    let selected = engine::run_selected_target(&project, &input, engine::TargetSelection::Primary);
    eprintln!("INPUT S668 Scalar complete_project={project:#?} complete_input={input:#?}");
    eprintln!(
        "RAW S668 Scalar complete_native_all={all:#?} complete_native_selected={selected:#?} complete_expected={expected:#?}"
    );
    let Ok(all) = all else {
        panic!("complete retained native all-target result unexpectedly failed: {all:#?}");
    };
    assert_eq!(all.primary, expected.clone());
    assert_eq!(all.extras, Vec::new());
    assert_eq!(
        selected,
        Ok(engine::SelectedTargetOutput::Primary(expected))
    );
}

#[test]
fn public_basic_sort_catalogues_retain_potential_key_and_predicate_input_demands() {
    for legacy_key in [true, false] {
        let mut p = sort668_program(
            SourceOwner::Primary,
            SortFilterOrder::FilterThenSort,
            Some(4),
        );
        p.expressions.extend([
            node(
                6,
                Expression::SourceField {
                    path: path(&["Ref", "Rows", if legacy_key { "Value" } else { "Flag" }]),
                    frame: None,
                },
            ),
            node(
                7,
                owned(named("Unused"), &["Rows", "Value"], Some(&["Rows"])),
            ),
            node(
                8,
                Expression::Const {
                    value: Value::Bool(false),
                },
            ),
            node(
                9,
                Expression::If {
                    condition: 8,
                    then: 7,
                    else_: 1,
                },
            ),
        ]);
        sort668_iteration(
            &mut p,
            SortFilterOrder::FilterThenSort,
            Some(if legacy_key { 4 } else { 6 }),
            if legacy_key { 6 } else { 1 },
            vec![codegen::SortKey {
                expression: 9,
                descending: true,
            }],
        );
        let plan = codegen::legacy_named_input_admission_plan_with_basic_owned_sort(&p).unwrap();
        eprintln!("RAW S668 complete key/predicate catalogue={plan:#?}");
        let availability = |name: &str| {
            plan.primary
                .entries
                .iter()
                .find(|entry| entry.name == name)
                .unwrap()
                .availability
        };
        assert_eq!(
            availability("Ref"),
            codegen::NamedInputAvailability::RequiredLegacy
        );
        assert_eq!(
            availability("Unused"),
            codegen::NamedInputAvailability::Optional
        );
        let files = sort668_result(
            "all potential keys retained lazily",
            &p,
            &public519_options("public_sort_catalogue_668"),
        )
        .unwrap();
        assert!(public519_source(&files).contains("fn expression_7("));
        assert!(public582_map_body(public519_source(&files), 9).contains("expression_7(context)"));
    }
    let p = sort668_program(named("Ref"), SortFilterOrder::SortThenFilter, Some(4));
    let plan = codegen::legacy_named_input_admission_plan_with_basic_owned_sort(&p).unwrap();
    assert_eq!(
        plan.primary
            .entries
            .iter()
            .find(|entry| entry.name == "Ref")
            .unwrap()
            .availability,
        codegen::NamedInputAvailability::Optional
    );
}

#[test]
fn public_basic_sort_keeps_windows_grouping_special_construction_and_functions_closed() {
    for order in [
        SortFilterOrder::FilterThenSort,
        SortFilterOrder::SortThenFilter,
    ] {
        for control in 0..9 {
            let mut p = sort668_program(SourceOwner::Primary, order, Some(4));
            let old = p.root.iteration.as_ref().unwrap();
            let windows = match control {
                0 => vec![SequenceWindow::SkipFirst { count: 3 }],
                1 => vec![SequenceWindow::First { count: 3 }],
                2 => vec![SequenceWindow::From { position: 3 }],
                3 => vec![SequenceWindow::FromTo { first: 3, last: 3 }],
                4 => vec![SequenceWindow::Last { count: 3 }],
                _ => Vec::new(),
            };
            let output = if control == 5 {
                IterationOutput::First
            } else {
                IterationOutput::Repeated
            };
            let mut iteration = IterationPlan::new(
                old.input().clone(),
                old.filter(),
                old.sort().cloned(),
                windows,
                output,
            );
            if control == 6 {
                iteration = iteration.with_grouping(GroupingPlan::By { key: 1 });
            }
            if control == 7 {
                iteration = iteration.with_filtered_grouping(GroupingPlan::By { key: 1 }, 4);
            }
            p.root.iteration = Some(iteration);
            if control == 5 {
                p.root.repeating = false;
                p.target.repeating = false;
            }
            if control == 8 {
                p.root.construction = TargetConstruction::CopyCurrentSource;
                let ir::SchemaKind::Group { children, .. } = &p.source.kind else {
                    unreachable!("fixture source is a complete Group")
                };
                p.target.kind = children
                    .iter()
                    .find(|child| child.name == "Rows")
                    .unwrap()
                    .kind
                    .clone();
                p.root.bindings.clear();
                p.root.children.clear();
            }
            let reason = if (6..=7).contains(&control) {
                codegen::OwnedSourceCapabilityReason::Grouping
            } else {
                codegen::OwnedSourceCapabilityReason::ScopeControl
            };
            filter644_invalid(
                sort668_result(
                    "complete closed scope envelope",
                    &p,
                    &public519_options("public_sort_closed_668"),
                ),
                sort668_scope_error(reason),
            );
        }
    }
    let mut special = sort668_program(SourceOwner::Primary, SortFilterOrder::FilterThenSort, None);
    let mut child = scope(&[]);
    child.target_field = "Copy".into();
    child.construction = TargetConstruction::CopyCurrentSource;
    special.root.children.push(child);
    special.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Value", ScalarType::Int),
            SchemaNode::scalar("Position", ScalarType::Int),
            SchemaNode::scalar("Root", ScalarType::String),
            SchemaNode::group("Copy", Vec::new()),
        ],
    )
    .repeating();
    let ir::SchemaKind::Group {
        children: source_fields,
        ..
    } = &special.source.kind
    else {
        unreachable!("fixture source is Group")
    };
    let ir::SchemaKind::Group {
        children: target_fields,
        ..
    } = &mut special.target.kind
    else {
        unreachable!("fixture target is Group")
    };
    target_fields
        .iter_mut()
        .find(|field| field.name == "Copy")
        .unwrap()
        .kind = source_fields
        .iter()
        .find(|field| field.name == "Rows")
        .unwrap()
        .kind
        .clone();
    let route = mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
        .descendant(mapping::SourceScopeStep::Child(0));
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Iteration {
                scope: route.clone(),
            },
            request: Some(codegen::OwnedSourceDiagnosticRequest::Iteration {
                owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                path: path(&["Rows"]),
            }),
            reason: codegen::OwnedSourceCapabilityReason::SpecialConstruction,
            origin: codegen::OwnedSourceCapabilityOrigin::Scope { scope: route },
        }),
    };
    filter644_invalid(
        sort668_result(
            "inherited owned scope keeps exact special child closure",
            &special,
            &public519_options("public_sort_special_668"),
        ),
        expected,
    );
    let mut p = sort668_program(SourceOwner::Primary, SortFilterOrder::FilterThenSort, None);
    p.user_functions.push(UserFunctionProgram {
        id: FunctionId::new(668),
        library: "sort_fixture".into(),
        name: "isolated".into(),
        parameters: Vec::new(),
        output_type: ScalarType::Int,
        expressions: vec![node(
            0,
            Expression::Const {
                value: Value::Int(7),
            },
        )],
        output: 0,
    });
    p.expressions.push(node(
        70,
        Expression::UserFunctionCall {
            function: FunctionId::new(668),
            args: Vec::new(),
        },
    ));
    sort668_iteration(
        &mut p,
        SortFilterOrder::FilterThenSort,
        None,
        70,
        Vec::new(),
    );
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 70,
                scope: Some(mapping::SourceScopeLocation::root(
                    mapping::SourceScopeTarget::Primary,
                )),
            },
            request: None,
            reason: codegen::OwnedSourceCapabilityReason::IsolatedFunction,
            origin: codegen::OwnedSourceCapabilityOrigin::FunctionCall {
                caller_namespace: codegen::OwnedSourceNamespace::Main,
                call: 70,
                callee: FunctionId::new(668),
            },
        }),
    };
    filter644_invalid(
        sort668_result(
            "actual key function remains isolated",
            &p,
            &public519_options("public_sort_function_668"),
        ),
        expected,
    );
}

fn sort668_multikey_program() -> Program {
    let mut p = sort668_program(SourceOwner::Primary, SortFilterOrder::FilterThenSort, None);
    p.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Value", ScalarType::Int),
                    SchemaNode::scalar("Flag", ScalarType::Bool),
                    SchemaNode::scalar("Tag", ScalarType::Int),
                    SchemaNode::scalar("ID", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    );
    p.expressions.extend([
        node(
            6,
            owned(SourceOwner::Primary, &["Rows", "Tag"], Some(&["Rows"])),
        ),
        node(
            7,
            owned(SourceOwner::Primary, &["Rows", "ID"], Some(&["Rows"])),
        ),
    ]);
    p.root = scope(&[
        ("Value", 1, ScalarType::Int),
        ("Tag", 6, ScalarType::Int),
        ("ID", 7, ScalarType::Int),
        ("Position", 2, ScalarType::Int),
        ("Root", 5, ScalarType::String),
    ]);
    p.root.repeating = true;
    p.root.iteration = Some(IterationPlan::new(
        codegen::OwnedSourceIteration::new(OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows"]),
        }),
        None,
        Some(codegen::SortPlan::new(
            codegen::SortKey {
                expression: 1,
                descending: false,
            },
            vec![codegen::SortKey {
                expression: 6,
                descending: true,
            }],
            SortFilterOrder::FilterThenSort,
        )),
        Vec::new(),
        IterationOutput::Repeated,
    ));
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Value", ScalarType::Int),
            SchemaNode::scalar("Tag", ScalarType::Int),
            SchemaNode::scalar("ID", ScalarType::Int),
            SchemaNode::scalar("Position", ScalarType::Int),
            SchemaNode::scalar("Root", ScalarType::String),
        ],
    )
    .repeating();
    p
}
fn sort668_order_program() -> Program {
    let mut p = sort668_program(SourceOwner::Primary, SortFilterOrder::SortThenFilter, None);
    p.expressions.extend([
        node(
            40,
            Expression::Const {
                value: Value::String("target-first".into()),
            },
        ),
        node(41, Expression::Raise { message: Some(40) }),
        node(42, Expression::Raise { message: Some(44) }),
        node(43, Expression::Raise { message: Some(45) }),
        node(
            44,
            Expression::Const {
                value: Value::String("predicate-before-key".into()),
            },
        ),
        node(
            45,
            Expression::Const {
                value: Value::String("key-later".into()),
            },
        ),
    ]);
    p.root.bindings[0].expression = 41;
    let mut extra = scope(&[("Value", 41, ScalarType::Int)]);
    extra.repeating = true;
    extra.iteration = Some(IterationPlan::new(
        codegen::OwnedSourceIteration::new(OwnedSourceRef {
            owner: named("Ref"),
            path: path(&["Rows"]),
        }),
        Some(42),
        Some(codegen::SortPlan::new(
            codegen::SortKey {
                expression: 43,
                descending: false,
            },
            Vec::new(),
            SortFilterOrder::FilterThenSort,
        )),
        Vec::new(),
        IterationOutput::Repeated,
    ));
    p.extra_targets.push(NamedTargetProgram {
        name: "Named".into(),
        target: SchemaNode::group("Output", vec![SchemaNode::scalar("Value", ScalarType::Int)])
            .repeating(),
        root: extra,
    });
    p
}
fn sort668_leaf_program() -> Program {
    let mut p = sort668_program(SourceOwner::Primary, SortFilterOrder::SortThenFilter, None);
    p.expressions.extend([
        node(0, position(SourceOwner::Primary, Some(&["Rows", "Rows"]))),
        node(6, owned(named("Ref"), &["Rows", "Value"], Some(&["Rows"]))),
        node(u32::MAX, position(named("Ref"), Some(&["Rows", "Rows"]))),
    ]);
    p.root.bindings = vec![Binding {
        target_field: "Value".into(),
        expression: 0,
        target_domain: ScalarTargetDomain::Single(ScalarType::Int),
        repeating: false,
    }];
    p.target =
        SchemaNode::group("Output", vec![SchemaNode::scalar("Value", ScalarType::Int)]).repeating();
    let mut extra = scope(&[("Value", u32::MAX, ScalarType::Int)]);
    extra.repeating = true;
    extra.iteration = Some(IterationPlan::new(
        codegen::OwnedSourceIteration::new(OwnedSourceRef {
            owner: named("Ref"),
            path: path(&["Rows"]),
        }),
        None,
        Some(codegen::SortPlan::new(
            codegen::SortKey {
                expression: 6,
                descending: false,
            },
            Vec::new(),
            SortFilterOrder::SortThenFilter,
        )),
        Vec::new(),
        IterationOutput::Repeated,
    ));
    p.extra_targets.push(NamedTargetProgram {
        name: "Named".into(),
        target: p.target.clone(),
        root: extra,
    });
    p
}
fn sort668_scalar_project() -> mapping::Project {
    mapping::Project {
        execution_contract: None,
        source: filter644_schema(),
        target: SchemaNode::scalar("Output", ScalarType::Int).repeating(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: mapping::Graph {
            nodes: std::collections::BTreeMap::from([(
                1,
                mapping::Node::OwnedSourceField(OwnedSourceField {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Primary,
                        path: path(&["Rows", "Value"]),
                    },
                    frame: Some(path(&["Rows"])),
                }),
            )]),
        },
        root: mapping::Scope {
            iteration: mapping::ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: path(&["Rows"]),
                },
                output_path: None,
            },
            construction: mapping::ScopeConstruction::Scalar { value: 1 },
            sort_by: Some(1),
            sort_filter_order: mapping::SortFilterOrder::FilterThenSort,
            ..Default::default()
        },
    }
}
pub(super) fn sort668_scalar_program() -> Program {
    codegen::lower_with_basic_owned_sort(&sort668_scalar_project())
        .expect("genuine native Scalar project passes explicit sorted lowering")
}
pub(super) fn sort668_host_programs() -> Vec<(&'static str, Program, &'static str)> {
    vec![
        (
            "primary_fts",
            sort668_program(
                SourceOwner::Primary,
                SortFilterOrder::FilterThenSort,
                Some(4),
            ),
            SORT668_PRIMARY_FTS,
        ),
        (
            "primary_stf",
            sort668_program(
                SourceOwner::Primary,
                SortFilterOrder::SortThenFilter,
                Some(4),
            ),
            SORT668_PRIMARY_STF,
        ),
        (
            "named_stf",
            sort668_program(named("Ref"), SortFilterOrder::SortThenFilter, Some(4)),
            SORT668_NAMED_STF,
        ),
        ("multikey", sort668_multikey_program(), SORT668_MULTIKEY),
        ("order", sort668_order_program(), SORT668_ORDER),
        ("leaf", sort668_leaf_program(), SORT668_LEAF),
        ("scalar", sort668_scalar_program(), SORT668_SCALAR),
    ]
}
const SORT668_PRIMARY_FTS: &str = r#"
#[test] fn run() {
 let p=input("P.root",&[30,10,20]);let a=all("S668-G01",&p,&[]);assert_eq!(a,Ok(ExecutionOutputs{primary:output("P.root",&[10,20]),extras:vec![]}));
 let a=selected("S668-G02",&p,&[],TargetSelection::Primary,limits());assert_eq!(a,Ok(SelectedTargetOutput::Primary(output("P.root",&[10,20]))));
 let p=fields([("Value",text("P.root")),("Rows",repeated([row(30),scalar(Value::Null),row(20),row(10)]))]);let a=all("S668-G03",&p,&[]);assert_eq!(a,Ok(ExecutionOutputs{primary:output("P.root",&[10,20]),extras:vec![]}));
 let p=input("P.root",&[]);let a=all("S668-G04",&p,&[]);assert_eq!(a,Ok(ExecutionOutputs{primary:repeated([]),extras:vec![]}));
 let p=input("P.root",&[30]);let a=all("S668-G05",&p,&[]);assert_eq!(a,Ok(ExecutionOutputs{primary:repeated([]),extras:vec![]}));
 let mut zero=limits();zero.max_roots=0;let a=selected("S668-G06",&p,&[],TargetSelection::Named("unknown"),zero);assert_eq!(a,Err(RuntimeError::UnknownTarget{name:"unknown".into()}));
}
"#;
const SORT668_PRIMARY_STF: &str = r#"
#[test] fn run() {
 let p=input("P.root",&[30,10,20]);let a=all("S668-G07",&p,&[]);assert_eq!(a,Ok(ExecutionOutputs{primary:output("P.root",&[20,30]),extras:vec![]}));
 let a=selected("S668-G08",&p,&[],TargetSelection::Primary,limits());assert_eq!(a,Ok(SelectedTargetOutput::Primary(output("P.root",&[20,30]))));
 let p=input("P.root",&[20,10]);let a=all("S668-G09",&p,&[]);assert_eq!(a,Ok(ExecutionOutputs{primary:output("P.root",&[20]),extras:vec![]}));
 let p=input("P.root",&[]);let a=all("S668-G10",&p,&[]);assert_eq!(a,Ok(ExecutionOutputs{primary:repeated([]),extras:vec![]}));
}
"#;
const SORT668_NAMED_STF: &str = r#"
#[test] fn run() {
 let p=input("P.root",&[30,10,20]);let n=input("N.root",&[3,1,2]);let supplied=[BasicNamedSourceInput{name:"Ref",instance:&n}];
 let a=all("S668-G11",&p,&supplied);assert_eq!(a,Ok(ExecutionOutputs{primary:output("N.root",&[2,3]),extras:vec![]}));
 let a=selected("S668-G12",&p,&supplied,TargetSelection::Primary,limits());assert_eq!(a,Ok(SelectedTargetOutput::Primary(output("N.root",&[2,3]))));
 let a=all("S668-G13",&p,&[]);assert_eq!(a,Err(RuntimeError::OwnedSource(OwnedSourceError::SourceUnavailable{name:"Ref".into()})));
 let n=input("N.root",&[]);let supplied=[BasicNamedSourceInput{name:"Ref",instance:&n}];let a=all("S668-G14",&p,&supplied);assert_eq!(a,Ok(ExecutionOutputs{primary:repeated([]),extras:vec![]}));
}
"#;
const SORT668_MULTIKEY: &str = r#"
fn tagged(value:i64,tag:i64,id:i64)->Instance { fields([("Value",int(value)),("Flag",scalar(Value::Bool(true))),("Tag",int(tag)),("ID",int(id))]) }
fn tagged_output()->Instance { repeated([(1,2,12),(1,2,13),(1,1,11),(2,1,21)].into_iter().enumerate().map(|(index,(value,tag,id))|fields([("Value",int(value)),("Tag",int(tag)),("ID",int(id)),("Position",int((index+1)as i64)),("Root",text("P.root"))]))) }
#[test] fn run() {
 let p=fields([("Value",text("P.root")),("Rows",repeated([tagged(2,1,21),tagged(1,1,11),tagged(1,2,12),tagged(1,2,13)]))]);
 let a=all("S668-G15",&p,&[]);assert_eq!(a,Ok(ExecutionOutputs{primary:tagged_output(),extras:vec![]}));
 let a=selected("S668-G16",&p,&[],TargetSelection::Primary,limits());assert_eq!(a,Ok(SelectedTargetOutput::Primary(tagged_output())));
 let p=input("P.root",&[]);let a=all("S668-G17",&p,&[]);assert_eq!(a,Ok(ExecutionOutputs{primary:repeated([]),extras:vec![]}));
}
"#;
const SORT668_ORDER: &str = r#"
#[test] fn run() {
 let p=input("P.root",&[30,10]);let n=input("N.root",&[3]);let supplied=[BasicNamedSourceInput{name:"Ref",instance:&n}];
 let a=all("S668-G18",&p,&supplied);assert_eq!(a,Err(RuntimeError::MappingException{node:41,message:Some("target-first".into())}));
 let a=selected("S668-G19",&p,&[],TargetSelection::Primary,limits());assert_eq!(a,Err(RuntimeError::MappingException{node:41,message:Some("target-first".into())}));
 let a=selected("S668-G20",&p,&supplied,TargetSelection::Named("Named"),limits());assert_eq!(a,Err(RuntimeError::MappingException{node:42,message:Some("predicate-before-key".into())}));
}
"#;
const SORT668_LEAF: &str = r#"
#[test] fn run() {
 let p=input("P.root",&[30]);let n=input("N.root",&[3]);let supplied=[BasicNamedSourceInput{name:"Ref",instance:&n}];let a=all("S668-G21",&p,&supplied);
 assert_eq!(a,Err(RuntimeError::OwnedSourceAtNode{node:0,source:OwnedSourceError::InactiveSourceFrame{owner:OwnedSourceIdentity::Primary,collection:Some(vec!["Rows".into(),"Rows".into()])}}));
 let p=input("P.root",&[]);let a=all("S668-G22",&p,&supplied);
 assert_eq!(a,Err(RuntimeError::OwnedSourceAtNode{node:u32::MAX,source:OwnedSourceError::InactiveSourceFrame{owner:OwnedSourceIdentity::Named("Ref".into()),collection:Some(vec!["Rows".into(),"Rows".into()])}}));
 let p=fields([("Value",text("P.root"))]);let n=fields([("Value",text("N.root"))]);let supplied=[BasicNamedSourceInput{name:"Ref",instance:&n}];let a=all("S668-G23",&p,&supplied);
 assert_eq!(a,Ok(ExecutionOutputs{primary:repeated([]),extras:vec![NamedOutput{name:"Named".into(),instance:repeated([])}]}));
 let mut zero=limits();zero.max_roots=0;let a=selected("S668-G24",&p,&[],TargetSelection::Named("unknown"),zero);assert_eq!(a,Err(RuntimeError::UnknownTarget{name:"unknown".into()}));
}
"#;

const SORT668_SCALAR: &str = r#"
#[test] fn run() {
 let p=input("P.root",&[30,10,20]);
 let a=all("S668-G25",&p,&[]);assert_eq!(a,Ok(ExecutionOutputs{primary:repeated([int(10),int(20),int(30)]),extras:vec![]}));
 let a=selected("S668-G26",&p,&[],TargetSelection::Primary,limits());assert_eq!(a,Ok(SelectedTargetOutput::Primary(repeated([int(10),int(20),int(30)]))));
}
"#;

#[test]
fn public_basic_sort_freezes_six_program_twenty_four_call_inventory_and_prior_filter_recipes() {
    let programs = sort668_host_programs();
    assert_eq!(
        programs
            .iter()
            .map(|(name, _, _)| *name)
            .collect::<Vec<_>>(),
        vec![
            "primary_fts",
            "primary_stf",
            "named_stf",
            "multikey",
            "order",
            "leaf",
            "scalar"
        ]
    );
    let joined = programs
        .iter()
        .map(|(_, _, check)| *check)
        .collect::<String>();
    assert_eq!(joined.matches("S668-G").count(), 26);
    for index in 1..=26 {
        assert_eq!(joined.matches(&format!("S668-G{index:02}")).count(), 1);
    }
    for (name, program, check) in programs {
        eprintln!(
            "INPUT S668 frozen name={name} complete_program={program:#?} complete_check={check}"
        );
        assert!(
            sort668_result(
                name,
                &program,
                &public519_options("public_sort_inventory_668")
            )
            .is_ok()
        );
    }
    let previous = filter644_host_programs();
    assert_eq!(previous.len(), 14);
    let joined = previous
        .iter()
        .map(|(_, _, check)| *check)
        .collect::<String>();
    assert_eq!(joined.matches("F644-G").count(), 24);
    for index in 1..=24 {
        assert_eq!(joined.matches(&format!("F644-G{index:02}")).count(), 1);
    }
}

#[test]
#[ignore = "builds and runs generated Rust consumers with cargo"]
fn public_basic_sort_generated_six_programs_retain_all_twenty_four_results() {
    let run = HostRun::new(
        "FERRULE_PUBLIC_RUST_SORT668_EVIDENCE_ROOT",
        "public_rust_sort668",
    );
    let (root, runtime) = (run.root.clone(), run.runtime.clone());
    assert!(root.is_absolute() && runtime.is_absolute());
    fs::create_dir(&root).expect("fresh namespace; retain failures");
    fs::create_dir(root.join("src")).unwrap();
    fs::create_dir(root.join("tests")).unwrap();
    let mut libraries = String::new();
    let mut checks = String::new();
    let mut inventory = String::new();
    for (name, program, check) in sort668_host_programs() {
        let options = public519_options("ferrule_public_sort_668");
        retained_file(
            &root.join(format!("{name}.Program.original")),
            format!("{program:#?}\n").as_bytes(),
        );
        retained_file(
            &root.join(format!("{name}.Options.original")),
            format!("{options:#?}\n").as_bytes(),
        );
        retained_file(
            &root.join(format!("{name}.checks.original.rs")),
            check.as_bytes(),
        );
        let actual = sort668_result(name, &program, &options);
        retained_file(
            &root.join(format!("{name}.renderer-result.original")),
            format!("{actual:#?}\n").as_bytes(),
        );
        let files = actual.expect("full inputs/artifacts/error retained before qualification");
        let source = public519_source(&files);
        retained_file(
            &root.join(format!("{name}.generated.original.rs")),
            source.as_bytes(),
        );
        libraries.push_str(&format!("pub mod {name} {{\n{source}\n}}\n"));
        checks.push_str(&format!("mod {name} {{\nuse ferrule_public_sort_668::{name}::*;\n{FILTER644_HOST_COMMON}\n{check}\n}}\n"));
        inventory.push_str(&format!(
            "{name} calls={}\n",
            check.matches("S668-G").count()
        ));
    }
    retained_file(
        &root.join("PROGRAM_CHECK_INVENTORY.original"),
        inventory.as_bytes(),
    );
    retained_file(&root.join("src/lib.rs"), libraries.as_bytes());
    retained_file(&root.join("tests/sort.rs"), checks.as_bytes());
    let options = Options {
        package_name: "ferrule_public_sort_668".into(),
        runtime_dependency: RuntimeDependency::Path(
            runtime
                .to_str()
                .expect("Unicode current runtime path")
                .into(),
        ),
    };
    retained_file(
        &root.join("Cargo.toml"),
        render_manifest(&options).as_bytes(),
    );
    retained_file(
        &root.join("COMMAND.original"),
        b"cargo test --offline --jobs 1 --test sort -- --nocapture --test-threads=1\n",
    );
    let actual = Command::new("cargo")
        .args([
            "test",
            "--offline",
            "--jobs",
            "1",
            "--test",
            "sort",
            "--",
            "--nocapture",
            "--test-threads=1",
        ])
        .current_dir(&root)
        .generated_host_output(&root);
    match &actual {
        Ok(output) => {
            retained_file(&root.join("stdout.complete.original"), &output.stdout);
            retained_file(&root.join("stderr.complete.original"), &output.stderr);
            retained_file(
                &root.join("status.complete.original"),
                format!("{:?}\n", output.status).as_bytes(),
            );
        }
        Err(error) => retained_file(
            &root.join("call-error.complete.original"),
            format!("{error:#?}\n").as_bytes(),
        ),
    }
    eprintln!("RAW S668 complete compiler/host return={actual:#?}");
    assert!(
        actual
            .expect("complete reached output or call error retained")
            .status
            .success()
    );
}
