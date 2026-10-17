use super::*;

fn iteration(p: &mut Program, windows: Vec<SequenceWindow>, output: IterationOutput) {
    let old = p.root.iteration.as_ref().unwrap();
    p.root.iteration = Some(IterationPlan::new(
        old.input().clone(),
        old.filter(),
        old.sort().cloned(),
        windows,
        output,
    ));
    if output == IterationOutput::First {
        p.target.repeating = false;
        p.root.repeating = false;
    }
}

fn program(owner: SourceOwner, windows: Vec<SequenceWindow>) -> Program {
    let mut p = filter644_program(owner);
    let old = p.root.iteration.as_ref().unwrap();
    p.root.iteration = Some(IterationPlan::new(
        old.input().clone(),
        None,
        None,
        windows,
        IterationOutput::Repeated,
    ));
    p.expressions.extend([
        node(
            702,
            Expression::Const {
                value: Value::Int(2),
            },
        ),
        node(
            703,
            Expression::Const {
                value: Value::Int(3),
            },
        ),
        node(
            704,
            Expression::Const {
                value: Value::Int(0),
            },
        ),
    ]);
    p
}

fn artifacts(p: &Program) -> ArtifactSet {
    emit_with_basic_owned_windows(p, &public519_options("public_windows_702")).unwrap()
}

#[test]
fn public_basic_windows_require_explicit_admission_and_keep_old_api_priority() {
    for window in [
        SequenceWindow::SkipFirst { count: 3 },
        SequenceWindow::First { count: 702 },
        SequenceWindow::From { position: 702 },
        SequenceWindow::FromTo {
            first: 702,
            last: 703,
        },
        SequenceWindow::Last { count: 702 },
    ] {
        let p = program(SourceOwner::Primary, vec![window]);
        let options = public519_options("window_old_gates_702");
        let ordinary = codegen::validate_program(&p).unwrap_err();
        filter644_invalid(emit(&p, &options), ordinary.clone());
        filter644_invalid(
            emit_with_basic_owned_sources(&p, &options),
            ordinary.clone(),
        );
        for actual in [
            crate::owned_basic::render_basic_owned_source(&p),
            crate::owned_basic::render_checked_basic_owned_source(&p),
        ] {
            match actual {
                Err(EmitError::InvalidProgram(error)) => assert_eq!(error, ordinary),
                other => panic!("private gate changed: {other:?}"),
            }
        }
        filter644_invalid(
            emit_with_basic_owned_filters(&p, &options),
            codegen::validate_basic_owned_filter_emission(&p).unwrap_err(),
        );
        filter644_invalid(
            emit_with_basic_owned_sort(&p, &options),
            codegen::validate_basic_owned_sort_emission(&p).unwrap_err(),
        );
        assert!(emit_with_basic_owned_windows(&p, &options).is_ok());
        let mut malformed = p.clone();
        malformed.expressions.push(node(
            1,
            Expression::Const {
                value: Value::Int(9),
            },
        ));
        let mut bad = options;
        bad.package_name = "0invalid".into();
        filter644_invalid(
            emit_with_basic_owned_windows(&malformed, &bad),
            ProgramValidationError::DuplicateExpression { node: 1 },
        );
        assert!(
            matches!(emit_with_basic_owned_windows(&p, &bad), Err(EmitError::InvalidPackageName(name)) if name == "0invalid")
        );
    }
}

#[test]
fn public_basic_windows_keep_parent_bound_order_implicit_first_and_grouped_sort_controls() {
    let mut p = program(SourceOwner::Primary, vec![]);
    iteration(
        &mut p,
        vec![
            SequenceWindow::First { count: 704 },
            SequenceWindow::FromTo {
                first: 702,
                last: 703,
            },
            SequenceWindow::Last { count: 3 },
        ],
        IterationOutput::First,
    );
    let files = artifacts(&p);
    let source = public519_source(&files);
    let expected = "let count_0 = codegen_runtime::item_count(704, expression_704(parent_context)?)?;\n            let window_0 = codegen_runtime::SequenceWindow::First(count_0);\n            let first_1 = codegen_runtime::item_count(702, expression_702(parent_context)?)?;\n            let last_1 = codegen_runtime::item_count(703, expression_703(parent_context)?)?;\n            let window_1 = codegen_runtime::SequenceWindow::FromTo { first: first_1, last: last_1 };\n            let count_2 = codegen_runtime::item_count(3, expression_3(parent_context)?)?;\n            let window_2 = codegen_runtime::SequenceWindow::Last(count_2);\n            Ok([window_0,window_1,window_2,codegen_runtime::SequenceWindow::First(1),])";
    assert!(source.contains(expected));
    assert!(source.contains(
        "|parent_context| -> Result<[codegen_runtime::SequenceWindow; 4], RuntimeError>"
    ));
    assert_eq!(
        source
            .matches("context.visit_windowed_source_cohort(")
            .count(),
        1
    );
    assert!(!source.contains("context.reindex_phase("));
    for order in [
        SortFilterOrder::FilterThenSort,
        SortFilterOrder::SortThenFilter,
    ] {
        let mut p = sort668_program(SourceOwner::Primary, order, Some(4));
        iteration(
            &mut p,
            vec![SequenceWindow::Last { count: 3 }],
            IterationOutput::Repeated,
        );
        let files = artifacts(&p);
        let source = public519_source(&files);
        assert!(source.contains(&format!(
            "([codegen_runtime::SortDirection::Ascending,], {}),",
            order == SortFilterOrder::FilterThenSort
        )));
        assert!(source.contains("let key_0 = expression_1(key_context)?;"));
        assert!(source.contains("let value = expression_4(predicate_context)?;"));
        assert_eq!(
            source
                .matches("context.visit_windowed_source_cohort(")
                .count(),
            1
        );
        assert!(!source.contains("context.visit_sorted_source_cohort("));
    }
}

fn legacy_program(literals: bool) -> Program {
    let mut p = program(
        SourceOwner::Primary,
        vec![SequenceWindow::SkipFirst { count: 3 }],
    );
    p.expressions
        .iter_mut()
        .find(|n| n.id == 1)
        .unwrap()
        .expression = Expression::SourceField {
        path: path(&["Value"]),
        frame: None,
    };
    p.expressions
        .iter_mut()
        .find(|n| n.id == 2)
        .unwrap()
        .expression = Expression::Position {
        collection: Vec::new(),
    };
    p.expressions
        .iter_mut()
        .find(|n| n.id == 5)
        .unwrap()
        .expression = Expression::Const {
        value: Value::String(
            if literals {
                r##"expression_literal 雪 " \ "##
            } else {
                "legacy"
            }
            .into(),
        ),
    };
    let collection = if literals { "expression_Rows" } else { "Rows" };
    p.root.iteration = Some(IterationPlan::new(
        codegen::SourceIteration::new(path(&[collection])),
        None,
        None,
        vec![SequenceWindow::SkipFirst { count: 3 }],
        IterationOutput::Repeated,
    ));
    if literals {
        if let ir::SchemaKind::Group { children, .. } = &mut p.source.kind {
            children.iter_mut().find(|n| n.name == "Rows").unwrap().name = collection.into();
        }
        if let ir::SchemaKind::Group { children, .. } = &mut p.target.kind {
            children
                .iter_mut()
                .find(|n| n.name == "Value")
                .unwrap()
                .name = "expression_Value".into();
        }
        p.root.bindings[0].target_field = "expression_Value".into();
    }
    p
}

fn mixed_program() -> Program {
    let p = program(
        SourceOwner::Primary,
        vec![SequenceWindow::First { count: 702 }],
    );
    let mut legacy = legacy_program(true);
    let mut owned = p.root.clone();
    owned.target_field = "Owned".into();
    legacy.root.target_field = "expression_Legacy".into();
    let mut combined = p;
    combined.root = scope(&[]);
    // Legacy expression IDs stay independent of the owned sibling's helpers.
    for node in &mut legacy.expressions {
        node.id += 1000;
    }
    for binding in &mut legacy.root.bindings {
        binding.expression += 1000;
    }
    let old = legacy.root.iteration.as_ref().unwrap();
    legacy.root.iteration = Some(IterationPlan::new(
        old.input().clone(),
        None,
        None,
        vec![SequenceWindow::SkipFirst { count: 1003 }],
        IterationOutput::Repeated,
    ));
    combined.expressions.extend(
        legacy
            .expressions
            .into_iter()
            .filter(|n| [1001, 1002, 1003, 1005].contains(&n.id)),
    );
    if let ir::SchemaKind::Group { children, .. } = &mut combined.source.kind {
        let ir::SchemaKind::Group {
            children: legacy_children,
            ..
        } = &legacy.source.kind
        else {
            unreachable!()
        };
        children.push(
            legacy_children
                .iter()
                .find(|n| n.name == "expression_Rows")
                .unwrap()
                .clone(),
        );
    }
    let mut owned_target = combined.target.clone();
    owned_target.name = "Owned".into();
    let mut legacy_target = legacy.target;
    legacy_target.name = "expression_Legacy".into();
    combined.target = SchemaNode::group("Output", vec![owned_target, legacy_target]);
    combined.root.children = vec![owned, legacy.root];
    combined
}

fn nested_program() -> Program {
    let mut p = program(
        SourceOwner::Primary,
        vec![SequenceWindow::Last { count: 702 }],
    );
    p.expressions.extend([
        node(
            710,
            owned(
                SourceOwner::Primary,
                &["Rows", "Rows", "Value"],
                Some(&["Rows", "Rows"]),
            ),
        ),
        node(711, position(SourceOwner::Primary, Some(&["Rows", "Rows"]))),
    ]);
    let mut child = scope(&[
        ("Value", 710, ScalarType::Int),
        ("Position", 711, ScalarType::Int),
    ]);
    child.target_field = "Nested".into();
    child.repeating = true;
    child.iteration = Some(IterationPlan::new(
        codegen::OwnedSourceIteration::new(OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows", "Rows"]),
        }),
        None,
        None,
        vec![SequenceWindow::First { count: 2 }],
        IterationOutput::Repeated,
    ));
    p.root.children.push(child);
    if let ir::SchemaKind::Group { children, .. } = &mut p.target.kind {
        children.push(
            SchemaNode::group(
                "Nested",
                vec![
                    SchemaNode::scalar("Value", ScalarType::Int),
                    SchemaNode::scalar("Position", ScalarType::Int),
                ],
            )
            .repeating(),
        );
    }
    p
}

fn mixed_ancestry_program(owned_parent: bool) -> Program {
    let mut p = program(SourceOwner::Primary, vec![]);
    if !owned_parent {
        p.root.iteration = Some(IterationPlan::new(
            codegen::SourceIteration::new(path(&["Rows"])),
            None,
            None,
            vec![],
            IterationOutput::Repeated,
        ));
    }
    // The parent still needs a paired context for its owned bindings. The child
    // uses only ordinary expressions and introduces no new repeated cursor.
    p.expressions.extend([
        node(
            710,
            Expression::SourceField {
                path: path(&["Value"]),
                frame: None,
            },
        ),
        node(711, Expression::Position { collection: vec![] }),
    ]);
    let mut child = scope(&[
        ("Value", 710, ScalarType::Int),
        ("Position", 711, ScalarType::Int),
    ]);
    child.target_field = "Legacy".into();
    child.iteration = Some(IterationPlan::new(
        codegen::SourceIteration::new(vec![]),
        None,
        None,
        vec![],
        IterationOutput::First,
    ));
    p.root.children.push(child);
    if let ir::SchemaKind::Group { children, .. } = &mut p.target.kind {
        children.push(SchemaNode::group(
            "Legacy",
            vec![
                SchemaNode::scalar("Value", ScalarType::Int),
                SchemaNode::scalar("Position", ScalarType::Int),
            ],
        ));
    }
    p
}

#[test]
fn public_basic_windows_mixed_ancestry_routes_only_owned_iterations_as_inherited_pairing() {
    let ordinary = artifacts(&mixed_ancestry_program(false));
    let source = public519_source(&ordinary);
    assert!(source.contains("fn scope_root(context: &BasicScopeContext<'_>)"));
    assert!(source.contains("context.walk_source_cohort(&[\"Rows\"])?"));
    assert!(source.contains("fn legacy_scope_root_0(context: &ScopeContext<'_>)"));
    assert!(source.contains("legacy_scope_root_0(context.legacy_view())"));
    assert!(source.contains(
        "let output_context = item_context.with_compact_last_position(outputs.len() + 1);"
    ));
    let owned = artifacts(&mixed_ancestry_program(true));
    let source = public519_source(&owned);
    assert!(
        source.contains("context.walk_owned_source_cohort(SourceOwnerRef::Primary, &[\"Rows\"])?")
    );
    assert!(source.contains("fn scope_root_0(context: &BasicScopeContext<'_>)"));
    assert!(!source.contains("fn legacy_scope_root_0("));
    assert!(source.contains("context.walk_source_cohort(&[])?"));
}

#[test]
fn public_basic_windows_compose_legacy_identifiers_without_changing_literals_or_paired_dispatch() {
    let files = artifacts(&mixed_program());
    let source = public519_source(&files);
    assert!(source.contains("fn legacy_scope_root_1(context: &ScopeContext<'_>)"));
    assert!(source.contains("legacy_scope_root_1(context.legacy_view())"));
    assert!(source.contains("fn legacy_expression_1001(context: &ScopeContext<'_>)"));
    assert!(source.contains("context.walk_source(&[\"expression_Rows\"]);"));
    assert!(source.contains("field(\"expression_Value\", scalar(value_0))"));
    assert!(source.contains(&rust_string(r##"expression_literal 雪 " \ "##)));
    assert!(!source.contains("legacy_expression_Rows"));
    assert!(!source.contains("legacy_expression_Value"));
    let files = artifacts(&nested_program());
    let source = public519_source(&files);
    assert!(source.contains("fn scope_root_0(context: &BasicScopeContext<'_>)"));
    assert!(!source.contains("fn legacy_scope_root_0("));
    assert!(source.contains("item_count(2, expression_2(parent_context)?)"));
}

#[test]
fn public_basic_windows_keep_potential_bound_dependencies_and_exact_unsigned_annotations() {
    for id in [0, u32::MAX] {
        let mut p = program(
            SourceOwner::Primary,
            vec![SequenceWindow::First { count: 712 }],
        );
        p.expressions.extend([
            node(id, owned(named("Unused"), &["Value"], Some(&[]))),
            node(
                710,
                Expression::SourceField {
                    path: path(&["Ref", "Value"]),
                    frame: None,
                },
            ),
            node(
                711,
                Expression::Const {
                    value: Value::Bool(false),
                },
            ),
            node(
                712,
                Expression::If {
                    condition: 711,
                    then: id,
                    else_: 710,
                },
            ),
        ]);
        let plan = codegen::legacy_named_input_admission_plan_with_basic_owned_windows(&p).unwrap();
        let availability = |name: &str| {
            plan.primary
                .entries
                .iter()
                .find(|e| e.name == name)
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
        let files = artifacts(&p);
        let source = public519_source(&files);
        assert!(public582_map_body(source, id).contains(&format!(".with_owned_source_node({id})")));
        assert!(public582_map_body(source, 712).contains(&format!("expression_{id}(context)")));
        assert!(source.contains("expression_712(parent_context)"));
    }
}

#[test]
fn public_basic_windows_admit_repeated_scalar_but_keep_scalar_first_and_special_domains_closed() {
    let mut p = sort668_scalar_program();
    iteration(
        &mut p,
        vec![SequenceWindow::Last { count: 1 }],
        IterationOutput::Repeated,
    );
    assert!(public519_source(&artifacts(&p)).contains(
        "let output = scalar(adapt_target_value(expression_1(item_context)?, ScalarType::Int));"
    ));
    iteration(&mut p, vec![], IterationOutput::First);
    filter644_invalid(
        emit_with_basic_owned_windows(&p, &public519_options("scalar_first")),
        ProgramValidationError::InvalidIterationOutput {
            target_path: vec![],
            output: IterationOutput::First,
        },
    );
    let mut p = program(
        SourceOwner::Primary,
        vec![SequenceWindow::First { count: 702 }],
    );
    let old = p.root.iteration.as_ref().unwrap().clone();
    p.root.iteration = Some(old.with_grouping(GroupingPlan::By { key: 1 }));
    filter644_invalid(
        emit_with_basic_owned_windows(&p, &public519_options("grouped")),
        sort668_scope_error(codegen::OwnedSourceCapabilityReason::Grouping),
    );
}

fn ordinary_check(expected: &[i64], root: &str) -> String {
    format!(
        r#"
#[test] fn run() {{
 let p=input("P.root",&[30,10,20]); let expected=output({root:?},&{expected:?});
 assert_eq!(all("window-all",&p,&[]),Ok(ExecutionOutputs{{primary:expected.clone(),extras:vec![]}}));
 assert_eq!(selected("window-selected",&p,&[],TargetSelection::Primary,limits()),Ok(SelectedTargetOutput::Primary(expected)));
}}
"#
    )
}

fn host_programs() -> Vec<(&'static str, Program, String)> {
    let mut cases = Vec::new();
    for (name, windows, expected) in [
        (
            "skip",
            vec![SequenceWindow::SkipFirst { count: 3 }],
            vec![10, 20],
        ),
        (
            "first",
            vec![SequenceWindow::First { count: 702 }],
            vec![30, 10],
        ),
        (
            "from",
            vec![SequenceWindow::From { position: 702 }],
            vec![10, 20],
        ),
        (
            "from_to",
            vec![SequenceWindow::FromTo {
                first: 702,
                last: 703,
            }],
            vec![10, 20],
        ),
        (
            "last",
            vec![SequenceWindow::Last { count: 702 }],
            vec![10, 20],
        ),
        (
            "compound",
            vec![
                SequenceWindow::SkipFirst { count: 3 },
                SequenceWindow::Last { count: 3 },
            ],
            vec![20],
        ),
        ("zero", vec![SequenceWindow::First { count: 704 }], vec![]),
        (
            "reversed",
            vec![SequenceWindow::FromTo {
                first: 703,
                last: 3,
            }],
            vec![],
        ),
    ] {
        cases.push((
            name,
            program(SourceOwner::Primary, windows),
            ordinary_check(&expected, "P.root"),
        ));
    }
    for (name, order, expected) in [
        ("filter_sort", SortFilterOrder::FilterThenSort, vec![20]),
        ("sort_filter", SortFilterOrder::SortThenFilter, vec![30]),
    ] {
        let mut p = sort668_program(SourceOwner::Primary, order, Some(4));
        iteration(
            &mut p,
            vec![SequenceWindow::Last { count: 3 }],
            IterationOutput::Repeated,
        );
        cases.push((name, p, ordinary_check(&expected, "P.root")));
    }
    let mut scalar = sort668_scalar_program();
    scalar.expressions.push(node(
        702,
        Expression::Const {
            value: Value::Int(2),
        },
    ));
    iteration(
        &mut scalar,
        vec![SequenceWindow::Last { count: 702 }],
        IterationOutput::Repeated,
    );
    cases.push(("scalar",scalar,r#"
#[test] fn run() { let p=input("P.root",&[30,10,20]); assert_eq!(all("scalar",&p,&[]),Ok(ExecutionOutputs{primary:repeated([int(20),int(30)]),extras:vec![]})); }
"#.into()));
    let mut first = program(SourceOwner::Primary, vec![]);
    iteration(
        &mut first,
        vec![SequenceWindow::SkipFirst { count: 3 }],
        IterationOutput::First,
    );
    cases.push(("implicit_first",first,r#"
#[test] fn run() {
 let p=input("P.root",&[30,10,20]); let expected=fields([("Value",int(10)),("Position",int(1)),("Root",text("P.root"))]);
 assert_eq!(all("first",&p,&[]),Ok(ExecutionOutputs{primary:expected,extras:vec![]}));
 let p=input("P.root",&[]); assert_eq!(all("empty-first",&p,&[]),Ok(ExecutionOutputs{primary:group([]),extras:vec![]}));
}
"#.into()));
    cases.push(("named",program(named("Ref"),vec![SequenceWindow::SkipFirst { count: 3 }]),r#"
#[test] fn run() {
 let p=input("P.root",&[30,10,20]); let n=input("N.root",&[3,1,2]); let supplied=[BasicNamedSourceInput{name:"Ref",instance:&n}];
 assert_eq!(all("named",&p,&supplied),Ok(ExecutionOutputs{primary:output("N.root",&[1,2]),extras:vec![]}));
 assert_eq!(all("unavailable",&p,&[]),Err(RuntimeError::OwnedSource(OwnedSourceError::SourceUnavailable{name:"Ref".into()})));
 let mut zero=limits(); zero.max_roots=0; assert_eq!(selected("unknown",&p,&[],TargetSelection::Named("unknown"),zero),Err(RuntimeError::UnknownTarget{name:"unknown".into()}));
}
"#.into()));
    let mut bound = program(
        SourceOwner::Primary,
        vec![SequenceWindow::First { count: 721 }],
    );
    bound.expressions.extend([
        node(
            720,
            Expression::Const {
                value: Value::String("parent-bound".into()),
            },
        ),
        node(721, Expression::Raise { message: Some(720) }),
    ]);
    cases.push(("bound_error",bound,r#"
#[test] fn run() { for values in [&[][..],&[30][..]] { let p=input("P.root",values); assert_eq!(all("bound-error",&p,&[]),Err(RuntimeError::MappingException{node:721,message:Some("parent-bound".into())})); } }
"#.into()));
    let mut batch = program(SourceOwner::Primary, vec![]);
    batch.expressions.extend([
        node(
            720,
            Expression::Const {
                value: Value::String("later-predicate".into()),
            },
        ),
        node(721, Expression::Raise { message: Some(720) }),
        node(
            722,
            Expression::Const {
                value: Value::Bool(true),
            },
        ),
        node(
            723,
            Expression::If {
                condition: 4,
                then: 721,
                else_: 722,
            },
        ),
        node(
            724,
            Expression::Const {
                value: Value::String("earlier-target".into()),
            },
        ),
        node(725, Expression::Raise { message: Some(724) }),
    ]);
    let input = batch.root.iteration.as_ref().unwrap().input().clone();
    batch.root.iteration = Some(IterationPlan::new(
        input,
        Some(723),
        None,
        vec![SequenceWindow::First { count: 704 }],
        IterationOutput::Repeated,
    ));
    batch.root.bindings[0].expression = 725;
    cases.push(("predicate_batch",batch,r#"
#[test] fn run() {
 let p=input("P.root",&[30,10]); assert_eq!(all("batch",&p,&[]),Err(RuntimeError::MappingException{node:721,message:Some("later-predicate".into())}));
 let p=input("P.root",&[30]); assert_eq!(all("zero",&p,&[]),Ok(ExecutionOutputs{primary:repeated([]),extras:vec![]}));
}
"#.into()));
    cases.push((
        "legacy_only",
        legacy_program(false),
        ordinary_check(&[10, 20], "legacy"),
    ));
    cases.push(("literal_names",legacy_program(true),r####"
#[test] fn run() {
 let p=fields([("Value",text("P.root")),("expression_Rows",repeated([row(30),row(10),row(20)]))]);
 let expected=repeated([(10,1),(20,2)].into_iter().map(|(v,pos)|fields([("expression_Value",int(v)),("Position",int(pos)),("Root",text(r##"expression_literal 雪 " \ "##))])));
 assert_eq!(all("literal",&p,&[]),Ok(ExecutionOutputs{primary:expected.clone(),extras:vec![]}));
 assert_eq!(selected("literal-selected",&p,&[],TargetSelection::Primary,limits()),Ok(SelectedTargetOutput::Primary(expected)));
}
"####.into()));
    let mut lazy = program(
        SourceOwner::Primary,
        vec![SequenceWindow::First { count: 712 }],
    );
    lazy.expressions.extend([
        node(710, owned(named("Unused"), &["Value"], Some(&[]))),
        node(
            711,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        node(
            712,
            Expression::If {
                condition: 711,
                then: 710,
                else_: 703,
            },
        ),
    ]);
    cases.push(("lazy_bound", lazy, ordinary_check(&[30, 10, 20], "P.root")));
    let mut eager = program(
        SourceOwner::Primary,
        vec![
            SequenceWindow::First { count: 704 },
            SequenceWindow::FromTo {
                first: 702,
                last: u32::MAX,
            },
        ],
    );
    eager.expressions.push(node(
        u32::MAX,
        Expression::Const {
            value: Value::Bool(true),
        },
    ));
    cases.push(("eager_bounds",eager,r#"
#[test] fn run() {
 for values in [&[][..],&[30,10,20][..]] {
  let p=input("P.root",values);
  assert_eq!(all("eager-bounds",&p,&[]),Err(RuntimeError::NotAnItemCount{node:u32::MAX,found:"bool"}));
 }
}
"#.into()));
    cases.push(("nested",nested_program(),r#"
#[test] fn run() {
 let p=input("P.root",&[30,10,20]);
 let expected=repeated([
  fields([("Value",int(10)),("Position",int(1)),("Root",text("P.root")),("Nested",repeated([fields([("Value",int(101)),("Position",int(1))])]))]),
  fields([("Value",int(20)),("Position",int(2)),("Root",text("P.root")),("Nested",repeated([fields([("Value",int(201)),("Position",int(1))]),fields([("Value",int(202)),("Position",int(2))])]))]),
 ]);
 assert_eq!(all("nested",&p,&[]),Ok(ExecutionOutputs{primary:expected,extras:vec![]}));
}
"#.into()));
    cases.push(("mixed",mixed_program(),r####"
#[test] fn run() {
 let p=fields([("Value",text("P.root")),("Rows",repeated([row(30),row(10),row(20)])),("expression_Rows",repeated([row(3),row(1),row(2)]))]);
 let expected=fields([("Owned",output("P.root",&[30,10])),("expression_Legacy",repeated([(1,1),(2,2)].into_iter().map(|(v,pos)|fields([("expression_Value",int(v)),("Position",int(pos)),("Root",text(r##"expression_literal 雪 " \ "##))]))))]);
 assert_eq!(all("mixed",&p,&[]),Ok(ExecutionOutputs{primary:expected,extras:vec![]}));
}
"####.into()));
    cases
}

#[test]
fn public_basic_windows_all_generated_models_pass_current_admission() {
    let cases = host_programs();
    assert_eq!(cases.len(), 21);
    for (_, p, _) in cases {
        artifacts(&p);
    }
}

#[test]
fn public_basic_windows_generated_public_consumers_execute_full_outputs_and_errors() {
    run_generated_consumers(host_programs(), true);
}

#[test]
fn public_basic_windows_generated_mixed_ancestry_preserves_native_positions() {
    let ordinary_check = r#"
#[test] fn run() {
 let p=input("P.root",&[30,10,20]);
 let expected=repeated([(30,1,1),(10,2,1),(20,3,1)].into_iter().map(|(value,parent_position,child_position)|fields([
  ("Value",int(value)),("Position",int(parent_position)),("Root",text("P.root")),
  ("Legacy",fields([("Value",int(value)),("Position",int(child_position))])),
 ])));
 assert_eq!(all("ordinary-ancestry",&p,&[]),Ok(ExecutionOutputs{primary:expected.clone(),extras:vec![]}));
 assert_eq!(selected("ordinary-ancestry-selected",&p,&[],TargetSelection::Primary,limits()),Ok(SelectedTargetOutput::Primary(expected)));
}
"#;
    let owned_check = r#"
#[test] fn run() {
 let p=input("P.root",&[30,10,20]);
 let expected=repeated([(30,1,1),(10,2,2),(20,3,3)].into_iter().map(|(value,parent_position,child_position)|fields([
  ("Value",int(value)),("Position",int(parent_position)),("Root",text("P.root")),
  ("Legacy",fields([("Value",int(value)),("Position",int(child_position))])),
 ])));
 assert_eq!(all("owned-ancestry",&p,&[]),Ok(ExecutionOutputs{primary:expected.clone(),extras:vec![]}));
 assert_eq!(selected("owned-ancestry-selected",&p,&[],TargetSelection::Primary,limits()),Ok(SelectedTargetOutput::Primary(expected)));
}
"#;
    run_generated_consumers(
        vec![
            (
                "ordinary_ancestry",
                mixed_ancestry_program(false),
                ordinary_check.into(),
            ),
            (
                "owned_ancestry",
                mixed_ancestry_program(true),
                owned_check.into(),
            ),
        ],
        false,
    );
}

fn run_generated_consumers(programs: Vec<(&str, Program, String)>, include_prior_profiles: bool) {
    let output = TempDir::new("basic_windows_702");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("codegen-runtime");
    let options = Options {
        package_name: "ferrule_window_consumers".into(),
        runtime_dependency: RuntimeDependency::Path(runtime.to_str().unwrap().into()),
    };
    let mut libraries = String::new();
    let mut checks = String::new();
    for (name, p, check) in programs {
        let files = emit_with_basic_owned_windows(&p, &options).unwrap();
        libraries.push_str(&format!(
            "pub mod {name} {{\n{}\n}}\n",
            public519_source(&files)
        ));
        checks.push_str(&format!("mod {name} {{\nuse ferrule_window_consumers::{name}::*;\n{FILTER644_HOST_COMMON}\n{check}\n}}\n"));
    }
    // Compile and execute the existing finite filter/sort public recipes against
    // this same runtime, without changing their models or full typed oracles.
    for (name, p, check) in filter644_host_programs()
        .into_iter()
        .filter(|_| include_prior_profiles)
    {
        let name = format!("prior_filter_{name}");
        let files = emit_with_basic_owned_filters(&p, &options).unwrap();
        libraries.push_str(&format!(
            "pub mod {name} {{\n{}\n}}\n",
            public519_source(&files)
        ));
        checks.push_str(&format!("mod {name} {{\nuse ferrule_window_consumers::{name}::*;\n{FILTER644_HOST_COMMON}\n{check}\n}}\n"));
    }
    for (name, p, check) in sort668_host_programs()
        .into_iter()
        .filter(|_| include_prior_profiles)
    {
        let name = format!("prior_sort_{name}");
        let files = emit_with_basic_owned_sort(&p, &options).unwrap();
        libraries.push_str(&format!(
            "pub mod {name} {{\n{}\n}}\n",
            public519_source(&files)
        ));
        checks.push_str(&format!("mod {name} {{\nuse ferrule_window_consumers::{name}::*;\n{FILTER644_HOST_COMMON}\n{check}\n}}\n"));
    }
    fs::create_dir_all(output.path().join("src")).unwrap();
    fs::create_dir_all(output.path().join("tests")).unwrap();
    fs::write(output.path().join("Cargo.toml"), render_manifest(&options)).unwrap();
    fs::write(output.path().join("src/lib.rs"), libraries).unwrap();
    fs::write(output.path().join("tests/windows.rs"), checks).unwrap();
    let result = Command::new("cargo")
        .args([
            "test",
            "--offline",
            "--jobs",
            "1",
            "--test",
            "windows",
            "--",
            "--test-threads=1",
        ])
        .current_dir(output.path())
        .generated_host_output(output.path())
        .unwrap();
    assert!(
        result.status.success(),
        "generated window consumers failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
